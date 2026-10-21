//! WebGPU renderer. Each frame, for each layer of the level in view, drawn
//! through that layer's transform (the camera, plus parallax: each layer
//! below the viewer's a little smaller about the view's center):
//! 1. its visible and remembered canvases;
//! 2. where it's void (the layer below shows through), in a mask;
//! 3. its visibility polygon into `now`, cleared every frame; and in world
//!    space into `seen`, never cleared, so it accumulates everything seen
//!    (for a layer below the viewer's, only where it shows through);
//! 4. on the viewer's layer, what lights cast into a light map: beyond the
//!    view radius the polygon is visible only where they light it (and
//!    remembered as seen only there), and the visible style is lit by
//!    them.
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
//! while in use (see [`crate::pages`]). Beyond the window of a level
//! played a window at a time, the composite shows the map instead (see
//! [`goblin_nightclub_core::map`]): the chunks of it in view on the GPU,
//! while there's room (see [`crate::map_pages`]). Everything else is sized
//! to the screen, at its own resolution, and made again when that changes.

use wasm_bindgen::JsError;
use web_sys::HtmlCanvasElement;

use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::map_pages::{MapOp, MapPages};
use crate::pages::{Op, Page, Pages};
use crate::shaders::{self, CompositeParams, MAP_CODES, NO_PAGE, TABLE_HEAD, Transform};
use goblin_nightclub_core::art::MeshPiece;
use goblin_nightclub_core::camera;
use goblin_nightclub_core::chunk::CHUNK;
use goblin_nightclub_core::draw::Vertex;
use goblin_nightclub_core::frame::{Frame, LayerFrame};
use goblin_nightclub_core::geom::{Rect, bounds};
use goblin_nightclub_core::layer::TILE;
use goblin_nightclub_core::level::MAX_VISIBLE;
use goblin_nightclub_core::map::{MapArt, MapTiles};
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::tileset::Sheet;

const MASK_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// What lights cast on the viewer's layer (see `light_map` in
/// composite.wgsl): their colors added up, so past 1 where they meet.
const LIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// MSAA sample count for the `now` masks, which anti-aliases their edges.
/// 1 gives hard pixel edges.
const MASK_SAMPLES: u32 = 4;

/// Pages of `seen` the atlas starts with room for, and the most it grows to
/// (if the device allows).
const PAGES_FIRST: u32 = 16;
const PAGES_MAX: u32 = 256;

/// Chunks of the map there's room for at first, and the most (if the
/// device allows): 4 KiB each.
const MAP_PAGES_FIRST: u32 = 64;
const MAP_PAGES_MAX: u32 = 8192;
const MAP_PAGE_BYTES: u64 = size_of::<MapTiles>() as u64;

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
    fn new(
        device: &wgpu::Device,
        label: &str,
        size: [u32; 2],
        format: wgpu::TextureFormat,
    ) -> Self {
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
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        });
        Self { view }
    }

    /// Color attachment drawing into `resolved`, cleared first.
    fn attachment<'a>(
        &'a self,
        resolved: &'a wgpu::TextureView,
    ) -> wgpu::RenderPassColorAttachment<'a> {
        let (view, resolve_target) = match &self.view {
            Some(msaa) => (msaa, Some(resolved)),
            None => (resolved, None),
        };
        // The samples needn't outlive the pass: they're resolved, and the
        // next pass clears them.
        let store = if self.view.is_some() {
            wgpu::StoreOp::Discard
        } else {
            wgpu::StoreOp::Store
        };
        wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
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

/// What a layer's `seen` pages are drawn from: its visible region and what
/// lights show (as triangles, and how far each reaches) and, below the
/// viewer, where it shows through; and the size of the pages.
fn seen_key(drawn: &[&[Vertex]], shaft: Option<&Mesh>, page_px: u32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in drawn.iter().copied().flatten() {
        v.pos.map(f32::to_bits).hash(&mut h);
        v.edge.map(f32::to_bits).hash(&mut h);
    }
    shaft.map(Mesh::id).hash(&mut h);
    page_px.hash(&mut h);
    h.finish()
}

/// A plain single-sample attachment, first `load`ed.
fn target(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}

/// The same, cleared first.
fn cleared(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    target(view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT))
}

/// The same, drawn over what's there.
fn kept(view: &wgpu::TextureView) -> wgpu::RenderPassColorAttachment<'_> {
    target(view, wgpu::LoadOp::Load)
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
    /// viewer's, drawn afresh for each page of a layer below; or, on the
    /// viewer's layer, of what's in line of sight, for what lights show.
    /// Read by `fs_mask_shaft`.
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

