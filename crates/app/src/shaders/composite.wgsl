// Combines the three visibility layers into the final image. Each pixel is
// styled by which layer applies there, according to the masks:
//   now  = 1 where currently visible
//   seen = 1 where ever visible
// Masks are anti-aliased, so styles blend smoothly across their edges.

// Distances are in world units; the view is in pixels, `zoom` to a world
// unit, and `seen` in pixels of the art, `seen_zoom` to a world unit.
struct Params {
    // Viewer position.
    origin: vec2<f32>,
    // View radius; 0 for unlimited.
    radius: f32,
    // Game time in seconds.
    time: f32,
    // The viewer's layer.
    top: u32,
    // Bit per slot (layers down from the viewer's): drawn this frame.
    present: u32,
    // Scale of each layer relative to the one above (parallax).
    scale: f32,
    // How much less far each layer down is seen, and the least it is.
    radius_step: f32,
    // World position of the view's top-left.
    camera: vec2<f32>,
    radius_min: f32,
    zoom: f32,
    seen_zoom: f32,
    // 1 when the viewer is under the open sky: the mist over lower layers
    // is light instead of dark.
    outdoors: f32,
    // The ground level: looking down from outdoors, the mist over layers at
    // or above it is light, over those below dark.
    ground: u32,
    // Layers below `ground` are seen this fraction as far.
    radius_underground: f32,
    // Edge of a page of `seen`, in its pixels.
    page_px: f32,
    _pad: f32,
    // Where world point (0, 0) is in the whole level (for a level drawn a
    // window at a time), so patterns pinned to the world stay put.
    offset: vec2<f32>,
};

@group(0) @binding(0) var<uniform> params: Params;
// Each layer drawn goes in a slot: its depth below the viewer's, `s` (0
// for the viewer's own).
// Per slot `s`, in view space (already moved and scaled for parallax):
// visible, remembered and unknown canvases at 3s, 3s + 1, 3s + 2; the
// `now` mask; and where it's void, showing the layer below.
@group(0) @binding(1) var canvases: texture_2d_array<f32>;
@group(0) @binding(2) var mask_now: texture_2d_array<f32>;
// Everywhere ever seen, in world space at `seen_zoom` pixels a unit, cut
// into pages `page_px` pixels square; each page in its own layer of the
// array, if anything on it has been seen.
@group(0) @binding(3) var mask_seen: texture_2d_array<f32>;
@group(0) @binding(4) var voids: texture_2d_array<f32>;
// Where each slot's pages in view are: per slot `s`, at 8s, the page
// column and row (as i32) of the top-left of a window of pages, its
// columns and rows, and where its entries start in this same array; each
// entry, row by row, the page's layer in `mask_seen`, or NO_PAGE.
@group(0) @binding(5) var<storage, read> pages: array<u32>;

// A page nothing on which has been seen.
const NO_PAGE: u32 = 0xffffffffu;

// Width (world units) of the band inside the radius where the visible style
// fades into the remembered one.
const RADIUS_FADE: f32 = 12.0;

// Anywhere never seen, and beyond the level's edge: a dark gray fog in the
// mist's billows, drifting with it, under whatever the unknown canvas
// holds. Its average gray, and how much it varies either way (a fraction
// of that) after its billows' contrast is stretched (see `mist_density`):
// at most about 0.13, below the remembered style's dithered grays
// (REMEMBERED_MIN and up), so what's known stands out against it.
const UNKNOWN_GRAY: f32 = 0.2;
const UNKNOWN_VARIATION: f32 = 0.3;
const UNKNOWN_CONTRAST: f32 = 1.0;

// Lowest gray (before dithering) for anything not black in the remembered
// style. At 0.15 a dot needs a threshold of at least 0.55, which every row
// and column of the 4x4 pattern has.
const REMEMBERED_MIN: f32 = 0.15;

// Between each layer and the one below it, seen down through its holes,
// hangs a sheet of translucent mist, drawn with parallax halfway between
// the two layers' (so it drifts against both as the view moves). Looking
// down from outdoors into a layer above the ground it's light haze;
// anywhere else (indoors, or into the ground) it's dark. Each layer down is
// seen through one more sheet, so the deeper the fainter.
const MIST_LIGHT: vec3<f32> = vec3(0.80, 0.85, 0.92);
const MIST_DARK: vec3<f32> = vec3(0.04, 0.03, 0.09);
// Average opacity of a sheet of each.
const MIST_LIGHT_ALPHA: f32 = 0.28;
const MIST_DARK_ALPHA: f32 = 0.45;
// How much the opacity varies across a sheet of each, either way, as a
// fraction of the average, and how much the billows' contrast is
// stretched first (the noise mostly stays near its middle). Dark mist
// over dark ground needs much more of both to show.
const MIST_LIGHT_VARIATION: f32 = 0.6;
const MIST_LIGHT_CONTRAST: f32 = 1.0;
const MIST_DARK_VARIATION: f32 = 0.95;
const MIST_DARK_CONTRAST: f32 = 1.0;
// Size of the mist's billows, in world units (a tile is 10).
const MIST_SCALE: f32 = 60.0;
// Drift, in world units a second of play.
const MIST_DRIFT: vec2<f32> = vec2(2.5, 1.0);
// Opacity steps, for the banded look of 16-bit translucency.
const MIST_STEPS: f32 = 6.0;

fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// Smooth value noise in [0, 1], a lattice point every unit.
fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2(1.0, 0.0));
    let c = hash(i + vec2(0.0, 1.0));
    let d = hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Opacity of a mist sheet at world point `world` (on the sheet), out of 1 at the sheet's average opacity, varying by `variation`
// either way after stretching the billows by `contrast`. In whole pixels
// of the art, like the tiles.
// Every sheet has the same pattern, drifting together: stacked down a deep
// hole, drawn at each sheet's own parallax, its copies line up at the
// view's center and part toward the edges, which shows the depth.
fn mist_density(world: vec2<f32>, variation: f32, contrast: f32) -> f32 {
    let p = floor((world + params.offset) * params.seen_zoom) / params.seen_zoom;
    var q = (p + MIST_DRIFT * params.time) / MIST_SCALE;
    // Warped a little, and each octave turned, so no grid shows.
    q += 0.6 * vec2(value_noise(q * 0.8 + 3.1), value_noise(q * 0.8 + 7.9)) - 0.3;
    let turn = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    let n = 0.5 * value_noise(q) + 0.3 * value_noise(turn * q * 2.1 + 5.2)
        + 0.2 * value_noise(turn * turn * q * 4.3 + 1.7);
    let stretched = clamp((2.0 * n - 1.0) * contrast, -1.0, 1.0);
    let v = 1.0 + variation * stretched;
    return round(v * MIST_STEPS) / MIST_STEPS;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    // One triangle covering the whole screen: (-1,-1) (3,-1) (-1,3).
    let p = vec2(f32((vi << 1u) & 2u), f32(vi & 2u));
    return vec4(p * 2.0 - 1.0, 0.0, 1.0);
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3(0.299, 0.587, 0.114));
}

// 4x4 ordered-dither threshold in (0, 1).
fn bayer(p: vec2<u32>) -> f32 {
    var m = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    return (m[(p.y % 4u) * 4u + p.x % 4u] + 0.5) / 16.0;
}

// View radius `d` layers below the viewer's (0 for unlimited).
// View radius on layer `a`, `d` layers below the viewer's (0 for
// unlimited): a step less per layer, and a fraction of that below ground.
fn radius_at(a: u32, d: u32) -> f32 {
    if params.radius <= 0.0 {
        return 0.0;
    }
    var r = max(params.radius - params.radius_step * f32(d), params.radius_min);
    if a < params.ground {
        r = max(r * params.radius_underground, params.radius_min);
    }
    return r;
}

// Full color, dimming slightly toward the edge of a finite view `radius`.
fn style_visible(c: vec3<f32>, dist: f32, radius: f32) -> vec3<f32> {
    if radius <= 0.0 {
        return c;
    }
    return c * mix(1.0, 0.75, smoothstep(0.0, radius, dist));
}

// Dimmed grayscale, dithered down to the four EGA grays (0, 1/3, 2/3, 1).
// The dither pattern is anchored to `p`, a pixel of the art (so its dots
// are as chunky as the art's at any zoom), and moves with the content
// instead of crawling over it as the camera pans. Faint detail
// (like the floor grid) is lifted to a minimum gray: enough that every row
// and column of the 4x4 pattern gets a dot, so thin lines in any direction
// always show.
fn style_remembered(c: vec3<f32>, p: vec2<u32>) -> vec3<f32> {
    var lum = luminance(c) * 0.75;
    if lum > 0.0 {
        lum = max(lum, REMEMBERED_MIN);
    }
    let level = clamp(floor(lum * 3.0 + bayer(p)), 0.0, 3.0) / 3.0;
    return vec3(level);
}

// The fog at world point `world`, under `c` (premultiplied).
fn style_unknown(c: vec4<f32>, world: vec2<f32>) -> vec3<f32> {
    let density = mist_density(world, UNKNOWN_VARIATION, UNKNOWN_CONTRAST);
    let fog = vec3(UNKNOWN_GRAY * density);
    return c.rgb + fog * (1.0 - c.a);
}

// Slot `s`'s `seen` at world point `world`: 0 on a page nothing on which
// has been seen, and outside its window (and so outside the level).
fn seen_at(s: u32, world: vec2<f32>) -> f32 {
    let at = world * params.seen_zoom;
    let page = vec2<i32>(floor(at / params.page_px));
    let window = 8u * s;
    let corner = vec2(bitcast<i32>(pages[window]), bitcast<i32>(pages[window + 1u]));
    let size = vec2(pages[window + 2u], pages[window + 3u]);
    let rel = page - corner;
    if any(rel < vec2(0)) || any(vec2<u32>(rel) >= size) {
        return 0.0;
    }
    let layer = pages[pages[window + 4u] + u32(rel.y) * size.x + u32(rel.x)];
    if layer == NO_PAGE {
        return 0.0;
    }
    let inside = vec2<i32>(at - vec2<f32>(page) * params.page_px);
    return textureLoad(mask_seen, inside, layer, 0).r;
}

