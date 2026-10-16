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
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var visible_layer: texture_2d<f32>;
@group(0) @binding(2) var remembered_layer: texture_2d<f32>;
@group(0) @binding(3) var unknown_layer: texture_2d<f32>;
@group(0) @binding(4) var mask_now: texture_2d<f32>;
@group(0) @binding(5) var mask_seen: texture_2d<f32>;

// Width (pixels) of the band inside the radius where the visible style
// fades into the remembered one.
const RADIUS_FADE: f32 = 12.0;

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

// Full color, dimming slightly toward the edge of a finite view radius.
fn style_visible(c: vec3<f32>, dist: f32) -> vec3<f32> {
    if params.radius <= 0.0 {
        return c;
    }
    return c * mix(1.0, 0.75, smoothstep(0.0, params.radius, dist));
}

// Dimmed grayscale, dithered down to the four EGA grays (0, 1/3, 2/3, 1).
fn style_remembered(c: vec3<f32>, p: vec2<u32>) -> vec3<f32> {
    let lum = luminance(c) * 0.75;
    let level = clamp(floor(lum * 3.0 + bayer(p)), 0.0, 3.0) / 3.0;
    return vec3(level);
}

fn style_unknown(c: vec3<f32>) -> vec3<f32> {
    return c;
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = vec2<i32>(frag.xy);
    let dist = distance(frag.xy, params.origin);

    var now = textureLoad(mask_now, px, 0).r;
    let seen = textureLoad(mask_seen, px, 0).r;
    if params.radius > 0.0 {
        now *= 1.0 - smoothstep(params.radius - RADIUS_FADE, params.radius, dist);
    }

    let visible = style_visible(textureLoad(visible_layer, px, 0).rgb, dist);
    let remembered = style_remembered(textureLoad(remembered_layer, px, 0).rgb, vec2<u32>(px));
    let unknown = style_unknown(textureLoad(unknown_layer, px, 0).rgb);
    return vec4(mix(mix(unknown, remembered, seen), visible, now), 1.0);
}
