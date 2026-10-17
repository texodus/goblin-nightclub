//! WebGPU renderer. Each frame, for each layer of the level in view:
//! 1. draws its visible, remembered and unknown canvases offscreen;
//! 2. draws where it's void (the layer below shows through) into a mask;
//! 3. draws its visibility polygon into two masks: `now`, cleared every
//!    frame, and `seen`, never cleared, so it accumulates everything seen
//!    (for a layer below the viewer's, only where it shows through).
//!
//! Then it composites to the screen: from the viewer's layer down through
//! void, each layer a little smaller about the screen center (parallax),
//! styling each pixel by its layer's masks. Everything per layer is kept in
//! world space, so `seen` survives the viewer changing layers.

use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

use crate::draw::Vertex;
use crate::frame::Frame;
use crate::level::MAX_LAYERS;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// MSAA sample count for the visibility masks, which anti-aliases their
/// edges. 1 gives hard pixel edges.
const MASK_SAMPLES: u32 = 4;

/// Scale of each layer relative to the one above it.
const PARALLAX: f32 = 0.97;

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
    top: u32,
    /// Bit per layer drawn this frame.
    present: u32,
    scale: f32,
    _pad: f32,
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

/// A 2D array texture: one view of the whole array to read from, and one
/// view per layer to draw into.
struct ArrayTexture {
    whole: wgpu::TextureView,
    layers: Vec<wgpu::TextureView>,
}

impl ArrayTexture {
    fn new(
        device: &wgpu::Device,
        label: &str,
        size: [u32; 2],
        layers: u32,
        format: wgpu::TextureFormat,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let whole = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let layers = (0..layers)
            .map(|i| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: i,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        Self { whole, layers }
    }
}

/// A multisampled texture drawn into and resolved to one layer of a mask
/// array (or, with MSAA off, that layer drawn into directly).
struct Msaa {
    view: Option<wgpu::TextureView>,
}

impl Msaa {
    fn new(device: &wgpu::Device, label: &str, size: [u32; 2]) -> Self {
        let view = (MASK_SAMPLES > 1).then(|| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: MASK_SAMPLES,
                    dimension: wgpu::TextureDimension::D2,
                    format: MASK_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        });
        Self { view }
    }

    /// Color attachment drawing into `resolved`. With `clear` the mask is
    /// reset first; otherwise new triangles accumulate onto what's there.
    fn attachment<'a>(
        &'a self,
        resolved: &'a wgpu::TextureView,
        clear: bool,
    ) -> wgpu::RenderPassColorAttachment<'a> {
        let (view, resolve_target) = match &self.view {
            Some(msaa) => (msaa, Some(resolved)),
            None => (resolved, None),
        };
        // The MSAA samples only need to outlive the pass when accumulating.
        let store = if clear && self.view.is_some() {
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

/// A plain single-sample attachment, cleared first.
fn cleared(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    }
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    globals: wgpu::BindGroup,
    shape_pipeline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,
    mask_shaft_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    unmask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_bind_group: wgpu::BindGroup,
    composite_params: wgpu::Buffer,
    /// Per layer `a`: visible, remembered, unknown at `3a`, `3a + 1`,
    /// `3a + 2`.
    canvases: ArrayTexture,
    now: ArrayTexture,
    seen: ArrayTexture,
    voids: ArrayTexture,
    shafts: ArrayTexture,
    /// Per layer, reading its shaft mask in `fs_mask_shaft`.
    shaft_bind_groups: Vec<wgpu::BindGroup>,
    now_msaa: Vec<Msaa>,
    seen_msaa: Vec<Msaa>,
    canvas_vertices: GrowBuffer,
    mask_vertices: GrowBuffer,
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
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
fn texture_entry(
    binding: u32,
    dimension: wgpu::TextureViewDimension,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: false },
            view_dimension: dimension,
            multisampled: false,
        },
        count: None,
    }
}

