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

//! Plane geometry on points as `[f32; 2]`, in world units (y down, so
//! clockwise on screen is clockwise here too): lengths, rectangles, convex
//! shapes and outline normals.

/// An axis-aligned rectangle: top-left, bottom-right.
pub type Rect = [[f32; 2]; 2];

/// A rectangle covering any level.
pub const EVERYWHERE: Rect = [[f32::NEG_INFINITY; 2], [f32::INFINITY; 2]];

/// A convex polygon, clockwise on screen.
pub type Shape = Vec<[f32; 2]>;

/// The length of `v`. Not `f32::hypot`, which guards against overflow no
/// level comes near, and which compiles to a slow software routine in
/// wasm: sight calls this a great deal every frame.
pub fn length(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// How far `b` is from `a`.
pub fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    length([b[0] - a[0], b[1] - a[1]])
}

/// The smallest rectangle around `points` (inside out, for none).
pub fn bounds(points: &[[f32; 2]]) -> Rect {
    points.iter().fold(
        [[f32::INFINITY; 2], [f32::NEG_INFINITY; 2]],
        |[lo, hi], p| {
            [
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            ]
        },
    )
}

/// The average of `points` (the center of a convex polygon's corners).
pub fn centroid(points: &[[f32; 2]]) -> [f32; 2] {
    let n = points.len() as f32;
    points
        .iter()
        .fold([0.0; 2], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n])
}

/// `p` moved `by` toward `to` (not past it, unless `by` is that far).
pub fn toward(p: [f32; 2], to: [f32; 2], by: f32) -> [f32; 2] {
    let d = distance(p, to);
    [
        p[0] + (to[0] - p[0]) / d * by,
        p[1] + (to[1] - p[1]) / d * by,
    ]
}

/// Unsigned area of a polygon, measured from its first point so slivers
/// far from the origin don't drown in rounding.
pub fn area(points: &[[f32; 2]]) -> f32 {
    let Some(&o) = points.first() else {
        return 0.0;
    };
    (1..points.len().saturating_sub(1))
        .map(|i| {
            let (a, b) = (points[i], points[i + 1]);
            (a[0] - o[0]) * (b[1] - o[1]) - (b[0] - o[0]) * (a[1] - o[1])
        })
        .sum::<f32>()
        .abs()
        / 2.0
}

/// The part of convex polygon `subject` inside convex polygon `clip`
/// (clockwise on screen), by Sutherland-Hodgman.
pub fn clip_convex(subject: &[[f32; 2]], clip: &[[f32; 2]]) -> Shape {
    let mut out = subject.to_vec();
    for i in 0..clip.len() {
        if out.is_empty() {
            break;
        }
        let (c0, c1) = (clip[i], clip[(i + 1) % clip.len()]);
        // Inside is to the right of each clip edge.
        let side =
            |p: [f32; 2]| (c1[0] - c0[0]) * (p[1] - c0[1]) - (c1[1] - c0[1]) * (p[0] - c0[0]);
        let input = std::mem::take(&mut out);
        for j in 0..input.len() {
            let (p, q) = (input[j], input[(j + 1) % input.len()]);
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
            }
        }
    }
    out
}

/// Points where segment `a`-`b` crosses the circle of `radius` around `c`.
pub fn circle_crossings(c: [f32; 2], radius: f32, a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
    // |a + t (b - a) - c|^2 = r^2, a quadratic in t.
    let d = [b[0] - a[0], b[1] - a[1]];
    let f = [a[0] - c[0], a[1] - c[1]];
    let qa = d[0] * d[0] + d[1] * d[1];
    let qb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
    let qc = f[0] * f[0] + f[1] * f[1] - radius * radius;
    let disc = qb * qb - 4.0 * qa * qc;
    if qa == 0.0 || disc < 0.0 {
        return Vec::new();
    }
    let root = disc.sqrt();
    [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)]
        .into_iter()
        .filter(|t| (0.0..=1.0).contains(t))
        .map(|t| [a[0] + d[0] * t, a[1] + d[1] * t])
        .collect()
}

/// Unit normal of `a`-`b` pointing to its right, which is the inside of an
/// outline running clockwise on screen (y down).
pub fn inward_normal(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let e = [b[0] - a[0], b[1] - a[1]];
    let len = length(e);
    [-e[1] / len, e[0] / len]
}

/// Corner `p` of an outline moved `width` inward, between edges with
/// inward normals `before` and `after`, along the bisector, so strips that
/// wide along the two edges meet exactly (at concave and sharp corners
/// too).
pub fn inset_corner(p: [f32; 2], before: [f32; 2], after: [f32; 2], width: f32) -> [f32; 2] {
    let k = width / (1.0 + before[0] * after[0] + before[1] * after[1]);
    [
        p[0] + (before[0] + after[0]) * k,
        p[1] + (before[1] + after[1]) * k,
    ]
}
