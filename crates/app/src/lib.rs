//! The browser entry point: JS owns the frame loop, input events and the
//! HTML interface and forwards them to [`app::App`], which runs a game or
//! the editor and draws it with the WebGPU [`renderer`]. Both are
//! compiled only for `wasm32`; everything else lives in the engine and
//! mode crates, which are tested natively.

#[cfg(target_arch = "wasm32")]
mod app;
#[cfg(target_arch = "wasm32")]
mod renderer;
