//! WebGPU renderer. Each frame, for each layer of the level in view, drawn
//! through that layer's transform (the camera, plus parallax: each layer
//! below the viewer's a little smaller about the view's center):
//! 1. its visible, remembered and unknown canvases;
//! 2. where it's void (the layer below shows through), in a mask;
//! 3. its visibility polygon into `now`, cleared every frame; and in world
//!    space into `seen`, never cleared, so it accumulates everything seen
//!    (for a layer below the viewer's, only where it shows through).
//!
//! What only changes with the level's tiles (each layer's tile art, voids,
//! liquid and shafts) comes as meshes, kept on the GPU while they last;
//! of those drawn through the camera, only the chunks in view are drawn.
//!
//! Then it composites to the screen, from the viewer's layer down through
//! void (and the mist hanging between the layers), styling each pixel by
//! its layer's masks, and draws the overlay (world space, on the viewer's
//! layer) and the interface (view space) on top. `seen` is kept in world space so it survives camera moves, zooming
//! and the viewer changing layers; it's sized to the level on the first
//! frame. Everything else is sized to the screen, at its own resolution,
//! and made again when that changes.

use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

use std::ops::Range;

use wgpu::util::DeviceExt;

use goblin_nightclub_core::draw::Vertex;
use goblin_nightclub_core::frame::Frame;
use goblin_nightclub_core::layer::TILE;
use goblin_nightclub_core::level::MAX_LAYERS;
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::tileset::Sheet;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// MSAA sample count for the `now` masks, which anti-aliases their edges.
/// 1 gives hard pixel edges.
const MASK_SAMPLES: u32 = 4;

/// Mirrors `Globals` in shapes.wgsl: how world positions land on a target.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Transform {
    screen: [f32; 2],
    camera: [f32; 2],
    center: [f32; 2],
    scale: f32,
    zoom: f32,
    cell: f32,
    _pad: [f32; 3],
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
    zoom: f32,
    seen_zoom: f32,
    /// 1 when the viewer is outdoors (light mist over lower layers, not
    /// dark).
    outdoors: f32,
    /// The ground level: the mist over layers below it is dark even
    /// outdoors.
    ground: u32,
    /// Layers below `ground` are seen this fraction as far.
    radius_underground: f32,
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

/// A tile sheet as a texture, for `textureLoad` in shapes.wgsl. Not sRGB,
/// so its colors reach the screen as is, like vertex colors.
fn upload_sheet(device: &wgpu::Device, queue: &wgpu::Queue, sheet: &Sheet) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width: sheet.size[0],
        height: sheet.size[1],
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tile sheet"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &sheet.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * sheet.size[0]),
            rows_per_image: Some(sheet.size[1]),
        },
        size,
    );
    texture.create_view(&Default::default())
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

