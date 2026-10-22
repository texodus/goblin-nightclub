//                                    ▗◡≝▔▔` ▔≝=◺▂             ∕        ▐▋  ▝▉
//                                  ╣▒◿▒≘≉◬_    _▒▆▒▀         ◞         ▐▋  ▐▌
//                                  ╔`     ▝◺◿▒▟▘╔╹ ▚◛                  ▐▌  ▐▏
//       G O B L I N               ▕▎_"▃▅≣P ▐▓▛▗▉▂_▂__◚◛_         __▂▂▂▂▟▙◓▀▀▀▀≝
//        N I G H T C L U B         ║`║║_▂▐╝ ▟◙██▛╬▔`_▂◙▒▖       ▕∏▃▃▃▃▂▐▌║▂▂▂▔▔
//                                  ▙⋡▜╥∺╝ ▗▟███▛Ƒ  ▐▛▔▔▜█▌      ▐╣ ▝▜███▋ ∭▔▔▔▔
//                                   ▁⋡▚▃▄▀▉Ƣ█▀▉╺▗▐∏▊   ╠█▎      ▐╣   ▁∭█▉ ▊ ∭▐█
//                          ▁▃◛◭◒≠◚▔▔╸ ╔▒∏▕▓▌ ▟▎ ▐▒██▖    ▜▎     ╠▎≕∏__  ◙∏▍ ╣█▌
//                     _◿▎`▂_ ▟▒║╣▂▂▂▂▞╜▔▜ ╬◙▏∏▎ ▐███◘⊠◚"` ▜     ╣ ▂▅▅▅_ ▐█▎▕▒█Ǩ
//              _▂_ _▂▂▒∭▒▒◛≝▔           ▕▶╣◚▔▔▔╠∏∏║▌ ┉━━≝▒▃▘   ▕▌▟█████▙ ∭ ▐▐▌▓
//    ▁▁▁▁_╓◛▋▝◠▬Ǟ◡▃╠◚≝≊■■■▆▆▤▀◚⊜⊛╥▄▃▃▂_║╳▞▗╝▟▛◚███████████▌    ▕▒████◚◚█▌▋ ∭▟▎╱
//   ▐▔▔"⋨∭◅▃╏◠≉╬▒▇▆▅┫ ▕▄▃∏◠━━◡▃▂║▃▃▃▄▙▄∭▟▟▙▓≝▬▬╩◘██████████    ▐∏██◘▜▁╔▕▒▎ ▒█▎╣
//   ▕╣◚◘▉▐▄▃▂▁▟▒╝◙▉◙Ů▒▒◚▜▛◙◘▒▒▌▍ ▔▔"▀▬ǡ∗▃∑◚▀▀◘◘▓████▀╣▟╢▒◙◙▙▖  ▐ ╬     ∭▐▏▐█▉▏▜
//    ĴĄ  ╲╲ ▔╠▛◘◘▓▍▅╣┇▐▒∭Ǭ▛╨◀∗▃▒▀Ň◘█▒⋯┳▂▃_▐█████████████████▌∑◚╣__%◺_⊋∛ ∭ ▐▐▉▌▝
//  ▔  ┗▌   ▔◚▜▍▝▀▀▜█▌▐▐▐██▙▄▃▂_ ▗╢▒▛◚▬∗▃▂◚▀╩╩╩◘◘◙╩◘Ů▔▔▔▔▔▔▔▔▔▝▜▒▒▒▒▒◘◘▣▤▌ ∭█▌▕▎
//  Copyright 2026, [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

// Combines the three visibility layers into the final image. Each pixel is
// styled by which layer applies there, according to the masks:
//   now  = 1 where currently visible
//   seen = 1 where ever visible
// Masks are anti-aliased, so styles blend smoothly across their edges.
// Beyond the window of a level played a window at a time, there's only the
// map: the tiles explored, in the remembered style.
//
// Compiled after common.wgsl (see `crate::shaders`).

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
    // The layer of the whole level the window's layer 0 is.
    base: u32,
    // Where world point (0, 0) is in the whole level (for a level drawn a
    // window at a time), so patterns pinned to the world stay put.
    offset: vec2<f32>,
    // Going from one layer to another: how opaque the viewer's layer is
    // (showing the one beneath through the rest), and how many layers
    // deeper than it is each is drawn (so how much smaller).
    fade: f32,
    sink: f32,
    // Size of the window (all of the level, unless it's played a window at
    // a time): beyond it, the map is shown.
    world: vec2<f32>,
    // The layer sight is from, the player's: what's visible is on it and
    // below it, as far as `radius` reaches there, and lit by `light_map`.
    // The viewer's (`top`), unless the view's been moved off it.
    sight: u32,
    _pad: u32,
    _pad2: vec2<u32>,
};

