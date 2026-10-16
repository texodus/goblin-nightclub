//! Line of sight for one viewer: an exact visibility polygon (for rendering)
//! plus point queries and tile exploration (for game logic). Both derive
//! from [`Level::los`]/[`Level::raycast`], so what is drawn as visible and
//! what the game treats as visible agree.

use std::f32::consts::TAU;

use crate::level::{Level, TILE};

/// Angular offset of the extra rays cast to either side of each corner,
/// sampling the boundary just before and just after it.
const CORNER_EPS: f32 = 1e-4;

/// Rays per full turn used to approximate the radius circle.
const ARC_SAMPLES: usize = 128;

/// Rays closer than this (radians) are merged; well below `CORNER_EPS`.
const MERGE_EPS: f32 = 1e-6;

/// Distances along one ray closer than this (pixels) are the same point.
const SAME_POINT: f32 = 0.05;

struct Ray {
    /// Normalized to `[0, TAU)`.
    angle: f32,
    /// Corners lying on this ray (several when collinear with the origin).
    corners: Vec<[f32; 2]>,
}

impl Ray {
    fn new(angle: f32, corners: Vec<[f32; 2]>) -> Self {
        Self {
            angle: angle.rem_euclid(TAU),
            corners,
        }
    }
}

fn dir(angle: f32) -> [f32; 2] {
    [angle.cos(), angle.sin()]
}

/// The point `t` pixels from `origin` at `angle`.
fn at(origin: [f32; 2], angle: f32, t: f32) -> [f32; 2] {
    [origin[0] + angle.cos() * t, origin[1] + angle.sin() * t]
}

