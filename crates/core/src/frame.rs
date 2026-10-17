//! Everything the renderer needs for one frame. Built by the game, so it
//! contains no GPU types.

use crate::draw::Canvas;

/// One layer of the level as seen this frame.
pub struct LayerFrame {
    /// Which layer (0 is the bottom).
    pub index: usize,
    /// Drawn where the viewer can currently see.
    pub visible: Canvas,
    /// Drawn where the viewer has seen before but can't now.
    pub remembered: Canvas,
    /// Drawn where the viewer has never seen.
    pub unknown: Canvas,
    /// Boundary of the visible region around `origin`, by angle; drawn as a
    /// triangle fan.
    pub polygon: Vec<[f32; 2]>,
    /// Convex pieces of solid lit along with the polygon (which stops at
    /// wall faces): the band one tile behind the visible stretches of wall.
    pub lit_shapes: Vec<Vec<[f32; 2]>>,
    /// Where this layer is void, as convex polygons: where the layer below
    /// shows through.
    pub voids: Vec<Vec<[f32; 2]>>,
    /// The part of `voids` that is water: drawn as a translucent tint over
    /// the layer below instead of simply showing it.
    pub water: Vec<Vec<[f32; 2]>>,
    /// Sprites of things standing on this layer: wherever they're opaque,
    /// the layer is drawn over any void beneath them rather than letting
    /// the layer below show through.
    pub standing: Canvas,
    /// For a layer below the viewer's, where it shows through (as convex
    /// polygons); only there may it be remembered as seen. `None` for the
    /// viewer's own layer.
    pub shaft: Option<Vec<Vec<[f32; 2]>>>,
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
    /// [`crate::tileset::ZOOM`]).
    pub zoom: f32,
    /// Screen pixels per interface pixel, for `ui`.
    pub ui_scale: f32,
    /// Is the viewer under the open sky? Then layers below fade into fog
    /// instead of darkening.
    pub outdoors: bool,
    /// The ground level: the lowest layer under the open sky. Looking down
    /// from outdoors, layers at or above it fog; those below darken.
    pub ground: usize,
    /// Size of the level, in world units.
    pub world: [f32; 2],
    /// Drawn in world space over everything, on the viewer's layer.
    pub overlay: Canvas,
    /// Drawn over everything in interface pixels (`ui_scale` screen
    /// pixels each): the interface.
    pub ui: Canvas,
}
