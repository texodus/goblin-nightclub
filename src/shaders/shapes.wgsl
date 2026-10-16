// Colored triangles in pixel space. `fs_color` draws them into a layer;
// `fs_mask` fills them with 1.0 into a single-channel visibility mask.

struct Globals {
    // Logical screen size in pixels.
    screen: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) px: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    // Pixel space (origin top-left, y down) -> clip space.
    let ndc = vec2(px.x / globals.screen.x * 2.0 - 1.0, 1.0 - px.y / globals.screen.y * 2.0);
    var out: VsOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_color(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}

@fragment
fn fs_mask() -> @location(0) vec4<f32> {
    return vec4(1.0);
}
