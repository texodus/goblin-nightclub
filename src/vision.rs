//! Line of sight for one viewer: an exact visibility polygon (for rendering)
//! plus point queries and tile exploration (for game logic). Both derive
//! from [`Level::los`]/[`Level::raycast`], so what is drawn as visible and
//! what the game treats as visible agree.

use std::f32::consts::TAU;

use crate::level::{Level, Shape, TILE};

/// Minimum angular offset of the extra rays cast to either side of each
/// corner, sampling the boundary just before and just after it.
const CORNER_EPS: f32 = 1e-4;

/// Rays per full turn used to approximate the radius circle.
const ARC_SAMPLES: usize = 128;

/// Rays closer than this (radians) are merged; well below `CORNER_EPS`.
const MERGE_EPS: f32 = 1e-6;

/// How far (pixels) the side casts at a corner pass it; well above the
/// vertex tolerance in [`Level::raycast`].
const SIDE_OFFSET: f32 = 1e-3;

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

/// Angular offset of the rays beside a corner `dist` pixels away: at least
/// `CORNER_EPS`, and wide enough to pass the corner by `2 * SIDE_OFFSET`,
/// clear of `raycast`'s vertex tolerance even for a corner right next to
/// the viewer.
fn corner_eps(dist: f32) -> f32 {
    CORNER_EPS.max(2.0 * SIDE_OFFSET / dist)
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

/// Points where segment `a`-`b` crosses the circle of `radius` around `c`.
fn circle_crossings(c: [f32; 2], radius: f32, a: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
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

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1]];
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1]) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    distance(p, [a[0] + ab[0] * t, a[1] + ab[1] * t])
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

pub struct Vision {
    origin: [f32; 2],
    /// View distance in pixels; `f32::INFINITY` for unlimited.
    radius: f32,
    /// Boundary of the visible region, by increasing angle around `origin`
    /// (clockwise on screen, since y points down). Render as a triangle fan
    /// centered on `origin`.
    polygon: Vec<[f32; 2]>,
    /// Solid shapes with a currently visible face.
    visible_solids: Vec<Shape>,
    cols: usize,
    explored: Vec<bool>,
}

