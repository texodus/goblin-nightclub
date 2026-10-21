//! The renderer's shaders, put together: each after what both share
//! (`shaders/common.wgsl`), and before that the constants they need from
//! Rust, written from Rust's own; and the uniform structs they read, laid
//! out as theirs are (checked by the tests). Built natively too, so the
//! tests can.

use goblin_nightclub_core::layer::TILE;

/// Marks a page in the page table nothing on which has been seen.
pub const NO_PAGE: u32 = u32::MAX;

/// Words per slot at the head of the page table (see `pages` in
/// composite.wgsl).
pub const TABLE_HEAD: usize = 8;

/// `Globals` in shapes.wgsl: how world positions land on a target.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Transform {
    pub screen: [f32; 2],
    pub camera: [f32; 2],
    pub center: [f32; 2],
    pub scale: f32,
    pub zoom: f32,
    pub cell: f32,
    /// How much smaller each layer is drawn than the one above it: so how
    /// much larger anything up from its layer (see `Vertex::z`) is.
    pub parallax: f32,
    /// Where world point (0, 0) is in the whole level.
    pub offset: [f32; 2],
}

/// `Params` in composite.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CompositeParams {
    pub origin: [f32; 2],
    /// 0 for unlimited.
    pub radius: f32,
    pub time: f32,
    pub top: u32,
    /// Bit per layer drawn this frame.
    pub present: u32,
    pub scale: f32,
    pub radius_step: f32,
    pub camera: [f32; 2],
    pub radius_min: f32,
    pub zoom: f32,
    pub seen_zoom: f32,
    /// 1 when the viewer is outdoors (light mist over lower layers, not
    /// dark).
    pub outdoors: f32,
    /// The ground level: the mist over layers below it is dark even
    /// outdoors.
    pub ground: u32,
    /// Layers below `ground` are seen this fraction as far.
    pub radius_underground: f32,
    /// Edge of a page of `seen`, in its pixels.
    pub page_px: f32,
    pub _pad: f32,
    /// Where world point (0, 0) is in the whole level.
    pub offset: [f32; 2],
    /// See [`goblin_nightclub_core::frame::Frame::fade`] and `sink`.
    pub fade: f32,
    pub sink: f32,
    pub _pad2: [f32; 2],
}

/// shapes.wgsl, ready to compile.
pub fn shapes() -> String {
    with_common(include_str!("shaders/shapes.wgsl"))
}

/// composite.wgsl, ready to compile.
pub fn composite() -> String {
    with_common(include_str!("shaders/composite.wgsl"))
}

/// `body` after common.wgsl and Rust's constants.
fn with_common(body: &str) -> String {
    format!(
        "// From Rust (see `crate::shaders`).\n\
         const TILE: f32 = {TILE:?};\n\
         const NO_PAGE: u32 = {NO_PAGE}u;\n\
         const TABLE_HEAD: u32 = {TABLE_HEAD}u;\n\n\
         {}\n{body}",
        include_str!("shaders/common.wgsl")
    )
}
