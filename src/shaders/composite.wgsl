// Combines the three visibility layers into the final image. Each pixel is
// styled by which layer applies there, according to the masks:
//   now  = 1 where currently visible
//   seen = 1 where ever visible
// Masks are anti-aliased, so styles blend smoothly across their edges.

struct Params {
    // Viewer position in pixels.
    origin: vec2<f32>,
    // View radius in pixels; 0 for unlimited.
    radius: f32,
    // Game time in seconds.
    time: f32,
    // The viewer's layer.
    top: u32,
    // Bit per layer: drawn this frame.
    present: u32,
    // Scale of each layer relative to the one above (parallax).
    scale: f32,
    // How much less far each layer down is seen, and the least it is.
    radius_step: f32,
    // World position of the view's top-left.
    camera: vec2<f32>,
    radius_min: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> params: Params;
// Per layer `a`, in view space (already moved and scaled for parallax):
// visible, remembered and unknown canvases at 3a, 3a + 1, 3a + 2; the
// `now` mask; and where it's void, showing the layer below.
@group(0) @binding(1) var canvases: texture_2d_array<f32>;
@group(0) @binding(2) var mask_now: texture_2d_array<f32>;
// Per layer, in world space: everywhere ever seen.
@group(0) @binding(3) var mask_seen: texture_2d_array<f32>;
@group(0) @binding(4) var voids: texture_2d_array<f32>;

// Width (pixels) of the band inside the radius where the visible style
// fades into the remembered one.
const RADIUS_FADE: f32 = 12.0;

// Lowest gray (before dithering) for anything not black in the remembered
// style. At 0.15 a dot needs a threshold of at least 0.55, which every row
// and column of the 4x4 pattern has.
const REMEMBERED_MIN: f32 = 0.15;

// Brightness of each layer relative to the one above it.
const LOWER_BRIGHTNESS: f32 = 0.55;

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
fn radius_at(d: u32) -> f32 {
    if params.radius <= 0.0 {
        return 0.0;
    }
    return max(params.radius - params.radius_step * f32(d), params.radius_min);
}

// Full color, dimming slightly toward the edge of a finite view `radius`.
fn style_visible(c: vec3<f32>, dist: f32, radius: f32) -> vec3<f32> {
    if radius <= 0.0 {
        return c;
    }
    return c * mix(1.0, 0.75, smoothstep(0.0, radius, dist));
}

// Dimmed grayscale, dithered down to the four EGA grays (0, 1/3, 2/3, 1).
// The dither pattern is anchored to world pixel `p`, so it moves with the
// content instead of crawling over it as the camera pans. Faint detail
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

fn style_unknown(c: vec3<f32>) -> vec3<f32> {
    return c;
}

// Layer `a` at view pixel `px`, world point `world`, styled by its masks,
// with view `radius` (0 for unlimited).
fn shade(a: u32, px: vec2<i32>, world: vec2<f32>, radius: f32) -> vec3<f32> {
    let dist = distance(world, params.origin);
    var now = textureLoad(mask_now, px, a, 0).r;
    let size = vec2<f32>(textureDimensions(mask_seen));
    var seen = 0.0;
    if all(world >= vec2(0.0)) && all(world < size) {
        seen = textureLoad(mask_seen, vec2<i32>(world), a, 0).r;
    }
    if radius > 0.0 {
        now *= 1.0 - smoothstep(radius - RADIUS_FADE, radius, dist);
    }
    let visible = style_visible(textureLoad(canvases, px, 3u * a, 0).rgb, dist, radius);
    // World pixel, wrapped to the dither pattern (it can be negative beyond
    // the level's edge on lower layers).
    let cell = vec2<u32>(((vec2<i32>(floor(world)) % 4) + 4) % 4);
    let remembered = style_remembered(textureLoad(canvases, px, 3u * a + 1u, 0).rgb, cell);
    let unknown = style_unknown(textureLoad(canvases, px, 3u * a + 2u, 0).rgb);
    return mix(mix(unknown, remembered, seen), visible, now);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let center = vec2<f32>(textureDimensions(voids)) / 2.0;
    // Down from the viewer's layer through void; each layer was drawn a
    // little smaller about the view's center (parallax).
    var scale = 1.0;
    for (var d = 0u; d <= params.top; d++) {
        let a = params.top - d;
        if ((params.present >> a) & 1u) == 0u {
            break;
        }
        if textureLoad(voids, px, a, 0).r > 0.5 {
            scale *= params.scale;
            continue;
        }
        let world = params.camera + center + (frag.xy - center) / scale;
        let dim = pow(LOWER_BRIGHTNESS, f32(d));
        return vec4(shade(a, px, world, radius_at(d)) * dim, 1.0);
    }
    return vec4(0.0, 0.0, 0.0, 1.0);
}
