//! Everything the renderer needs for one frame. Built by the game, so it
//! contains no GPU types.

use crate::draw::Layer;

pub struct Frame {
    /// Drawn where the viewer can currently see.
    pub visible: Layer,
    /// Drawn where the viewer has seen before but can't now.
    pub remembered: Layer,
    /// Drawn where the viewer has never seen.
    pub unknown: Layer,
    /// Boundary of the visible region around `origin`, by angle; drawn as a
    /// triangle fan.
    pub polygon: Vec<[f32; 2]>,
    /// Convex solid shapes lit along with the polygon: those with a visible
    /// face, which the polygon itself stops short of.
    pub lit_shapes: Vec<Vec<[f32; 2]>>,
    pub origin: [f32; 2],
    /// View radius in pixels; `f32::INFINITY` for unlimited.
    pub radius: f32,
    /// Game time in seconds, for animated styles.
    pub time: f32,
}
