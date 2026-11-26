mod font_cache;
pub mod quads;
pub mod scene;
pub mod text;

use std::cell::RefCell;
use std::rc::Rc;
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

/// Backends to try first on this platform.
///
/// On Windows, DX12 avoids initializing the Vulkan loader and enumerating its
/// adapters during startup. `WGPU_BACKEND` still wins when the user sets it.
fn configured_backends() -> (wgpu::Backends, bool) {
    if let Some(backends) = wgpu::Backends::from_env() {
        return (backends, true);
    }

    #[cfg(target_os = "windows")]
    let backends = wgpu::Backends::DX12;
    #[cfg(target_os = "macos")]
    let backends = wgpu::Backends::METAL;
    #[cfg(all(unix, not(target_os = "macos")))]
    let backends = wgpu::Backends::VULKAN | wgpu::Backends::GL;
    #[cfg(not(any(unix, windows)))]
    let backends = wgpu::Backends::all();

    (backends, false)
}

async fn create_gpu(
    window: Arc<Window>,
    event_loop: &ActiveEventLoop,
    backends: wgpu::Backends,
) -> Result<(wgpu::Surface<'static>, Arc<GpuContext>)> {
    let mut descriptor = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(
        event_loop.owned_display_handle(),
    ));
    descriptor.backends = backends;

    let instance = wgpu::Instance::new(descriptor);
    let surface = instance
        .create_surface(window)
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

    let gpu = Arc::new(GpuContext {
        instance,
        adapter,
        device: Arc::new(device),
        queue: Arc::new(queue),
    });

    Ok((surface, gpu))
}

/// Compute the grid that fits in a content area.
pub fn grid_size_for(width: f32, height: f32, metrics: CellMetrics) -> GridSize {
    GridSize {
        cols: (width.max(1.0) / metrics.width).floor().max(2.0) as usize,
        lines: (height.max(1.0) / metrics.height).floor().max(1.0) as usize,
    }
}

/// GPU objects that can be shared by every window.
pub struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

/// Handles shared by secondary windows when `render.mode = "shared"`.
#[derive(Clone)]
pub struct SharedRenderResources {
    pub(crate) gpu: Arc<GpuContext>,
    pub(crate) text: Rc<RefCell<text::SharedText>>,
}

/// Owns the GPU surface and the two rendering pipelines.
pub struct Renderer {
    gpu: Arc<GpuContext>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    quads: quads::QuadRenderer,
    text: text::TextPipeline,
    window: Arc<Window>,
    scale_factor: f64,
    trim_atlas: bool,
}

/// Start loading fonts on a worker thread. Callers can do this before
/// creating the window so the system font scan overlaps window creation too.
pub(crate) fn start_font_worker(
    preferred: Option<String>,
) -> std::io::Result<std::thread::JoinHandle<text::FontResources>> {
    std::thread::Builder::new()
        .name("font-load".into())
        .spawn(move || text::FontResources::load(preferred.as_deref()))
}

impl Renderer {
    pub async fn new(
        window: Arc<Window>,
        event_loop: &ActiveEventLoop,
        config: &Config,
        shared: Option<&SharedRenderResources>,
    ) -> Result<Self> {
        Self::new_with_font_worker(window, event_loop, config, shared, None).await
    }

    pub(crate) async fn new_with_font_worker(
        window: Arc<Window>,
        event_loop: &ActiveEventLoop,
        config: &Config,
        shared: Option<&SharedRenderResources>,
        preloaded_font_worker: Option<std::thread::JoinHandle<text::FontResources>>,
    ) -> Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let scale_factor = window.scale_factor();

        // Scanning every system font takes hundreds of milliseconds. Run it
        // alongside GPU initialization instead of after it.
        let font_worker = if shared.is_none() {
            match preloaded_font_worker {
                Some(worker) => Some(worker),
                None => {
                    let preferred = config.font_family.clone();
                    match start_font_worker(preferred) {
                        Ok(worker) => Some(worker),
                        Err(error) => {
                            log::warn!("failed to start the font worker: {error}");
                            None
                        }
                    }
                }
            }
        } else {
            None
        };

