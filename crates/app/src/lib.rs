//! The browser entry point: JS owns the frame loop, input events and the
//! HTML interface and forwards them to `app::App`, which runs a game or
//! the editor and draws it with the WebGPU `renderer`. Both are compiled
//! only for `wasm32`; everything else lives in the engine and mode crates,
//! which are tested natively, as are the renderer's [`shaders`] and its
//! page policies.

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod generated;
#[cfg(target_arch = "wasm32")]
mod http;
#[cfg(target_arch = "wasm32")]
mod renderer;
// Used only by the renderer, but tested natively.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod map_pages;
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod pages;
pub mod shaders;
