//! WebGPU renderer. Each frame, for each layer of the level in view, drawn
//! through that layer's transform (the camera, plus parallax: each layer
//! below the viewer's a little smaller about the view's center):
//! 1. its visible, remembered and unknown canvases;
//! 2. where it's void (the layer below shows through), in a mask;
//! 3. its visibility polygon into `now`, cleared every frame; and in world
//!    space into `seen`, never cleared, so it accumulates everything seen
//!    (for a layer below the viewer's, only where it shows through).
//!
//! Layers drawn go in slots by how far below the viewer's they are, so
//! what's kept per layer is bounded by [`MAX_VISIBLE`], not the level's
//! depth.
//!
//! What only changes with the level's tiles (each layer's tile art, voids,
//! liquid and shafts) comes as meshes, kept on the GPU while they last;
//! of those drawn through the camera, only the chunks in view are drawn.
//!
//! Then it composites to the screen, from the viewer's layer down through
//! void (and the mist hanging between the layers), styling each pixel by
//! its layer's masks, and draws the overlay (world space, on the viewer's
//! layer) and the interface (view space) on top. `seen` is kept in world
//! space so it survives camera moves, zooming and the viewer changing
//! layers, in pages a chunk of the level each, only those seen and only
//! while in use (see [`crate::pages`]). Everything else is sized to the
//! screen, at its own resolution, and made again when that changes.

use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use wgpu::util::DeviceExt;

use crate::pages::{Op, Page, Pages};
use goblin_nightclub_core::camera::Rect;
use goblin_nightclub_core::chunk::CHUNK;
use goblin_nightclub_core::draw::Vertex;
use goblin_nightclub_core::frame::{Frame, LayerFrame};
use goblin_nightclub_core::layer::TILE;
use goblin_nightclub_core::level::MAX_VISIBLE;
use goblin_nightclub_core::level::MeshPiece;
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::tileset::Sheet;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// MSAA sample count for the `now` masks, which anti-aliases their edges.
/// 1 gives hard pixel edges.
const MASK_SAMPLES: u32 = 4;

/// Pages of `seen` the atlas starts with room for, and the most it grows to
/// (if the device allows).
const PAGES_FIRST: u32 = 16;
const PAGES_MAX: u32 = 256;

/// Marks a page in the page table nothing on which has been seen; mirrors
/// `NO_PAGE` in composite.wgsl.
const NO_PAGE: u32 = u32::MAX;

/// Words per slot at the head of the page table (see `pages` in
/// composite.wgsl).
const TABLE_HEAD: usize = 8;

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
    /// How much smaller each layer is drawn than the one above it: so how
    /// much larger anything up from its layer (see `Vertex::z`) is.
    parallax: f32,
    /// Where world point (0, 0) is in the whole level.
    offset: [f32; 2],
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
    /// Edge of a page of `seen`, in its pixels.
    page_px: f32,
    _pad: f32,
    /// Where world point (0, 0) is in the whole level.
    offset: [f32; 2],
    /// See [`Frame::fade`] and [`Frame::sink`].
    fade: f32,
    sink: f32,
    _pad2: [f32; 2],
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

    /// Write `data` from the start, growing first if need be: whether it
    /// grew (so anything bound to the old buffer must be bound again).
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        let len = data.len() as u64;
        let grow = len > self.buffer.size();
        if grow {
            self.buffer = Self::alloc(device, self.label, self.usage, len.next_power_of_two());
        }
        queue.write_buffer(&self.buffer, 0, data);
        grow
    }
}

/// A 2D array texture: one view of the whole array to read from, and one
/// view per layer to draw into.
struct ArrayTexture {
    texture: wgpu::Texture,
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
        Self::with_usage(
            device,
            label,
            size,
            layers,
            format,
            wgpu::TextureUsages::empty(),
        )
    }

    /// Also usable as `usage`.
    fn with_usage(
        device: &wgpu::Device,
        label: &str,
        size: [u32; 2],
        layers: u32,
        format: wgpu::TextureFormat,
        usage: wgpu::TextureUsages,
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | usage,
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
        Self {
            texture,
            whole,
            layers,
        }
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

/// Layer `slot` of an array texture, to copy to or from.
fn at(texture: &wgpu::Texture, slot: u32) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d {
            x: 0,
            y: 0,
            z: slot,
        },
        aspect: wgpu::TextureAspect::All,
    }
}

