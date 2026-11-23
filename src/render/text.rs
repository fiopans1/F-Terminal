use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight, Wrap, fontdb,
};

use crate::config::Config;
use crate::layout::Rect;
use crate::render::scene::{Label, Scene};
use crate::term::color::Theme;
use crate::term::frame::Frame;

/// Size of a single terminal cell in physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct CellMetrics {
    pub width: f32,
    pub height: f32,
}

/// Font and atlas resources that can be shared by every window.
pub(crate) struct SharedText {
    font_system: FontSystem,
    swash_cache: SwashCache,
    cache: Cache,
    atlases: HashMap<wgpu::TextureFormat, TextAtlasBundle>,
    family: String,
    ui_family: String,
}

struct TextAtlasBundle {
    atlas: TextAtlas,
    renderer: TextRenderer,
}

/// CPU-side font state that can be built on a worker thread while the GPU
/// device is being initialized.
pub(crate) struct FontResources {
    font_system: FontSystem,
    swash_cache: SwashCache,
    family: String,
    ui_family: String,
}

impl FontResources {
    /// Scan system fonts and pick the terminal and UI families.
    ///
    /// `FontSystem::new` loads every installed face and is one of the slowest
    /// parts of startup, so callers should run it concurrently with GPU setup.
    pub(crate) fn load(preferred: Option<&str>) -> Self {
        let mut font_system = FontSystem::new();
        let family = choose_terminal_family(&mut font_system, preferred);
        let ui_family = choose_ui_family(&mut font_system, &family);

        Self {
            font_system,
            swash_cache: SwashCache::new(),
            family,
            ui_family,
        }
    }
}

impl SharedText {
    #[cfg(test)]
    pub(crate) fn new(device: &wgpu::Device, config: &Config) -> Self {
        Self::new_with_fonts(device, FontResources::load(config.font_family.as_deref()))
    }

    pub(crate) fn new_with_fonts(device: &wgpu::Device, fonts: FontResources) -> Self {
        Self {
            font_system: fonts.font_system,
            swash_cache: fonts.swash_cache,
            cache: Cache::new(device),
            atlases: HashMap::new(),
            family: fonts.family,
            ui_family: fonts.ui_family,
        }
    }
}

/// Glyphon based text renderer. Terminal panes each keep a shaped buffer so
/// their text can be reused between frames; sidebar labels get short-lived
/// buffers because there are only a handful of them.
pub struct TextPipeline {
    shared: Rc<RefCell<SharedText>>,
    format: wgpu::TextureFormat,
    viewport: Viewport,
    pane_buffers: HashMap<u64, Buffer>,
    metrics: CellMetrics,
    font_size: f32,
}

