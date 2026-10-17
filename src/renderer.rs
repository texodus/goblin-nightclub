//! WebGPU renderer. Each frame, for each layer of the level in view, drawn
//! through that layer's transform (the camera, plus parallax: each layer
//! below the viewer's a little smaller about the view's center):
//! 1. its visible, remembered and unknown canvases;
//! 2. where it's void (the layer below shows through), in a mask;
//! 3. its visibility polygon into `now`, cleared every frame; and in world
//!    space into `seen`, never cleared, so it accumulates everything seen
//!    (for a layer below the viewer's, only where it shows through).
//!
//! Then it composites to the screen, from the viewer's layer down through
//! void, styling each pixel by its layer's masks, and draws the overlay
//! (world space, on the viewer's layer) and the interface (view space) on
//! top. `seen` is kept in world space so it survives camera moves and the
//! viewer changing layers; it's sized to the level on the first frame.

use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

use crate::draw::Vertex;
use crate::frame::Frame;
use crate::level::MAX_LAYERS;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// MSAA sample count for the `now` masks, which anti-aliases their edges.
/// 1 gives hard pixel edges.
const MASK_SAMPLES: u32 = 4;

/// Scale of each layer relative to the one above it.
const PARALLAX: f32 = 0.97;

/// Mirrors `Globals` in shapes.wgsl: how world positions land on a target.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Transform {
    screen: [f32; 2],
    camera: [f32; 2],
    center: [f32; 2],
    scale: f32,
    _pad: f32,
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
    radius_step: f32,
    camera: [f32; 2],
    radius_min: f32,
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

/// A uniform buffer holding a `Transform`, and its bind group.
struct Uniform {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Uniform {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("transform"),
            size: size_of::<Transform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transform"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self { buffer, bind_group }
    }
}

/// What's sized to the level rather than the view.
struct World {
    size: [f32; 2],
    seen: ArrayTexture,
    shafts: ArrayTexture,
    /// Per layer, reading its shaft mask in `fs_mask_shaft`.
    shaft_bind_groups: Vec<wgpu::BindGroup>,
    composite_bind_group: wgpu::BindGroup,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shape_pipeline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,
    seen_pipeline: wgpu::RenderPipeline,
    seen_shaft_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,
    unmask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    shaft_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    composite_params: wgpu::Buffer,
    /// Per layer, onto the view; then onto the whole level, and the view
    /// with no camera (for the interface).
    layer_transforms: Vec<Uniform>,
    world_transform: Uniform,
    ui_transform: Uniform,
    /// Per layer `a`, in view space: visible, remembered, unknown at `3a`,
    /// `3a + 1`, `3a + 2`.
    canvases: ArrayTexture,
    now: ArrayTexture,
    voids: ArrayTexture,
    now_msaa: Vec<Msaa>,
    world: Option<World>,
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