/// What a frame draws of one layer, worked out before any of it is.
struct LayerPlan<'f> {
    layer: &'f LayerFrame,
    /// How far below the viewer's it is: which textures it's drawn into.
    /// `None` for a layer only in sight (see [`Frame::sighted`]), not drawn.
    slot: Option<usize>,
    /// Its first piece's in [`Renderer::shifts`].
    first: u32,
    /// Its canvases, in the canvas vertices.
    canvases: [Range<u32>; 2],
    /// What's visible, what lights light, the props seen and whatever
    /// stands on it, in the mask vertices.
    visible: Range<u32>,
    lights: Range<u32>,
    props_seen: Range<u32>,
    standing: Range<u32>,
    /// The pages of `seen` what's visible goes into this frame.
    pages: Vec<PageDraw>,
}

impl<'f> LayerPlan<'f> {
    /// Layer `layer` in slot `slot` (if drawn), its pieces' shifts from
    /// `first`; its vertices and pages yet to be worked out.
    fn new(layer: &'f LayerFrame, slot: Option<usize>, first: u32) -> Self {
        Self {
            layer,
            slot,
            first,
            canvases: [0..0, 0..0],
            visible: 0..0,
            lights: 0..0,
            props_seen: 0..0,
            standing: 0..0,
            pages: Vec::new(),
        }
    }
}

/// What a frame draws, worked out (its vertices and page table uploaded)
/// before any of it is drawn.
struct FramePlan<'f> {
    layers: Vec<LayerPlan<'f>>,
    /// The overlay and the interface, in the canvas vertices.
    overlay: Range<u32>,
    ui: Range<u32>,
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
    /// Per slot `s`: visible and remembered at `2s` and `2s + 1`.
    canvases: ArrayTexture,
    now: ArrayTexture,
    voids: ArrayTexture,
    now_msaa: Vec<Msaa>,
    /// What lights cast on the viewer's layer.
    light: wgpu::TextureView,
    light_msaa: Msaa,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    shape_pipeline: wgpu::RenderPipeline,
    mask_pipeline: wgpu::RenderPipeline,
    /// The same, but only where a sprite is opaque: the props seen.
    mask_cutout_pipeline: wgpu::RenderPipeline,
    /// What lights cast, into the light map.
    light_pipeline: wgpu::RenderPipeline,
    /// What's visible into `seen`: within the view radius (beyond it,
    /// only what lights show, through `seen_shaft_pipeline`).
    seen_near_pipeline: wgpu::RenderPipeline,
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
    /// The chunks of the map on the GPU, a slot each, and which are where
    /// (see `map_tiles` in composite.wgsl).
    map_tiles: wgpu::Buffer,
    map_pages: MapPages,
    /// Where each layer's chunks in view are, and what the map's codes are
    /// drawn as (see `map_table` and `map_art` in composite.wgsl), the
    /// art the map's (see [`goblin_nightclub_core::map::Map::art`]).
    map_table: GrowBuffer,
    map_art: GrowBuffer,
    map_art_of: Option<Arc<[MapArt]>>,
    /// Reads `screen`, `seen`, `page_table`, the map's and the sheet; made
    /// again with any.
    composite_bind_group: Option<wgpu::BindGroup>,
    canvas_vertices: GrowBuffer,
    mask_vertices: GrowBuffer,
    /// What goes into those each frame, kept to reuse the memory.
    canvas_scratch: Vec<Vertex>,
    mask_scratch: Vec<Vertex>,
    /// By mesh id, those on the GPU: drawn, or offered to be (see
    /// [`Renderer::preload`]). A layer's meshes are mostly its chunks'
    /// (see [`goblin_nightclub_core::art::LayerMeshes`]), shared with the windows before and after
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

/// A storage buffer read in a fragment shader.
fn storage_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A buffer for `pages` chunks of the map.
fn map_buffer(device: &wgpu::Device, pages: u32) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("map"),
        size: pages as u64 * MAP_PAGE_BYTES,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// No chunks of the map on the GPU yet, with room for as many as `buffer`
/// holds, growing as far as `device` allows.
fn map_pages(device: &wgpu::Device, buffer: &wgpu::Buffer) -> MapPages {
    let first = (buffer.size() / MAP_PAGE_BYTES) as u32;
    let most = device.limits().max_storage_buffer_binding_size / MAP_PAGE_BYTES;
    MapPages::new(first, MAP_PAGES_MAX.min(most as u32))
}

