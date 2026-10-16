//! WebGPU renderer. Each frame it:
//! 1. draws each visibility layer's sprites into its own offscreen texture;
//! 2. draws the visibility polygon into two masks: `now`, cleared every
//!    frame, and `seen`, never cleared, so it accumulates everything seen;
//! 3. composites the layers to the screen, styling each pixel by the masks.

use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

use crate::frame::Frame;
use crate::sprite::Sprite;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// MSAA sample count for the masks, which anti-aliases visibility edges.
/// 1 gives hard pixel edges.
const MASK_SAMPLES: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    screen: [f32; 2],
    _pad: [f32; 2],
}

/// Mirrors `Params` in composite.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct CompositeParams {
    origin: [f32; 2],
    /// 0 for unlimited.
    radius: f32,
    time: f32,
}

/// A vertex buffer that grows (to the next power of two) as needed.
struct GrowBuffer {
    buffer: wgpu::Buffer,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl GrowBuffer {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        let usage = usage | wgpu::BufferUsages::COPY_DST;
        let buffer = Self::alloc(device, label, usage, 4096);
        Self {
            buffer,
            label,
            usage,
        }
    }

    fn alloc(
        device: &wgpu::Device,
        label: &str,
        usage: wgpu::BufferUsages,
        size: u64,
    ) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    }

    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        let len = data.len() as u64;
        if len > self.buffer.size() {
            self.buffer = Self::alloc(device, self.label, self.usage, len.next_power_of_two());
        }
        queue.write_buffer(&self.buffer, 0, data);
    }
}

/// A single-sample mask texture (what the compositor reads), plus the
/// multisampled texture drawn into and resolved from when MSAA is on.
struct Mask {
    resolved: wgpu::TextureView,
    msaa: Option<wgpu::TextureView>,
}

