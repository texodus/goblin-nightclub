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

//! Moving a box through a layer without passing into its solid space.

use super::tile::COLLIDE_EPS;
use super::*;

/// Longest substep (pixels) when moving a box: under half a tile and under
/// half the smallest box, so nothing passes through a wall.
const MAX_STEP: f32 = 2.0;

/// Rounds of pushing a box out of overlapping tiles per substep.
const PUSH_ROUNDS: usize = 4;

impl Layer {
    /// Does the pixel-space box `pos..pos+size` overlap solid space?
    /// Touching a surface doesn't count.
    pub fn collides(&self, pos: [f32; 2], size: [f32; 2]) -> bool {
        self.tiles_under(pos, size)
            .any(|t| self.box_hits_tile(t, pos, size))
    }

    /// Tiles the box overlaps by more than `COLLIDE_EPS`.
    fn tiles_under(&self, pos: [f32; 2], size: [f32; 2]) -> impl Iterator<Item = [i32; 2]> {
        let lo = |a: usize| ((pos[a] + COLLIDE_EPS) / TILE).floor() as i32;
        let hi = |a: usize| ((pos[a] + size[a] - COLLIDE_EPS) / TILE).ceil() as i32;
        let (x0, x1, y0, y1) = (lo(0), hi(0), lo(1), hi(1));
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| [x, y]))
    }

    fn box_hits_tile(&self, tile: [i32; 2], pos: [f32; 2], size: [f32; 2]) -> bool {
        self.solid(tile[0], tile[1]).hits_box(tile, pos, size)
    }

    /// Move a box by `delta`, sliding along anything it runs into. Moves in
    /// substeps short enough not to skip through a wall, pushing the box
    /// back out of solid space after each.
    pub fn move_box(&self, mut pos: [f32; 2], size: [f32; 2], delta: [f32; 2]) -> [f32; 2] {
        let steps = (length(delta) / MAX_STEP).ceil().max(1.0);
        let step = [delta[0] / steps, delta[1] / steps];
        for _ in 0..steps as usize {
            let next = self.push_out([pos[0] + step[0], pos[1] + step[1]], size);
            // Wedged somewhere pushes can't settle: hold still rather than
            // overlap or jitter.
            if self.collides(next, size) {
                break;
            }
            pos = next;
        }
        pos
    }

    /// Push an overlapping box out of solid space. Each overlapping tile
    /// asks for a move of at least some depth along some normal; take the
    /// smallest move satisfying all of them at once (so a box pressed into
    /// a concave corner ends up in the corner, not short of it), and repeat
    /// in case that move found new overlaps.
    fn push_out(&self, mut pos: [f32; 2], size: [f32; 2]) -> [f32; 2] {
        for _ in 0..PUSH_ROUNDS {
            let constraints: Vec<([f32; 2], f32)> = self
                .tiles_under(pos, size)
                .filter_map(|t| self.tile_push(t, pos, size))
                .collect();
            if constraints.is_empty() {
                break;
            }
            let Some([dx, dy]) = smallest_move(&constraints) else {
                break;
            };
            pos = [pos[0] + dx, pos[1] + dy];
        }
        pos
    }

    /// The shortest way out of tile `t`'s solid part, as `(normal, depth)`,
    /// by separating axes: the four axis directions, plus the face's outward
    /// normal for a slope. Only moves out through an exposed face count, so
    /// a box sliding along a wall built of many tiles isn't snagged sideways
    /// at the seams between them. `None` if they don't overlap.
    fn tile_push(&self, t: [i32; 2], pos: [f32; 2], size: [f32; 2]) -> Option<([f32; 2], f32)> {
        let tile = self.solid(t[0], t[1]);
        let shape = tile.shape(0, 0)?;
        let origin = [t[0] as f32 * TILE, t[1] as f32 * TILE];
        let shape: Vec<[f32; 2]> = shape
            .iter()
            .map(|p| [p[0] + origin[0], p[1] + origin[1]])
            .collect();
        let corners = [
            pos,
            [pos[0] + size[0], pos[1]],
            [pos[0], pos[1] + size[1]],
            [pos[0] + size[0], pos[1] + size[1]],
        ];
        let range = |points: &[[f32; 2]], n: [f32; 2]| {
            points
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                    let d = p[0] * n[0] + p[1] * n[1];
                    (lo.min(d), hi.max(d))
                })
        };

        // (direction, whether the face that way is exposed: the neighbor
        // there doesn't cover the side facing this tile).
        let exposed = |dx: i32, dy: i32, side: Side| !self.solid(t[0] + dx, t[1] + dy).covers(side);
        let mut pushes = vec![
            ([1.0, 0.0], exposed(1, 0, Side::W)),
            ([-1.0, 0.0], exposed(-1, 0, Side::E)),
            ([0.0, 1.0], exposed(0, 1, Side::N)),
            ([0.0, -1.0], exposed(0, -1, Side::S)),
        ];
        if let Solid::Slope(c) = tile {
            // Away from the solid corner; the face always borders open space.
            let (_, g) = Solid::slope_plane(c);
            let r = std::f32::consts::FRAC_1_SQRT_2;
            pushes.push(([-g[0] * r, -g[1] * r], true));
        }

        let mut best: Option<([f32; 2], f32)> = None;
        let mut fallback: Option<([f32; 2], f32)> = None;
        for (n, exposed) in pushes {
            let (_, shape_hi) = range(&shape, n);
            let (box_lo, _) = range(&corners, n);
            let depth = shape_hi - box_lo;
            // Separated along this axis (or its opposite): no overlap.
            let (shape_lo, _) = range(&shape, n);
            let (_, box_hi) = range(&corners, n);
            if depth <= COLLIDE_EPS || box_hi - shape_lo <= COLLIDE_EPS {
                return None;
            }
            let slot = if exposed { &mut best } else { &mut fallback };
            if slot.is_none_or(|(_, d)| depth < d) {
                *slot = Some((n, depth));
            }
        }
        best.or(fallback)
    }
}

/// The smallest move `m` with `m . n >= depth` for every `(n, depth)`. In
/// 2D the optimum has at most two of these tight, so try each one alone and
/// each pair together, keeping the shortest move that satisfies them all.
/// `None` if they conflict (e.g. walls closer together than the box).
fn smallest_move(constraints: &[([f32; 2], f32)]) -> Option<[f32; 2]> {
    let ok = |m: [f32; 2]| {
        constraints
            .iter()
            .all(|(n, d)| m[0] * n[0] + m[1] * n[1] >= d - COLLIDE_EPS / 2.0)
    };
    let mut candidates: Vec<[f32; 2]> = constraints
        .iter()
        .map(|(n, d)| [n[0] * d, n[1] * d])
        .collect();
    for (i, (a, da)) in constraints.iter().enumerate() {
        for (b, db) in &constraints[i + 1..] {
            let det = a[0] * b[1] - a[1] * b[0];
            if det.abs() > 1e-6 {
                candidates.push([(da * b[1] - db * a[1]) / det, (a[0] * db - b[0] * da) / det]);
            }
        }
    }
    candidates
        .into_iter()
        .filter(|m| ok(*m))
        .min_by(|x, y| length(*x).total_cmp(&length(*y)))
}