/// A uniform buffer holding a `Transform`, and its bind group (which also
/// holds the tile sheet, so it's made again when that changes).
struct Uniform {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl Uniform {
    fn new(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sheet: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("transform"),
            size: size_of::<Transform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = Self::bind(device, layout, &buffer, sheet, sampler);
        Self { buffer, bind_group }
    }

    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        buffer: &wgpu::Buffer,
        sheet: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("transform"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(sheet),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// Point the bind group at another sheet.
    fn rebind(
        &mut self,
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sheet: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
    ) {
        self.bind_group = Self::bind(device, layout, &self.buffer, sheet, sampler);
    }
}

/// What's sized to the level rather than the screen, a layer each.
struct World {
    /// In pixels.
    size: [u32; 2],
    depth: usize,
    seen: ArrayTexture,
    shafts: ArrayTexture,
    /// Per layer, reading its shaft mask in `fs_mask_shaft`.
    shaft_bind_groups: Vec<wgpu::BindGroup>,
    /// Per layer, the shaft mesh its shaft mask was last filled from.
    shaft_filled: Vec<Option<u64>>,
}

/// A mesh kept on the GPU, and which mesh it is.
struct Uploaded {
    id: u64,
    /// `None` for a mesh with no triangles.
    buffer: Option<wgpu::Buffer>,
}

/// Which of a layer's meshes, in [`Renderer::meshes`].
const TILES: usize = 0;
const VOIDS: usize = 1;
const LIQUID: usize = 2;
const SHAFT: usize = 3;
const FRINGES: usize = 4;
const TRIMS: usize = 5;

/// What's sized to the screen, a layer each.
struct Screen {
    size: [u32; 2],
    depth: usize,
    /// Per layer `a`: visible, remembered, unknown at `3a`, `3a + 1`,
    /// `3a + 2`.
    canvases: ArrayTexture,
    now: ArrayTexture,
    voids: ArrayTexture,
    now_msaa: Vec<Msaa>,
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
    water_pipeline: wgpu::RenderPipeline,
    unmask_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    transform_layout: wgpu::BindGroupLayout,
    shaft_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    composite_params: wgpu::Buffer,
    /// The tile sheet (a white pixel until one is set), and how it's
    /// sampled.
    sheet: wgpu::TextureView,
    sampler: wgpu::Sampler,
    /// Per layer, onto the view; then onto the whole level, and the view
    /// with no camera (for the interface).
    layer_transforms: Vec<Uniform>,
    world_transform: Uniform,
    ui_transform: Uniform,
    config: wgpu::SurfaceConfiguration,
    screen: Option<Screen>,
    world: Option<World>,
    /// Reads `screen` and `world`; made again with either.
    composite_bind_group: Option<wgpu::BindGroup>,
    canvas_vertices: GrowBuffer,
    mask_vertices: GrowBuffer,
    /// What goes into those each frame, kept to reuse the memory.
    canvas_scratch: Vec<Vertex>,
    mask_scratch: Vec<Vertex>,
    /// Per layer: its tiles, voids, liquid, shaft, fringes and trims
    /// meshes, as last drawn.
    meshes: Vec<[Option<Uploaded>; 6]>,
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

        // The sheet is sampled filtered (see `sharp` in shapes.wgsl).
        let transform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("transform"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
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

        // Canvases and masks all draw triangles from the same vertices;
        // masks ignore the color, and mostly the sheet.
        let shapes = device.create_shader_module(wgpu::include_wgsl!("shaders/shapes.wgsl"));
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2,
                1 => Float32x2,
                2 => Float32x2,
                3 => Float32x4,
                4 => Float32x4,
                5 => Float32,
            ],
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
        let water_pipeline = triangles("water", plain, "fs_mask_half", MASK_FORMAT.into(), 1);
        let unmask_pipeline = triangles("unmask", plain, "fs_unmask", MASK_FORMAT.into(), 1);

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