impl Mask {
    fn new(device: &wgpu::Device, label: &str, size: wgpu::Extent3d) -> Self {
        let resolved = texture_view(
            device,
            label,
            size,
            MASK_FORMAT,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let msaa = (MASK_SAMPLES > 1).then(|| {
            texture_view(
                device,
                label,
                size,
                MASK_FORMAT,
                MASK_SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            )
        });
        Self { resolved, msaa }
    }

    /// Color attachment that draws into the mask. With `clear` the mask is
    /// reset first; otherwise new triangles accumulate onto what's there.
    fn attachment(&self, clear: bool) -> wgpu::RenderPassColorAttachment<'_> {
        let (view, resolve_target) = match &self.msaa {
            Some(msaa) => (msaa, Some(&self.resolved)),
            None => (&self.resolved, None),
        };
        // The MSAA samples only need to outlive the pass when accumulating.
        let store = if clear && self.msaa.is_some() {
            wgpu::StoreOp::Discard
        } else {
            wgpu::StoreOp::Store
        };
        wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: if clear {
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
                } else {
                    wgpu::LoadOp::Load
                },
                store,
            },
        }
    }
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    globals: wgpu::BindGroup,
    sprite_pipeline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_group: wgpu::BindGroup,
    composite_params: wgpu::Buffer,
    /// Offscreen targets for the visible, remembered and unknown layers.
    layers: [wgpu::TextureView; 3],
    mask_now: Mask,
    mask_seen: Mask,
    instances: GrowBuffer,
    fan: GrowBuffer,
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn texture_view(
    device: &wgpu::Device,
    label: &str,
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    sample_count: u32,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A texture read with `textureLoad` in a fragment shader.
fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

impl Renderer {
    pub async fn new(canvas: HtmlCanvasElement, width: u32, height: u32) -> Result<Self, JsError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
            .map_err(js_err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await
            .map_err(js_err)?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(js_err)?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| js_err("surface not supported by adapter"))?;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        surface.configure(&device, &config);
        let format = config.format;
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        // Screen size, shared by the sprite and mask pipelines.
        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals = Globals {
            screen: [width as f32, height as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&globals_buf, 0, bytemuck::bytes_of(&globals));
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buf.as_entire_binding(),
            }],
        });
        let globals_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("globals"),
                bind_group_layouts: &[Some(&globals_layout)],
                ..Default::default()
            });

        let sprite_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/sprite.wgsl"));
        let sprite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &sprite_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Sprite>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x4
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &sprite_shader,
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

        let mask_shader = device.create_shader_module(wgpu::include_wgsl!("shaders/mask.wgsl"));
        let mask_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mask"),
            layout: Some(&globals_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &mask_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<[f32; 2]>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &mask_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(MASK_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: MASK_SAMPLES,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });

        let layer_usage =
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let layers = ["visible layer", "remembered layer", "unknown layer"]
            .map(|label| texture_view(&device, label, size, format, 1, layer_usage));
        let mask_now = Mask::new(&device, "mask now", size);
        let mask_seen = Mask::new(&device, "mask seen", size);

        let composite_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite params"),
            size: size_of::<CompositeParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                texture_entry(5),
            ],
        });
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: composite_params.as_entire_binding(),
                },
                texture(1, &layers[0]),
                texture(2, &layers[1]),
                texture(3, &layers[2]),
                texture(4, &mask_now.resolved),
                texture(5, &mask_seen.resolved),
            ],
        });
        let composite_shader =
            device.create_shader_module(wgpu::include_wgsl!("shaders/composite.wgsl"));
        let composite_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("composite"),
                bind_group_layouts: &[Some(&composite_layout)],
                ..Default::default()
            });
        let composite_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite"),
            layout: Some(&composite_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &composite_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &composite_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        let instances = GrowBuffer::new(&device, "sprite instances", wgpu::BufferUsages::VERTEX);
        let fan = GrowBuffer::new(&device, "visibility fan", wgpu::BufferUsages::VERTEX);
        Ok(Self {
            surface,
            device,
            queue,
            globals,
            sprite_pipeline,
            mask_pipeline,
            composite_pipeline,
            composite_bind_group,
            composite_params,
            layers,
            mask_now,
            mask_seen,
            instances,
            fan,
        })
    }

    pub fn draw(&mut self, frame: &Frame) {
        // All layers' sprites share one buffer; each layer draws its range.
        let mut sprites = Vec::new();
        let ranges = [&frame.visible, &frame.remembered, &frame.unknown].map(|layer| {
            let start = sprites.len() as u32;
            sprites.extend_from_slice(layer);
            start..sprites.len() as u32
        });
        self.instances
            .write(&self.device, &self.queue, bytemuck::cast_slice(&sprites));

        // Visible area as a triangle list: the polygon fanning out from the
        // origin, plus two triangles per lit rect.
        let poly = &frame.polygon;
        let mut fan: Vec<[f32; 2]> = (0..poly.len())
            .flat_map(|i| [frame.origin, poly[i], poly[(i + 1) % poly.len()]])
            .collect();
        for &([x, y], [w, h]) in &frame.lit_rects {
            let (x1, y1) = (x + w, y + h);
            fan.extend([[x, y], [x1, y], [x, y1], [x, y1], [x1, y], [x1, y1]]);
        }
        self.fan
            .write(&self.device, &self.queue, bytemuck::cast_slice(&fan));

        let params = CompositeParams {
            origin: frame.origin,
            radius: if frame.radius.is_finite() {
                frame.radius
            } else {
                0.0
            },
            time: frame.time,
        };
        self.queue
            .write_buffer(&self.composite_params, 0, bytemuck::bytes_of(&params));

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => return,
        };
        let screen = output.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());

        for (view, range) in self.layers.iter().zip(ranges) {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if !range.is_empty() {
                pass.set_pipeline(&self.sprite_pipeline);
                pass.set_bind_group(0, &self.globals, &[]);
                pass.set_vertex_buffer(0, self.instances.buffer.slice(..));
                pass.draw(0..6, range);
            }
        }

        for (mask, clear) in [(&self.mask_now, true), (&self.mask_seen, false)] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("mask"),
                color_attachments: &[Some(mask.attachment(clear))],
                ..Default::default()
            });
            if !fan.is_empty() {
                pass.set_pipeline(&self.mask_pipeline);
                pass.set_bind_group(0, &self.globals, &[]);
                pass.set_vertex_buffer(0, self.fan.buffer.slice(..));
                pass.draw(0..fan.len() as u32, 0..1);
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &screen,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &self.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }
}
