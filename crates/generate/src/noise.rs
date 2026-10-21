//! Smooth noise and hashes of where things are, so the same place comes
//! out the same however the world is made.

/// Value noise with three octaves, in [0, 1): smooth hills of wavelength
/// about one unit of `(x, y)`.
pub(crate) fn fbm(seed: u32, x: f32, y: f32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let (mut fx, mut fy) = (x, y);
    for octave in 0..3u32 {
        sum += value_noise(seed.wrapping_add(octave.wrapping_mul(0x51ED_270B)), fx, fy) * amp;
        norm += amp;
        amp *= 0.5;
        fx *= 2.0;
        fy *= 2.0;
    }
    (sum / norm).clamp(0.0, 0.999_99)
}

/// Bilinear value noise: a hashed value at each lattice point, smoothly
/// interpolated between.
pub(crate) fn value_noise(seed: u32, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(tx), smooth(ty));
    let at = |dx: f32, dy: f32| lattice(seed, (x0 + dx) as i32, (y0 + dy) as i32);
    let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * sx;
    let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

pub(crate) fn lattice(seed: u32, x: i32, y: i32) -> f32 {
    let mut h = seed ^ (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The quantiles of `values`: for a share (0 to 1), the value that share
/// of them are below.
pub(crate) fn quantiles(mut values: Vec<f32>) -> impl Fn(f32) -> f32 {
    values.sort_by(f32::total_cmp);
    move |share| values[(share.clamp(0.0, 1.0) * (values.len() - 1) as f32) as usize]
}
