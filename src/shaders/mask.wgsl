// Fills triangles (in pixel space) with 1.0 into a single-channel mask.

struct Globals {
    // Logical screen size in pixels.
    screen: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

@vertex
fn vs_main(@location(0) px: vec2<f32>) -> @builtin(position) vec4<f32> {
    // Pixel space (origin top-left, y down) -> clip space.
    let ndc = vec2(px.x / globals.screen.x * 2.0 - 1.0, 1.0 - px.y / globals.screen.y * 2.0);
    return vec4(ndc, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4(1.0);
}