// How much slot `s` at view pixel `px` and world point `world` is visible
// now, and has been seen, with view `radius` and `through` as for `shade`.
fn knowledge(s: u32, px: vec2<i32>, world: vec2<f32>, radius: f32, through: f32) -> vec2<f32> {
    var now = textureLoad(mask_now, px, s, 0).r;
    if radius > 0.0 {
        let dist = distance(world, params.origin);
        now *= 1.0 - smoothstep(radius - RADIUS_FADE, radius, dist);
    }
    return vec2(now, max(seen_at(s, world), through));
}

// Slot `s` at view pixel `px` and world point `world`, styled by its
// masks, with view `radius` (0 for unlimited). `through` is how much the
// hole this pixel looks down has been seen: a lower layer is remembered
// where it has been seen itself, or where the hole showing it has (the
// hole's sides, which parallax brings into view beside it, are seen with
// the hole, but lie under the layers above, so never in its own `seen`).
// The canvases hold premultiplied color (drawn over transparent black), so
// the result is too; water is the translucent case.
fn shade(s: u32, px: vec2<i32>, world: vec2<f32>, radius: f32, through: f32) -> vec3<f32> {
    let dist = distance(world, params.origin);
    let known = knowledge(s, px, world, radius, through);
    let now = known.x;
    let seen = known.y;
    let visible = style_visible(textureLoad(canvases, px, 3u * s, 0).rgb, dist, radius);
    // Pixel of the art, wrapped to the dither pattern (it can be negative
    // beyond the level's edge on lower layers).
    let dots = vec2<i32>(floor(world * params.seen_zoom));
    let cell = vec2<u32>(((dots % 4) + 4) % 4);
    let remembered = style_remembered(textureLoad(canvases, px, 3u * s + 1u, 0).rgb, cell);
    let unknown = style_unknown(textureLoad(canvases, px, 3u * s + 2u, 0), world);
    return mix(mix(unknown, remembered, seen), visible, now);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let center = vec2<f32>(textureDimensions(voids)) / 2.0;
    // Down from the viewer's layer through void; each layer was drawn a
    // little smaller about the view's center (parallax).
    var scale = 1.0;
    // How much the hole looked down has been seen: the least `seen` of the
    // void passed through, on the way down.
    var through = 1.0;
    // Front to back: the color gathered so far (premultiplied), and how
    // much of what's beneath still shows through the water and mist above.
    var color = vec3(0.0);
    var remaining = 1.0;
    // What a hole through every layer drawn shows at its bottom: bedrock,
    // black where the hole is known, else the unknown style (as the last
    // layer passed through has it).
    var bedrock = vec3(0.0);
    for (var d = 0u; d <= params.top; d++) {
        let a = params.top - d;
        if ((params.present >> d) & 1u) == 0u {
            break;
        }
        let world = params.camera + (center + (frag.xy - center) / scale) / params.zoom;
        let mask = textureLoad(voids, px, d, 0).r;
        let below = select(0.0, through, d > 0u);
        let radius = radius_at(a, d);
        if mask <= 0.25 {
            color += shade(d, px, world, radius, below) * remaining;
            return vec4(color, 1.0);
        }
        let known = knowledge(d, px, world, radius, below);
        let unknown = (1.0 - known.x) * (1.0 - known.y);
        bedrock = style_unknown(textureLoad(canvases, px, 3u * d + 2u, 0), world) * unknown;
        if mask < 0.75 {
            // Water: its styled (premultiplied) color over what's below.
            // Where it's never been seen, the unknown style is opaque, so
            // nothing beneath shows through that part.
            let alpha = textureLoad(canvases, px, 3u * d, 0).a;
            color += shade(d, px, world, radius, below) * remaining;
            remaining *= (1.0 - alpha) * (1.0 - unknown);
        }
        through = min(through, seen_at(d, world));
        // The mist down this hole, onto the next layer: there only where
        // the hole is known, in the remembered style's gray where it isn't
        // in view.
        if ((params.present >> (d + 1u)) & 1u) != 0u {
            let sheet = scale * sqrt(params.scale);
            let at = params.camera + (center + (frag.xy - center) / sheet) / params.zoom;
            let light = params.outdoors > 0.5 && a - 1u >= params.ground;
            let tint = select(MIST_DARK, MIST_LIGHT, light);
            let opacity = select(MIST_DARK_ALPHA, MIST_LIGHT_ALPHA, light);
            let gray = vec3(luminance(tint) * 0.75);
            let variation = select(MIST_DARK_VARIATION, MIST_LIGHT_VARIATION, light);
            let contrast = select(MIST_DARK_CONTRAST, MIST_LIGHT_CONTRAST, light);
            let density = mist_density(at, variation, contrast);
            let alpha = clamp(opacity * density, 0.0, 1.0) * max(known.x, known.y);
            color += mix(gray, tint, known.x) * alpha * remaining;
            remaining *= 1.0 - alpha;
        }
        scale *= params.scale;
    }
    return vec4(color + bedrock * remaining, 1.0);
}
