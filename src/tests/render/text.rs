use super::*;
use crate::render::scene::{PaneLayer, Scene};
use crate::term::color::Theme;
use crate::term::frame::{Frame, Row, Run};

fn has_emoji_font(font_system: &FontSystem) -> bool {
    font_system.db().faces().any(|face| {
        face.post_script_name.to_ascii_lowercase().contains("emoji")
            || face
                .families
                .iter()
                .any(|(name, _)| name.to_ascii_lowercase().contains("emoji"))
    })
}

#[test]
fn non_ascii_text_uses_advanced_shaping() {
    assert_eq!(shaping_for("plain ascii"), Shaping::Basic);
    assert_eq!(shaping_for("prompt$ 🤖"), Shaping::Advanced);
}

#[test]
fn advanced_shaping_finds_an_emoji_fallback() {
    let mut font_system = FontSystem::new();
    if !has_emoji_font(&font_system) {
        return;
    }

    let family = choose_terminal_family(&mut font_system, None);
    let cell = measure_cell(&mut font_system, &family, 14.0, 18.2);
    let mut buffer = Buffer::new(&mut font_system, Metrics::new(14.0, 18.2));
    buffer.set_wrap(Wrap::None);
    buffer.set_size(Some(400.0), Some(18.2));

    let text = "a🤖b";
    let attrs = Attrs::new().family(Family::Name(&family));
    buffer.set_text(text, &attrs, shaping_for(text), None);
    buffer.shape_until_scroll(&mut font_system, false);

    let emoji_start = 'a'.len_utf8();
    let run = buffer.layout_runs().next().expect("shaped run");
    let glyph = run
        .glyphs
        .iter()
        .find(|glyph| glyph.start <= emoji_start && glyph.end > emoji_start)
        .expect("emoji glyph");
    let face = font_system.db().face(glyph.font_id).expect("font face");
    let from_emoji_font = face.post_script_name.to_ascii_lowercase().contains("emoji")
        || face
            .families
            .iter()
            .any(|(name, _)| name.to_ascii_lowercase().contains("emoji"));

    assert!(
        from_emoji_font,
        "emoji was shaped with {}",
        face.post_script_name
    );
    assert!(
        glyph.w >= cell.width * 1.5,
        "emoji advance {} is too narrow for a two-cell glyph (cell {})",
        glyph.w,
        cell.width
    );
}

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
        background: theme.background,
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

fn has_color(pixels: &[u8]) -> bool {
    pixels.as_chunks::<4>().0.iter().any(|pixel| {
        let max = pixel[0].max(pixel[1]).max(pixel[2]);
        let min = pixel[0].min(pixel[1]).min(pixel[2]);
        max > 30 && max - min > 25
    })
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
    let mut pipeline = TextPipeline::new_isolated(&device, &queue, format, 1.0, &Config::default());

    let left = test_frame("hello", &theme);
    let right = test_frame("world", &theme);
    let scene = Scene {
        panes: vec![
            PaneLayer {
                id: 1,
                content: Rect::new(4.0, 4.0, 220.0, 50.0),
                frame: &left,
                dirty: true,
                dirty_rows: None,
                ghost: None,
            },
            PaneLayer {
                id: 2,
                content: Rect::new(234.0, 4.0, 220.0, 50.0),
                frame: &right,
                dirty: true,
                dirty_rows: None,
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
fn renders_color_emoji_with_fallback() {
    let (device, queue) = test_device();
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let theme = Theme::tokyo_night();
    let config = Config::default();
    let mut pipeline = TextPipeline::new_isolated(&device, &queue, format, 1.0, &config);

    {
        let shared = pipeline.shared.borrow();
        if !has_emoji_font(&shared.font_system) {
            return;
        }
    }

    let frame = test_frame("🤖", &theme);
    let scene = Scene {
        panes: vec![PaneLayer {
            id: 1,
            content: Rect::new(4.0, 4.0, 120.0, 40.0),
            frame: &frame,
            dirty: true,
            dirty_rows: None,
            ghost: None,
        }],
        labels: vec![],
        quads: vec![],
    };
    let width = 140;
    let height = 60;
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

    assert!(has_ink(&data), "expected rasterized emoji ink");
    assert!(
        has_color(&data),
        "expected the emoji fallback glyph to keep its color"
    );
}

#[test]
fn shared_text_atlas_survives_alternating_window_renders() {
    let (device, queue) = test_device();
    let config = Config::default();
    let shared = Rc::new(RefCell::new(SharedText::new(&device, &config)));

    let first_format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let second_format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let mut first = TextPipeline::new(&device, &queue, first_format, 1.0, &config, shared.clone());
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
            dirty_rows: None,
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
            dirty_rows: None,
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
