// Triangles in world space, drawn through a transform (see `Globals`),
// each showing the tile sheet times its color. `fs_color` draws them into
// a canvas; `fs_mask` fills them with 1.0 into a single-channel mask.
//
// A fringe (see `goblin_nightclub_core::fringe`) is tile art past the edge of its
// tile: its cell repeats, and it's cut raggedly, pixel by pixel of the
// art, out to a distance from its edge that varies with a noise pinned to
// the world, so the cut is the same in every pass and doesn't crawl as the
// view moves. Its outermost pixels have its color blended over them.
//
// Compiled after common.wgsl (see `crate::shaders`).

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
    // How much smaller each layer is drawn than the one above it: so how
    // much larger anything up from its layer is (see `z` in `vs_main`).
    parallax: f32,
    // Where world point (0, 0) is in the whole level (for a level drawn a
    // window at a time), so patterns pinned to the world stay put.
    offset: vec2<f32>,
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
    // World position.
    @location(3) world: vec2<f32>,
    // For a fringe: a point on its edge and the edge's outward normal (or
    // a zero normal to measure from the point), and how far it may reach;
    // 0 for anything else.
    @location(4) @interpolate(flat) edge: vec4<f32>,
    @location(5) @interpolate(flat) reach: f32,
};

@vertex
fn vs_main(
    @location(0) world: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) cell: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) edge: vec4<f32>,
    @location(5) reach: f32,
    // Where the draw moves its vertices to: a chunk's piece of a layer
    // from its chunk's corner to where that is (see `Renderer::shifts`).
    @location(6) shift: vec2<f32>,
    // How far up from its layer it is, in layers (a prop's art): drawn
    // that much larger, as layers further down are drawn smaller.
    @location(7) z: f32,
) -> VsOut {
    let at = world + shift;
    let scale = globals.scale * pow(1.0 / globals.parallax, z);
    // World -> target pixels (camera and zoom, then scale about the
    // center) -> clip space (origin top-left, y down).
    let px = globals.center + ((at - globals.camera) * globals.zoom - globals.center) * scale;
    let ndc = vec2(px.x / globals.screen.x * 2.0 - 1.0, 1.0 - px.y / globals.screen.y * 2.0);
    var out: VsOut;
    out.clip = vec4(ndc, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    out.cell = cell;
    out.world = at;
    out.edge = vec4(edge.xy + shift, edge.zw);
    out.reach = reach;
    return out;
}

// Size of the bumps along a fringe's ragged edge, in pixels of the art.
const FRINGE_GRAIN: f32 = 3.0;

// Does a fringe from `edge`, reaching at most `reach`, cover world point
// `p`? Out to a fraction of `reach` that varies along the edge: decided
// where `p` is level with on it, so it covers solidly in from its ragged
// end, and strips along the same line, in neighboring tiles, line up. From
// a point, the fraction is the point's: a strip's at its end.
fn fringe_covers(p: vec2<f32>, edge: vec4<f32>, reach: f32) -> bool {
    var d = distance(p, edge.xy);
    var foot = edge.xy;
    if dot(edge.zw, edge.zw) > 0.0 {
        d = dot(p - edge.xy, edge.zw);
        foot = p - edge.zw * d;
    }
    let q = (foot + globals.offset) * globals.cell / TILE / FRINGE_GRAIN;
    let n = 0.65 * value_noise(q) + 0.35 * value_noise(q * 2.3 + 17.0);
    // Spread out, as the noise mostly stays near its middle: often all
    // the way, sometimes none of it.
    return d <= reach * clamp(n * 2.2 - 0.55, 0.0, 1.0);
}

// A fringe at world point `world`: 0 where it's cut, 1 where it shows, 2
// where it shows and the pixel of the art beyond it doesn't (its rim).
// Whole pixels of the art: decided at each one's center.
fn fringe_state(world: vec2<f32>, edge: vec4<f32>, reach: f32) -> u32 {
    let art = globals.cell / TILE;
    let p = (floor(world * art) + 0.5) / art;
    if !fringe_covers(p, edge, reach) {
        return 0u;
    }
    var out = edge.zw;
    if dot(out, out) == 0.0 {
        out = normalize(p - edge.xy + vec2(1e-6));
    }
    if fringe_covers(p + round(out) / art, edge, reach) {
        return 1u;
    }
    return 2u;
}

// `uv` past the sides of its cell moved back in: the cell repeating.
fn repeat(uv: vec2<f32>, cell: vec2<f32>) -> vec2<f32> {
    let d = uv - cell;
    return cell + d - floor(d / globals.cell) * globals.cell;
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
// With `repeats`, the cell repeats past its sides rather than clamping.
fn sharp(uv: vec2<f32>, cell: vec2<f32>, repeats: bool) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(sheet));
    let edge = floor(uv + 0.5);
    let per_px = max(fwidth(uv), vec2(1e-4));
    var p = edge + clamp((uv - edge) / per_px, vec2(-0.5), vec2(0.5));
    if repeats {
        p = repeat(p, cell);
    }
    p = clamp(p, cell + 0.5, cell + globals.cell - 0.5);
    return textureSampleLevel(sheet, filtered, p / size, 0.0);
}

