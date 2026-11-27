use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use glyphon::{
    Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache,
    TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight, Wrap, fontdb,
};

use crate::config::Config;
#[cfg(test)]
use crate::layout::Rect;
use crate::render::font_cache;
use crate::render::scene::{Label, Scene};
use crate::term::color::Theme;
use crate::term::frame::Row;

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
    /// Load system fonts and pick the terminal and UI families.
    ///
    /// A persistent metadata index avoids re-parsing every installed font on
    /// warm starts. Any cache problem falls back to `FontSystem::new`, which
    /// scans the system and is one of the slowest parts of startup, so callers
    /// should run this concurrently with GPU setup.
    pub(crate) fn load(preferred: Option<&str>) -> Self {
        let mut font_system = match font_cache::load() {
            Some(font_system) => font_system,
            None => {
                let started = Instant::now();
                let font_system = FontSystem::new();
                log::info!(
                    "font index cache miss: scanned {} faces in {:?}",
                    font_system.db().len(),
                    started.elapsed()
                );
                font_cache::store(font_system.db());
                font_system
            }
        };
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MeasureKey {
    text: String,
    font_bits: u32,
    bold: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct LabelKey {
    text: String,
    font_bits: u32,
    width_bits: u32,
    color: u32,
    bold: bool,
}

/// Glyphon based text renderer. Terminal panes each keep a shaped buffer so
/// their text can be reused between frames. Chrome labels are cached by
/// content and style because they repeat across frames.
struct PaneTextBuffers {
    rows: Vec<Buffer>,
    cols: usize,
    width_bits: u32,
}

pub struct TextPipeline {
    shared: Rc<RefCell<SharedText>>,
    format: wgpu::TextureFormat,
    viewport: Viewport,
    pane_buffers: HashMap<u64, PaneTextBuffers>,
    measure_cache: HashMap<MeasureKey, f32>,
    label_cache: HashMap<LabelKey, usize>,
    label_buffers: Vec<Buffer>,
    metrics: CellMetrics,
    font_size: f32,
}

const MEASURE_CACHE_LIMIT: usize = 512;
const LABEL_CACHE_LIMIT: usize = 512;

/// Basic shaping is cheaper and keeps the terminal grid aligned, but it
/// deliberately skips font fallback. Any non-ASCII row may need a symbol,
/// CJK or emoji face, so those rows (and labels) use advanced shaping.
fn shaping_for(text: &str) -> Shaping {
    if text.is_ascii() {
        Shaping::Basic
    } else {
        Shaping::Advanced
    }
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
            measure_cache: HashMap::new(),
            label_cache: HashMap::new(),
            label_buffers: Vec::new(),
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
        let key = MeasureKey {
            text: text.to_owned(),
            font_bits: font_size.to_bits(),
            bold,
        };
        if let Some(width) = self.measure_cache.get(&key) {
            return *width;
        }

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
        buffer.set_text(text, &attrs, shaping_for(text), None);
        buffer.shape_until_scroll(font_system, false);

        let width = buffer
            .layout_runs()
            .map(|run| run.line_w)
            .fold(0.0, f32::max);
        if self.measure_cache.len() >= MEASURE_CACHE_LIMIT {
            self.measure_cache.clear();
        }
        self.measure_cache.insert(key, width);
        width
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
        self.measure_cache.clear();
        self.label_cache.clear();
        self.label_buffers.clear();
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
            measure_cache: _,
            label_cache,
            label_buffers,
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
            let width_bits = pane.content.width.to_bits();
            let reset = pane_buffers.get(&pane.id).is_none_or(|buffers| {
                buffers.cols != pane.frame.cols
                    || buffers.rows.len() != pane.frame.rows.len()
                    || buffers.width_bits != width_bits
            });
            if reset {
                let rows = pane
                    .frame
                    .rows
                    .iter()
                    .map(|_| new_row_buffer(font_system, *font_size, metrics.height))
                    .collect();
                pane_buffers.insert(
                    pane.id,
                    PaneTextBuffers {
                        rows,
                        cols: pane.frame.cols,
                        width_bits,
                    },
                );
            }

            if !pane.dirty && !reset {
                continue;
            }
            let Some(buffers) = pane_buffers.get_mut(&pane.id) else {
                continue;
            };
            let style = RowBufferStyle {
                family,
                font_size: *font_size,
                metrics: *metrics,
                width: pane.content.width,
                theme,
            };
            let dirty_rows = if reset {
                None
            } else {
                pane.dirty_rows.as_ref()
            };
            match dirty_rows {
                Some(rows) => {
                    for row in rows {
                        if let (Some(buffer), Some(source)) =
                            (buffers.rows.get_mut(*row), pane.frame.rows.get(*row))
                        {
                            fill_row_buffer(buffer, source, font_system, style);
                        }
                    }
                }
                None => {
                    for (buffer, source) in buffers.rows.iter_mut().zip(&pane.frame.rows) {
                        fill_row_buffer(buffer, source, font_system, style);
                    }
                }
            }
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
            buffer.set_text(ghost.text.as_str(), &attrs, shaping_for(&ghost.text), None);
            buffer.shape_until_scroll(font_system, false);
            ghost_buffers.push(buffer);
            ghost_layouts.push((left, top, width, ghost.color));
        }

        if label_cache.len() >= LABEL_CACHE_LIMIT {
            label_cache.clear();
            label_buffers.clear();
        }
        let mut label_indices = Vec::with_capacity(scene.labels.len());
        for label in &scene.labels {
            let color = ((label.color.r as u32) << 16)
                | ((label.color.g as u32) << 8)
                | label.color.b as u32;
            let key = LabelKey {
                text: label.text.clone(),
                font_bits: label.font_size.to_bits(),
                width_bits: label.width.to_bits(),
                color,
                bold: label.bold,
            };
            let index = if let Some(index) = label_cache.get(&key) {
                *index
            } else {
                let index = label_buffers.len();
                label_buffers.push(build_label_buffer(font_system, label, ui_family));
                label_cache.insert(key, index);
                index
            };
            label_indices.push(index);
        }

        let mut pane_areas = Vec::new();
        for pane in &scene.panes {
            let Some(buffers) = pane_buffers.get(&pane.id) else {
                continue;
            };
            for (row_index, buffer) in buffers.rows.iter().enumerate() {
                let top = pane.content.y + row_index as f32 * metrics.height;
                if top + metrics.height > pane.content.bottom() + 0.5 {
                    break;
                }
                pane_areas.push(TextArea {
                    buffer,
                    left: pane.content.x,
                    top,
                    scale: 1.0,
                    bounds: TextBounds {
                        left: pane.content.x.floor() as i32,
                        top: top.floor() as i32,
                        right: pane.content.right().ceil() as i32,
                        bottom: (top + metrics.height).ceil() as i32,
                    },
                    default_color: Color::rgb(
                        theme.foreground.r,
                        theme.foreground.g,
                        theme.foreground.b,
                    ),
                    custom_glyphs: &[],
                });
            }
        }
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
            .zip(label_indices.iter())
            .map(|(label, index)| {
                label_text_area(
                    label_buffers
                        .get(*index)
                        .expect("label cache index must be valid"),
                    label,
                )
            });

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
            pane_areas.into_iter().chain(ghost_areas).chain(label_areas),
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

fn new_row_buffer(font_system: &mut FontSystem, font_size: f32, line_height: f32) -> Buffer {
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));
    buffer.set_wrap(Wrap::None);
    buffer
}