fn angle(from: [f32; 2], to: [f32; 2]) -> f32 {
    (to[1] - from[1]).atan2(to[0] - from[0])
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

/// Point at fraction `s` along one face of the tile with corner `(x0, y0)`.
type FacePoint = fn(f32, f32, f32) -> [f32; 2];

pub struct Vision {
    origin: [f32; 2],
    /// View distance in pixels; `f32::INFINITY` for unlimited.
    radius: f32,
    /// Boundary of the visible region, by increasing angle around `origin`
    /// (clockwise on screen, since y points down). Render as a triangle fan
    /// centered on `origin`.
    polygon: Vec<[f32; 2]>,
    /// `(pos, size)` of blocks with a currently visible face.
    visible_blocks: Vec<([f32; 2], [f32; 2])>,
    cols: usize,
    explored: Vec<bool>,
}

impl Vision {
    pub fn new(level: &Level, radius: f32) -> Self {
        Self {
            origin: [0.0; 2],
            radius,
            polygon: Vec::new(),
            visible_blocks: Vec::new(),
            cols: level.cols,
            explored: vec![false; level.cols * level.rows],
        }
    }

    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    pub fn polygon(&self) -> &[[f32; 2]] {
        &self.polygon
    }

    /// Blocks with a currently visible face, as `(pos, size)`. The polygon
    /// stops at wall faces, so these are what's seen of solid space.
    pub fn visible_blocks(&self) -> &[([f32; 2], [f32; 2])] {
        &self.visible_blocks
    }

    /// Recompute visibility from `origin` (a pixel-space point in open space).
    pub fn update(&mut self, level: &Level, origin: [f32; 2]) {
        self.origin = origin;
        self.rebuild_polygon(level);
        self.find_visible_blocks(level);
        self.mark_explored(level);
    }

    /// Can the viewer currently see pixel-space point `p`?
    pub fn can_see(&self, level: &Level, p: [f32; 2]) -> bool {
        let d = [p[0] - self.origin[0], p[1] - self.origin[1]];
        d[0].hypot(d[1]) <= self.radius && level.los(self.origin, p)
    }

    /// Has any part of tile `(x, y)` ever been visible?
    pub fn is_explored(&self, x: usize, y: usize) -> bool {
        self.explored[y * self.cols + x]
    }

    fn rebuild_polygon(&mut self, level: &Level) {
        let origin = self.origin;
        let mut rays: Vec<Ray> = Vec::new();
        for c in level.corners() {
            // Corners outside the radius can't shape the visible region.
            if distance(origin, c) > self.radius {
                continue;
            }
            let a = angle(origin, c);
            rays.extend([
                Ray::new(a - CORNER_EPS, vec![]),
                Ray::new(a, vec![c]),
                Ray::new(a + CORNER_EPS, vec![]),
            ]);
        }
        if self.radius.is_finite() {
            rays.extend(
                (0..ARC_SAMPLES).map(|i| Ray::new(i as f32 * TAU / ARC_SAMPLES as f32, vec![])),
            );
        }
        rays.sort_by(|x, y| x.angle.total_cmp(&y.angle));

        // Rays closer together than float precision can resolve may sort in
        // an order that disagrees with their actual directions, so merge
        // them (including across the wrap at TAU), pooling their corners.
        rays.dedup_by(|next, prev| {
            let close = next.angle - prev.angle < MERGE_EPS;
            if close {
                prev.corners.append(&mut next.corners);
            }
            close
        });
        if rays.len() > 1 && rays[0].angle + TAU - rays[rays.len() - 1].angle < MERGE_EPS {
            let mut last = rays.pop().unwrap();
            rays[0].corners.append(&mut last.corners);
        }

        let cast = |angle: f32| level.raycast(origin, dir(angle), self.radius);
        let mut hits: Vec<(Ray, f32)> = rays
            .into_iter()
            .map(|ray| {
                let t = cast(ray.angle);
                (ray, t)
            })
            .collect();
        if self.radius.is_finite() {
            hits = self.refine_arc(level, hits);
        }

        // Plain rays contribute their hit point. At a corner ray the
        // boundary can jump along the ray: it arrives at the distance where
        // the previous ray hit (just before the corner), runs out as far as
        // the ray itself reaches, and leaves at the distance where the next
        // ray hit. E.g. at a shadow edge it goes corner -> far wall, or far
        // wall -> corner; with collinear corners, corner -> far -> corner.
        // All three points lie on the ray itself so they stay in angular
        // order.
        let n = hits.len();
        self.polygon.clear();
        for i in 0..n {
            let (ray, t) = &hits[i];
            if ray.corners.is_empty() {
                self.polygon.push(at(origin, ray.angle, *t));
                continue;
            }
            // Aim exactly at the nearest corner rather than trusting
            // cos/sin of the rounded angle, so the cast reliably passes
            // through the corner's vertex and the graze rule applies.
            let nearest = *ray
                .corners
                .iter()
                .min_by(|a, b| distance(origin, **a).total_cmp(&distance(origin, **b)))
                .unwrap();
            let len = distance(origin, nearest);
            let aim = [
                (nearest[0] - origin[0]) / len,
                (nearest[1] - origin[1]) / len,
            ];
            let reach = level.raycast(origin, aim, self.radius);
            let along = |d: f32| [origin[0] + aim[0] * d, origin[1] + aim[1] * d];
            let arrive = hits[(i + n - 1) % n].1.min(reach);
            let leave = hits[(i + 1) % n].1.min(reach);
            // A distance matching a corner snaps to it exactly.
            let snap = |d: f32| {
                ray.corners
                    .iter()
                    .find(|c| (distance(origin, **c) - d).abs() < SAME_POINT)
                    .copied()
                    .unwrap_or_else(|| along(d))
            };
            let mut last = f32::NAN;
            for (d, point) in [
                (arrive, snap(arrive)),
                (reach, snap(reach)),
                (leave, snap(leave)),
            ] {
                if last.is_nan() || (d - last).abs() >= SAME_POINT {
                    self.polygon.push(point);
                    last = d;
                }
            }
        }
    }

    /// Every visible stretch of a block's face is an edge of the polygon, so
    /// a block is visible exactly when a polygon vertex lies on its outline.
    fn find_visible_blocks(&mut self, level: &Level) {
        const ON: f32 = 1e-2;
        self.visible_blocks.clear();
        self.visible_blocks
            .extend(level.blocks().filter(|([x, y], [w, h])| {
                self.polygon.iter().any(|[px, py]| {
                    *px >= x - ON && *px <= x + w + ON && *py >= y - ON && *py <= y + h + ON
                })
            }));
    }

    /// Where neighboring rays switch between reaching the radius and
    /// hitting a wall, the wall crosses the circle somewhere between them.
    /// That crossing isn't a corner, so bisect for it and add rays on both
    /// sides; otherwise the chord between the two rays cuts off visible floor.
    fn refine_arc(&self, level: &Level, hits: Vec<(Ray, f32)>) -> Vec<(Ray, f32)> {
        const STEPS: usize = 16;
        let reaches = |t: f32| t >= self.radius - 1e-3;
        let cast = |angle: f32| {
            let t = level.raycast(self.origin, dir(angle), self.radius);
            (Ray::new(angle, vec![]), t)
        };

        // Bisection brackets (lo, hi, whether lo reaches the radius),
        // computed before `hits` is consumed.
        let n = hits.len();
        let brackets: Vec<Option<(f32, f32, bool)>> = (0..n)
            .map(|i| {
                let ((a, ta), (b, tb)) = (&hits[i], &hits[(i + 1) % n]);
                // Corner rays get their own treatment in `rebuild_polygon`.
                if !a.corners.is_empty() || !b.corners.is_empty() || reaches(*ta) == reaches(*tb) {
                    return None;
                }
                let wrap = if i + 1 == n { TAU } else { 0.0 };
                Some((a.angle, b.angle + wrap, reaches(*ta)))
            })
            .collect();

        let mut out = Vec::with_capacity(n + 8);
        for (hit, bracket) in hits.into_iter().zip(brackets) {
            out.push(hit);
            if let Some((mut lo, mut hi, lo_reaches)) = bracket {
                for _ in 0..STEPS {
                    let mid = (lo + hi) / 2.0;
                    if reaches(cast(mid).1) == lo_reaches {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                out.extend([cast(lo), cast(hi)]);
            }
        }
        out
    }

    /// A floor tile is explored once its center or a (slightly inset) corner
    /// is seen. A solid tile is explored once a point on one of its open
    /// faces is seen.
    fn mark_explored(&mut self, level: &Level) {
        const INSET: f32 = 0.5;
        const FACE_SAMPLES: [f32; 3] = [0.1, 0.5, 0.9];
        let reach = self.radius + TILE * std::f32::consts::SQRT_2;

        for y in 0..level.rows {
            for x in 0..level.cols {
                let i = y * self.cols + x;
                if self.explored[i] {
                    continue;
                }
                let (x0, y0) = (x as f32 * TILE, y as f32 * TILE);
                let (cx, cy) = (x0 + TILE / 2.0, y0 + TILE / 2.0);
                if (cx - self.origin[0]).hypot(cy - self.origin[1]) > reach {
                    continue;
                }

                let (xi, yi) = (x as i32, y as i32);
                self.explored[i] = if level.is_solid(xi, yi) {
                    // (neighbor offset, point on the shared face at s)
                    let faces: [([i32; 2], FacePoint); 4] = [
                        ([0, -1], |x0, y0, s| [x0 + s * TILE, y0]),
                        ([0, 1], |x0, y0, s| [x0 + s * TILE, y0 + TILE]),
                        ([-1, 0], |x0, y0, s| [x0, y0 + s * TILE]),
                        ([1, 0], |x0, y0, s| [x0 + TILE, y0 + s * TILE]),
                    ];
                    faces.iter().any(|([dx, dy], point)| {
                        !level.is_solid(xi + dx, yi + dy)
                            && FACE_SAMPLES
                                .iter()
                                .any(|&s| self.can_see(level, point(x0, y0, s)))
                    })
                } else {
                    let (a, b) = (INSET, TILE - INSET);
                    [
                        [cx, cy],
                        [x0 + a, y0 + a],
                        [x0 + b, y0 + a],
                        [x0 + a, y0 + b],
                        [x0 + b, y0 + b],
                    ]
                    .into_iter()
                    .any(|p| self.can_see(level, p))
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rng::Rng;

    trait OpenPoint {
        fn open_point(&mut self, level: &Level) -> [f32; 2];
    }

    impl OpenPoint for Rng {
        fn open_point(&mut self, level: &Level) -> [f32; 2] {
            loop {
                let p = [
                    self.unit() * level.cols as f32 * TILE,
                    self.unit() * level.rows as f32 * TILE,
                ];
                if !level.collides(p, [0.0, 0.0]) {
                    return p;
                }
            }
        }
    }

    fn levels() -> impl Iterator<Item = (Level, [f32; 2])> {
        let spawn = [165.0, 105.0];
        (0..40).map(move |seed| (Level::generate(seed, 32, 20, spawn), spawn))
    }

    /// Crossing-number point-in-polygon test.
    fn inside(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
        let mut inside = false;
        for i in 0..poly.len() {
            let [ax, ay] = poly[i];
            let [bx, by] = poly[(i + 1) % poly.len()];
            if (ay > p[1]) != (by > p[1]) && p[0] < ax + (p[1] - ay) / (by - ay) * (bx - ax) {
                inside = !inside;
            }
        }
        inside
    }

    #[test]
    fn los_is_symmetric() {
        let mut rng = Rng::new(7);
        for (level, _) in levels() {
            for _ in 0..200 {
                let (a, b) = (rng.open_point(&level), rng.open_point(&level));
                assert_eq!(level.los(a, b), level.los(b, a), "{a:?} <-> {b:?}");
            }
        }
    }

    #[test]
    fn los_blocked_by_wall_and_diagonal_pinch() {
        let level = Level::from_ascii(&[
            "......", //
            "..#...", //
            "...#..", //
            "......", //
        ]);
        // Straight through the wall tile at (2, 1).
        assert!(!level.los([5.0, 15.0], [45.0, 15.0]));
        // Through open tiles except exactly at the pinch vertex (30, 20)
        // between (2,1) and (3,2).
        assert!(!level.los([15.0, 35.0], [45.0, 5.0]));
        // Grazing a single corner, (20, 20) of tile (1,1), is allowed.
        let single = Level::from_ascii(&["....", ".#..", "....", "...."]);
        assert!(single.los([5.0, 35.0], [35.0, 5.0]));
        // A point on an open face is visible.
        assert!(level.los([25.0, 5.0], [25.0, 10.0]));
    }

    #[test]
    fn polygon_matches_los() {
        let mut rng = Rng::new(11);
        for (level, spawn) in levels() {
            // Tile-centered and grid-vertex origins line up with many
            // corners at once; random ones cover the general case.
            let mut origins = vec![spawn, [160.0, 100.0]];
            origins.extend((0..4).map(|_| rng.open_point(&level)));
            for origin in origins {
                for radius in [f32::INFINITY, 80.0] {
                    let mut vision = Vision::new(&level, radius);
                    vision.update(&level, origin);
                    check_polygon(&level, &vision, &mut rng);
                }
            }
        }
    }

    /// Visible, allowing a hair of rotation: points on a line that exactly
    /// grazes a corner are visible or not only up to rounding.
    fn visible_ish(level: &Level, vision: &Vision, p: [f32; 2]) -> bool {
        let o = vision.origin();
        let (a, d) = (angle(o, p), distance(o, p));
        vision.can_see(level, p)
            || [a - 1e-5, a + 1e-5]
                .iter()
                .any(|a| vision.can_see(level, at(o, *a, d)))
    }

    fn check_polygon(level: &Level, vision: &Vision, rng: &mut Rng) {
        let origin = vision.origin();
        let poly = vision.polygon();
        let ctx = format!("origin {origin:?} radius {}", vision.radius());

        // Vertices are visible, and so is everything inside each fan
        // triangle (sampled slightly inward to avoid edge rounding).
        for (i, v) in poly.iter().enumerate() {
            // Nudged inward, since vertices on the radius circle round
            // either way.
            let v_in = [
                origin[0] + 0.9999 * (v[0] - origin[0]),
                origin[1] + 0.9999 * (v[1] - origin[1]),
            ];
            assert!(visible_ish(level, vision, v_in), "vertex {v:?}, {ctx}");
            let w = poly[(i + 1) % poly.len()];
            // Slivers thinner than f32 can place a sample in (e.g. the
            // 1e-6 rad wedge just past a corner) can't be tested this way.
            let (dv, dw) = (
                [v[0] - origin[0], v[1] - origin[1]],
                [w[0] - origin[0], w[1] - origin[1]],
            );
            let sin = (dv[0] * dw[1] - dv[1] * dw[0]) / (dv[0].hypot(dv[1]) * dw[0].hypot(dw[1]));
            if sin.abs() < 1e-5 {
                continue;
            }
            for _ in 0..4 {
                let (mut s, mut t) = (rng.unit(), rng.unit());
                if s + t > 1.0 {
                    (s, t) = (1.0 - s, 1.0 - t);
                }
                let k = 0.99;
                let p = [
                    origin[0] + k * (s * (v[0] - origin[0]) + t * (w[0] - origin[0])),
                    origin[1] + k * (s * (v[1] - origin[1]) + t * (w[1] - origin[1])),
                ];
                assert!(visible_ish(level, vision, p), "fan point {p:?}, {ctx}");
            }
        }

        // Conversely, visible points lie inside the polygon. With a finite
        // radius the arc is approximated by chords, so stay clear of it.
        let limit = vision.radius() * 0.99;
        for _ in 0..200 {
            let p = rng.open_point(level);
            let d = (p[0] - origin[0]).hypot(p[1] - origin[1]);
            if d < limit && vision.can_see(level, p) {
                let q = [
                    origin[0] + 0.995 * (p[0] - origin[0]),
                    origin[1] + 0.995 * (p[1] - origin[1]),
                ];
                assert!(inside(poly, q), "visible {p:?} outside polygon, {ctx}");
            }
        }
    }

    #[test]
    fn radius_limits_sight() {
        let level = Level::from_ascii(&["..........", "..........", ".........."]);
        let origin = [5.0, 15.0];
        let mut near = Vision::new(&level, 30.0);
        near.update(&level, origin);
        assert!(near.can_see(&level, [34.0, 15.0]));
        assert!(!near.can_see(&level, [36.0, 15.0]));
        for [x, y] in near.polygon() {
            assert!((x - origin[0]).hypot(y - origin[1]) <= 30.0 + 1e-3);
        }

        let mut far = Vision::new(&level, f32::INFINITY);
        far.update(&level, origin);
        assert!(far.can_see(&level, [95.0, 25.0]));
    }

    #[test]
    fn blocks_visible_by_their_faces() {
        let level = Level::from_ascii(&[
            "..........", //
            "..#.#.....", //
            "..........", //
        ]);
        // Looking down from row 0, the second block's top face shows over
        // the first.
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update(&level, [5.0, 5.0]);
        let lit: Vec<_> = v.visible_blocks().iter().map(|(p, _)| *p).collect();
        assert!(lit.contains(&[20.0, 10.0]));
        assert!(
            lit.contains(&[40.0, 10.0]),
            "top face visible over the first"
        );

        // One row high, so the second block hides behind the first.
        let level = Level::from_ascii(&["..#.#."]);
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update(&level, [5.0, 5.0]);
        let lit: Vec<_> = v.visible_blocks().iter().map(|(p, _)| *p).collect();
        assert_eq!(lit, vec![[20.0, 0.0]], "second block fully hidden");

        let mut short = Vision::new(&level, 10.0);
        short.update(&level, [5.0, 5.0]);
        assert!(short.visible_blocks().is_empty(), "beyond the radius");
    }

    #[test]
    fn exploration_accumulates() {
        let level = Level::from_ascii(&[
            "..........", //
            "....#.....", //
            "....#.....", //
            "....#.....", //
            "..........", //
        ]);
        let mut vision = Vision::new(&level, f32::INFINITY);
        vision.update(&level, [15.0, 25.0]);
        assert!(vision.is_explored(1, 2));
        assert!(vision.is_explored(4, 2), "wall face seen");
        assert!(!vision.is_explored(7, 2), "behind the wall");

        // Walk around the wall; earlier tiles stay explored.
        vision.update(&level, [75.0, 25.0]);
        assert!(vision.is_explored(7, 2));
        assert!(vision.is_explored(1, 2));
    }
}