/// Two triangles per rect `(pos, size)`.
fn rect_triangles(rects: &[([f32; 2], [f32; 2])]) -> impl Iterator<Item = Vertex> + '_ {
    rects.iter().flat_map(|&([x, y], [w, h])| {
        let (x1, y1) = (x + w, y + h);
        [[x, y], [x1, y], [x, y1], [x, y1], [x1, y], [x1, y1]].map(|pos| Vertex {
            pos,
            ..Default::default()
        })
    })
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
        let size = [width, height];

        // Screen size, shared by the shape and mask pipelines.
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
        let shaft_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shaft"),
            entries: &[texture_entry(0, wgpu::TextureViewDimension::D2)],
        });
        let globals_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("globals"),
                bind_group_layouts: &[Some(&globals_layout)],
                ..Default::default()
            });
        let shaft_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shaft"),
                bind_group_layouts: &[Some(&globals_layout), Some(&shaft_layout)],
                ..Default::default()
            });

        // Canvases and masks all draw colored triangles from the same
        // vertices; masks just ignore the color.
        let shapes = device.create_shader_module(wgpu::include_wgsl!("shaders/shapes.wgsl"));
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
        };
        let triangles =
            |label, layout: &wgpu::PipelineLayout, fs, target: wgpu::ColorTargetState, samples| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: &shapes,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_layout.clone())],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shapes,
                        entry_point: Some(fs),
                        compilation_options: Default::default(),
                        targets: &[Some(target)],
                    }),
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState {
                        count: samples,
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                })
            };
        let shape_pipeline = triangles(
            "shapes",
            &globals_pipeline_layout,
            "fs_color",
            wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
            1,
        );
        let mask_pipeline = triangles(
            "mask",
            &globals_pipeline_layout,
            "fs_mask",
            MASK_FORMAT.into(),
            MASK_SAMPLES,
        );
        let mask_shaft_pipeline = triangles(
            "mask in shaft",
            &shaft_pipeline_layout,
            "fs_mask_shaft",
            MASK_FORMAT.into(),
            MASK_SAMPLES,
        );
        let fill_pipeline = triangles(
            "fill",
            &globals_pipeline_layout,
            "fs_mask",
            MASK_FORMAT.into(),
            1,
        );
        let unmask_pipeline = triangles(
            "unmask",
            &globals_pipeline_layout,
            "fs_unmask",
            MASK_FORMAT.into(),
            1,
        );

        let layers = MAX_LAYERS as u32;
        let canvases = ArrayTexture::new(&device, "canvases", size, 3 * layers, format);
        let now = ArrayTexture::new(&device, "mask now", size, layers, MASK_FORMAT);
        let seen = ArrayTexture::new(&device, "mask seen", size, layers, MASK_FORMAT);
        let voids = ArrayTexture::new(&device, "voids", size, layers, MASK_FORMAT);
        let shafts = ArrayTexture::new(&device, "shafts", size, layers, MASK_FORMAT);
        let shaft_bind_groups = shafts
            .layers
            .iter()
            .map(|view| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shaft"),
                    layout: &shaft_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    }],
                })
            })
            .collect();
        let now_msaa = (0..MAX_LAYERS)
            .map(|_| Msaa::new(&device, "now msaa", size))
            .collect();
        let seen_msaa = (0..MAX_LAYERS)
            .map(|_| Msaa::new(&device, "seen msaa", size))
            .collect();

        let composite_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite params"),
            size: size_of::<CompositeParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let array = wgpu::TextureViewDimension::D2Array;
        let composite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::FRAGMENT),
                texture_entry(1, array),
                texture_entry(2, array),
                texture_entry(3, array),
                texture_entry(4, array),
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
                texture(1, &canvases.whole),
                texture(2, &now.whole),
                texture(3, &seen.whole),
                texture(4, &voids.whole),
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

        let canvas_vertices =
            GrowBuffer::new(&device, "canvas vertices", wgpu::BufferUsages::VERTEX);
        let mask_vertices = GrowBuffer::new(&device, "mask vertices", wgpu::BufferUsages::VERTEX);
        Ok(Self {
            surface,
            device,
            queue,
            globals,
            shape_pipeline,
            mask_pipeline,
            mask_shaft_pipeline,
            fill_pipeline,
            unmask_pipeline,
            composite_pipeline,
            composite_bind_group,
            composite_params,
            canvases,
            now,
            seen,
            voids,
            shafts,
            shaft_bind_groups,
            now_msaa,
            seen_msaa,
            canvas_vertices,
            mask_vertices,
        })
    }

    pub fn draw(&mut self, frame: &Frame) {
        // All vertices go in two buffers; each pass draws its range.
        let mut canvas_vertices = Vec::new();
        let mut mask_vertices: Vec<Vertex> = Vec::new();
        let push = |into: &mut Vec<Vertex>, vertices: &mut dyn Iterator<Item = Vertex>| {
            let start = into.len() as u32;
            into.extend(vertices);
            start..into.len() as u32
        };
        struct Ranges {
            canvases: [std::ops::Range<u32>; 3],
            visible: std::ops::Range<u32>,
            voids: std::ops::Range<u32>,
            standing: std::ops::Range<u32>,
            shaft: Option<std::ops::Range<u32>>,
        }
        let ranges: Vec<Ranges> = frame
            .layers
            .iter()
            .map(|l| {
                let canvases = [&l.visible, &l.remembered, &l.unknown]
                    .map(|c| push(&mut canvas_vertices, &mut c.vertices.iter().copied()));
                // Visible area as a triangle list: the polygon fanning out
                // from the origin, plus each (convex) lit shape fanned from
                // its first point.
                let poly = &l.polygon;
                let fan = (0..poly.len())
                    .flat_map(|i| [frame.origin, poly[i], poly[(i + 1) % poly.len()]])
                    .chain(l.lit_shapes.iter().flat_map(|shape| {
                        (1..shape.len().saturating_sub(1))
                            .flat_map(move |i| [shape[0], shape[i], shape[i + 1]])
                    }))
                    .map(|pos| Vertex {
                        pos,
                        ..Default::default()
                    });
                let visible = push(&mut mask_vertices, &mut fan.into_iter());
                let voids = push(&mut mask_vertices, &mut rect_triangles(&l.voids));
                let standing = push(&mut mask_vertices, &mut rect_triangles(&l.standing));
                let shaft = l
                    .shaft
                    .as_ref()
                    .map(|rects| push(&mut mask_vertices, &mut rect_triangles(rects)));
                Ranges {
                    canvases,
                    visible,
                    voids,
                    standing,
                    shaft,
                }
            })
            .collect();
        self.canvas_vertices.write(
            &self.device,
            &self.queue,
            bytemuck::cast_slice(&canvas_vertices),
        );
        self.mask_vertices.write(
            &self.device,
            &self.queue,
            bytemuck::cast_slice(&mask_vertices),
        );

        let present = frame
            .layers
            .iter()
            .fold(0u32, |bits, l| bits | 1 << l.index);
        let params = CompositeParams {
            origin: frame.origin,
            radius: if frame.radius.is_finite() {
                frame.radius
            } else {
                0.0
            },
            time: frame.time,
            top: frame.top as u32,
            present,
            scale: PARALLAX,
            _pad: 0.0,
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

        // One pass drawing `range` of a vertex buffer with `pipeline`.
        let mut pass = |attachment: wgpu::RenderPassColorAttachment,
                        pipeline: &wgpu::RenderPipeline,
                        extra: Option<&wgpu::BindGroup>,
                        buffer: &wgpu::Buffer,
                        range: std::ops::Range<u32>| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("layer"),
                color_attachments: &[Some(attachment)],
                ..Default::default()
            });
            if !range.is_empty() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.globals, &[]);
                if let Some(group) = extra {
                    pass.set_bind_group(1, group, &[]);
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range, 0..1);
            }
        };

        for (l, r) in frame.layers.iter().zip(&ranges) {
            let a = l.index;
            for (i, range) in r.canvases.iter().enumerate() {
                let attachment = wgpu::RenderPassColorAttachment {
                    view: &self.canvases.layers[3 * a + i],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                };
                let buffer = &self.canvas_vertices.buffer;
                pass(
                    attachment,
                    &self.shape_pipeline,
                    None,
                    buffer,
                    range.clone(),
                );
            }
            let masks = &self.mask_vertices.buffer;
            pass(
                cleared(&self.voids.layers[a]),
                &self.fill_pipeline,
                None,
                masks,
                r.voids.clone(),
            );
            // Then clear it again under whatever stands on the layer.
            let keep = wgpu::RenderPassColorAttachment {
                view: &self.voids.layers[a],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            };
            pass(keep, &self.unmask_pipeline, None, masks, r.standing.clone());
            if let Some(shaft) = &r.shaft {
                pass(
                    cleared(&self.shafts.layers[a]),
                    &self.fill_pipeline,
                    None,
                    masks,
                    shaft.clone(),
                );
            }
            let now = self.now_msaa[a].attachment(&self.now.layers[a], true);
            pass(now, &self.mask_pipeline, None, masks, r.visible.clone());
            let seen = self.seen_msaa[a].attachment(&self.seen.layers[a], false);
            match &r.shaft {
                // Below the viewer: remembered as seen only where it shows
                // through.
                Some(_) => pass(
                    seen,
                    &self.mask_shaft_pipeline,
                    Some(&self.shaft_bind_groups[a]),
                    masks,
                    r.visible.clone(),
                ),
                None => pass(seen, &self.mask_pipeline, None, masks, r.visible.clone()),
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
