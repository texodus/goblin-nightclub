//! A 2D WebAssembly + WebGPU game. JS owns the frame loop and input events
//! and forwards them to `app::App`; Rust owns game state and rendering.
//!
//! Browser/GPU code is compiled only for `wasm32`, so game logic can be unit
//! tested natively with `cargo test`.

#[cfg(target_arch = "wasm32")]
mod app;
pub mod draw;
pub mod entity;
pub mod frame;
pub mod game;
pub mod generate;
pub mod layer;
pub mod level;
pub mod memory;
pub mod palette;
pub mod path;
#[cfg(target_arch = "wasm32")]
mod renderer;
pub mod rng;
pub mod vision;

/// Logical (internal) resolution. The canvas is upscaled with CSS
/// `image-rendering: pixelated`, so every game pixel stays crisp.
pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 200;