impl TextPipeline {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        scale_factor: f64,
        config: &Config,
        shared: Rc<RefCell<SharedText>>,
    ) -> Self {
        let _ = queue;
        let (font_size, metrics) = {
            let mut shared_ref = shared.borrow_mut();
            let family = shared_ref.family.clone();
            let font_size = config.font_size * scale_factor as f32;
            let line_height = font_size * config.line_height;
            let metrics =
                measure_cell(&mut shared_ref.font_system, &family, font_size, line_height);
            (font_size, metrics)
        };

        let viewport = {
            let shared_ref = shared.borrow();
            Viewport::new(device, &shared_ref.cache)
        };

        Self {
            shared,
            format,
            viewport,
            pane_buffers: HashMap::new(),
            metrics,
            font_size,
        }
    }

    #[cfg(test)]
    pub fn new_isolated(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        scale_factor: f64,
        config: &Config,
    ) -> Self {
        let shared = Rc::new(RefCell::new(SharedText::new(device, config)));
        Self::new(device, queue, format, scale_factor, config, shared)
    }

    pub(crate) fn shared_text(&self) -> Rc<RefCell<SharedText>> {
        self.shared.clone()
    }

    pub fn cell_metrics(&self) -> CellMetrics {
        self.metrics
    }

    /// Shape a short UI string and return its width in physical pixels.
    pub fn measure_ui_text(&mut self, text: &str, font_size: f32, bold: bool) -> f32 {
        let line_height = (font_size * 1.35).max(1.0);
        let mut shared = self.shared.borrow_mut();
        let SharedText {
            font_system,
            ui_family,
            ..
        } = &mut *shared;

        let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(Some(10_000.0), Some(line_height));

        let attrs = Attrs::new()
            .family(Family::Name(ui_family))
            .weight(if bold { Weight::BOLD } else { Weight::NORMAL });
        buffer.set_text(text, &attrs, Shaping::Basic, None);
        buffer.shape_until_scroll(font_system, false);

        buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max)
    }

    pub fn update_viewport(&mut self, queue: &wgpu::Queue, width: u32, height: u32) {
        self.viewport.update(
            queue,
            Resolution {
                width: width.max(1),
                height: height.max(1),
            },
        );
    }

    pub fn set_scale_factor(&mut self, scale_factor: f64, config: &Config) {
        let font_size = config.font_size * scale_factor as f32;
        let line_height = font_size * config.line_height;
        let metrics = {
            let mut shared = self.shared.borrow_mut();
            let family = shared.family.clone();
            measure_cell(&mut shared.font_system, &family, font_size, line_height)
        };
        self.font_size = font_size;
        self.metrics = metrics;
        self.pane_buffers.clear();
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene<'_>,
        theme: &Theme,
    ) {
        let Self {
            shared,
            format,
            viewport,
            pane_buffers,
            metrics,
            font_size,
        } = self;
        let mut shared_ref = shared.borrow_mut();
        let SharedText {
            font_system,
            swash_cache,
            cache,
            atlases,
            family,
            ui_family,
        } = &mut *shared_ref;

        pane_buffers.retain(|id, _| scene.panes.iter().any(|pane| pane.id == *id));

        for pane in &scene.panes {
            let needs_fill = pane.dirty || !pane_buffers.contains_key(&pane.id);
            if !needs_fill {
                continue;
            }

            let buffer = pane_buffers.entry(pane.id).or_insert_with(|| {
                let mut buffer = Buffer::new(font_system, Metrics::new(*font_size, metrics.height));
                buffer.set_wrap(Wrap::None);
                buffer
            });

            fill_pane_buffer(
                buffer,
                pane.frame,
                font_system,
                family,
                *font_size,
                *metrics,
                theme,
            );
        }

        let mut ghost_buffers = Vec::with_capacity(scene.panes.len());
        let mut ghost_layouts = Vec::with_capacity(scene.panes.len());
        for pane in &scene.panes {
            let Some(ghost) = &pane.ghost else {
                continue;
            };
            let left = pane.content.x + ghost.col as f32 * metrics.width;
            let top = pane.content.y + ghost.row as f32 * metrics.height;
            let width = pane.content.right() - left;
            if width < 1.0 || top + metrics.height > pane.content.bottom() + 0.5 {
                continue;
            }

            let mut buffer = Buffer::new(font_system, Metrics::new(*font_size, metrics.height));
            buffer.set_wrap(Wrap::None);
            buffer.set_size(Some(width), Some(metrics.height));
            let attrs = Attrs::new().family(Family::Name(family)).color(Color::rgb(
                ghost.color.r,
                ghost.color.g,
                ghost.color.b,
            ));
            buffer.set_text(ghost.text.as_str(), &attrs, Shaping::Basic, None);
            buffer.shape_until_scroll(font_system, false);
            ghost_buffers.push(buffer);
            ghost_layouts.push((left, top, width, ghost.color));
        }

        let mut label_buffers = Vec::with_capacity(scene.labels.len());
        for label in &scene.labels {
            label_buffers.push(build_label_buffer(font_system, label, ui_family));
        }

        let pane_areas = scene.panes.iter().filter_map(|pane| {
            let buffer = pane_buffers.get(&pane.id)?;
            Some(pane_text_area(buffer, pane.content, theme))
        });
        let ghost_areas =
            ghost_buffers
                .iter()
                .zip(&ghost_layouts)
                .map(|(buffer, (left, top, width, color))| TextArea {
                    buffer,
                    left: *left,
                    top: *top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: left.floor() as i32,
                        top: top.floor() as i32,
                        right: (*left + *width).ceil() as i32,
                        bottom: (*top + metrics.height).ceil() as i32,
                    },
                    default_color: Color::rgb(color.r, color.g, color.b),
                    custom_glyphs: &[],
                });
        let label_areas = scene
            .labels
            .iter()
            .zip(label_buffers.iter())
            .map(|(label, buffer)| label_text_area(buffer, label));

        let bundle = atlases.entry(*format).or_insert_with(|| {
            let mut atlas = TextAtlas::new(device, queue, cache, *format);
            let renderer =
                TextRenderer::new(&mut atlas, device, wgpu::MultisampleState::default(), None);
            TextAtlasBundle { atlas, renderer }
        });

        if let Err(error) = bundle.renderer.prepare(
            device,
            queue,
            font_system,
            &mut bundle.atlas,
            viewport,
            pane_areas.chain(ghost_areas).chain(label_areas),
            swash_cache,
        ) {
            log::error!("text prepare failed: {error}");
        }
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        let shared = self.shared.borrow();
        let Some(bundle) = shared.atlases.get(&self.format) else {
            return;
        };
        if let Err(error) = bundle.renderer.render(&bundle.atlas, &self.viewport, pass) {
            log::error!("text render failed: {error}");
        }
    }

    pub fn trim(&mut self) {
        let mut shared = self.shared.borrow_mut();
        if let Some(bundle) = shared.atlases.get_mut(&self.format) {
            bundle.atlas.trim();
        }
    }
}