        let (surface, gpu) = if let Some(shared) = shared {
            let surface = shared
                .gpu
                .instance
                .create_surface(window.clone())
                .context("failed to create the wgpu surface")?;
            (surface, shared.gpu.clone())
        } else {
            let (backends, explicit) = configured_backends();
            match create_gpu(window.clone(), event_loop, backends).await {
                Ok(pair) => pair,
                Err(error) if !explicit && backends != wgpu::Backends::all() => {
                    log::warn!(
                        "GPU backend {backends:?} is unavailable; retrying with all backends: {error:#}"
                    );
                    create_gpu(window.clone(), event_loop, wgpu::Backends::all()).await?
                }
                Err(error) => return Err(error),
            }
        };

        let capabilities = surface.get_capabilities(&gpu.adapter);
        let adapter_info = gpu.adapter.get_info();
        log::debug!("GPU: {} ({:?})", adapter_info.name, adapter_info.backend);
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
        surface.configure(gpu.device.as_ref(), &surface_config);

        let quads = quads::QuadRenderer::new(
            gpu.device.as_ref(),
            gpu.queue.as_ref(),
            format,
            [width as f32, height as f32],
        );

        let preloaded_fonts = font_worker.and_then(|worker| match worker.join() {
            Ok(fonts) => Some(fonts),
            Err(_) => {
                log::warn!("the font worker panicked; loading fonts on the UI thread");
                None
            }
        });
        let shared_text = if let Some(shared) = shared {
            shared.text.clone()
        } else {
            let fonts = preloaded_fonts
                .unwrap_or_else(|| text::FontResources::load(config.font_family.as_deref()));
            Rc::new(RefCell::new(text::SharedText::new_with_fonts(
                gpu.device.as_ref(),
                fonts,
            )))
        };
        let text = text::TextPipeline::new(
            gpu.device.as_ref(),
            gpu.queue.as_ref(),
            format,
            scale_factor,
            config,
            shared_text,
        );

        Ok(Self {
            gpu,
            surface,
            config: surface_config,
            quads,
            text,
            window,
            scale_factor,
            // Only the window that owns the shared atlas trims it. Secondary
            // windows would otherwise clear the in-use set on every frame.
            trim_atlas: shared.is_none(),
        })
    }

    pub fn shared_resources(&self) -> SharedRenderResources {
        SharedRenderResources {
            gpu: self.gpu.clone(),
            text: self.text.shared_text(),
        }
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
        self.text.set_scale_factor(scale_factor, config);
    }

    /// Recompute the terminal cell metrics after `font_size` changes.
    pub fn refresh_font_metrics(&mut self, config: &Config) {
        self.text.set_scale_factor(self.scale_factor, config);
    }

    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        let width = size.width.max(1);
        let height = size.height.max(1);
        if self.config.width == width && self.config.height == height {
            return;
        }

        self.config.width = width;
        self.config.height = height;
        self.surface
            .configure(self.gpu.device.as_ref(), &self.config);
        self.quads
            .set_screen(self.gpu.queue.as_ref(), width as f32, height as f32);
    }

    pub fn render(&mut self, scene: &Scene<'_>, theme: &Theme) -> Result<()> {
        let width = self.config.width;
        let height = self.config.height;

        let frame_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => frame,
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout => {
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                // The next visible/focus event will ask for a redraw. Do not
                // build and shape a scene that cannot be presented.
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface
                    .configure(self.gpu.device.as_ref(), &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .gpu
                    .instance
                    .create_surface(self.window.clone())
                    .context("failed to recreate the lost surface")?;
                self.surface
                    .configure(self.gpu.device.as_ref(), &self.config);
                self.window.request_redraw();
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface validation error");
            }
        };

        // Acquire the surface first: when it is occluded or the compositor is
        // not ready, avoid rebuilding/shaping the whole scene for nothing.
        self.text
            .update_viewport(self.gpu.queue.as_ref(), width, height);
        self.text.prepare(
            self.gpu.device.as_ref(),
            self.gpu.queue.as_ref(),
            scene,
            theme,
        );

        self.quads.clear();
        for quad in &scene.quads {
            self.quads.push(*quad);
        }
        self.quads
            .prepare(self.gpu.device.as_ref(), self.gpu.queue.as_ref());

        let view = frame_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .gpu
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

        self.gpu.queue.submit(Some(encoder.finish()));
        self.gpu.queue.present(frame_texture);
        if self.trim_atlas {
            self.text.trim();
        }

        Ok(())
    }
}
