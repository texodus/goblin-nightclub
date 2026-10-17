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
    /// This layer's void tiles, as rects `(pos, size)`: where the layer
    /// below shows through.
    pub voids: Vec<([f32; 2], [f32; 2])>,
    /// Rects of things standing on this layer, drawn over any void beneath
    /// them rather than letting the layer below show through.
    pub standing: Vec<([f32; 2], [f32; 2])>,
    /// For a layer below the viewer's, the tiles where it shows through
    /// (as rects); only those may be remembered as seen. `None` for the
    /// viewer's own layer.
    pub shaft: Option<Vec<([f32; 2], [f32; 2])>>,
}

pub struct Frame {
    /// The viewer's layer.
    pub top: usize,
    /// The viewer's layer and each one below it that shows through, top
    /// first.
    pub layers: Vec<LayerFrame>,
    pub origin: [f32; 2],
    /// View radius on the viewer's layer, in pixels; `f32::INFINITY` for
    /// unlimited.
    pub radius: f32,
    /// How much less far each layer below the viewer's is seen, and the
    /// least any is.
    pub radius_step: f32,
    pub radius_min: f32,
    /// Game time in seconds, for animated styles.
    pub time: f32,
    /// Top-left of the view, in world pixels.
    pub camera: [f32; 2],
    /// Size of the view, in pixels.
    pub view: [f32; 2],
    /// Size of the level, in pixels.
    pub world: [f32; 2],
    /// Drawn in world space over everything, on the viewer's layer.
    pub overlay: Canvas,
    /// Drawn in view space over everything: the interface.
    pub ui: Canvas,
}