fn fill_pane_buffer(
    buffer: &mut Buffer,
    frame: &Frame,
    font_system: &mut FontSystem,
    family: &str,
    font_size: f32,
    metrics: CellMetrics,
    theme: &Theme,
) {
    let text_width = (frame.cols as f32 * metrics.width).max(1.0);
    let text_height = (frame.rows.len().max(1) as f32 * metrics.height).max(1.0);

    buffer.set_metrics(Metrics::new(font_size, metrics.height));
    buffer.set_size(Some(text_width), Some(text_height));

    let default_attrs = Attrs::new().family(Family::Name(family)).color(Color::rgb(
        theme.foreground.r,
        theme.foreground.g,
        theme.foreground.b,
    ));

    let mut spans: Vec<(&str, Attrs)> = Vec::with_capacity(frame.rows.len() * 2 + 1);
    if frame.rows.is_empty() {
        spans.push((" ", default_attrs.clone()));
    }

    for (row_index, row) in frame.rows.iter().enumerate() {
        for run in &row.runs {
            let Some(text) = row.text.get(run.range.clone()) else {
                continue;
            };
            if text.is_empty() {
                continue;
            }
            let attrs = Attrs::new().family(Family::Name(family)).color(Color::rgb(
                run.color.r,
                run.color.g,
                run.color.b,
            ));
            spans.push((text, attrs));
        }

        if row_index + 1 < frame.rows.len() {
            spans.push(("\n", default_attrs.clone()));
        }
    }

    buffer.set_rich_text(spans, &default_attrs, Shaping::Basic, None);
    buffer.shape_until_scroll(font_system, false);
}

fn pane_text_area<'a>(buffer: &'a Buffer, content: Rect, theme: &Theme) -> TextArea<'a> {
    TextArea {
        buffer,
        left: content.x,
        top: content.y,
        scale: 1.0,
        bounds: TextBounds {
            left: content.x.floor() as i32,
            top: content.y.floor() as i32,
            right: content.right().ceil() as i32,
            bottom: content.bottom().ceil() as i32,
        },
        default_color: Color::rgb(theme.foreground.r, theme.foreground.g, theme.foreground.b),
        custom_glyphs: &[],
    }
}

fn build_label_buffer(font_system: &mut FontSystem, label: &Label, ui_family: &str) -> Buffer {
    let line_height = (label.font_size * 1.35).max(1.0);
    let mut buffer = Buffer::new(font_system, Metrics::new(label.font_size, line_height));
    buffer.set_wrap(Wrap::None);
    buffer.set_size(Some(label.width.max(1.0)), Some(line_height));

    let attrs = Attrs::new()
        .family(Family::Name(ui_family))
        .color(Color::rgb(label.color.r, label.color.g, label.color.b))
        .weight(if label.bold {
            Weight::BOLD
        } else {
            Weight::NORMAL
        });

    buffer.set_text(label.text.as_str(), &attrs, Shaping::Basic, None);
    buffer.shape_until_scroll(font_system, false);
    buffer
}

fn label_text_area<'a>(buffer: &'a Buffer, label: &Label) -> TextArea<'a> {
    let line_height = (label.font_size * 1.35).max(1.0);
    TextArea {
        buffer,
        left: label.x,
        top: label.y,
        scale: 1.0,
        bounds: TextBounds {
            left: label.x.floor() as i32,
            top: label.y.floor() as i32,
            right: (label.x + label.width).ceil() as i32,
            bottom: (label.y + line_height + 2.0).ceil() as i32,
        },
        default_color: Color::rgb(label.color.r, label.color.g, label.color.b),
        custom_glyphs: &[],
    }
}

