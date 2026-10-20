//! Everything the renderer needs for one frame. Built by the game, so it
//! contains no GPU types.

use std::sync::Arc;

use crate::camera::{self, Rect};
use crate::draw::Canvas;
use crate::layer::TILE;
use crate::level::LayerMeshes;
use crate::mesh::Mesh;

/// One layer of the level as seen this frame.
pub struct LayerFrame {
    /// Which layer (0 is the bottom).
    pub index: usize,
    /// What's drawn of the layer whatever's seen: its tiles, then the
    /// fringes where materials meet and the trims along their edges, drawn
    /// under both `visible` and `remembered` (wherever fringes and trims
    /// show, the layer isn't void); and where it's void, and liquid.
    pub meshes: Arc<LayerMeshes>,
    /// Drawn over `tiles` where the viewer can currently see.
    pub visible: Canvas,
    /// Drawn over `tiles` where the viewer has seen before but can't now.
    pub remembered: Canvas,
    /// Drawn where the viewer has never seen.
    pub unknown: Canvas,
    /// Boundary of the visible region around `origin`, by angle; drawn as a
    /// triangle fan.
    pub polygon: Vec<[f32; 2]>,
    /// Convex pieces of solid lit along with the polygon (which stops at
    /// wall faces): the band one tile behind the visible stretches of wall.
    pub lit_shapes: Vec<Vec<[f32; 2]>>,
    /// Sprites of things standing on this layer: wherever they're opaque,
    /// the layer is drawn over any void beneath them rather than letting
    /// the layer below show through.
    pub standing: Canvas,
    /// For a layer below the viewer's, where it shows through; only there
    /// may it be remembered as seen. `None` for the viewer's own layer.
    pub shaft: Option<Arc<Mesh>>,
}

pub struct Frame {
    /// Layers in the level.
    pub depth: usize,
    /// The viewer's layer.
    pub top: usize,
    /// The viewer's layer and each one below it that shows through, top
    /// first.
    pub layers: Vec<LayerFrame>,
    pub origin: [f32; 2],
    /// View radius on the viewer's layer, in world units; `f32::INFINITY` for
    /// unlimited.
    pub radius: f32,
    /// How much less far each layer below the viewer's is seen, and the
    /// least any is.
    pub radius_step: f32,
    pub radius_min: f32,
    /// Layers below `ground` are seen this fraction as far (1 for no
    /// change), applied after the step per layer.
    pub radius_underground: f32,
    /// Game time in seconds, for animated styles.
    pub time: f32,
    /// Top-left of the view, in world units.
    pub camera: [f32; 2],
    /// Size of the view, in screen pixels.
    pub view: [f32; 2],
    /// Screen pixels per world unit (the art is at its own size at
    /// `cell / TILE`, see [`crate::tileset::Tileset::zoom`]).
    pub zoom: f32,
    /// Edge of a cell of the tile sheet, in pixels.
    pub cell: u32,
    /// Screen pixels per interface pixel, for `ui`.
    pub ui_scale: f32,
    /// Is the viewer under the open sky? Then the mist over layers below is
    /// light instead of dark.
    pub outdoors: bool,
    /// The ground level: the lowest layer under the open sky. Looking down
    /// from outdoors, the mist over layers at or above it is light, over
    /// those below dark.
    pub ground: usize,
    /// Size of the level, in world units: of the window onto it, for a
    /// level played a window at a time (see [`crate::world`]), whose
    /// coordinates are all the window's.
    pub world: [f32; 2],
    /// Where the window is in the whole level: its top-left tile, and the
    /// layer its layer 0 is. What's kept across windows (what's been seen)
    /// and patterns pinned to the world go by these.
    pub offset: [i32; 2],
    pub base: usize,
    /// Whether what's visible is remembered as seen; the editor shows
    /// everything and remembers none of it.
    pub remember: bool,
    /// Drawn in world space over everything, on the viewer's layer.
    pub overlay: Canvas,
    /// Drawn over everything in interface pixels (`ui_scale` screen
    /// pixels each): the interface.
    pub ui: Canvas,
}

impl Frame {
    /// How much smaller each layer below the viewer's is drawn than the
    /// one above it (see [`camera::parallax`]).
    pub fn parallax(&self) -> f32 {
        camera::parallax(self.zoom, self.cell as f32 / TILE * self.ui_scale)
    }

    /// The part of layer `index` in view, padded a little.
    pub fn layer_view(&self, index: usize) -> Rect {
        let scale = self.parallax().powi((self.top - index) as i32);
        camera::layer_view(self.camera, self.view, self.zoom, scale)
    }
}