        let transform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transform"),
            entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let shaft_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shaft"),
            entries: &[texture_entry(0, wgpu::TextureViewDimension::D2)],
        });
        let transform_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("transform"),
                bind_group_layouts: &[Some(&transform_layout)],
                ..Default::default()
            });
        let shaft_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shaft"),
                bind_group_layouts: &[Some(&transform_layout), Some(&shaft_layout)],
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
        let plain = &transform_pipeline_layout;
        let shape_pipeline = triangles(
            "shapes",
            plain,
            "fs_color",
            wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            },
            1,
        );
        let mask_pipeline = triangles("mask", plain, "fs_mask", MASK_FORMAT.into(), MASK_SAMPLES);
        let seen_pipeline = triangles("seen", plain, "fs_mask", MASK_FORMAT.into(), 1);
        let seen_shaft_pipeline = triangles(
            "seen in shaft",
            &shaft_pipeline_layout,
            "fs_mask_shaft",
            MASK_FORMAT.into(),
            1,
        );
        let fill_pipeline = triangles("fill", plain, "fs_mask", MASK_FORMAT.into(), 1);
        let unmask_pipeline = triangles("unmask", plain, "fs_unmask", MASK_FORMAT.into(), 1);

        let layers = MAX_LAYERS as u32;
        let canvases = ArrayTexture::new(&device, "canvases", size, 3 * layers, format);
        let now = ArrayTexture::new(&device, "mask now", size, layers, MASK_FORMAT);
        let voids = ArrayTexture::new(&device, "voids", size, layers, MASK_FORMAT);
        let now_msaa = (0..MAX_LAYERS)
            .map(|_| Msaa::new(&device, "now msaa", size))
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

        let layer_transforms = (0..MAX_LAYERS)
            .map(|_| Uniform::new(&device, &transform_layout))
            .collect();
        let world_transform = Uniform::new(&device, &transform_layout);
        let ui_transform = Uniform::new(&device, &transform_layout);
        let canvas_vertices =
            GrowBuffer::new(&device, "canvas vertices", wgpu::BufferUsages::VERTEX);
        let mask_vertices = GrowBuffer::new(&device, "mask vertices", wgpu::BufferUsages::VERTEX);
        Ok(Self {
            surface,
            device,
            queue,
            shape_pipeline,
            mask_pipeline,
            seen_pipeline,
            seen_shaft_pipeline,
            fill_pipeline,
            unmask_pipeline,
            composite_pipeline,
            shaft_layout,
            composite_layout,
            composite_params,
            layer_transforms,
            world_transform,
            ui_transform,
            canvases,
            now,
            voids,
            now_msaa,
            world: None,
            canvas_vertices,
            mask_vertices,
        })
    }

    /// (Re)create what's sized to the level, if its size changed.
    fn ensure_world(&mut self, size: [f32; 2]) {
        if self.world.as_ref().is_some_and(|w| w.size == size) {
            return;
        }
        let px = [size[0].ceil() as u32, size[1].ceil() as u32];
        let layers = MAX_LAYERS as u32;
        let seen = ArrayTexture::new(&self.device, "mask seen", px, layers, MASK_FORMAT);
        let shafts = ArrayTexture::new(&self.device, "shafts", px, layers, MASK_FORMAT);
        let shaft_bind_groups = shafts
            .layers
            .iter()
            .map(|view| {
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("shaft"),
                    layout: &self.shaft_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    }],
                })
            })
            .collect();
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        let composite_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.composite_params.as_entire_binding(),
                },
                texture(1, &self.canvases.whole),
                texture(2, &self.now.whole),
                texture(3, &seen.whole),
                texture(4, &self.voids.whole),
            ],
        });
        self.world = Some(World {
            size,
            seen,
            shafts,
            shaft_bind_groups,
            composite_bind_group,
        });
    }

    pub fn draw(&mut self, frame: &Frame) {
        self.ensure_world(frame.world);

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
        let overlay = push(
            &mut canvas_vertices,
            &mut frame.overlay.vertices.iter().copied(),
        );
        let ui = push(&mut canvas_vertices, &mut frame.ui.vertices.iter().copied());
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

        // Transforms: each layer onto the view, smaller the deeper it is;
        // the whole level onto world-sized targets; the interface as is.
        let center = [frame.view[0] / 2.0, frame.view[1] / 2.0];
        for l in &frame.layers {
            let t = Transform {
                screen: frame.view,
                camera: frame.camera,
                center,
                scale: PARALLAX.powi((frame.top - l.index) as i32),
                _pad: 0.0,
            };
            let buffer = &self.layer_transforms[l.index].buffer;
            self.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&t));
        }
        let flat = |screen| Transform {
            screen,
            camera: [0.0; 2],
            center: [0.0; 2],
            scale: 1.0,
            _pad: 0.0,
        };
        self.queue.write_buffer(
            &self.world_transform.buffer,
            0,
            bytemuck::bytes_of(&flat(frame.world)),
        );
        self.queue.write_buffer(
            &self.ui_transform.buffer,
            0,
            bytemuck::bytes_of(&flat(frame.view)),
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
            radius_step: frame.radius_step,
            camera: frame.camera,
            radius_min: frame.radius_min,
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
        let world = self.world.as_ref().unwrap();
        let mut encoder = self.device.create_command_encoder(&Default::default());

        // One pass drawing `range` of a vertex buffer with `pipeline`,
        // through `transform`.
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    attachment: wgpu::RenderPassColorAttachment,
                    pipeline: &wgpu::RenderPipeline,
                    transform: &wgpu::BindGroup,
                    extra: Option<&wgpu::BindGroup>,
                    buffer: &wgpu::Buffer,
                    range: std::ops::Range<u32>| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pass"),
                color_attachments: &[Some(attachment)],
                ..Default::default()
            });
            if !range.is_empty() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, transform, &[]);
                if let Some(group) = extra {
                    pass.set_bind_group(1, group, &[]);
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range, 0..1);
            }
        };
        let keep = |view| wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        };

        for (l, r) in frame.layers.iter().zip(&ranges) {
            let a = l.index;
            let transform = &self.layer_transforms[a].bind_group;
            let flat = &self.world_transform.bind_group;
            let (canvas, masks) = (&self.canvas_vertices.buffer, &self.mask_vertices.buffer);
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
                pass(
                    &mut encoder,
                    attachment,
                    &self.shape_pipeline,
                    transform,
                    None,
                    canvas,
                    range.clone(),
                );
            }
            let voids = &self.voids.layers[a];
            pass(
                &mut encoder,
                cleared(voids),
                &self.fill_pipeline,
                transform,
                None,
                masks,
                r.voids.clone(),
            );
            // Then clear it again under whatever stands on the layer.
            pass(
                &mut encoder,
                keep(voids),
                &self.unmask_pipeline,
                transform,
                None,
                masks,
                r.standing.clone(),
            );
            let now = self.now_msaa[a].attachment(&self.now.layers[a], true);
            pass(
                &mut encoder,
                now,
                &self.mask_pipeline,
                transform,
                None,
                masks,
                r.visible.clone(),
            );
            let seen = keep(&world.seen.layers[a]);
            match &r.shaft {
                // Below the viewer: remembered as seen only where it shows
                // through.
                Some(shaft) => {
                    let shafts = &world.shafts.layers[a];
                    pass(
                        &mut encoder,
                        cleared(shafts),
                        &self.fill_pipeline,
                        flat,
                        None,
                        masks,
                        shaft.clone(),
                    );
                    let group = Some(&world.shaft_bind_groups[a]);
                    pass(
                        &mut encoder,
                        seen,
                        &self.seen_shaft_pipeline,
                        flat,
                        group,
                        masks,
                        r.visible.clone(),
                    );
                }
                None => pass(
                    &mut encoder,
                    seen,
                    &self.seen_pipeline,
                    flat,
                    None,
                    masks,
                    r.visible.clone(),
                ),
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
            pass.set_bind_group(0, &world.composite_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        // Overlay on the viewer's layer, then the interface, over it all.
        let canvas = &self.canvas_vertices.buffer;
        let top = &self.layer_transforms[frame.top].bind_group;
        pass(
            &mut encoder,
            keep(&screen),
            &self.shape_pipeline,
            top,
            None,
            canvas,
            overlay,
        );
        let ui_group = &self.ui_transform.bind_group;
        pass(
            &mut encoder,
            keep(&screen),
            &self.shape_pipeline,
            ui_group,
            None,
            canvas,
            ui,
        );

        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }
}