/// What `art` the map's codes are drawn as, for `map_art` in
/// composite.wgsl.
fn map_art_words(art: &[MapArt]) -> Vec<u32> {
    let mut words = vec![0u32; 2 * MAP_CODES];
    let mut cells = Vec::new();
    for (code, a) in art.iter().enumerate().take(MAP_CODES) {
        let start = (2 * MAP_CODES + cells.len()) as u32;
        words[2 * code] = start | (a.cells.len() as u32) << 16;
        words[2 * code + 1] = a.alpha.to_bits();
        cells.extend(a.cells.iter().map(|c| c[0] as u32 | (c[1] as u32) << 16));
    }
    words.extend(cells);
    words
}

/// A polygon around `center` (by angle, as a sight polygon is), fanned out
/// from it, then each of the convex `shapes` fanned from its first point:
/// as a triangle list.
fn fan<'a>(
    center: [f32; 2],
    polygon: &'a [[f32; 2]],
    shapes: &'a [Vec<[f32; 2]>],
) -> impl Iterator<Item = [f32; 2]> + 'a {
    let n = polygon.len();
    (0..n)
        .flat_map(move |i| [center, polygon[i], polygon[(i + 1) % n]])
        .chain(shapes.iter().flat_map(|shape| {
            (1..shape.len().saturating_sub(1)).flat_map(move |i| [shape[0], shape[i], shape[i + 1]])
        }))
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
        let module = |label, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            })
        };
        let shapes = module("shapes", shaders::shapes());
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
        // Lights' colors add up; how much they show is the most of any.
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let most = wgpu::BlendComponent {
            operation: wgpu::BlendOperation::Max,
            ..add
        };
        let light_pipeline = triangles(
            "light",
            plain,
            "fs_light",
            wgpu::ColorTargetState {
                format: LIGHT_FORMAT,
                blend: Some(wgpu::BlendState {
                    color: add,
                    alpha: most,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            },
            MASK_SAMPLES,
        );
        let seen_near_pipeline =
            triangles("seen near", plain, "fs_mask_near", MASK_FORMAT.into(), 1);
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
                storage_entry(5),
                texture_entry(6, wgpu::TextureViewDimension::D2),
                storage_entry(7),
                storage_entry(8),
                storage_entry(9),
                texture_entry(10, wgpu::TextureViewDimension::D2),
            ],
        });
        let composite_shader = module("composite", shaders::composite());
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
        let map_tiles = map_buffer(&device, MAP_PAGES_FIRST);
        let map_pages = map_pages(&device, &map_tiles);
        let map_table = GrowBuffer::new(&device, "map table", wgpu::BufferUsages::STORAGE);
        let map_art = GrowBuffer::new(&device, "map art", wgpu::BufferUsages::STORAGE);
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
            light_pipeline,
            seen_near_pipeline,
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
            map_tiles,
            map_pages,
            map_table,
            map_art,
            map_art_of: None,
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
        self.map_pages = map_pages(&self.device, &self.map_tiles);
        self.map_art_of = None;
    }

    /// Draw from `sheet` from now on.
    pub fn set_sheet(&mut self, sheet: &Sheet) {
        self.sheet = upload_sheet(&self.device, &self.queue, sheet);
        self.composite_bind_group = None;
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
            canvases: ArrayTexture::new(device, "canvases", size, 2 * slots, format),
            now: ArrayTexture::new(device, "mask now", size, slots, MASK_FORMAT),
            voids: ArrayTexture::new(device, "voids", size, slots, MASK_FORMAT),
            now_msaa: (0..MAX_VISIBLE)
                .map(|_| Msaa::new(device, "now msaa", size, MASK_FORMAT))
                .collect(),
            light: device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("light"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: LIGHT_FORMAT,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&Default::default()),
            light_msaa: Msaa::new(device, "light msaa", size, LIGHT_FORMAT),
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
                    texture(6, &screen.light),
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: self.map_art.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: self.map_table.buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 9,
                        resource: self.map_tiles.as_entire_binding(),
                    },
                    texture(10, &self.sheet),
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
        let plan = self.plan(frame);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.apply_page_ops(&mut encoder);
        self.ensure_composite();
        self.write_uniforms(frame, &plan);
        let screen = output.texture.create_view(&Default::default());
        self.encode(&mut encoder, frame, &plan, &screen);
        self.queue.submit([encoder.finish()]);
        self.queue.present(output);
    }

    /// Work out what `frame` draws, before any of it is: the textures it
    /// needs, its meshes on the GPU (and where each piece goes), its
    /// vertices uploaded, which pages of `seen` each layer shows (the page
    /// table, uploaded) and which it draws what's visible into.
    fn plan<'f>(&mut self, frame: &'f Frame) -> FramePlan<'f> {
        // `seen` is at the art's own size, whatever the view's zoom, so
        // zooming keeps what's been seen; its pages are a chunk of the level
        // each.
        let page_px = CHUNK as u32 * frame.cell;
        let page_world = CHUNK as f32 * TILE;
        self.ensure_seen(page_px);
        self.ensure_screen();
        self.collect_leaving();
        // The level may be drawn a window at a time: its layers and pages
        // go by where they are in the whole level.
        let page_offset = frame.offset.map(|t| t.div_euclid(CHUNK as i32));
        let page = |index: usize, px: i32, py: i32| -> Page {
            (frame.base + index, px + page_offset[0], py + page_offset[1])
        };

        // Top first, a slot each: how far below the viewer's it is. Each
        // layer's meshes on the GPU, and where each of its pieces goes: its
        // first in `shifts`.
        let mut shifts = vec![[0.0f32; 2]];
        let mut layers = Vec::new();
        for l in frame
            .layers
            .iter()
            .filter(|l| frame.top - l.index < MAX_VISIBLE)
        {
            let shafts = l
                .shaft
                .as_deref()
                .into_iter()
                .chain(l.seen_through.as_deref());
            for mesh in l.meshes.all().chain(shafts) {
                self.keep(mesh);
            }
            let slot = Some(frame.top - l.index);
            layers.push(LayerPlan::new(l, slot, shifts.len() as u32));
            shifts.extend((0..l.meshes.pieces.len()).map(|i| l.meshes.corner(i)));
        }
        // Those only in sight: what's visible of them goes into `seen`, and
        // that's all.
        for l in &frame.sighted {
            if let Some(shaft) = &l.seen_through {
                self.keep(shaft);
            }
            layers.push(LayerPlan::new(l, None, 0));
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
        // How far the viewer sees by itself, for the triangles of what's
        // visible (see `fs_mask_near`).
        let radius = if frame.radius.is_finite() {
            frame.radius
        } else {
            f32::MAX
        };
        for lp in &mut layers {
            let l = lp.layer;
            lp.canvases =
                [&l.visible, &l.remembered].map(|c| push(&mut canvas_vertices, &c.vertices));
            // Visible area as a triangle list: the polygon fanning out from
            // the origin, plus each (convex) lit shape fanned from its first
            // point; each vertex with how far the viewer sees by itself.
            let start = mask_vertices.len() as u32;
            let near = [frame.origin[0], frame.origin[1], radius, 0.0];
            mask_vertices.extend(
                fan(frame.origin, &l.polygon, &l.lit_shapes).map(|pos| Vertex {
                    pos,
                    edge: near,
                    ..Default::default()
                }),
            );
            lp.visible = start..mask_vertices.len() as u32;
            // Likewise what each light lights, with its center, radius and
            // color.
            let start = mask_vertices.len() as u32;
            for lit in &l.lights {
                let edge = [lit.center[0], lit.center[1], lit.radius, 0.0];
                mask_vertices.extend(fan(lit.center, lit.polygon(), lit.band()).map(|pos| {
                    Vertex {
                        pos,
                        color: lit.color,
                        edge,
                        ..Default::default()
                    }
                }));
            }
            lp.lights = start..mask_vertices.len() as u32;
            lp.props_seen = push(&mut mask_vertices, &l.props_seen.vertices);
            lp.standing = push(&mut mask_vertices, &l.standing.vertices);
        }
        let overlay = push(&mut canvas_vertices, &frame.overlay.vertices);
        let ui = push(&mut canvas_vertices, &frame.ui.vertices);

        // Below the viewer's, a layer's pages matter only where it shows
        // through; below the player's, it's seen only where it shows
        // through to theirs.
        let within = |shaft: &Option<Arc<Mesh>>, rect: Rect| {
            shaft
                .as_ref()
                .is_none_or(|shaft| shaft.ranges(rect).next().is_some())
        };
        let shows = |l: &LayerFrame, rect: Rect| within(&l.shaft, rect);
        let in_sight = |l: &LayerFrame, rect: Rect| within(&l.seen_through, rect);
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
        for lp in &layers {
            let Some(slot) = lp.slot else {
                continue;
            };
            let l = lp.layer;
            let [xs, ys] = pages_over(frame.layer_view(l.index), frame.world, page_world);
            let head = TABLE_HEAD * slot;
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
        for lp in &mut layers {
            let l = lp.layer;
            let of = |r: &Range<u32>| &mask_vertices[r.start as usize..r.end as usize];
            let visible = of(&lp.visible);
            if !frame.remember || visible.is_empty() {
                continue;
            }
            let key = seen_key(
                &[visible, of(&lp.lights)],
                l.seen_through.as_deref(),
                page_px,
            );
            let layer = frame.base + l.index;
            if seen.drawn.get(&layer) == Some(&key) {
                continue;
            }
            let points: Vec<[f32; 2]> = visible.iter().map(|v| v.pos).collect();
            let [xs, ys] = pages_over(bounds(&points), frame.world, page_world);
            let mut complete = true;
            for py in ys {
                for px in xs.clone() {
                    let rect = page_rect(px, py);
                    if !in_sight(l, rect) {
                        continue;
                    }
                    match seen.pages.write(page(l.index, px, py)) {
                        Some(slot) => lp.pages.push(PageDraw { slot, rect }),
                        None => complete = false,
                    }
                }
            }
            if complete {
                seen.drawn.insert(layer, key);
            } else {
                seen.drawn.remove(&layer);
            }
        }
        seen.drawn.retain(|layer, _| {
            layers
                .iter()
                .any(|lp| frame.base + lp.layer.index == *layer)
        });
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
        self.plan_map(frame);
        FramePlan {
            layers,
            overlay,
            ui,
        }
    }

    /// The map beyond the window, for the composite: what its codes are
    /// drawn as, where each layer's chunks in view are (down to the bottom
    /// of the whole level), and those chunks on the GPU, while there's room.
    /// Chunks inside the window are left out: the composite never shows the
    /// map there.
    fn plan_map(&mut self, frame: &Frame) {
        let layers = frame.base + frame.top + 1;
        let mut table = vec![0u32; TABLE_HEAD * layers];
        if let Some(map) = &frame.map {
            let art = map.art();
            if !self
                .map_art_of
                .as_ref()
                .is_some_and(|a| Arc::ptr_eq(a, art))
            {
                let words = map_art_words(art);
                if self
                    .map_art
                    .write(&self.device, &self.queue, bytemuck::cast_slice(&words))
                {
                    self.composite_bind_group = None;
                }
                self.map_art_of = Some(art.clone());
            }
            self.map_pages.begin_frame();
            let edge = CHUNK as f32 * TILE;
            // The window, in chunks of the whole level.
            let origin = frame.offset.map(|t| t.div_euclid(CHUNK as i32));
            let window = frame.world.map(|w| (w / edge).ceil() as i32);
            let inside = |cx: i32, cy: i32| {
                (0..window[0]).contains(&(cx - origin[0]))
                    && (0..window[1]).contains(&(cy - origin[1]))
            };
            let parallax = frame.parallax();
            for a in 0..layers {
                let Some([lo, hi]) = map.bounds(a) else {
                    continue;
                };
                let depth = (layers - 1 - a) as f32 + frame.sink;
                let view =
                    camera::layer_view(frame.camera, frame.view, frame.zoom, parallax.powf(depth));
                let [xs, ys] = [0, 1].map(|axis| {
                    let first = (view[0][axis] / edge).floor() as i32 + origin[axis];
                    let last = (view[1][axis] / edge).ceil() as i32 + origin[axis];
                    let (first, last) = (first.max(lo[axis]), last.min(hi[axis]));
                    first..last.max(first)
                });
                let (head, start) = (TABLE_HEAD * a, table.len() as u32);
                table[head..head + 5].copy_from_slice(&[
                    xs.start as u32,
                    ys.start as u32,
                    xs.len() as u32,
                    ys.len() as u32,
                    start,
                ]);
                for cy in ys {
                    for cx in xs.clone() {
                        let key = (a, cx, cy);
                        let slot = map
                            .chunk(key)
                            .filter(|_| !inside(cx, cy))
                            .and_then(|chunk| self.map_pages.slot(key, chunk.version));
                        table.push(slot.unwrap_or(NO_PAGE));
                    }
                }
            }
            for op in self.map_pages.take_ops() {
                match op {
                    MapOp::Grow(capacity) => {
                        let buffer = map_buffer(&self.device, capacity);
                        let mut encoder = self.device.create_command_encoder(&Default::default());
                        encoder.copy_buffer_to_buffer(
                            &self.map_tiles,
                            0,
                            &buffer,
                            0,
                            self.map_tiles.size(),
                        );
                        self.queue.submit([encoder.finish()]);
                        self.map_tiles = buffer;
                        self.composite_bind_group = None;
                    }
                    MapOp::Upload { slot, key } => {
                        let tiles = &map.chunk(key).unwrap().tiles[..];
                        self.queue.write_buffer(
                            &self.map_tiles,
                            slot as u64 * MAP_PAGE_BYTES,
                            bytemuck::cast_slice(tiles),
                        );
                    }
                }
            }
        }
        if self
            .map_table
            .write(&self.device, &self.queue, bytemuck::cast_slice(&table))
        {
            self.composite_bind_group = None;
        }
    }

    /// The transforms (each slot onto the view, smaller the deeper it is;
    /// each page drawn onto its page; the interface as is) and the
    /// composite's parameters, for `frame` as `plan` draws it.
    fn write_uniforms(&mut self, frame: &Frame, plan: &FramePlan) {
        // The camera on a whole pixel, so the tile art stays crisp.
        let zoom = frame.zoom;
        let cell = frame.cell as f32;
        let seen_zoom = cell / TILE;
        let page_px = CHUNK as u32 * frame.cell;
        let camera = frame.camera.map(|c| (c * zoom).round() / zoom);
        // Patterns pinned to the world go by where it is in the whole level.
        let offset = frame.offset.map(|t| t as f32 * TILE);
        let center = [frame.view[0] / 2.0, frame.view[1] / 2.0];
        let parallax = frame.parallax();
        for (lp, slot) in plan.layers.iter().filter_map(|lp| Some((lp, lp.slot?))) {
            let t = Transform {
                screen: frame.view,
                camera,
                center,
                scale: frame.scale(lp.layer.index),
                zoom,
                cell,
                parallax,
                offset,
            };
            let buffer = &self.layer_transforms[slot].buffer;
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
        let pages = plan.layers.iter().flat_map(|lp| &lp.pages);
        while self.page_transforms.len() < pages.clone().count() {
            let uniform = Uniform::new(
                &self.device,
                &self.transform_layout,
                &self.sheet,
                &self.sampler,
            );
            self.page_transforms.push(uniform);
        }
        for (d, uniform) in pages.zip(&self.page_transforms) {
            let t = flat([page_px as f32; 2], d.rect[0], seen_zoom);
            self.queue
                .write_buffer(&uniform.buffer, 0, bytemuck::bytes_of(&t));
        }
        self.queue.write_buffer(
            &self.ui_transform.buffer,
            0,
            bytemuck::bytes_of(&flat(frame.view, [0.0; 2], frame.ui_scale)),
        );

        let present = plan
            .layers
            .iter()
            .filter_map(|lp| lp.slot)
            .fold(0u32, |bits, slot| bits | 1 << slot);
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
            base: frame.base as u32,
            sight: frame.sight as u32,
            _pad: 0,
            _pad2: [0; 2],
            offset,
            fade: frame.fade,
            sink: frame.sink,
            world: frame.world,
        };
        self.queue
            .write_buffer(&self.composite_params, 0, bytemuck::bytes_of(&params));
    }

    /// Every pass of `frame` as `plan` has it, onto `screen`: each layer's,
    /// then the composite, then the overlay and the interface over it.
    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        plan: &FramePlan,
        screen: &wgpu::TextureView,
    ) {
        let mut onto = self.page_transforms.iter();
        let mut draws = Vec::new();
        for lp in &plan.layers {
            if let Some(slot) = lp.slot {
                self.encode_layer(encoder, frame, lp, slot, &mut draws);
            }
            self.encode_seen(encoder, lp, &mut onto);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(target(
                    screen,
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                ))],
                ..Default::default()
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, self.composite_bind_group.as_ref().unwrap(), &[]);
            pass.draw(0..3, 0..1);
        }
        // Overlay on the player's layer, if it's drawn, then the interface,
        // over it all.
        let canvas = &self.canvas_vertices.buffer;
        let shapes = &self.shape_pipeline;
        let sight = plan.layers.iter().find(|lp| lp.layer.index == frame.sight);
        if let Some(slot) = sight.and_then(|lp| lp.slot) {
            let onto = &self.layer_transforms[slot].bind_group;
            let overlay = [(canvas, plan.overlay.clone(), 0)];
            self.pass(encoder, kept(screen), onto, None, &[(shapes, &overlay)]);
        }
        let ui = [(canvas, plan.ui.clone(), 0)];
        self.pass(
            encoder,
            kept(screen),
            &self.ui_transform.bind_group,
            None,
            &[(shapes, &ui)],
        );
    }

    /// The passes of one layer drawn, in slot `s`: its canvases with the
    /// tiles under them and the props over them; where it's void; what's
    /// visible now, and on the player's layer, what lights cast. `draws` is
    /// scratch space.
    fn encode_layer<'a>(
        &'a self,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        lp: &LayerPlan,
        s: usize,
        draws: &mut Vec<Draw<'a>>,
    ) {
        let l = lp.layer;
        let view = frame.layer_view(l.index);
        let transform = &self.layer_transforms[s].bind_group;
        let (canvas, masks) = (&self.canvas_vertices.buffer, &self.mask_vertices.buffer);
        let screen = self.screen.as_ref().unwrap();
        let first = lp.first;
        // The draws of the layer's meshes, side by side in `draws`: the
        // tiles, then fringes and trims over them (the pieces', then those
        // along their edges), then the props over everything, then where
        // it's void and liquid.
        draws.clear();
        self.piece_draws(l, first, |p| &p.tiles, view, draws);
        let edging = draws.len();
        self.piece_draws(l, first, |p| &p.fringes, view, draws);
        self.mesh_draws(&l.meshes.fringes, view, 0, draws);
        self.piece_draws(l, first, |p| &p.trims, view, draws);
        self.mesh_draws(&l.meshes.trims, view, 0, draws);
        let props = draws.len();
        self.piece_draws(l, first, |p| &p.props, view, draws);
        let voids = draws.len();
        self.piece_draws(l, first, |p| &p.voids, view, draws);
        let liquid = draws.len();
        self.piece_draws(l, first, |p| &p.liquid, view, draws);
        let draws = &draws[..];
        let (under, over) = (&draws[..props], &draws[props..voids]);

        let shapes = &self.shape_pipeline;
        for (i, range) in lp.canvases.iter().enumerate() {
            let attachment = cleared(&screen.canvases.layers[2 * s + i]);
            let own = [(canvas, range.clone(), 0)];
            self.pass(
                encoder,
                attachment,
                transform,
                None,
                &[(shapes, under), (shapes, &own), (shapes, over)],
            );
        }
        let void = &screen.voids.layers[s];
        self.pass(
            encoder,
            cleared(void),
            transform,
            None,
            &[(&self.fill_pipeline, &draws[voids..liquid])],
        );
        // Liquid is void that tints what shows through: mark it half.
        // Fringes, trims and props cover the void and water they reach over;
        // and it's cleared again under whatever stands on the layer.
        let standing = [(masks, lp.standing.clone(), 0)];
        self.pass(
            encoder,
            kept(void),
            transform,
            None,
            &[
                (&self.water_pipeline, &draws[liquid..]),
                (&self.unmask_pipeline, &draws[edging..voids]),
                (&self.unmask_pipeline, &standing),
            ],
        );
        // What's visible now: the polygon (and lit solid), and the props
        // seen where their art is opaque.
        let visible = [(masks, lp.visible.clone(), 0)];
        let props_seen = [(masks, lp.props_seen.clone(), 0)];
        let now = screen.now_msaa[s].attachment(&screen.now.layers[s]);
        self.pass(
            encoder,
            now,
            transform,
            None,
            &[
                (&self.mask_pipeline, &visible),
                (&self.mask_cutout_pipeline, &props_seen),
            ],
        );
        // On the player's layer, what lights cast.
        let lights = [(masks, lp.lights.clone(), 0)];
        if l.index == frame.sight {
            let light = screen.light_msaa.attachment(&screen.light);
            self.pass(
                encoder,
                light,
                transform,
                None,
                &[(&self.light_pipeline, &lights)],
            );
        }
    }

    /// What's visible of a layer, into each page of `seen` it covers
    /// (through the next of `onto`).
    fn encode_seen<'a>(
        &'a self,
        encoder: &mut wgpu::CommandEncoder,
        lp: &LayerPlan,
        onto: &mut std::slice::Iter<'a, Uniform>,
    ) {
        let l = lp.layer;
        let masks = &self.mask_vertices.buffer;
        let seen = self.seen.as_ref().unwrap();
        let visible = [(masks, lp.visible.clone(), 0)];
        let props_seen = [(masks, lp.props_seen.clone(), 0)];
        let lights = [(masks, lp.lights.clone(), 0)];
        for d in &lp.pages {
            let onto = &onto.next().unwrap().bind_group;
            let page = &seen.atlas.layers[d.slot as usize];
            match &l.seen_through {
                // Below the player: remembered as seen only where it shows
                // through to them.
                Some(shaft) => {
                    let mut stencil = Vec::new();
                    self.mesh_draws(shaft, d.rect, 0, &mut stencil);
                    self.through(encoder, page, onto, &stencil, &visible);
                }
                // The player's layer: what's in sight within the radius, what
                // lights show wherever it's in sight, and the props seen.
                None => {
                    self.pass(
                        encoder,
                        kept(page),
                        onto,
                        None,
                        &[(&self.seen_near_pipeline, &visible)],
                    );
                    if !lp.lights.is_empty() {
                        self.through(encoder, page, onto, &visible, &lights);
                    }
                    self.pass(
                        encoder,
                        kept(page),
                        onto,
                        None,
                        &[(&self.seen_cutout_pipeline, &props_seen)],
                    );
                }
            }
        }
    }

    /// Draw `draws` into page `page` of `seen` (through `onto`) only where
    /// `stencil` covers it, drawn first into the page-sized scratch texture.
    fn through(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        page: &wgpu::TextureView,
        onto: &wgpu::BindGroup,
        stencil: &[Draw],
        draws: &[Draw],
    ) {
        let seen = self.seen.as_ref().unwrap();
        self.pass(
            encoder,
            cleared(&seen.shaft),
            onto,
            None,
            &[(&self.fill_pipeline, stencil)],
        );
        let group = Some(&seen.shaft_bind_group);
        self.pass(
            encoder,
            kept(page),
            onto,
            group,
            &[(&self.seen_shaft_pipeline, draws)],
        );
    }

    /// One pass into `attachment` through `transform` (and `extra`, as bind
    /// group 1), drawing each group of draws with its pipeline: each range
    /// of a vertex buffer moved by its shift. A pass that would only keep
    /// what's there is left out.
    fn pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        attachment: wgpu::RenderPassColorAttachment,
        transform: &wgpu::BindGroup,
        extra: Option<&wgpu::BindGroup>,
        groups: &[(&wgpu::RenderPipeline, &[Draw])],
    ) {
        let drawn = |d: &&Draw| !d.1.is_empty();
        let keeps = matches!(attachment.ops.load, wgpu::LoadOp::Load);
        if keeps
            && !groups
                .iter()
                .any(|(_, draws)| draws.iter().any(|d| drawn(&d)))
        {
            return;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pass"),
            color_attachments: &[Some(attachment)],
            ..Default::default()
        });
        pass.set_bind_group(0, transform, &[]);
        if let Some(group) = extra {
            pass.set_bind_group(1, group, &[]);
        }
        pass.set_vertex_buffer(1, self.shifts.buffer.slice(..));
        for (pipeline, draws) in groups {
            let mut ready = false;
            for (buffer, range, shift) in draws.iter().filter(drawn) {
                if !ready {
                    pass.set_pipeline(pipeline);
                    ready = true;
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(range.clone(), *shift..*shift + 1);
            }
        }
    }

    /// Onto `out`, mesh `m`'s chunks that reach into `view`, moved by shift
    /// `shift` (`view` is where they are before that): as few draws as there
    /// are runs of them side by side in its buffer.
    fn mesh_draws<'a>(&'a self, m: &Mesh, view: Rect, shift: u32, out: &mut Vec<Draw<'a>>) {
        let Some(Kept {
            buffer: Some(buffer),
            ..
        }) = self.meshes.get(&m.id())
        else {
            return;
        };
        let mut run: Option<Range<u32>> = None;
        for r in m.ranges(view) {
            match &mut run {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => {
                    if let Some(done) = run.replace(r) {
                        out.push((buffer, done, shift));
                    }
                }
            }
        }
        if let Some(done) = run {
            out.push((buffer, done, shift));
        }
    }

    /// Onto `out`, of each piece of layer `l` (its first shift `first`),
    /// mesh `of`, where it reaches into `view`.
    fn piece_draws<'a>(
        &'a self,
        l: &LayerFrame,
        first: u32,
        of: fn(&MeshPiece) -> &Mesh,
        view: Rect,
        out: &mut Vec<Draw<'a>>,
    ) {
        for (i, piece) in l.meshes.pieces.iter().enumerate() {
            let c = l.meshes.corner(i);
            let local = view.map(|p| [p[0] - c[0], p[1] - c[1]]);
            self.mesh_draws(of(piece), local, first + i as u32, out);
        }
    }
}