fn choose_terminal_family(font_system: &mut FontSystem, preferred: Option<&str>) -> String {
    if let Some(preferred) = preferred {
        return preferred.to_owned();
    }

    // Ordered by platform preference; checking all of them is harmless.
    const CANDIDATES: &[&str] = &[
        "SF Mono",
        "Menlo",
        "Monaco",
        "Cascadia Mono",
        "Consolas",
        "JetBrains Mono",
        "Fira Code",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Ubuntu Mono",
        "Courier New",
    ];

    let database = font_system.db_mut();
    for candidate in CANDIDATES {
        if installed(database, candidate) {
            return (*candidate).to_owned();
        }
    }

    if let Some(face) = database.faces().find(|face| face.monospaced)
        && let Some((family, _)) = face.families.first()
    {
        return family.clone();
    }

    "monospace".to_owned()
}

fn choose_ui_family(font_system: &mut FontSystem, fallback: &str) -> String {
    const CANDIDATES: &[&str] = &[
        "SF Pro Text",
        "SF Pro",
        "Helvetica Neue",
        "Segoe UI",
        "Cantarell",
        "Ubuntu",
        "DejaVu Sans",
        "Arial",
    ];

    let database = font_system.db_mut();
    for candidate in CANDIDATES {
        if installed(database, candidate) {
            return (*candidate).to_owned();
        }
    }

    fallback.to_owned()
}

fn installed(database: &fontdb::Database, name: &str) -> bool {
    database.faces().any(|face| {
        face.families
            .iter()
            .any(|(family, _)| family.eq_ignore_ascii_case(name))
    })
}

