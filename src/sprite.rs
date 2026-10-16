//! The renderer's per-instance data, kept free of GPU types so game code
//! can build it on any target.

use crate::palette::Color;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Sprite {
    pos: [f32; 2],
    size: [f32; 2],
    color: Color,
}

impl Sprite {
    pub fn rect(pos: [f32; 2], size: [f32; 2], color: Color) -> Self {
        Self { pos, size, color }
    }
}