#[derive(Clone, Copy)]
struct RowBufferStyle<'a> {
    family: &'a str,
    font_size: f32,
    metrics: CellMetrics,
    width: f32,
    theme: &'a Theme,
}

fn fill_row_buffer(
    buffer: &mut Buffer,
    row: &Row,
    font_system: &mut FontSystem,
    style: RowBufferStyle<'_>,
) {
    let RowBufferStyle {
        family,
        font_size,
        metrics,
        width,
        theme,
    } = style;
    buffer.set_metrics(Metrics::new(font_size, metrics.height));
    buffer.set_size(Some(width.max(1.0)), Some(metrics.height));

    let default_attrs = Attrs::new().family(Family::Name(family)).color(Color::rgb(
        theme.foreground.r,
        theme.foreground.g,
        theme.foreground.b,
    ));

    if row.text.is_empty() {
        buffer.set_text(" ", &default_attrs, Shaping::Basic, None);
        buffer.shape_until_scroll(font_system, false);
        return;
    }

    let mut spans: Vec<(&str, Attrs)> = Vec::with_capacity(row.runs.len().max(1));
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
    if spans.is_empty() {
        spans.push((row.text.as_str(), default_attrs.clone()));
    }
    buffer.set_rich_text(spans, &default_attrs, shaping_for(&row.text), None);
    buffer.shape_until_scroll(font_system, false);
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

    buffer.set_text(label.text.as_str(), &attrs, shaping_for(&label.text), None);
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
#[path = "../tests/render/text.rs"]
mod tests;