fn measure_cell(
    font_system: &mut FontSystem,
    family: &str,
    font_size: f32,
    line_height: f32,
) -> CellMetrics {
    let mut probe = Buffer::new(font_system, Metrics::new(font_size, line_height));
    probe.set_wrap(Wrap::None);
    probe.set_size(Some(20_000.0), Some(line_height));

    let attrs = Attrs::new().family(Family::Name(family));
    probe.set_text("MMMMMMMMMMMMMMMMMMMM", &attrs, Shaping::Basic, None);
    probe.shape_until_scroll(font_system, false);

    let width = probe
        .layout_runs()
        .next()
        .map(|run| run.line_w / 20.0)
        .filter(|width| *width > 0.0)
        .unwrap_or(font_size * 0.6);

    CellMetrics {
        width,
        height: line_height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::scene::{PaneLayer, Scene};
    use crate::term::color::Theme;
    use crate::term::frame::{Frame, Row, Run};

    fn test_device() -> (wgpu::Device, wgpu::Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            force_fallback_adapter: false,
            compatible_surface: None,
            apply_limit_buckets: false,
        }))
        .expect("no GPU adapter available for the offscreen text test");
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("failed to create the test device")
    }

    fn test_frame(text: &str, theme: &Theme) -> Frame {
        Frame {
            rows: vec![Row {
                text: text.into(),
                runs: vec![Run {
                    range: 0..text.len(),
                    color: theme.foreground,
                }],
                backgrounds: vec![],
            }],
            cols: 10,
            cursor: None,
        }
    }

    #[derive(Clone, Copy)]
    struct Target {
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    }

    fn render_pipeline(
        pipeline: &mut TextPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scene: &Scene<'_>,
        theme: &Theme,
        target: Target,
    ) -> wgpu::Texture {
        let Target {
            width,
            height,
            format,
        } = target;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen frame"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        pipeline.update_viewport(queue, width, height);
        pipeline.prepare(device, queue, scene, theme);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("offscreen text pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pipeline.render(&mut pass);
        }
        queue.submit(Some(encoder.finish()));
        texture
    }

    fn read_texture(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
    ) -> Vec<u8> {
        let bytes_per_row = width * 4;
        let alignment = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = bytes_per_row.div_ceil(alignment) * alignment;
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("offscreen readback"),
            size: (padded_bytes_per_row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let slice = readback.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll failed");
        receiver
            .recv()
            .expect("map callback dropped")
            .expect("buffer map failed");

        let mapped = slice.get_mapped_range().expect("mapped range");
        let mut packed = Vec::with_capacity((bytes_per_row * height) as usize);
        for row in 0..height as usize {
            let start = row * padded_bytes_per_row as usize;
            packed.extend_from_slice(&mapped[start..start + bytes_per_row as usize]);
        }
        drop(mapped);
        drop(readback);
        packed
    }

    fn has_ink(pixels: &[u8]) -> bool {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| pixel[0] > 10 || pixel[1] > 10 || pixel[2] > 10)
    }

    /// Offscreen render of a pane and a UI label. Verifies that glyphon
    /// actually rasterizes glyphs with the same format the window uses.
    #[test]
    fn renders_glyphs_offscreen() {
        let (device, queue) = test_device();

        let format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let width = 460u32;
        let height = 120u32;
        let theme = Theme::tokyo_night();
        let mut pipeline =
            TextPipeline::new_isolated(&device, &queue, format, 1.0, &Config::default());

        let left = test_frame("hello", &theme);
        let right = test_frame("world", &theme);
        let scene = Scene {
            panes: vec![
                PaneLayer {
                    id: 1,
                    content: Rect::new(4.0, 4.0, 220.0, 50.0),
                    frame: &left,
                    dirty: true,
                    ghost: None,
                },
                PaneLayer {
                    id: 2,
                    content: Rect::new(234.0, 4.0, 220.0, 50.0),
                    frame: &right,
                    dirty: true,
                    ghost: None,
                },
            ],
            labels: vec![Label {
                text: "Terminals".into(),
                x: 4.0,
                y: 76.0,
                width: 200.0,
                font_size: 14.0,
                color: theme.sidebar_text,
                bold: true,
            }],
            quads: vec![],
        };

        let texture = render_pipeline(
            &mut pipeline,
            &device,
            &queue,
            &scene,
            &theme,
            Target {
                width,
                height,
                format,
            },
        );
        let data = read_texture(&device, &queue, &texture, width, height);

        let mut left_pixels = 0usize;
        let mut right_pixels = 0usize;
        let mut label_pixels = 0usize;
        for row in 0..height as usize {
            for column in 0..width as usize {
                let offset = (row * width as usize + column) * 4;
                if data[offset] > 10 || data[offset + 1] > 10 || data[offset + 2] > 10 {
                    if row > 70 {
                        label_pixels += 1;
                    } else if column < 230 {
                        left_pixels += 1;
                    } else {
                        right_pixels += 1;
                    }
                }
            }
        }

        assert!(
            left_pixels > 10,
            "expected rasterized glyphs in the left pane, found {left_pixels}"
        );
        assert!(
            right_pixels > 10,
            "expected rasterized glyphs in the right pane, found {right_pixels}"
        );
        assert!(
            label_pixels > 10,
            "expected rasterized UI label glyphs, found {label_pixels}"
        );
    }

    #[test]
    fn shared_text_atlas_survives_alternating_window_renders() {
        let (device, queue) = test_device();
        let config = Config::default();
        let shared = Rc::new(RefCell::new(SharedText::new(&device, &config)));

        let first_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let second_format = wgpu::TextureFormat::Bgra8UnormSrgb;
        let mut first =
            TextPipeline::new(&device, &queue, first_format, 1.0, &config, shared.clone());
        let mut second = TextPipeline::new(&device, &queue, second_format, 1.5, &config, shared);

        let theme = Theme::tokyo_night();
        let first_frame = test_frame("alpha", &theme);
        let second_frame = test_frame("beta", &theme);
        let first_scene = Scene {
            panes: vec![PaneLayer {
                id: 1,
                content: Rect::new(4.0, 4.0, 180.0, 60.0),
                frame: &first_frame,
                dirty: true,
                ghost: None,
            }],
            labels: vec![],
            quads: vec![],
        };
        let second_scene = Scene {
            panes: vec![PaneLayer {
                id: 2,
                content: Rect::new(4.0, 4.0, 180.0, 60.0),
                frame: &second_frame,
                dirty: true,
                ghost: None,
            }],
            labels: vec![],
            quads: vec![],
        };

        let first_texture = render_pipeline(
            &mut first,
            &device,
            &queue,
            &first_scene,
            &theme,
            Target {
                width: 220,
                height: 90,
                format: first_format,
            },
        );
        let second_texture = render_pipeline(
            &mut second,
            &device,
            &queue,
            &second_scene,
            &theme,
            Target {
                width: 260,
                height: 120,
                format: second_format,
            },
        );
        // Render the first window again after the second one used the same
        // shared atlas and vertex buffers.
        let first_again = render_pipeline(
            &mut first,
            &device,
            &queue,
            &first_scene,
            &theme,
            Target {
                width: 220,
                height: 90,
                format: first_format,
            },
        );

        assert!(has_ink(&read_texture(
            &device,
            &queue,
            &first_texture,
            220,
            90
        )));
        assert!(has_ink(&read_texture(
            &device,
            &queue,
            &second_texture,
            260,
            120
        )));
        assert!(has_ink(&read_texture(
            &device,
            &queue,
            &first_again,
            220,
            90
        )));
    }
}