@group(0) @binding(0) var<uniform> params: Params;
// Each layer drawn goes in a slot: its depth below the viewer's, `s` (0
// for the viewer's own).
// Per slot `s`, in view space (already moved and scaled for parallax):
// visible and remembered canvases at 2s and 2s + 1; the
// `now` mask; and where it's void, showing the layer below.
@group(0) @binding(1) var canvases: texture_2d_array<f32>;
@group(0) @binding(2) var mask_now: texture_2d_array<f32>;
// Everywhere ever seen, in world space at `seen_zoom` pixels a unit, cut
// into pages `page_px` pixels square; each page in its own layer of the
// array, if anything on it has been seen.
@group(0) @binding(3) var mask_seen: texture_2d_array<f32>;
@group(0) @binding(4) var voids: texture_2d_array<f32>;
// Where each slot's pages in view are: per slot `s`, at `TABLE_HEAD * s`, the page
// column and row (as i32) of the top-left of a window of pages, its
// columns and rows, and where its entries start in this same array; each
// entry, row by row, the page's layer in `mask_seen`, or NO_PAGE.
@group(0) @binding(5) var<storage, read> pages: array<u32>;
// On the player's layer, in view space like `now`, what lights cast: their
// colors, added up, as bright as they are there (rgb), and how much what
// they light shows (a), wherever it's in line of sight.
@group(0) @binding(6) var light_map: texture_2d<f32>;
// The map (see `goblin_nightclub_core::map`). What each of its codes is
// drawn as: per code, two words: where its cells start in this same array
// (the low 16 bits) and how many there are (the high), then how opaque it
// is (f32 bits); after the codes', the cells, each its column and row in
// the sheet (16 bits each).
@group(0) @binding(7) var<storage, read> map_art: array<u32>;
// Where each layer's chunks in view are, laid out as `pages` is, but per
// layer of the whole level from the bottom, a chunk a page, and each entry
// the chunk's slot in `map_tiles` (or NO_PAGE).
@group(0) @binding(8) var<storage, read> map_table: array<u32>;
// Per slot, a chunk's tiles, row by row, a word each: its quarters' codes,
// a byte each, in the order of `QUARTERS` (N, E, S, W); 0 for unexplored.
@group(0) @binding(9) var<storage, read> map_tiles: array<u32>;
// The tile sheet, which the map's art is from.
@group(0) @binding(10) var sheet: texture_2d<f32>;

// Away from the open sky, the viewer carries a light: brightest close by,
// dimmer toward the edge of sight, its color partly cast (as a light's:
// see `LIGHT_HEART` in common.wgsl). However many lights meet, nothing's
// lit brighter than `LIGHT_MOST`.
const OWN_LIGHT: vec3<f32> = vec3(1.0, 0.82, 0.6);
const LIGHT_MOST: f32 = 1.3;

// Anywhere never seen, and beyond the level's edge: a dark gray fog in the
// mist's billows, drifting with it. Its average gray, and how much it varies either way (a fraction
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

// View radius on layer `a` (0 for unlimited): a step less per layer below
// the one sight is from, and a fraction of that below ground. (Above it,
// nothing's in sight.)
fn radius_at(a: u32) -> f32 {
    if params.radius <= 0.0 {
        return 0.0;
    }
    let below = f32(max(params.sight, a) - a);
    var r = max(params.radius - params.radius_step * below, params.radius_min);
    if a < params.ground {
        r = max(r * params.radius_underground, params.radius_min);
    }
    return r;
}

