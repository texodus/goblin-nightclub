// Triangles in world space, drawn through a transform (see `Globals`),
// each showing the tile sheet times its color. `fs_color` draws them into
// a canvas; `fs_mask` fills them with 1.0 into a single-channel mask.

struct Globals {
    // Size of the target in pixels.
    screen: vec2<f32>,
    // World position of the target's top-left.
    camera: vec2<f32>,
    // Point (in target pixels) scaled about, and the scale (parallax).
    center: vec2<f32>,
    scale: f32,
    // Target pixels per world unit.
    zoom: f32,
    // Edge of a cell of the sheet, in pixels.
    cell: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var sheet: texture_2d<f32>;
@group(0) @binding(2) var filtered: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Position in the sheet, in pixels.
    @location(1) uv: vec2<f32>,
    // Top-left of the cell `uv` is in, in pixels.
    @location(2) @interpolate(flat) cell: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) world: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cell: vec2<f32>,
    @location(3) color: vec4<f32>,
) -> VsOut {
    // World -> target pixels (camera and zoom, then scale about the
    // center) -> clip space (origin top-left, y down).
    let px = globals.center + ((world - globals.camera) * globals.zoom - globals.center) * globals.scale;
    let ndc = vec2(px.x / globals.screen.x * 2.0 - 1.0, 1.0 - px.y / globals.screen.y * 2.0);
    var out: VsOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    out.cell = cell;
    return out;
}

// The sheet's pixel at `uv`, nearest, clamped to the sheet.
fn texel(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(sheet));
    let p = clamp(vec2<i32>(floor(uv)), vec2(0), size - 1);
    return textureLoad(sheet, p, 0);
}

// The sheet at `uv`, sharp at any scale: each pixel of the art a solid
// square, with only the screen pixels straddling an edge between two of
// them blended, by how much of each they cover. So the art keeps even,
// crisp pixels at zooms that aren't whole numbers, and doesn't shimmer as
// it moves. Clamped inside the cell, so neighbors in the sheet don't bleed
// in at a tile's edge.
fn sharp(uv: vec2<f32>, cell: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(sheet));
    let edge = floor(uv + 0.5);
    let per_px = max(fwidth(uv), vec2(1e-4));
    var p = edge + clamp((uv - edge) / per_px, vec2(-0.5), vec2(0.5));
    p = clamp(p, cell + 0.5, cell + globals.cell - 0.5);
    return textureSampleLevel(sheet, filtered, p / size, 0.0);
}

@fragment
fn fs_color(in: VsOut) -> @location(0) vec4<f32> {
    return sharp(in.uv, in.cell) * in.color;
}

@fragment
fn fs_mask() -> @location(0) vec4<f32> {
    return vec4(1.0);
}

// Clears a mask where a sprite is opaque: where something stands on a
// layer, it isn't void.
@fragment
fn fs_unmask(in: VsOut) -> @location(0) vec4<f32> {
    if texel(in.uv).a < 0.5 {
        discard;
    }
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
