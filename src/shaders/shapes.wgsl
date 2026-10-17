// Colored triangles in world space, drawn through a transform (see
// `Globals`). `fs_color` draws them into a canvas; `fs_mask` fills them
// with 1.0 into a single-channel mask.

struct Globals {
    // Size of the target in pixels.
    screen: vec2<f32>,
    // World position of the target's top-left.
    camera: vec2<f32>,
    // Point (in target pixels) scaled about, and the scale (parallax).
    center: vec2<f32>,
    scale: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) world: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    // World -> target pixels (camera, then scale about the center) ->
    // clip space (origin top-left, y down).
    let px = globals.center + (world - globals.camera - globals.center) * globals.scale;
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

// Clears a mask: where something stands on a layer, it isn't void.
@fragment
fn fs_unmask() -> @location(0) vec4<f32> {
    return vec4(0.0);
}

// Like `fs_mask`, but only where `shaft` is set: a layer below the viewer's
// may only be remembered as seen where it shows through.
@group(1) @binding(0) var shaft: texture_2d<f32>;

@fragment
fn fs_mask_shaft(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    if textureLoad(shaft, vec2<i32>(frag.xy), 0).r < 0.5 {
        discard;
    }
    return vec4(1.0);
}