/// Bytes a row of a page takes in a buffer it's copied to: rows of a
/// texture copied to a buffer start every 256 bytes.
fn padded_row(page_px: u32) -> u32 {
    page_px.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
}

/// The pages (columns, then rows, each from inclusive to exclusive) that
/// `rect` covers within a level `world` units in size, `page` units each.
fn pages_over(rect: Rect, world: [f32; 2], page: f32) -> [Range<i32>; 2] {
    [0, 1].map(|axis| {
        let lo = (rect[0][axis].max(0.0) / page).floor() as i32;
        let hi = (rect[1][axis].min(world[axis]) / page).ceil() as i32;
        lo..hi.max(lo)
    })
}

/// What a layer's `seen` pages are drawn from: its visible region (as
/// triangles) and, below the viewer, where it shows through; and the size
/// of the pages.
fn seen_key(visible: &[Vertex], shaft: Option<&Mesh>, page_px: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in visible {
        v.pos.map(f32::to_bits).hash(&mut h);
    }
    shaft.map(Mesh::id).hash(&mut h);
    page_px.hash(&mut h);
    h.finish()
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

/// What's been seen: the pages of each layer's `seen` mask on the GPU,
/// in world space at the art's own size (see [`crate::pages`]).
struct Seen {
    pages: Pages,
    /// Pixels a side of a page: a chunk of the level.
    page_px: u32,
    /// A page each.
    atlas: ArrayTexture,
    /// A page's worth of where the layer it's of shows through to the
    /// viewer's, drawn afresh for each page of a layer below; read by
    /// `fs_mask_shaft`.
    shaft: wgpu::TextureView,
    shaft_bind_group: wgpu::BindGroup,
    /// Pages being copied back to the CPU.
    leaving: Vec<Leaving>,
    /// Per layer, what its pages were last drawn from (see `seen_key`):
    /// while that's unchanged they needn't be drawn again.
    drawn: HashMap<usize, u64>,
}

/// A page on its way back to the CPU: its pixels, in a buffer being mapped.
struct Leaving {
    page: Page,
    buffer: wgpu::Buffer,
    /// Set once mapping ends: whether it worked.
    mapped: Rc<Cell<Option<bool>>>,
}

/// One page of a layer's `seen` mask to draw what's visible into.
struct PageDraw {
    slot: u32,
    /// Its world rectangle.
    rect: Rect,
}

/// A mesh kept on the GPU (see [`Renderer::keep`]).
struct Kept {
    /// `None` for a mesh with no triangles.
    buffer: Option<wgpu::Buffer>,
    /// Whether it's been drawn or offered since the last frame drawn.
    used: bool,
}

/// A draw of a range of a vertex buffer's vertices, moved by an entry of
/// [`Renderer::shifts`] (0, none).
type Draw<'a> = (&'a wgpu::Buffer, Range<u32>, u32);

/// What's sized to the screen, a slot each.
struct Screen {
    size: [u32; 2],
    /// Per slot `s`: visible, remembered, unknown at `3s`, `3s + 1`,
    /// `3s + 2`.
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
    /// The same, but only where a sprite is opaque: the props seen.
    mask_cutout_pipeline: wgpu::RenderPipeline,
    seen_pipeline: wgpu::RenderPipeline,
    seen_cutout_pipeline: wgpu::RenderPipeline,
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
    /// Per slot, onto the view; per page drawn this frame, onto the page
    /// (made as needed); and the view with no camera (for the interface).
    layer_transforms: Vec<Uniform>,
    page_transforms: Vec<Uniform>,
    ui_transform: Uniform,
    config: wgpu::SurfaceConfiguration,
    screen: Option<Screen>,
    seen: Option<Seen>,
    /// Where each slot's pages in view are, for the composite (see `pages`
    /// in composite.wgsl).
    page_table: GrowBuffer,
    /// Reads `screen`, `seen` and `page_table`; made again with any.
    composite_bind_group: Option<wgpu::BindGroup>,
    canvas_vertices: GrowBuffer,
    mask_vertices: GrowBuffer,
    /// What goes into those each frame, kept to reuse the memory.
    canvas_scratch: Vec<Vertex>,
    mask_scratch: Vec<Vertex>,
    /// By mesh id, those on the GPU: drawn, or offered to be (see
    /// [`Renderer::preload`]). A layer's meshes are mostly its chunks'
    /// (see [`goblin_nightclub_core::level::LayerMeshes`]), shared with the windows before and after
    /// it, so they stay as long as any has them.
    meshes: HashMap<u64, Kept>,
    /// What each draw moves its vertices by, per frame: nothing, then
    /// where each piece of each layer drawn goes.
    shifts: GrowBuffer,
}

