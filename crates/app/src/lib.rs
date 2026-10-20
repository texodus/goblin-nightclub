//! The browser entry point: JS owns the frame loop, input events and the
//! HTML interface and forwards them to [`app::App`], which runs a game or
//! the editor and draws it with the WebGPU [`renderer`]. Both are
//! compiled only for `wasm32`; everything else lives in the engine and
//! mode crates, which are tested natively.

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
mod pages;

#[cfg(test)]
mod shader_tests {
    use wgpu::naga;

    /// The renderer's shaders parse and validate: the browser checks them
    /// only at runtime, and the renderer is built only for wasm32.
    #[test]
    fn shaders_are_valid() {
        let shaders = [
            ("shapes", include_str!("shaders/shapes.wgsl")),
            ("composite", include_str!("shaders/composite.wgsl")),
        ];
        for (name, source) in shaders {
            let module = naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::empty(),
            )
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(source)));
        }
    }
}