@fragment
fn fs_color(in: VsOut) -> @location(0) vec4<f32> {
    let fringe = in.reach > 0.0;
    let c = sharp(in.uv, in.cell, fringe);
    if !fringe {
        return c * in.color;
    }
    let state = fringe_state(in.world, in.edge, in.reach);
    if state == 0u {
        discard;
    }
    if state == 2u {
        return vec4(mix(c.rgb, in.color.rgb, in.color.a), c.a);
    }
    return c;
}

@fragment
fn fs_mask() -> @location(0) vec4<f32> {
    return vec4(1.0);
}

// Marks water in the void mask: half, where void is 1.
@fragment
fn fs_mask_half() -> @location(0) vec4<f32> {
    return vec4(0.5);
}

// Clears a mask where a sprite (or a fringe) is opaque: where something
// stands on a layer, or covers its edge, it isn't void.
@fragment
fn fs_unmask(in: VsOut) -> @location(0) vec4<f32> {
    var uv = in.uv;
    if in.reach > 0.0 {
        if fringe_state(in.world, in.edge, in.reach) == 0u {
            discard;
        }
        uv = repeat(uv, in.cell);
    }
    if texel(uv).a < 0.5 {
        discard;
    }
    return vec4(0.0);
}

// Like `fs_mask`, but only where a sprite is opaque: the props seen, shown
// whole where their art is, not just as far as the polygon reaches.
@fragment
fn fs_mask_cutout(in: VsOut) -> @location(0) vec4<f32> {
    if texel(in.uv).a < 0.5 {
        discard;
    }
    return vec4(1.0);
}

// Like `fs_mask`, but only within `edge.z` of `edge.xy`: what's seen within
// the view radius, remembered as seen (beyond it, only what lights show is).
@fragment
fn fs_mask_near(in: VsOut) -> @location(0) vec4<f32> {
    if distance(in.world, in.edge.xy) > in.edge.z {
        discard;
    }
    return vec4(1.0);
}

// What a light (see `LIGHT_HEART` in common.wgsl), its center and radius
// in `edge` and its color in `color`,
// casts at `world`: its tinted brightness (added up where lights meet),
// and how much what it lights shows (the most of any).
@fragment
fn fs_light(in: VsOut) -> @location(0) vec4<f32> {
    let d = distance(in.world, in.edge.xy);
    let r = in.edge.z;
    let t = clamp(d / r, 0.0, 1.0);
    let bright = mix(LIGHT_RIM, LIGHT_HEART, (1.0 - t) * (1.0 - t));
    let tint = mix(vec3(1.0), in.color.rgb, LIGHT_WARMTH);
    let shown = 1.0 - smoothstep(r - FADE, r, d);
    return vec4(tint * bright * shown, shown);
}

// Like `fs_mask`, but only where `shaft` is set: a layer below the viewer's
// may only be remembered as seen where it shows through, and what lights
// show only where it's in line of sight.
@group(1) @binding(0) var shaft: texture_2d<f32>;

@fragment
fn fs_mask_shaft(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    if textureLoad(shaft, vec2<i32>(frag.xy), 0).r < 0.5 {
        discard;
    }
    return vec4(1.0);
}
