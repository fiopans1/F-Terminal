use std::mem::size_of;

use bytemuck::{Pod, Zeroable};

/// A screen-space rectangle. Coordinates are physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Quad {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Corner radius in physical pixels. `0.0` draws a sharp rectangle.
    pub radius: f32,
    /// Linear RGBA.
    pub color: [f32; 4],
}

impl Quad {
    pub fn new(x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> Self {
        Self {
            x,
            y,
            width,
            height,
            radius: 0.0,
            color,
        }
    }

    pub fn rounded(x: f32, y: f32, width: f32, height: f32, radius: f32, color: [f32; 4]) -> Self {
        Self {
            x,
            y,
            width,
            height,
            radius,
            color,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    position: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
    radius: f32,
    _padding: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    screen: [f32; 2],
    _padding: [f32; 2],
}

const CAPACITY: usize = 1024;

/// Instanced colored-rectangle renderer used for cell backgrounds, cursor,
/// selection and (later) the tab bar and split dividers.
pub struct QuadRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    instances: Vec<Instance>,
    vertex_buffer: wgpu::Buffer,
    capacity: usize,
    count: u32,
}

impl QuadRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        screen: [f32; 2],
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("quads.wgsl").into()),
        });

        let globals = Globals {
            screen,
            _padding: [0.0; 2],
        };
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("quad globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform, 0, bytemuck::bytes_of(&globals));

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("quad bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let attributes = wgpu::vertex_attr_array![
            0 => Float32x2,
            1 => Float32x2,
            2 => Float32x4,
            3 => Float32,
        ];
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Instance>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout)],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let vertex_buffer = create_instance_buffer(device, CAPACITY);

        Self {
            pipeline,
            bind_group,
            uniform,
            instances: Vec::with_capacity(CAPACITY),
            vertex_buffer,
            capacity: CAPACITY,
            count: 0,
        }
    }

    pub fn set_screen(&self, queue: &wgpu::Queue, width: f32, height: f32) {
        let globals = Globals {
            screen: [width, height],
            _padding: [0.0; 2],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&globals));
    }

    pub fn clear(&mut self) {
        self.instances.clear();
        self.count = 0;
    }

    pub fn push(&mut self, quad: Quad) {
        if quad.width <= 0.0 || quad.height <= 0.0 || quad.color[3] <= 0.0 {
            return;
        }
        self.instances.push(Instance {
            position: [quad.x, quad.y],
            size: [quad.width, quad.height],
            color: quad.color,
            radius: quad.radius,
            _padding: [0.0; 3],
        });
    }

    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        self.count = self.instances.len() as u32;
        if self.instances.is_empty() {
            return;
        }

        if self.instances.len() > self.capacity {
            let capacity = self.instances.len().next_power_of_two();
            self.vertex_buffer = create_instance_buffer(device, capacity);
            self.capacity = capacity;
        }

        queue.write_buffer(
            &self.vertex_buffer,
            0,
            bytemuck::cast_slice(&self.instances),
        );
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.count == 0 {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        pass.draw(0..6, 0..self.count);
    }
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("quad instances"),
        size: (capacity * size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