        let sheet = upload_sheet(&device, &queue, &Sheet::white());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sheet"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = || Uniform::new(&device, &transform_layout, &sheet, &sampler);
        let layer_transforms = (0..MAX_LAYERS).map(|_| uniform()).collect();
        let world_transform = uniform();
        let ui_transform = uniform();
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
            water_pipeline,
            unmask_pipeline,
            composite_pipeline,
            transform_layout,
            shaft_layout,
            composite_layout,
            composite_params,
            sheet,
            sampler,
            layer_transforms,
            world_transform,
            ui_transform,
            config,
            screen: None,
            world: None,
            composite_bind_group: None,
            canvas_vertices,
            mask_vertices,
            canvas_scratch: Vec::new(),
            mask_scratch: Vec::new(),
            meshes: (0..MAX_LAYERS).map(|_| Default::default()).collect(),
        })
    }

    /// The canvas's size in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    /// Forget what's been seen, for a new level (the next frame makes it
    /// afresh).
    pub fn reset(&mut self) {
        self.world = None;
        self.composite_bind_group = None;
        self.meshes.fill_with(Default::default);
    }

    /// Draw from `sheet` from now on.
    pub fn set_sheet(&mut self, sheet: &Sheet) {
        self.sheet = upload_sheet(&self.device, &self.queue, sheet);
        let all = self
            .layer_transforms
            .iter_mut()
            .chain([&mut self.world_transform, &mut self.ui_transform]);
        for uniform in all {
            uniform.rebind(
                &self.device,
                &self.transform_layout,
                &self.sheet,
                &self.sampler,
            );
        }
    }

    /// The canvas is now `width` x `height` pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        if [width, height] == [self.config.width, self.config.height] || width * height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// (Re)create what's sized to the screen, for `depth` layers, if that
    /// changed.
    fn ensure_screen(&mut self, depth: usize) {
        let size = [self.config.width, self.config.height];
        if self
            .screen
            .as_ref()
            .is_some_and(|s| s.size == size && s.depth == depth)
        {
            return;
        }
        let device = &self.device;
        let layers = depth as u32;
        let format = self.config.format;
        self.screen = Some(Screen {
            size,
            depth,
            canvases: ArrayTexture::new(device, "canvases", size, 3 * layers, format),
            now: ArrayTexture::new(device, "mask now", size, layers, MASK_FORMAT),
            voids: ArrayTexture::new(device, "voids", size, layers, MASK_FORMAT),
            now_msaa: (0..depth)
                .map(|_| Msaa::new(device, "now msaa", size))
                .collect(),
        });
        self.composite_bind_group = None;
    }

    /// (Re)create what's sized to the level, `px` pixels and `depth`
    /// layers, if that changed.
    fn ensure_world(&mut self, px: [u32; 2], depth: usize) {
        if self
            .world
            .as_ref()
            .is_some_and(|w| w.size == px && w.depth == depth)
        {
            return;
        }
        let layers = depth as u32;
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
        self.world = Some(World {
            size: px,
            depth,
            seen,
            shafts,
            shaft_bind_groups,
            shaft_filled: vec![None; depth],
        });
        self.composite_bind_group = None;
    }

    fn ensure_composite(&mut self) {
        if self.composite_bind_group.is_some() {
            return;
        }
        let (screen, world) = (self.screen.as_ref().unwrap(), self.world.as_ref().unwrap());
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        self.composite_bind_group =
            Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("composite"),
                layout: &self.composite_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.composite_params.as_entire_binding(),
                    },
                    texture(1, &screen.canvases.whole),
                    texture(2, &screen.now.whole),
                    texture(3, &world.seen.whole),
                    texture(4, &screen.voids.whole),
                ],
            }));
    }

    /// Keep `mesh` on the GPU as layer `layer`'s mesh `which`, unless it
    /// already is.
    fn upload(&mut self, layer: usize, which: usize, mesh: &Mesh) {
        let kept = &mut self.meshes[layer][which];
        if kept.as_ref().is_some_and(|u| u.id == mesh.id()) {
            return;
        }
        let buffer = (!mesh.vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh"),
                    contents: bytemuck::cast_slice(&mesh.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        *kept = Some(Uploaded {
            id: mesh.id(),
            buffer,
        });
    }

    pub fn draw(&mut self, frame: &Frame) {
        // The camera on a whole pixel, so the tile art stays crisp. What's
        // sized to the level is at the art's own size, whatever the view's
        // zoom, so zooming keeps what's been seen.
        let zoom = frame.zoom;
        let cell = frame.cell as f32;
        let seen_zoom = cell / TILE;
        let world_px = frame.world.map(|w| (w * seen_zoom).ceil());
        self.ensure_world(world_px.map(|w| w as u32), frame.depth);
        self.ensure_screen(frame.depth);
        self.ensure_composite();
        let camera = frame.camera.map(|c| (c * zoom).round() / zoom);
        for l in &frame.layers {
            self.upload(l.index, TILES, &l.tiles);
            self.upload(l.index, FRINGES, &l.fringes);
            self.upload(l.index, TRIMS, &l.trims);
            self.upload(l.index, VOIDS, &l.voids);
            self.upload(l.index, LIQUID, &l.liquid);
            if let Some(shaft) = &l.shaft {
                self.upload(l.index, SHAFT, shaft);
            }
        }

        // Everything else goes in two buffers; each pass draws its range.
        let mut canvas_vertices = std::mem::take(&mut self.canvas_scratch);
        let mut mask_vertices = std::mem::take(&mut self.mask_scratch);
        canvas_vertices.clear();
        mask_vertices.clear();
        let push = |into: &mut Vec<Vertex>, vertices: &[Vertex]| {
            let start = into.len() as u32;
            into.extend_from_slice(vertices);
            start..into.len() as u32
        };
        struct Ranges {
            canvases: [Range<u32>; 3],
            visible: Range<u32>,
            standing: Range<u32>,
        }
        let ranges: Vec<Ranges> = frame
            .layers
            .iter()
            .map(|l| {
                let canvases = [&l.visible, &l.remembered, &l.unknown]
                    .map(|c| push(&mut canvas_vertices, &c.vertices));
                // Visible area as a triangle list: the polygon fanning out
                // from the origin, plus each (convex) lit shape fanned from
                // its first point.
                let poly = &l.polygon;
                let start = mask_vertices.len() as u32;
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
                mask_vertices.extend(fan);
                let visible = start..mask_vertices.len() as u32;
                let standing = push(&mut mask_vertices, &l.standing.vertices);
                Ranges {
                    canvases,
                    visible,
                    standing,
                }
            })
            .collect();
        let overlay = push(&mut canvas_vertices, &frame.overlay.vertices);
        let ui = push(&mut canvas_vertices, &frame.ui.vertices);
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
        self.canvas_scratch = canvas_vertices;
        self.mask_scratch = mask_vertices;

        // Transforms: each layer onto the view, smaller the deeper it is;
        // the whole level onto world-sized targets; the interface as is.
        let center = [frame.view[0] / 2.0, frame.view[1] / 2.0];
        let parallax = frame.parallax();
        for l in &frame.layers {
            let t = Transform {
                screen: frame.view,
                camera,
                center,
                scale: parallax.powi((frame.top - l.index) as i32),
                zoom,
                cell,
                _pad: [0.0; 3],
            };
            let buffer = &self.layer_transforms[l.index].buffer;
            self.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&t));
        }
        let flat = |screen, zoom| Transform {
            screen,
            camera: [0.0; 2],
            center: [0.0; 2],
            scale: 1.0,
            zoom,
            cell,
            _pad: [0.0; 3],
        };
        self.queue.write_buffer(
            &self.world_transform.buffer,
            0,
            bytemuck::bytes_of(&flat(world_px, seen_zoom)),
        );
        self.queue.write_buffer(
            &self.ui_transform.buffer,
            0,
            bytemuck::bytes_of(&flat(frame.view, frame.ui_scale)),
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
            scale: parallax,
            radius_step: frame.radius_step,
            camera,
            radius_min: frame.radius_min,
            zoom,
            seen_zoom,
            outdoors: if frame.outdoors { 1.0 } else { 0.0 },
            ground: frame.ground as u32,
            radius_underground: frame.radius_underground,
        };
        self.queue
            .write_buffer(&self.composite_params, 0, bytemuck::bytes_of(&params));

        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => return,
        };
        // A layer's shaft mask is filled again only when its shaft changes
        // (the viewer changed layers, or the level did).
        let world = self.world.as_mut().unwrap();
        let refill: Vec<bool> = frame
            .layers
            .iter()
            .map(|l| {
                let id = l.shaft.as_ref().map(|m| m.id());
                let fresh = id.is_some() && world.shaft_filled[l.index] != id;
                if fresh {
                    world.shaft_filled[l.index] = id;
                }
                fresh
            })
            .collect();

        let screen = output.texture.create_view(&Default::default());
        let world = self.world.as_ref().unwrap();
        let screen_textures = self.screen.as_ref().unwrap();
        let mut encoder = self.device.create_command_encoder(&Default::default());

        // One pass with `pipeline` through `transform`, drawing each range
        // of a vertex buffer in turn.
        type Draws<'a> = [(&'a wgpu::Buffer, Range<u32>)];
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    attachment: wgpu::RenderPassColorAttachment,
                    pipeline: &wgpu::RenderPipeline,
                    transform: &wgpu::BindGroup,
                    extra: Option<&wgpu::BindGroup>,
                    draws: &Draws| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pass"),
                color_attachments: &[Some(attachment)],
                ..Default::default()
            });
            let mut ready = false;
            for (buffer, range) in draws.iter().filter(|(_, r)| !r.is_empty()) {
                if !ready {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, transform, &[]);
                    if let Some(group) = extra {
                        pass.set_bind_group(1, group, &[]);
                    }
                    ready = true;
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range.clone(), 0..1);
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
        // Layer `a`'s mesh `which`: the chunks of it in `view` (`None` for
        // all of it).
        let mesh = |a: usize, which: usize, m: &Mesh, view: Option<_>| {
            let Some(Uploaded {
                buffer: Some(buffer),
                ..
            }) = &self.meshes[a][which]
            else {
                return Vec::new();
            };
            let ranges = view.map_or_else(|| vec![m.all()], |v| m.ranges(v));
            ranges.into_iter().map(|r| (buffer, r)).collect::<Vec<_>>()
        };

        for ((l, r), refill) in frame.layers.iter().zip(&ranges).zip(refill) {
            let a = l.index;
            let view = Some(frame.layer_view(a));
            let transform = &self.layer_transforms[a].bind_group;
            let flat = &self.world_transform.bind_group;
            let (canvas, masks) = (&self.canvas_vertices.buffer, &self.mask_vertices.buffer);
            // The tiles, then fringes and trims over them.
            let mut tiles = mesh(a, TILES, &l.tiles, view);
            let edging = [
                mesh(a, FRINGES, &l.fringes, view),
                mesh(a, TRIMS, &l.trims, view),
            ]
            .concat();
            tiles.extend(edging.iter().cloned());
            for (i, range) in r.canvases.iter().enumerate() {
                let attachment = wgpu::RenderPassColorAttachment {
                    view: &screen_textures.canvases.layers[3 * a + i],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                };
                // The tiles under the visible and remembered canvases.
                let under = if i < 2 { tiles.as_slice() } else { &[] };
                let mut draws = under.to_vec();
                draws.push((canvas, range.clone()));
                pass(
                    &mut encoder,
                    attachment,
                    &self.shape_pipeline,
                    transform,
                    None,
                    &draws,
                );
            }
            let voids = &screen_textures.voids.layers[a];
            pass(
                &mut encoder,
                cleared(voids),
                &self.fill_pipeline,
                transform,
                None,
                &mesh(a, VOIDS, &l.voids, view),
            );
            // Liquid is void that tints what shows through: mark it half.
            pass(
                &mut encoder,
                keep(voids),
                &self.water_pipeline,
                transform,
                None,
                &mesh(a, LIQUID, &l.liquid, view),
            );
            // Fringes and trims cover the void and water they reach over.
            pass(
                &mut encoder,
                keep(voids),
                &self.unmask_pipeline,
                transform,
                None,
                &edging,
            );
            // Then clear it again under whatever stands on the layer.
            pass(
                &mut encoder,
                keep(voids),
                &self.unmask_pipeline,
                transform,
                None,
                &[(masks, r.standing.clone())],
            );
            let now = screen_textures.now_msaa[a].attachment(&screen_textures.now.layers[a], true);
            pass(
                &mut encoder,
                now,
                &self.mask_pipeline,
                transform,
                None,
                &[(masks, r.visible.clone())],
            );
            let seen = keep(&world.seen.layers[a]);
            match &l.shaft {
                // Below the viewer: remembered as seen only where it shows
                // through.
                Some(shaft) => {
                    if refill {
                        pass(
                            &mut encoder,
                            cleared(&world.shafts.layers[a]),
                            &self.fill_pipeline,
                            flat,
                            None,
                            &mesh(a, SHAFT, shaft, None),
                        );
                    }
                    let group = Some(&world.shaft_bind_groups[a]);
                    pass(
                        &mut encoder,
                        seen,
                        &self.seen_shaft_pipeline,
                        flat,
                        group,
                        &[(masks, r.visible.clone())],
                    );
                }
                None => pass(
                    &mut encoder,
                    seen,
                    &self.seen_pipeline,
                    flat,
                    None,
                    &[(masks, r.visible.clone())],
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
            pass.set_bind_group(0, self.composite_bind_group.as_ref().unwrap(), &[]);
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
            &[(canvas, overlay)],
        );
        let ui_group = &self.ui_transform.bind_group;
        pass(
            &mut encoder,
            keep(&screen),
            &self.shape_pipeline,
            ui_group,
            None,
            &[(canvas, ui)],
        );

        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }
}