impl Vision {
    pub fn new(level: &Level, radius: f32) -> Self {
        Self {
            origin: [0.0; 2],
            radius,
            polygon: Vec::new(),
            visible_solids: Vec::new(),
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

    /// Solid shapes with a currently visible face. The polygon stops at
    /// wall faces, so these are what's seen of solid space.
    pub fn visible_solids(&self) -> &[Shape] {
        &self.visible_solids
    }

    /// Recompute visibility from `origin` (a pixel-space point in open space).
    pub fn update(&mut self, level: &Level, origin: [f32; 2]) {
        self.origin = origin;
        self.rebuild_polygon(level);
        self.find_visible_solids(level);
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
        // Angular zones around each corner (center, half-width) in which a
        // plain ray can't be trusted to land on the right side of it.
        let mut zones: Vec<(f32, f32)> = Vec::new();
        for c in level.corners() {
            // Corners outside the radius can't shape the visible region.
            if distance(origin, c) > self.radius {
                continue;
            }
            let (a, eps) = (angle(origin, c), corner_eps(distance(origin, c)));
            zones.push((a.rem_euclid(TAU), eps / 2.0));
            rays.extend([
                Ray::new(a - eps, vec![]),
                Ray::new(a, vec![c]),
                Ray::new(a + eps, vec![]),
            ]);
        }
        if self.radius.is_finite() {
            rays.extend(
                (0..ARC_SAMPLES).map(|i| Ray::new(i as f32 * TAU / ARC_SAMPLES as f32, vec![])),
            );
            // Along a wall, the boundary can only switch between wall and
            // arc where the wall crosses the circle; elsewhere it switches at
            // corners. So sample either side of every such crossing, or the
            // chord between neighboring rays could cut off visible floor.
            let eps = corner_eps(self.radius);
            for (a, b) in level.outline_edges() {
                for p in circle_crossings(origin, self.radius, a, b) {
                    let angle = angle(origin, p);
                    rays.extend([Ray::new(angle - eps, vec![]), Ray::new(angle + eps, vec![])]);
                }
            }
        }
        rays.sort_by(|x, y| x.angle.total_cmp(&y.angle));

        // A plain ray passing within `raycast`'s tolerance of a corner may
        // count as grazing it on the wrong side, and land between the corner
        // and its near side. The corner's own rays cover that angle, so drop
        // plain rays in each corner's zone (which its side rays sit outside).
        zones.sort_by(|x, y| x.0.total_cmp(&y.0));
        let in_zone = |a: f32| {
            let i = zones.partition_point(|z| z.0 < a);
            [i.wrapping_sub(1), i, 0, zones.len().wrapping_sub(1)]
                .iter()
                .filter_map(|&j| zones.get(j))
                .any(|&(center, half)| {
                    let d = (a - center).rem_euclid(TAU);
                    d.min(TAU - d) < half
                })
        };
        rays.retain(|r| !r.corners.is_empty() || !in_zone(r.angle));

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
        // Plain rays contribute their hit point. At a corner ray the
        // boundary can jump along the ray: it arrives from just before the
        // ray at some distance, runs out as far as the ray reaches, and
        // leaves for just after it at some distance. E.g. at a shadow edge it
        // goes corner -> far wall, or far wall -> corner; with collinear
        // corners, corner -> far -> corner. All three points lie on the ray
        // itself so they stay in angular order.
        self.polygon.clear();
        for ray in &rays {
            if ray.corners.is_empty() {
                self.polygon.push(at(origin, ray.angle, cast(ray.angle)));
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
            // ...but cap it by the farther of two casts just to either
            // side, so a ray threading exactly between corners on both sides
            // doesn't see along a zero-width line. The side casts clear the
            // corner by `SIDE_OFFSET`, well past `raycast`'s vertex tolerance.
            let side = (SIDE_OFFSET / len).clamp(MERGE_EPS, corner_eps(len) / 2.0);
            let (before, after) = (cast(ray.angle - side), cast(ray.angle + side));
            let reach = level
                .raycast(origin, aim, self.radius)
                .min(before.max(after));
            let along = |d: f32| [origin[0] + aim[0] * d, origin[1] + aim[1] * d];

            // Where the boundary arrives at (and leaves) this ray can only be
            // one of its corners, or its reach (a surface continuing across
            // it). The side casts say which: snap each to the nearest. Their
            // own distances aren't used directly, since against a surface
            // seen at a grazing angle they drift fast with angle.
            let corner_dists: Vec<f32> = ray
                .corners
                .iter()
                .map(|c| distance(origin, *c))
                .filter(|d| *d <= reach + SAME_POINT)
                .collect();
            let snap_dist = |d: f32| {
                corner_dists
                    .iter()
                    .copied()
                    .chain([reach])
                    .min_by(|x, y| (x - d).abs().total_cmp(&(y - d).abs()))
                    .unwrap()
            };
            let arrive = snap_dist(before.min(reach));
            let leave = snap_dist(after.min(reach));
            // A distance matching a corner is placed exactly on it.
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

    /// Every visible stretch of a solid's face is an edge of the polygon, so
    /// a solid is visible exactly when a polygon vertex lies on its outline.
    fn find_visible_solids(&mut self, level: &Level) {
        const ON: f32 = 1e-2;
        let on_outline = |shape: &Shape, p: [f32; 2]| {
            (0..shape.len())
                .any(|i| segment_distance(p, shape[i], shape[(i + 1) % shape.len()]) <= ON)
        };
        self.visible_solids.clear();
        for shape in level.solids() {
            if self.polygon.iter().any(|p| on_outline(shape, *p)) {
                self.visible_solids.push(shape.clone());
            }
        }
    }

    /// A tile is explored once any of its sight samples (in its open part,
    /// or on an exposed face) is seen.
    fn mark_explored(&mut self, level: &Level) {
        let reach = self.radius + TILE * std::f32::consts::SQRT_2;
        for y in 0..level.rows {
            for x in 0..level.cols {
                let i = y * self.cols + x;
                if self.explored[i] {
                    continue;
                }
                let center = [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE];
                if distance(center, self.origin) > reach {
                    continue;
                }
                self.explored[i] = level
                    .sight_samples(x as i32, y as i32)
                    .into_iter()
                    .any(|p| self.can_see(level, p));
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
                // Clear of solid space, not just within `collides`'s
                // tolerance of it: symmetry only holds for open points.
                if is_open_point(level, p) {
                    return p;
                }
            }
        }
    }

    /// Tile soups are dense, so far slower to check than generated levels.
    const SOUPS: u32 = 12;

    /// Generated levels, then tile soups full of slopes, each with a
    /// suggested origin (which may be solid in a soup).
    fn levels() -> impl Iterator<Item = (Level, [f32; 2])> {
        let spawn = [165.0, 105.0];
        let generated = (0..40).map(move |seed| (Level::generate(seed, 32, 20, spawn), spawn));
        let soups = (0..SOUPS).map(move |seed| (Level::soup(seed, 32, 20), spawn));
        generated.chain(soups)
    }

    /// Clear of solid space in every direction (unlike a zero-size
    /// `collides` check, which never fires exactly on a grid vertex).
    fn is_open_point(level: &Level, p: [f32; 2]) -> bool {
        !level.collides([p[0] - 0.01, p[1] - 0.01], [0.02, 0.02])
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
            // corners (and slope faces) at once; random ones cover the
            // general case.
            let mut origins = vec![spawn, [160.0, 100.0]];
            for _ in 0..3 {
                let [x, y] = [rng.range(0, level.cols - 1), rng.range(0, level.rows - 1)];
                origins.push([x as f32 * TILE + 5.0, y as f32 * TILE + 5.0]);
                origins.push([x as f32 * TILE, y as f32 * TILE]);
            }
            origins.extend((0..4).map(|_| rng.open_point(&level)));
            for origin in origins.into_iter().filter(|o| is_open_point(&level, *o)) {
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

    /// Visible, and still visible a hair of rotation either way: points
    /// seen only within rounding of a grazing line needn't be in the polygon.
    fn visible_with_margin(level: &Level, vision: &Vision, p: [f32; 2]) -> bool {
        let o = vision.origin();
        let (a, d) = (angle(o, p), distance(o, p));
        vision.can_see(level, p)
            && [a - 1e-5, a + 1e-5]
                .iter()
                .all(|a| vision.can_see(level, at(o, *a, d)))
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
            if d < limit && visible_with_margin(level, vision, p) {
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
    fn solids_visible_by_their_faces() {
        let level = Level::from_ascii(&[
            "..........", //
            "..#.#.....", //
            "..........", //
        ]);
        // Looking down from row 0, the second block's top face shows over
        // the first.
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update(&level, [5.0, 5.0]);
        let lit: Vec<_> = v.visible_solids().iter().map(|s| s[0]).collect();
        assert!(lit.contains(&[20.0, 10.0]));
        assert!(
            lit.contains(&[40.0, 10.0]),
            "top face visible over the first"
        );

        // One row high, so the second block hides behind the first.
        let level = Level::from_ascii(&["..#.#."]);
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update(&level, [5.0, 5.0]);
        let lit: Vec<_> = v.visible_solids().iter().map(|s| s[0]).collect();
        assert_eq!(lit, vec![[20.0, 0.0]], "second block fully hidden");

        let mut short = Vision::new(&level, 10.0);
        short.update(&level, [5.0, 5.0]);
        assert!(short.visible_solids().is_empty(), "beyond the radius");
    }

    #[test]
    fn slopes_explored_by_face_side_or_open_half() {
        // Slopes at (2,1) and (6,1); a wall at column 4 hides the second
        // from a viewer on the left.
        let level = Level::from_ascii(&[
            "....#....", //
            "..◸.#.◿..", //
            "....#....", //
        ]);
        let mut vision = Vision::new(&level, f32::INFINITY);
        // From the lower right of the first slope: its face and open half.
        vision.update(&level, [35.0, 25.0]);
        assert!(vision.is_explored(2, 1));
        assert!(!vision.is_explored(6, 1), "behind the wall");

        // From straight left, only its left side is in view.
        let mut side = Vision::new(&level, f32::INFINITY);
        side.update(&level, [5.0, 15.0]);
        assert!(side.is_explored(2, 1), "left side seen");

        // From above left of the second slope (right of the wall): only
        // its diagonal face and open half face this way.
        let mut face = Vision::new(&level, f32::INFINITY);
        face.update(&level, [55.0, 5.0]);
        assert!(face.is_explored(6, 1));
    }

    #[test]
    fn slopes_light_up_as_visible_solids() {
        let level = Level::from_ascii(&["....#....", "..◸.#.◿..", "....#...."]);
        let mut vision = Vision::new(&level, f32::INFINITY);
        vision.update(&level, [35.0, 25.0]);
        let lit = vision.visible_solids();
        let has = |p: [f32; 2]| lit.iter().any(|s| s.contains(&p));
        // The first slope's triangle (with its corner at (20,10)) is lit;
        // the second's (corner at (70,20)) is behind the wall.
        assert!(has([20.0, 10.0]) && has([30.0, 10.0]) && has([20.0, 20.0]));
        assert!(!has([70.0, 20.0]));
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
