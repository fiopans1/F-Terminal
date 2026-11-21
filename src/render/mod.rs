pub mod quads;
pub mod scene;
pub mod text;

use std::sync::Arc;

use anyhow::{Context, Result};
use winit::dpi::PhysicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::config::Config;
use crate::render::scene::Scene;
use crate::term::GridSize;
use crate::term::color::{Theme, to_linear};

pub use text::CellMetrics;

/// Compute the grid that fits in a content area.
pub fn grid_size_for(width: f32, height: f32, metrics: CellMetrics) -> GridSize {
    GridSize {
        cols: (width.max(1.0) / metrics.width).floor().max(2.0) as usize,
        lines: (height.max(1.0) / metrics.height).floor().max(1.0) as usize,
    }
}

/// Owns the GPU surface and the two rendering pipelines.
pub struct Renderer {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    format: wgpu::TextureFormat,
    quads: quads::QuadRenderer,
    text: text::TextPipeline,
    window: Arc<Window>,
    scale_factor: f64,
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        event_loop: &ActiveEventLoop,
        config: &Config,
    ) -> Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let scale_factor = window.scale_factor();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
            Box::new(event_loop.owned_display_handle()),
        ));

        let surface = instance
            .create_surface(window.clone())
            .context("failed to create the wgpu surface")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
                apply_limit_buckets: false,
            })
            .await
            .context("no compatible GPU adapter found")?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .context("failed to create the wgpu device")?;

        let capabilities = surface.get_capabilities(&adapter);
        let format = capabilities
            .formats
            .iter()
            .copied()
            .find(|format| {
                matches!(
                    format,
                    wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
                )
            })
            .or_else(|| capabilities.formats.first().copied())
            .context("surface reports no supported formats")?;

        let present_mode = if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Fifo)
        {
            wgpu::PresentMode::Fifo
        } else {
            capabilities.present_modes[0]
        };

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        surface.configure(&device, &surface_config);

        let quads =
            quads::QuadRenderer::new(&device, &queue, format, [width as f32, height as f32]);
        let text = text::TextPipeline::new(&device, &queue, format, scale_factor, config);

        Ok(Self {
            instance,
            surface,
            device,
            queue,
            config: surface_config,
            format,
            quads,
            text,
            window,
            scale_factor,
        })
    }

    pub fn cell_metrics(&self) -> CellMetrics {
        self.text.cell_metrics()
    }

    /// Measure a short UI string in physical pixels.
    pub fn measure_ui_text(&mut self, text: &str, font_size: f32, bold: bool) -> f32 {
        self.text.measure_ui_text(text, font_size, bold)
    }

    pub fn scale_factor(&self) -> f64 {
        self.scale_factor
    }

    /// Convert a logical size to physical pixels.
    pub fn physical(&self, logical: f32) -> f32 {
        (logical * self.scale_factor as f32).round()
    }

    pub fn set_scale_factor(&mut self, scale_factor: f64, config: &Config) {
        if (self.scale_factor - scale_factor).abs() < f64::EPSILON {
            return;
        }
        self.scale_factor = scale_factor;
        self.text =
            text::TextPipeline::new(&self.device, &self.queue, self.format, scale_factor, config);
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        let width = size.width.max(1);
        let height = size.height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.quads
            .set_screen(&self.queue, width as f32, height as f32);
    }

    pub fn render(&mut self, scene: &Scene<'_>, theme: &Theme) -> Result<()> {
        let width = self.config.width;
        let height = self.config.height;

        self.text.update_viewport(&self.queue, width, height);
        self.text.prepare(&self.device, &self.queue, scene, theme);

        self.quads.clear();
        for quad in &scene.quads {
            self.quads.push(*quad);
        }
        self.quads.prepare(&self.device, &self.queue);

        let frame_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .context("failed to recreate the lost surface")?;
                self.surface.configure(&self.device, &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface validation error");
            }
        };

        let view = frame_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });

        {
            let background = to_linear(theme.background, 1.0);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("terminal pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: background[0] as f64,
                            g: background[1] as f64,
                            b: background[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.quads.render(&mut pass);
            self.text.render(&mut pass);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame_texture);
        self.text.trim();

        Ok(())
    }
}