// Full color `c`, `dist` from the viewer with view `radius` (0 for
// unlimited), lit: outdoors by daylight, dimming slightly toward the edge of
// sight; anywhere else by the viewer's own light, warm and dimming more, and
// by `lights` (what lights cast there, on the viewer's layer).
fn style_visible(c: vec3<f32>, lights: vec3<f32>, dist: f32, radius: f32) -> vec3<f32> {
    if radius <= 0.0 {
        return c;
    }
    if params.outdoors > 0.5 {
        return c * mix(1.0, 0.75, smoothstep(0.0, radius, dist));
    }
    let t = clamp(dist / radius, 0.0, 1.0);
    let own = mix(LIGHT_RIM, LIGHT_HEART, (1.0 - t) * (1.0 - t))
        * (1.0 - smoothstep(radius - FADE, radius, dist));
    let light = mix(vec3(1.0), OWN_LIGHT, LIGHT_WARMTH) * own + lights;
    return c * min(light, vec3(LIGHT_MOST));
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

// The fog at world point `world`.
fn style_unknown(world: vec2<f32>) -> vec3<f32> {
    return vec3(UNKNOWN_GRAY * mist_density(world, UNKNOWN_VARIATION, UNKNOWN_CONTRAST));
}

// Slot `s`'s `seen` at world point `world`: 0 on a page nothing on which
// has been seen, and outside its window (and so outside the level).
fn seen_at(s: u32, world: vec2<f32>) -> f32 {
    let at = world * params.seen_zoom;
    let page = vec2<i32>(floor(at / params.page_px));
    let window = TABLE_HEAD * s;
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

// A spot on the map: its quarter's code and how opaque that is (0 for
// void), its tile in the whole level, and where in the tile it is (0 to 1
// across and down).
struct Spot {
    code: u32,
    alpha: f32,
    tile: vec2<i32>,
    uv: vec2<f32>,
};

// The map at world point `world` (in the window's coordinates, as
// everything else is) of layer `a` of the whole level: code 0 where it's
// not been explored, or isn't on the GPU.
fn map_at(a: u32, world: vec2<f32>) -> Spot {
    let at = world / TILE;
    let local = floor(at);
    let tile = vec2<i32>(local) + vec2<i32>(round(params.offset / TILE));
    var spot = Spot(0u, 1.0, tile, at - local);
    let chunk = vec2<i32>(floor(vec2<f32>(tile) / f32(CHUNK)));
    let head = TABLE_HEAD * a;
    let corner = vec2(bitcast<i32>(map_table[head]), bitcast<i32>(map_table[head + 1u]));
    let size = vec2(map_table[head + 2u], map_table[head + 3u]);
    let rel = chunk - corner;
    if any(rel < vec2(0)) || any(vec2<u32>(rel) >= size) {
        return spot;
    }
    let slot = map_table[map_table[head + 4u] + u32(rel.y) * size.x + u32(rel.x)];
    if slot == NO_PAGE {
        return spot;
    }
    let inside = vec2<u32>(tile - chunk * i32(CHUNK));
    let word = map_tiles[(slot * CHUNK + inside.y) * CHUNK + inside.x];
    // The quarter it's in: the triangle between the tile's diagonals.
    let uv = spot.uv;
    var q = 1u;
    if uv.y < min(uv.x, 1.0 - uv.x) {
        q = 0u;
    } else if uv.y > max(uv.x, 1.0 - uv.x) {
        q = 2u;
    } else if uv.x < 0.5 {
        q = 3u;
    }
    spot.code = (word >> (8u * q)) & 0xffu;
    spot.alpha = bitcast<f32>(map_art[2u * spot.code + 1u]);
    return spot;
}

// Of `count` cells, the one for tile `tile`: as `tileset::variant` picks.
fn variant(count: u32, tile: vec2<i32>) -> u32 {
    var h = (u32(tile.x) * 0x9E3779B1u) ^ (u32(tile.y) * 0x85EBCA77u);
    h ^= h >> 15u;
    h *= 0x2C1B3C6Du;
    h ^= h >> 12u;
    return h % count;
}

// `spot`'s art, premultiplied (as the canvases are): transparent for void.
fn map_color(spot: Spot) -> vec4<f32> {
    let entry = map_art[2u * spot.code];
    let count = entry >> 16u;
    if count == 0u {
        return vec4(0.0);
    }
    let cell = map_art[(entry & 0xffffu) + variant(count, spot.tile)];
    let px = u32(params.page_px) / CHUNK;
    let inside = min(vec2<u32>(spot.uv * f32(px)), vec2(px - 1u));
    let c = textureLoad(sheet, vec2(cell & 0xffffu, cell >> 16u) * px + inside, 0);
    let alpha = c.a * spot.alpha;
    return vec4(c.rgb * alpha, alpha);
}

// `spot`, at world point `world`, styled: remembered if it's been
// explored, else unknown.
fn shade_map(spot: Spot, world: vec2<f32>) -> vec3<f32> {
    if spot.code == 0u {
        return style_unknown(world);
    }
    let dots = vec2<i32>(floor(world * params.seen_zoom));
    let cell = vec2<u32>(((dots % 4) + 4) % 4);
    return style_remembered(map_color(spot).rgb, cell);
}

// The sheet of mist hanging under a layer drawn `scale` times its size,
// down a hole onto the layer below, at view pixel `xy` (`center` the
// view's): `light` haze or dark, `known` as the hole is, and counting only
// `weight` (see `fs_main`). Its color (multiply by the opacity) and its
// opacity.
fn mist(xy: vec2<f32>, center: vec2<f32>, scale: f32, light: bool, known: Known, weight: f32) -> vec4<f32> {
    let sheet = scale * sqrt(params.scale);
    let at = params.camera + (center + (xy - center) / sheet) / params.zoom;
    let tint = select(MIST_DARK, MIST_LIGHT, light);
    let opacity = select(MIST_DARK_ALPHA, MIST_LIGHT_ALPHA, light);
    let gray = vec3(luminance(tint) * 0.75);
    let variation = select(MIST_DARK_VARIATION, MIST_LIGHT_VARIATION, light);
    let contrast = select(MIST_DARK_CONTRAST, MIST_LIGHT_CONTRAST, light);
    let density = mist_density(at, variation, contrast);
    let alpha = clamp(opacity * density, 0.0, 1.0) * max(known.now, known.seen) * weight;
    return vec4(mix(gray, tint, known.now), alpha);
}

// What's known of a layer at a pixel: how much it's visible now, how much
// it's been seen (or the hole showing it has), and how much it's been seen
// itself.
struct Known {
    now: f32,
    seen: f32,
    own: f32,
};

// What's known of slot `s` at world point `world`, with view `radius` (0
// for unlimited), `shown` (how much lights show it, past the radius) and
// `through` as for `shade`. In line of sight, it's visible within the
// radius, and beyond it where lights show it.
fn knowledge(s: u32, px: vec2<i32>, world: vec2<f32>, radius: f32, shown: f32, through: f32) -> Known {
    var now = textureLoad(mask_now, px, s, 0).r;
    if radius > 0.0 {
        let dist = distance(world, params.origin);
        now *= max(1.0 - smoothstep(radius - FADE, radius, dist), shown);
    }
    let own = seen_at(s, world);
    return Known(now, max(own, through), own);
}

// Slot `s` at view pixel `px` and world point `world`, styled by what's
// `known` of it, with view `radius` (0 for unlimited) and `lights` as for
// `style_visible`. A lower layer is remembered where it has been seen
// itself, or where the hole showing it has (`through` in `knowledge`: the
// hole's sides, which parallax brings into view beside it, are seen with the
// hole, but lie under the layers above, so never in its own `seen`). The
// canvases hold premultiplied color (drawn over transparent black), so the
// result is too; water is the translucent case. Only the styles that show
// are worked out.
fn shade(s: u32, px: vec2<i32>, world: vec2<f32>, radius: f32, known: Known, lights: vec3<f32>) -> vec3<f32> {
    var color = vec3(0.0);
    if known.now > 0.0 {
        let dist = distance(world, params.origin);
        color = style_visible(textureLoad(canvases, px, 2u * s, 0).rgb, lights, dist, radius);
        if known.now >= 1.0 {
            return color;
        }
    }
    var past = vec3(0.0);
    if known.seen > 0.0 {
        // Pixel of the art, wrapped to the dither pattern (it can be
        // negative beyond the level's edge on lower layers).
        let dots = vec2<i32>(floor(world * params.seen_zoom));
        let cell = vec2<u32>(((dots % 4) + 4) % 4);
        past = style_remembered(textureLoad(canvases, px, 2u * s + 1u, 0).rgb, cell);
    }
    if known.seen < 1.0 {
        past = mix(style_unknown(world), past, known.seen);
    }
    return mix(past, color, known.now);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let center = vec2<f32>(textureDimensions(voids)) / 2.0;
    // Down from the viewer's layer through void; each layer was drawn a
    // little smaller about the view's center (parallax).
    var scale = pow(params.scale, params.sink);
    // How much the hole looked down has been seen: the least `seen` of the
    // void passed through, on the way down.
    var through = 1.0;
    // Front to back: the color gathered so far (premultiplied), and how
    // much of what's beneath still shows through the water and mist above.
    var color = vec3(0.0);
    var remaining = 1.0;
    // What a hole through every layer drawn shows at its bottom: bedrock,
    // black where the hole is known, else the unknown style (as the last
    // layer passed through has it, where it is on that layer).
    var unknown = 0.0;
    var bottom = vec2(0.0);
    // Down to the bottom of the whole level, which only the map may show.
    let deepest = params.base + params.top;
    for (var d = 0u; d <= deepest; d++) {
        let world = params.camera + (center + (frag.xy - center) / scale) / params.zoom;
        // The viewer's layer, fading in or out, counts only so much; the
        // layer beneath shows through the rest of it, solid or not.
        let weight = select(1.0, params.fade, d == 0u);
        if any(world < vec2(0.0)) || any(world >= params.world) {
            // Beyond the window, the map: never in view.
            let a = deepest - d;
            let spot = map_at(a, world);
            let next = a > 0u;
            if spot.alpha >= 1.0 {
                color += shade_map(spot, world) * remaining * weight;
                if weight >= 1.0 || !next {
                    return vec4(color, 1.0);
                }
                remaining *= 1.0 - weight;
                through = 0.0;
                scale *= params.scale;
                continue;
            }
            unknown = 0.0;
            bottom = world;
            if spot.alpha > 0.0 {
                color += shade_map(spot, world) * remaining * weight;
                remaining *= 1.0 - weight * map_color(spot).a;
            }
            if next {
                let base = i32(params.base);
                let light = params.outdoors > 0.5 && i32(a) - 1 - base >= i32(params.ground);
                let m = mist(frag.xy, center, scale, light, Known(0.0, 1.0, 1.0), weight);
                color += m.rgb * m.a * remaining;
                remaining *= 1.0 - m.a;
            }
            scale *= params.scale;
            continue;
        }
        if d > params.top || ((params.present >> d) & 1u) == 0u {
            break;
        }
        let a = params.top - d;
        let mask = textureLoad(voids, px, d, 0).r;
        let below = select(0.0, through, d > 0u);
        let radius = radius_at(a);
        // What lights cast, on the player's layer.
        var lights = vec4(0.0);
        if a == params.sight {
            lights = textureLoad(light_map, px, 0);
        }
        let known = knowledge(d, px, world, radius, lights.a, below);
        let next = ((params.present >> (d + 1u)) & 1u) != 0u;
        if mask <= 0.25 {
            color += shade(d, px, world, radius, known, lights.rgb) * remaining * weight;
            if weight >= 1.0 || !next {
                return vec4(color, 1.0);
            }
            // Under its floor, the layer beneath is as known as it is.
            remaining *= 1.0 - weight;
            through = 0.0;
            scale *= params.scale;
            continue;
        }
        unknown = (1.0 - known.now) * (1.0 - known.seen);
        bottom = world;
        if mask < 0.75 {
            // Water: its styled (premultiplied) color over what's below.
            // Where it's never been seen, the unknown style is opaque, so
            // nothing beneath shows through that part.
            let alpha = textureLoad(canvases, px, 2u * d, 0).a;
            color += shade(d, px, world, radius, known, lights.rgb) * remaining * weight;
            remaining *= 1.0 - weight * (1.0 - (1.0 - alpha) * (1.0 - unknown));
        }
        through = min(through, known.own);
        // The mist down this hole, onto the next layer: there only where
        // the hole is known, in the remembered style's gray where it isn't
        // in view.
        if next {
            let light = params.outdoors > 0.5 && a - 1u >= params.ground;
            let m = mist(frag.xy, center, scale, light, known, weight);
            color += m.rgb * m.a * remaining;
            remaining *= 1.0 - m.a;
        }
        scale *= params.scale;
    }
    if unknown > 0.0 && remaining > 0.0 {
        color += style_unknown(bottom) * unknown * remaining;
    }
    return vec4(color, 1.0);
}