/// `mesh`'s triangles on the GPU, if it has any.
fn mesh_buffer(device: &wgpu::Device, mesh: &Mesh) -> Option<wgpu::Buffer> {
    (!mesh.vertices.is_empty()).then(|| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mesh"),
            contents: bytemuck::cast_slice(&mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        })
    })
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
                7 => Float32,
            ],
        };
        // And where each draw moves them to (see `Renderer::shifts`).
        let shift_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<[f32; 2]>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![6 => Float32x2],
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
                        buffers: &[Some(vertex_layout.clone()), Some(shift_layout.clone())],
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
        let mask_cutout_pipeline = triangles(
            "mask cutout",
            plain,
            "fs_mask_cutout",
            MASK_FORMAT.into(),
            MASK_SAMPLES,
        );
        let seen_pipeline = triangles("seen", plain, "fs_mask", MASK_FORMAT.into(), 1);
        let seen_cutout_pipeline = triangles(
            "seen cutout",
            plain,
            "fs_mask_cutout",
            MASK_FORMAT.into(),
            1,
        );
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
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
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
        let layer_transforms = (0..MAX_VISIBLE).map(|_| uniform()).collect();
        let ui_transform = uniform();
        let page_table = GrowBuffer::new(&device, "page table", wgpu::BufferUsages::STORAGE);
        let canvas_vertices =
            GrowBuffer::new(&device, "canvas vertices", wgpu::BufferUsages::VERTEX);
        let mask_vertices = GrowBuffer::new(&device, "mask vertices", wgpu::BufferUsages::VERTEX);
        let shifts = GrowBuffer::new(&device, "shifts", wgpu::BufferUsages::VERTEX);
        Ok(Self {
            surface,
            device,
            queue,
            shape_pipeline,
            mask_pipeline,
            mask_cutout_pipeline,
            seen_pipeline,
            seen_cutout_pipeline,
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
            page_transforms: Vec::new(),
            ui_transform,
            config,
            screen: None,
            seen: None,
            page_table,
            composite_bind_group: None,
            canvas_vertices,
            mask_vertices,
            canvas_scratch: Vec::new(),
            mask_scratch: Vec::new(),
            meshes: HashMap::new(),
            shifts,
        })
    }

    /// The canvas's size in pixels.
    pub fn size(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    /// Forget what's been seen, for a new level (the next frame makes it
    /// afresh).
    pub fn reset(&mut self) {
        self.seen = None;
        self.composite_bind_group = None;
        self.meshes.clear();
    }

    /// Draw from `sheet` from now on.
    pub fn set_sheet(&mut self, sheet: &Sheet) {
        self.sheet = upload_sheet(&self.device, &self.queue, sheet);
        let all = self
            .layer_transforms
            .iter_mut()
            .chain(&mut self.page_transforms)
            .chain([&mut self.ui_transform]);
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

    /// (Re)create what's sized to the screen, if that changed.
    fn ensure_screen(&mut self) {
        let size = [self.config.width, self.config.height];
        if self.screen.as_ref().is_some_and(|s| s.size == size) {
            return;
        }
        let device = &self.device;
        let slots = MAX_VISIBLE as u32;
        let format = self.config.format;
        self.screen = Some(Screen {
            size,
            canvases: ArrayTexture::new(device, "canvases", size, 3 * slots, format),
            now: ArrayTexture::new(device, "mask now", size, slots, MASK_FORMAT),
            voids: ArrayTexture::new(device, "voids", size, slots, MASK_FORMAT),
            now_msaa: (0..MAX_VISIBLE)
                .map(|_| Msaa::new(device, "now msaa", size))
                .collect(),
        });
        self.composite_bind_group = None;
    }

    /// A texture array of `capacity` pages `page_px` pixels square.
    fn atlas(device: &wgpu::Device, page_px: u32, capacity: u32) -> ArrayTexture {
        let copies = wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
        ArrayTexture::with_usage(
            device,
            "seen pages",
            [page_px; 2],
            capacity,
            MASK_FORMAT,
            copies,
        )
    }

    /// (Re)create the pages of what's been seen, `page_px` pixels square,
    /// if that changed (forgetting what's been seen).
    fn ensure_seen(&mut self, page_px: u32) {
        if self.seen.as_ref().is_some_and(|s| s.page_px == page_px) {
            return;
        }
        let device = &self.device;
        let max = device.limits().max_texture_array_layers.min(PAGES_MAX);
        let first = PAGES_FIRST.min(max);
        let shaft = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("page shaft"),
                size: wgpu::Extent3d {
                    width: page_px,
                    height: page_px,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: MASK_FORMAT,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let shaft_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("page shaft"),
            layout: &self.shaft_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&shaft),
            }],
        });
        self.seen = Some(Seen {
            pages: Pages::new((page_px * page_px) as usize, first, max),
            page_px,
            atlas: Self::atlas(device, page_px, first),
            shaft,
            shaft_bind_group,
            leaving: Vec::new(),
            drawn: HashMap::new(),
        });
        self.composite_bind_group = None;
    }

    fn ensure_composite(&mut self) {
        if self.composite_bind_group.is_some() {
            return;
        }
        let (screen, seen) = (self.screen.as_ref().unwrap(), self.seen.as_ref().unwrap());
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
                    texture(3, &seen.atlas.whole),
                    texture(4, &screen.voids.whole),
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: self.page_table.buffer.as_entire_binding(),
                    },
                ],
            }));
    }

    /// Keep `mesh` on the GPU, putting it there unless it is, through
    /// the next frame drawn; after that, only if it's kept again.
    fn keep(&mut self, mesh: &Mesh) {
        let device = &self.device;
        let kept = self.meshes.entry(mesh.id()).or_insert_with(|| Kept {
            buffer: mesh_buffer(device, mesh),
            used: false,
        });
        kept.used = true;
    }

    /// Put `meshes` on the GPU ahead of drawing them, those not there
    /// already: a level's being made (offered a few more each frame, as
    /// they're made), so the frame it's first drawn needn't. Those not
    /// offered again (or drawn) before the next frame's drawn are let go.
    pub fn preload<'a>(&mut self, meshes: impl IntoIterator<Item = &'a Mesh>) {
        for mesh in meshes {
            self.keep(mesh);
        }
    }

    /// Hand the pages whose pixels have reached the CPU to [`Pages`].
    fn collect_leaving(&mut self) {
        let Some(seen) = &mut self.seen else {
            return;
        };
        let px = seen.page_px as usize;
        let row = padded_row(seen.page_px) as usize;
        let pages = &mut seen.pages;
        seen.leaving.retain(|l| {
            let Some(mapped) = l.mapped.get() else {
                return true;
            };
            let pixels = mapped
                .then(|| l.buffer.slice(..).get_mapped_range().ok())
                .flatten()
                .map(|data| data.chunks(row).flat_map(|r| &r[..px]).copied().collect());
            // Lost on the way, it's as if nothing on it had been seen.
            pages.stored(l.page, &pixels.unwrap_or_else(|| vec![0; px * px]));
            if mapped {
                l.buffer.unmap();
            }
            false
        });
    }

    /// Carry out what [`Pages`] decided: grow the atlas and copy evicted
    /// pages out first (submitted at once), then clear new pages (in
    /// `encoder`, before anything's drawn into them) and fill those coming
    /// back (written before `encoder` runs).
    fn apply_page_ops(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let seen = self.seen.as_mut().unwrap();
        let ops = seen.pages.take_ops();
        if ops.is_empty() {
            return;
        }
        let px = seen.page_px;
        let page = wgpu::Extent3d {
            width: px,
            height: px,
            depth_or_array_layers: 1,
        };
        let mut first = self.device.create_command_encoder(&Default::default());
        let mut leaving = Vec::new();
        for op in &ops {
            match op {
                Op::Grow(capacity) => {
                    let old = seen.atlas.layers.len() as u32;
                    let atlas = Self::atlas(&self.device, px, *capacity);
                    let all = wgpu::Extent3d {
                        depth_or_array_layers: old,
                        ..page
                    };
                    first.copy_texture_to_texture(
                        at(&seen.atlas.texture, 0),
                        at(&atlas.texture, 0),
                        all,
                    );
                    seen.atlas = atlas;
                    self.composite_bind_group = None;
                }
                Op::Evict { page: which, slot } => {
                    let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("leaving page"),
                        size: (padded_row(px) * px) as u64,
                        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    first.copy_texture_to_buffer(
                        at(&seen.atlas.texture, *slot),
                        wgpu::TexelCopyBufferInfo {
                            buffer: &buffer,
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: 0,
                                bytes_per_row: Some(padded_row(px)),
                                rows_per_image: Some(px),
                            },
                        },
                        page,
                    );
                    leaving.push(Leaving {
                        page: *which,
                        buffer,
                        mapped: Rc::new(Cell::new(None)),
                    });
                }
                Op::Clear(_) | Op::Upload(..) => {}
            }
        }
        self.queue.submit([first.finish()]);
        for l in leaving {
            let mapped = l.mapped.clone();
            l.buffer
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| mapped.set(Some(r.is_ok())));
            seen.leaving.push(l);
        }
        for op in ops {
            match op {
                Op::Clear(slot) => {
                    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("clear page"),
                        color_attachments: &[Some(cleared(&seen.atlas.layers[slot as usize]))],
                        ..Default::default()
                    });
                }
                Op::Upload(slot, pixels) => self.queue.write_texture(
                    at(&seen.atlas.texture, slot),
                    &pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(px),
                        rows_per_image: Some(px),
                    },
                    page,
                ),
                Op::Grow(_) | Op::Evict { .. } => {}
            }
        }
    }

    pub fn draw(&mut self, frame: &Frame) {
        // Nothing's decided about pages for a frame that isn't drawn.
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            _ => return,
        };
        // The camera on a whole pixel, so the tile art stays crisp. `seen`
        // is at the art's own size, whatever the view's zoom, so zooming
        // keeps what's been seen; its pages are a chunk of the level each.
        let zoom = frame.zoom;
        let cell = frame.cell as f32;
        let seen_zoom = cell / TILE;
        let page_world = CHUNK as f32 * TILE;
        let page_px = CHUNK as u32 * frame.cell;
        self.ensure_seen(page_px);
        self.ensure_screen();
        self.collect_leaving();
        let camera = frame.camera.map(|c| (c * zoom).round() / zoom);
        // The level may be drawn a window at a time: its layers and pages,
        // and patterns pinned to the world, go by where they are in the
        // whole level.
        let offset = frame.offset.map(|t| t as f32 * TILE);
        let page_offset = frame.offset.map(|t| t.div_euclid(CHUNK as i32));
        let page = |index: usize, px: i32, py: i32| -> Page {
            (frame.base + index, px + page_offset[0], py + page_offset[1])
        };
        // Top first, a slot each: how far below the viewer's it is.
        let layers: Vec<_> = frame
            .layers
            .iter()
            .filter(|l| frame.top - l.index < MAX_VISIBLE)
            .collect();
        let slot = |index: usize| frame.top - index;
        // Each layer's meshes on the GPU, and where each of its pieces
        // goes: its first in `shifts` at `firsts`.
        let mut shifts = vec![[0.0f32; 2]];
        let mut firsts = Vec::new();
        for l in &layers {
            for mesh in l.meshes.all().chain(l.shaft.as_deref()) {
                self.keep(mesh);
            }
            firsts.push(shifts.len() as u32);
            shifts.extend((0..l.meshes.pieces.len()).map(|i| l.meshes.corner(i)));
        }
        self.shifts
            .write(&self.device, &self.queue, bytemuck::cast_slice(&shifts));

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
            props_seen: Range<u32>,
            standing: Range<u32>,
        }
        let ranges: Vec<Ranges> = layers
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
                let props_seen = push(&mut mask_vertices, &l.props_seen.vertices);
                let standing = push(&mut mask_vertices, &l.standing.vertices);
                Ranges {
                    canvases,
                    visible,
                    props_seen,
                    standing,
                }
            })
            .collect();
        let overlay = push(&mut canvas_vertices, &frame.overlay.vertices);
        let ui = push(&mut canvas_vertices, &frame.ui.vertices);

        // Below the viewer's, a layer's pages matter only where it shows
        // through.
        let shows = |l: &LayerFrame, rect: Rect| {
            l.shaft
                .as_ref()
                .is_none_or(|shaft| !shaft.ranges(rect).is_empty())
        };
        let page_rect = |px: i32, py: i32| -> Rect {
            let at = [px as f32 * page_world, py as f32 * page_world];
            [at, [at[0] + page_world, at[1] + page_world]]
        };
        let seen = self.seen.as_mut().unwrap();
        seen.pages.begin_frame();
        // The pages each slot shows: a window over its part of the level in
        // view (see `pages` in composite.wgsl). First, so they're in use
        // this frame before anything needs room: a page drawn into for the
        // first time below is missing until the next, but that's where it's
        // visible now, which shows whatever's been seen.
        let mut table = vec![0u32; TABLE_HEAD * MAX_VISIBLE];
        for l in &layers {
            let [xs, ys] = pages_over(frame.layer_view(l.index), frame.world, page_world);
            let head = TABLE_HEAD * slot(l.index);
            let size = [xs.len() as u32, ys.len() as u32];
            let start = table.len() as u32;
            table[head..head + 5].copy_from_slice(&[
                xs.start as u32,
                ys.start as u32,
                size[0],
                size[1],
                start,
            ]);
            for py in ys {
                for px in xs.clone() {
                    let page = shows(l, page_rect(px, py))
                        .then(|| seen.pages.read(page(l.index, px, py)))
                        .flatten();
                    table.push(page.unwrap_or(NO_PAGE));
                }
            }
        }
        // What's visible goes into the pages it covers, unless it's what
        // they were last drawn from. A page there's no room for now is
        // drawn next frame.
        let draws: Vec<Vec<PageDraw>> = layers
            .iter()
            .zip(&ranges)
            .map(|(l, r)| {
                let visible = &mask_vertices[r.visible.start as usize..r.visible.end as usize];
                if !frame.remember || visible.is_empty() {
                    return Vec::new();
                }
                let key = seen_key(visible, l.shaft.as_deref(), page_px);
                let layer = frame.base + l.index;
                if seen.drawn.get(&layer) == Some(&key) {
                    return Vec::new();
                }
                let fold = |f: fn(f32, f32) -> f32, start: f32| {
                    [0, 1].map(|axis| visible.iter().map(|v| v.pos[axis]).fold(start, f))
                };
                let bounds = [
                    fold(f32::min, f32::INFINITY),
                    fold(f32::max, f32::NEG_INFINITY),
                ];
                let [xs, ys] = pages_over(bounds, frame.world, page_world);
                let mut complete = true;
                let mut draws = Vec::new();
                for py in ys {
                    for px in xs.clone() {
                        let rect = page_rect(px, py);
                        if !shows(l, rect) {
                            continue;
                        }
                        match seen.pages.write(page(l.index, px, py)) {
                            Some(slot) => draws.push(PageDraw { slot, rect }),
                            None => complete = false,
                        }
                    }
                }
                if complete {
                    seen.drawn.insert(layer, key);
                } else {
                    seen.drawn.remove(&layer);
                }
                draws
            })
            .collect();
        seen.drawn
            .retain(|layer, _| layers.iter().any(|l| frame.base + l.index == *layer));
        if self
            .page_table
            .write(&self.device, &self.queue, bytemuck::cast_slice(&table))
        {
            self.composite_bind_group = None;
        }
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
        self.meshes
            .retain(|_, kept| std::mem::replace(&mut kept.used, false));

        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.apply_page_ops(&mut encoder);
        self.ensure_composite();

        // Transforms: each slot onto the view, smaller the deeper it is;
        // each page drawn onto its page; the interface as is.
        let center = [frame.view[0] / 2.0, frame.view[1] / 2.0];
        let parallax = frame.parallax();
        for l in &layers {
            let t = Transform {
                screen: frame.view,
                camera,
                center,
                scale: frame.scale(l.index),
                zoom,
                cell,
                parallax: frame.parallax(),
                offset,
            };
            let buffer = &self.layer_transforms[slot(l.index)].buffer;
            self.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&t));
        }
        let flat = |screen, camera, zoom| Transform {
            screen,
            camera,
            center: [0.0; 2],
            scale: 1.0,
            zoom,
            cell,
            parallax: 1.0,
            offset,
        };
        let pages_drawn = draws.iter().map(Vec::len).sum();
        while self.page_transforms.len() < pages_drawn {
            let uniform = Uniform::new(
                &self.device,
                &self.transform_layout,
                &self.sheet,
                &self.sampler,
            );
            self.page_transforms.push(uniform);
        }
        for (d, uniform) in draws.iter().flatten().zip(&self.page_transforms) {
            let t = flat([page_px as f32; 2], d.rect[0], seen_zoom);
            self.queue
                .write_buffer(&uniform.buffer, 0, bytemuck::bytes_of(&t));
        }
        self.queue.write_buffer(
            &self.ui_transform.buffer,
            0,
            bytemuck::bytes_of(&flat(frame.view, [0.0; 2], frame.ui_scale)),
        );

        let present = layers
            .iter()
            .fold(0u32, |bits, l| bits | 1 << slot(l.index));
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
            page_px: page_px as f32,
            _pad: 0.0,
            offset,
            fade: frame.fade,
            sink: frame.sink,
            _pad2: [0.0; 2],
        };
        self.queue
            .write_buffer(&self.composite_params, 0, bytemuck::bytes_of(&params));

        let screen = output.texture.create_view(&Default::default());
        let seen = self.seen.as_ref().unwrap();
        let screen_textures = self.screen.as_ref().unwrap();

        // One pass with `pipeline` through `transform`, drawing each range
        // of a vertex buffer in turn, moved by its shift.
        let shift_buffer = &self.shifts.buffer;
        let pass = |encoder: &mut wgpu::CommandEncoder,
                    attachment: wgpu::RenderPassColorAttachment,
                    pipeline: &wgpu::RenderPipeline,
                    transform: &wgpu::BindGroup,
                    extra: Option<&wgpu::BindGroup>,
                    draws: &[Draw]| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pass"),
                color_attachments: &[Some(attachment)],
                ..Default::default()
            });
            let mut ready = false;
            for (buffer, range, shift) in draws.iter().filter(|(_, r, _)| !r.is_empty()) {
                if !ready {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, transform, &[]);
                    if let Some(group) = extra {
                        pass.set_bind_group(1, group, &[]);
                    }
                    pass.set_vertex_buffer(1, shift_buffer.slice(..));
                    ready = true;
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range.clone(), *shift..*shift + 1);
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
        // Mesh `m`'s chunks that reach into `view`, moved by shift
        // `shift` (`view` is where they are before that): as few draws as
        // there are runs of them side by side in its buffer.
        let mesh = |m: &Mesh, view: Rect, shift: u32| -> Vec<Draw> {
            let Some(Kept {
                buffer: Some(buffer),
                ..
            }) = self.meshes.get(&m.id())
            else {
                return Vec::new();
            };
            let mut ranges: Vec<Range<u32>> = Vec::new();
            for r in m.ranges(view) {
                match ranges.last_mut() {
                    Some(last) if last.end == r.start => last.end = r.end,
                    _ => ranges.push(r),
                }
            }
            ranges.into_iter().map(|r| (buffer, r, shift)).collect()
        };
        // Of each piece of layer `l` (its first shift `first`), mesh `of`,
        // where it reaches into `view`.
        let pieces = |l: &LayerFrame, first: u32, of: fn(&MeshPiece) -> &Mesh, view: Rect| {
            let mut draws = Vec::new();
            for (i, piece) in l.meshes.pieces.iter().enumerate() {
                let c = l.meshes.corner(i);
                let local = view.map(|p| [p[0] - c[0], p[1] - c[1]]);
                draws.extend(mesh(of(piece), local, first + i as u32));
            }
            draws
        };

        let mut page_transforms = self.page_transforms.iter();
        for (((l, r), page_draws), &first) in layers.iter().zip(&ranges).zip(&draws).zip(&firsts) {
            let (a, s) = (l.index, slot(l.index));
            let view = frame.layer_view(a);
            let transform = &self.layer_transforms[s].bind_group;
            let (canvas, masks) = (&self.canvas_vertices.buffer, &self.mask_vertices.buffer);
            // The tiles, then fringes and trims over them: the pieces', then
            // those along their edges.
            let m = &l.meshes;
            let mut tiles = pieces(l, first, |p| &p.tiles, view);
            let edging = [
                pieces(l, first, |p| &p.fringes, view),
                mesh(&m.fringes, view, 0),
                pieces(l, first, |p| &p.trims, view),
                mesh(&m.trims, view, 0),
            ]
            .concat();
            tiles.extend(edging.iter().cloned());
            // Props over all of it, and whoever's about too.
            let props = pieces(l, first, |p| &p.props, view);
            for (i, range) in r.canvases.iter().enumerate() {
                let attachment = wgpu::RenderPassColorAttachment {
                    view: &screen_textures.canvases.layers[3 * s + i],
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                };
                // The tiles under the visible and remembered canvases, and
                // the props over them.
                let (under, over) = if i < 2 {
                    (tiles.as_slice(), props.as_slice())
                } else {
                    (&[][..], &[][..])
                };
                let mut draws = under.to_vec();
                draws.push((canvas, range.clone(), 0));
                draws.extend(over.iter().cloned());
                pass(
                    &mut encoder,
                    attachment,
                    &self.shape_pipeline,
                    transform,
                    None,
                    &draws,
                );
            }
            let voids = &screen_textures.voids.layers[s];
            pass(
                &mut encoder,
                cleared(voids),
                &self.fill_pipeline,
                transform,
                None,
                &pieces(l, first, |p| &p.voids, view),
            );
            // Liquid is void that tints what shows through: mark it half.
            pass(
                &mut encoder,
                keep(voids),
                &self.water_pipeline,
                transform,
                None,
                &pieces(l, first, |p| &p.liquid, view),
            );
            // Fringes, trims and props cover the void and water they reach
            // over.
            pass(
                &mut encoder,
                keep(voids),
                &self.unmask_pipeline,
                transform,
                None,
                &[edging, props].concat(),
            );
            // Then clear it again under whatever stands on the layer.
            pass(
                &mut encoder,
                keep(voids),
                &self.unmask_pipeline,
                transform,
                None,
                &[(masks, r.standing.clone(), 0)],
            );
            // What's visible now: the polygon (and lit solid), and the props
            // seen where their art is opaque.
            let now = screen_textures.now_msaa[s].attachment(&screen_textures.now.layers[s], true);
            {
                let mut now = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("now"),
                    color_attachments: &[Some(now)],
                    ..Default::default()
                });
                now.set_bind_group(0, transform, &[]);
                now.set_vertex_buffer(0, masks.slice(..));
                now.set_vertex_buffer(1, shift_buffer.slice(..));
                let draws = [
                    (&self.mask_pipeline, r.visible.clone()),
                    (&self.mask_cutout_pipeline, r.props_seen.clone()),
                ];
                for (pipeline, range) in draws.into_iter().filter(|(_, r)| !r.is_empty()) {
                    now.set_pipeline(pipeline);
                    now.draw(range, 0..1);
                }
            }
            // What's visible, into each page of `seen` it covers.
            for d in page_draws {
                let onto = &page_transforms.next().unwrap().bind_group;
                let page = keep(&seen.atlas.layers[d.slot as usize]);
                let visible = [(masks, r.visible.clone(), 0)];
                match &l.shaft {
                    // Below the viewer: remembered as seen only where it
                    // shows through.
                    Some(shaft) => {
                        pass(
                            &mut encoder,
                            cleared(&seen.shaft),
                            &self.fill_pipeline,
                            onto,
                            None,
                            &mesh(shaft, d.rect, 0),
                        );
                        let group = Some(&seen.shaft_bind_group);
                        let pipeline = &self.seen_shaft_pipeline;
                        pass(&mut encoder, page, pipeline, onto, group, &visible);
                    }
                    None => {
                        pass(
                            &mut encoder,
                            page,
                            &self.seen_pipeline,
                            onto,
                            None,
                            &visible,
                        );
                        if !r.props_seen.is_empty() {
                            pass(
                                &mut encoder,
                                keep(&seen.atlas.layers[d.slot as usize]),
                                &self.seen_cutout_pipeline,
                                onto,
                                None,
                                &[(masks, r.props_seen.clone(), 0)],
                            );
                        }
                    }
                }
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
        let top = &self.layer_transforms[0].bind_group;
        pass(
            &mut encoder,
            keep(&screen),
            &self.shape_pipeline,
            top,
            None,
            &[(canvas, overlay, 0)],
        );
        let ui_group = &self.ui_transform.bind_group;
        pass(
            &mut encoder,
            keep(&screen),
            &self.shape_pipeline,
            ui_group,
            None,
            &[(canvas, ui, 0)],
        );

        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }
}
