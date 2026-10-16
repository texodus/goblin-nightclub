//! Everything the renderer needs for one frame. Built by the game, so it
//! contains no GPU types.

use crate::sprite::Sprite;

pub struct Frame {
    /// Drawn where the viewer can currently see.
    pub visible: Vec<Sprite>,
    /// Drawn where the viewer has seen before but can't now.
    pub remembered: Vec<Sprite>,
    /// Drawn where the viewer has never seen.
    pub unknown: Vec<Sprite>,
    /// Boundary of the visible region around `origin`, by angle; drawn as a
    /// triangle fan.
    pub polygon: Vec<[f32; 2]>,
    /// Solid rects `(pos, size)` lit along with the polygon: blocks with a
    /// visible face, which the polygon itself stops short of.
    pub lit_rects: Vec<([f32; 2], [f32; 2])>,
    pub origin: [f32; 2],
    /// View radius in pixels; `f32::INFINITY` for unlimited.
    pub radius: f32,
    /// Game time in seconds, for animated styles.
    pub time: f32,
}
