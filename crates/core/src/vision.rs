//! Line of sight for one viewer: an exact visibility polygon (for rendering)
//! plus point queries and tile exploration (for game logic). Both derive
//! from [`Layer::los`]/[`Layer::raycast`], so what is drawn as visible and
//! what the game treats as visible agree.
//!
//! Props can block sight too (see [`crate::prop`]), each as a small circle
//! around its tile's center ([`Circle`]): the polygon follows the near side
//! of each, with a ray to either side of it and a few across it, and its
//! shadow behind. Circles are found by the angle they span from the
//! viewer, so each ray tests only the few it might hit; so a forest costs
//! about as much as its circles in view, a few rays each.
//!
//! A cheaper alternative, if circles in the polygon cost too much: leave
//! props out of it, and have the renderer draw each one's shadow (a wedge
//! from its circle's tangents outward) into the visible mask, an instanced
//! draw per chunk of each chunk's circles, kept on the GPU. That's next to
//! nothing per frame however many there are, and shadows could be partial
//! (a tree dims what's behind it; several hide it). What's seen of NPCs
//! and what's remembered would then need a matching test here: how much
//! light gets through the circles along a line.
//!
//! Lights (a torch's: see [`Lamp`]) show more than the viewer's own radius:
//! what a lamp lights, the viewer sees wherever it has a line of sight to
//! it, however far, so long as the lamp lights something the viewer sees
//! by itself. What a lamp lights is what it would see, were it a viewer:
//! worked out once, since lamps stay put. The viewer's polygon then goes
//! as far as any such lamp's light does, and is seen where it's within the
//! viewer's radius or lit.

use std::f32::consts::TAU;
use std::sync::{Arc, OnceLock};

use crate::draw::inward_normal;
use crate::layer::{Edge, Layer, Line, Shape, TILE};
use crate::palette::Color;

/// Minimum angular offset of the extra rays cast to either side of each
/// corner, sampling the boundary just before and just after it.
const CORNER_EPS: f32 = 1e-4;

/// Rays per full turn used to approximate the radius circle.
const ARC_SAMPLES: usize = 128;

/// Rays closer than this (radians) are merged; well below `CORNER_EPS`.
const MERGE_EPS: f32 = 1e-6;

/// How far (pixels) the side casts at a corner pass it; well above the
/// vertex tolerance in [`Layer::raycast`].
const SIDE_OFFSET: f32 = 1e-3;

/// Distances along one ray closer than this (pixels) are the same point.
const SAME_POINT: f32 = 0.05;

struct Ray {
    /// Normalized to `[0, TAU)`.
    angle: f32,
    /// Corners lying on this ray (several when collinear with the origin).
    corners: Vec<[f32; 2]>,
    /// Where it grazes a circle's side: how far the circle is along it,
    /// whether that's where the circle's shadow starts (the boundary comes
    /// in from beyond it) or ends (and it goes back out), and the circle's
    /// center.
    tangent: Option<(f32, bool, [f32; 2])>,
}

impl Ray {
    fn new(angle: f32, corners: Vec<[f32; 2]>) -> Self {
        Self {
            angle: angle.rem_euclid(TAU),
            corners,
            tangent: None,
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

/// The part of convex polygon `subject` inside convex polygon `clip`
/// (clockwise on screen), by Sutherland-Hodgman.
fn clip_convex(subject: &[[f32; 2]], clip: &[[f32; 2]]) -> Vec<[f32; 2]> {
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

/// Unsigned area of a polygon, measured from its first point so slivers
/// far from the origin don't drown in rounding.
fn area(points: &[[f32; 2]]) -> f32 {
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

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    crate::length([b[0] - a[0], b[1] - a[1]])
}

/// A circle sight can't pass: a prop's (see [`crate::prop`]), around its
/// tile's center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub center: [f32; 2],
    pub radius: f32,
}

/// A light on a layer: a prop's (see [`crate::tileset::Light`]), at its
/// tile's center.
pub struct Lamp {
    pub center: [f32; 2],
    /// How far it reaches, in world units.
    pub radius: f32,
    pub color: Color,
    lit: OnceLock<Arc<Lit>>,
}

impl Lamp {
    pub fn new(center: [f32; 2], radius: f32, color: Color) -> Self {
        Self {
            center,
            radius,
            color,
            lit: OnceLock::new(),
        }
    }

    /// What it lights of `layer`, the one it's on, with `circles` in the
    /// way: the same every time, so worked out once.
    pub fn lit(&self, layer: &Layer, circles: &[Circle]) -> &Arc<Lit> {
        self.lit
            .get_or_init(|| Arc::new(Lit::new(layer, self, circles)))
    }
}

/// What a lamp lights: what it would see, were it a viewer.
pub struct Lit {
    pub center: [f32; 2],
    pub radius: f32,
    pub color: Color,
    polygon: Vec<[f32; 2]>,
    band: Vec<Shape>,
    /// The stretches of its layer's outline edges it lights (see
    /// [`Vision::stretches`]), by edge.
    stretches: Vec<(usize, f32, f32)>,
    /// The tiles any of which it lights.
    tiles: Vec<[usize; 2]>,
    blocking: Circles,
}

impl Lit {
    fn new(layer: &Layer, lamp: &Lamp, circles: &[Circle]) -> Self {
        let mut v = Vision::new(layer, lamp.radius);
        let around = Around {
            circles,
            lamps: &[],
        };
        v.update_through(layer, layer, None, None, lamp.center, around);
        Self {
            center: lamp.center,
            radius: lamp.radius,
            color: lamp.color,
            stretches: {
                let mut s = std::mem::take(&mut v.stretches);
                s.sort_unstable_by_key(|s| s.0);
                s
            },
            tiles: v.explored_tiles().collect(),
            polygon: std::mem::take(&mut v.polygon),
            band: std::mem::take(&mut v.wall_band),
            blocking: std::mem::take(&mut v.blocking),
        }
    }

    /// The boundary of what it lights, by angle around its center (see
    /// [`Vision::polygon`]).
    pub fn polygon(&self) -> &[[f32; 2]] {
        &self.polygon
    }

    /// The solid it lights, as convex pieces (see [`Vision::wall_band`]).
    pub fn band(&self) -> &[Shape] {
        &self.band
    }

    /// Does it light point `p`? `level` is what blocks sight on its layer.
    pub fn lights(&self, level: &Layer, p: [f32; 2]) -> bool {
        distance(self.center, p) <= self.radius
            && level.los(self.center, p)
            && self.blocking.clear(p)
    }

    /// The stretches of outline edge `k` it lights: from and to, along it.
    fn lit_along(&self, k: usize) -> impl Iterator<Item = (f32, f32)> + '_ {
        let first = self.stretches.partition_point(|s| s.0 < k);
        (self.stretches[first..].iter())
            .take_while(move |s| s.0 == k)
            .map(|s| (s.1, s.2))
    }
}

/// What stands on a layer that matters to sight, besides its tiles.
#[derive(Clone, Copy, Default)]
pub struct Around<'a> {
    /// Props' circles in the way of sight.
    pub circles: &'a [Circle],
    /// Lights.
    pub lamps: &'a [Lamp],
}

/// Bins the circles in reach are sorted into, by the angles they span
/// from the viewer.
const CIRCLE_BINS: usize = 256;

/// The circles in reach of the viewer, by the angles they span.
#[derive(Default)]
struct Circles {
    origin: [f32; 2],
    circles: Vec<Circle>,
    /// Where each bin's indices into `circles` start in `items`, then the
    /// end.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Circles {
    /// Those of `circles` within `radius` of `origin`, which mustn't be in
    /// one.
    fn gather(&mut self, origin: [f32; 2], radius: f32, circles: &[Circle]) {
        self.origin = origin;
        self.circles.clear();
        self.circles.extend(circles.iter().filter(|c| {
            let d = distance(origin, c.center);
            d > c.radius && d - c.radius <= radius
        }));
        // Each one's bins, then sorted into them (counted, then placed).
        let bins = |c: &Circle| {
            let d = distance(origin, c.center);
            // A hair wider, against rounding between this and a ray's angle.
            let half = (c.radius / d).asin() + 1e-4;
            let a = angle(origin, c.center);
            let step = TAU / CIRCLE_BINS as f32;
            let (lo, hi) = (
                ((a - half) / step).floor() as i32,
                ((a + half) / step).floor() as i32,
            );
            (lo..=hi).map(|b| b.rem_euclid(CIRCLE_BINS as i32) as usize)
        };
        let mut starts = vec![0u32; CIRCLE_BINS + 1];
        for c in &self.circles {
            for b in bins(c) {
                starts[b + 1] += 1;
            }
        }
        for b in 1..starts.len() {
            starts[b] += starts[b - 1];
        }
        let mut next = starts.clone();
        self.items = vec![0; *starts.last().unwrap() as usize];
        for (i, c) in self.circles.iter().enumerate() {
            for b in bins(c) {
                self.items[next[b] as usize] = i as u32;
                next[b] += 1;
            }
        }
        self.starts = starts;
    }

    /// The circles a ray at `angle` might hit.
    fn near(&self, angle: f32) -> impl Iterator<Item = &Circle> {
        let b = ((angle.rem_euclid(TAU) / TAU * CIRCLE_BINS as f32) as usize).min(CIRCLE_BINS - 1);
        let items = match self.starts.get(b + 1) {
            Some(&end) => &self.items[self.starts[b] as usize..end as usize],
            None => &[],
        };
        items.iter().map(|&i| &self.circles[i as usize])
    }

    /// How far a ray from the origin at `angle`, heading `dir` (a unit
    /// vector), goes before a circle stops it: at most `max`.
    fn cast(&self, angle: f32, dir: [f32; 2], max: f32) -> f32 {
        self.near(angle)
            .filter_map(|c| hit(self.origin, dir, c))
            .fold(max, f32::min)
    }

    /// The same, but for the circle centered at `skip`.
    fn cast_except(&self, angle: f32, dir: [f32; 2], max: f32, skip: [f32; 2]) -> f32 {
        self.near(angle)
            .filter(|c| c.center != skip)
            .filter_map(|c| hit(self.origin, dir, c))
            .fold(max, f32::min)
    }

    /// Does no circle stand between the origin and `p` (but any `p` is in:
    /// a prop's own tile is seen at its center)?
    fn clear(&self, p: [f32; 2]) -> bool {
        let d = distance(self.origin, p);
        if d == 0.0 || self.circles.is_empty() {
            return true;
        }
        let dir = [(p[0] - self.origin[0]) / d, (p[1] - self.origin[1]) / d];
        self.near(angle(self.origin, p))
            .filter(|c| distance(p, c.center) > c.radius)
            .all(|c| hit(self.origin, dir, c).is_none_or(|t| t >= d))
    }
}

/// How far along a ray from `origin` (outside `c`) heading `dir` (a unit
/// vector) it meets circle `c`, if it does.
fn hit(origin: [f32; 2], dir: [f32; 2], c: &Circle) -> Option<f32> {
    // How far along the ray its center is, and how far to one side: worked
    // out directly (not as a difference of squares, which rounding swamps
    // for a ray that grazes it), so which side of it a ray passes is
    // decided as finely as rays are aimed.
    let to = [c.center[0] - origin[0], c.center[1] - origin[1]];
    let along = to[0] * dir[0] + to[1] * dir[1];
    let aside = (to[0] * dir[1] - to[1] * dir[0]).abs();
    if aside >= c.radius {
        return None;
    }
    let t = along - (c.radius * c.radius - aside * aside).sqrt();
    (t >= 0.0).then_some(t)
}

pub struct Vision {
    origin: [f32; 2],
    /// View distance in pixels; `f32::INFINITY` for unlimited.
    radius: f32,
    /// How far the polygon goes: the radius, or further as far as the
    /// light of any lamp near enough goes.
    reach: f32,
    /// What the lamps that light anything the viewer sees by itself light.
    lit: Vec<Arc<Lit>>,
    /// Boundary of the visible region, by increasing angle around `origin`
    /// (clockwise on screen, since y points down). Render as a triangle fan
    /// centered on `origin`.
    polygon: Vec<[f32; 2]>,
    /// Lit solid, as convex pieces: within one tile of the visible stretches
    /// of wall, or for a layer seen from above, whole walls whose tops show.
    wall_band: Vec<Shape>,
    /// The visible stretches of outline edges: each edge's index, and
    /// from and to, along it from its `a`.
    stretches: Vec<(usize, f32, f32)>,
    /// Per outline edge of the level: visible now, and ever seen.
    edge_visible: Vec<bool>,
    edge_seen: Vec<bool>,
    cols: usize,
    explored: Vec<bool>,
    /// Where this layer shows through to the viewer's (see
    /// [`crate::level::Level::shaft`]); everywhere when `None`.
    shaft: Option<Vec<bool>>,
    /// The circles of props in the way, from the last update.
    blocking: Circles,
    /// The tiles of the props among them seen (see [`Vision::props_seen`]).
    props_seen: Vec<[i32; 2]>,
}

impl Vision {
    pub fn new(level: &Layer, radius: f32) -> Self {
        Self {
            origin: [0.0; 2],
            radius,
            reach: radius,
            lit: Vec::new(),
            polygon: Vec::new(),
            wall_band: Vec::new(),
            stretches: Vec::new(),
            edge_visible: vec![false; level.outline_edges().len()],
            edge_seen: vec![false; level.outline_edges().len()],
            cols: level.cols,
            explored: vec![false; level.cols * level.rows],
            shaft: None,
            blocking: Circles::default(),
            props_seen: Vec::new(),
        }
    }

    pub fn origin(&self) -> [f32; 2] {
        self.origin
    }

    pub fn radius(&self) -> f32 {
        self.radius
    }

    /// Change the view distance, from the next update on.
    pub fn set_radius(&mut self, radius: f32) {
        self.radius = radius;
    }

    /// How far the polygon goes: see [`Vision::polygon`].
    pub fn reach(&self) -> f32 {
        self.reach
    }

    /// Boundary of what's in line of sight, out to [`Vision::reach`]: the
    /// radius, or further where lamps light what's beyond it. Seen within
    /// the radius, and beyond it where [`Vision::lights`] light it.
    pub fn polygon(&self) -> &[[f32; 2]] {
        &self.polygon
    }

    /// What's lit by the lamps that light anything seen within the radius:
    /// seen too, wherever it's in line of sight.
    pub fn lights(&self) -> &[Arc<Lit>] {
        &self.lit
    }

    /// The tiles of the props in the way of sight that are seen (any of
    /// their circles): each shown whole, wherever its art is opaque, since
    /// the polygon itself stops at the near side of its circle.
    pub fn props_seen(&self) -> &[[i32; 2]] {
        &self.props_seen
    }

    /// Solid within one tile of the visible stretches of wall, as convex
    /// pieces: what's seen of solid space, since the polygon itself stops at
    /// wall faces.
    pub fn wall_band(&self) -> &[Shape] {
        &self.wall_band
    }

    /// For each of the level's outline edges: is any of it visible now?
    pub fn edge_visible(&self) -> &[bool] {
        &self.edge_visible
    }

    /// For each of the level's outline edges: has any of it ever been seen?
    pub fn edge_seen(&self) -> &[bool] {
        &self.edge_seen
    }

    /// Recompute visibility from `origin` (a pixel-space point in open space).
    pub fn update(&mut self, level: &Layer, origin: [f32; 2]) {
        self.update_through(level, level, None, None, origin, Around::default());
    }

    /// Recompute visibility of a layer `own` seen from above: through the
    /// combined walls `sight` of every layer from it up to the viewer's,
    /// and only within `shaft` (tiles where the layers above are void).
    /// The polygon follows `sight`; faces, the wall band and exploration
    /// belong to `own`.
    ///
    /// Seen from above, it's walls' tops that show, so with `above` (the
    /// walls of just the layers above `own`, which is then also `sight`)
    /// each wall tile whose top is in view lights up whole, instead of a
    /// band behind visible faces.
    ///
    /// Sight can't pass `around`'s circles either (props in the way: see
    /// [`Circle`]), and each seen lights up whole. Its lamps (on `own`, the
    /// layer they're on, which `sight` must block as `own` does) show what
    /// they light beyond the radius, if they light anything seen within
    /// it.
    pub fn update_through(
        &mut self,
        sight: &Layer,
        own: &Layer,
        shaft: Option<&[bool]>,
        above: Option<&Layer>,
        origin: [f32; 2],
        around: Around,
    ) {
        self.origin = origin;
        // The lamps whose light may reach into the radius: seen as far as
        // their light goes.
        let near: Vec<&Lamp> = (around.lamps.iter())
            .filter(|l| distance(origin, l.center) < self.radius + l.radius)
            .collect();
        self.reach = (near.iter())
            .map(|l| distance(origin, l.center) + l.radius)
            .fold(self.radius, f32::max);
        self.blocking.gather(origin, self.reach, around.circles);
        // Kept (it's read until the next update), but rarely changes.
        match (&mut self.shaft, shaft) {
            (Some(mine), Some(shaft)) if mine.as_slice() != shaft => mine.clone_from_slice(shaft),
            (Some(_), Some(_)) => {}
            (mine, shaft) => *mine = shaft.map(<[bool]>::to_vec),
        }
        self.rebuild_polygon(sight);
        // Of those, the ones lighting anything seen within the radius.
        self.lit.clear();
        for lamp in near {
            let lit = lamp.lit(own, around.circles);
            if self.sees_lit(sight, own, lit) {
                self.lit.push(lit.clone());
            }
        }
        self.find_wall_band(own);
        if let Some(above) = above {
            self.find_wall_tops(own, above);
        }
        self.light_circles(sight);
        self.mark_explored(sight, own);
    }

    /// Note each circle any of which is seen (its near side, just in
    /// front of it), and explore its tile.
    fn light_circles(&mut self, sight: &Layer) {
        let origin = self.origin;
        self.props_seen.clear();
        for i in 0..self.blocking.circles.len() {
            let c = self.blocking.circles[i];
            let d = distance(origin, c.center);
            let toward = [(origin[0] - c.center[0]) / d, (origin[1] - c.center[1]) / d];
            let front = c.radius + 0.5;
            let p = [
                c.center[0] + toward[0] * front,
                c.center[1] + toward[1] * front,
            ];
            if !self.can_see(sight, p) {
                continue;
            }
            let tile = c.center.map(|v| (v / TILE).floor() as i32);
            self.props_seen.push(tile);
            if tile[0] >= 0 && tile[1] >= 0 && (tile[0] as usize) < self.cols {
                let i = tile[1] as usize * self.cols + tile[0] as usize;
                if let Some(e) = self.explored.get_mut(i) {
                    *e = true;
                }
            }
        }
    }

    /// Light each of `own`'s wall tiles whose top is in view: in the shaft,
    /// in range, and some point of it (center, or a corner pulled slightly
    /// in) not hidden by `above`. Its faces count as seen, and the tile as
    /// explored.
    ///
    /// A wall tile beside the shaft (so under a wall or floor of a layer
    /// above) lights up too if it borders the shaft where that's in view.
    /// Exactly covered, it couldn't be seen; but lower layers are drawn a
    /// little smaller (parallax), which brings the edge of such a wall
    /// into view at the side of the hole.
    fn find_wall_tops(&mut self, own: &Layer, above: &Layer) {
        let mut lit = Vec::new();
        self.wall_band.clear();
        let (origin, radius) = (self.origin, self.radius);
        let visible = |p: [f32; 2]| distance(p, origin) <= radius && above.los(origin, p);
        // What's seen of a tile is in it or on its border, so only tiles
        // within a tile of the radius can be.
        let (xs, ys) = self.tiles_within(own, radius + TILE);
        for y in ys {
            for x in xs.clone() {
                let Some(shape) = own.solid(x as i32, y as i32).shape(x, y) else {
                    continue;
                };
                let seen = if self.in_shaft([x as i32, y as i32]) {
                    let n = shape.len() as f32;
                    let c = shape
                        .iter()
                        .fold([0.0; 2], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
                    std::iter::once(c)
                        .chain(shape.iter().map(|p| {
                            let d = distance(*p, c);
                            [
                                p[0] + (c[0] - p[0]) / d * 0.5,
                                p[1] + (c[1] - p[1]) / d * 0.5,
                            ]
                        }))
                        .any(visible)
                } else {
                    self.borders_void_in_view(x as i32, y as i32, &visible)
                };
                if seen {
                    lit.push([x as i32, y as i32]);
                    self.explored[y * self.cols + x] = true;
                    self.wall_band.push(shape);
                }
            }
        }
        if lit.is_empty() {
            return;
        }
        lit.sort_unstable();
        for k in own.edges_near(origin, radius + 2.0 * TILE) {
            if lit.binary_search(&own.outline_edges()[k].solid).is_ok() {
                self.edge_visible[k] = true;
                self.edge_seen[k] = true;
            }
        }
    }

    /// The columns and rows of tiles of `level` with any of them within
    /// `reach` of the origin (all of them for an infinite reach), and
    /// maybe a few more.
    fn tiles_within(
        &self,
        level: &Layer,
        reach: f32,
    ) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let span = |axis: usize, n: usize| {
            if !reach.is_finite() {
                return 0..n;
            }
            let lo = ((self.origin[axis] - reach) / TILE).floor().max(0.0) as usize;
            let hi = (((self.origin[axis] + reach) / TILE).ceil().max(0.0) as usize).min(n);
            lo.min(hi)..hi
        };
        (span(0, level.cols), span(1, level.rows))
    }

    /// Is a neighbor of tile `(x, y)` (across a side or a corner) in the
    /// shaft, with the point of it next to `(x, y)` (the middle of the
    /// shared side, or the shared corner, pulled slightly in) `visible`?
    fn borders_void_in_view(&self, x: i32, y: i32, visible: &impl Fn([f32; 2]) -> bool) -> bool {
        const INSET: f32 = 0.5;
        (-1..=1).any(|dy: i32| {
            (-1..=1).any(|dx: i32| {
                if (dx, dy) == (0, 0) || !self.in_shaft([x + dx, y + dy]) {
                    return false;
                }
                let at =
                    |t: i32, d: i32| (t as f32 + 0.5 + d as f32 * 0.5) * TILE + d as f32 * INSET;
                visible([at(x, dx), at(y, dy)])
            })
        })
    }

    /// Does tile `t` show through to the viewer?
    fn in_shaft(&self, t: [i32; 2]) -> bool {
        let Some(shaft) = &self.shaft else {
            return true;
        };
        let rows = (shaft.len() / self.cols) as i32;
        t[0] >= 0
            && t[1] >= 0
            && t[0] < self.cols as i32
            && t[1] < rows
            && shaft[t[1] as usize * self.cols + t[0] as usize]
    }

    /// Can the viewer currently see pixel-space point `p`? `level` is what
    /// blocks sight (the sight grid, for a layer seen from above).
    pub fn can_see(&self, level: &Layer, p: [f32; 2]) -> bool {
        let d = distance(self.origin, p);
        d <= self.reach
            && (d <= self.radius || self.lit.iter().any(|l| l.lights(level, p)))
            && self.in_line(level, p)
    }

    /// Is `p` in line of sight, however far?
    fn in_line(&self, level: &Layer, p: [f32; 2]) -> bool {
        let tile = [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32];
        self.in_shaft(tile) && level.los(self.origin, p) && self.blocking.clear(p)
    }

    /// Is anything `lit` lights seen within the radius? Only its tiles' sight
    /// samples are looked at (see [`Layer::sight_samples`]), of `own`.
    fn sees_lit(&self, sight: &Layer, own: &Layer, lit: &Lit) -> bool {
        let near = self.radius + TILE;
        let center = |[x, y]: [usize; 2]| [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE];
        (lit.tiles.iter())
            .filter(|t| distance(center(**t), self.origin) <= near)
            .flat_map(|&[x, y]| own.sight_samples(x as i32, y as i32))
            .any(|p| {
                distance(p, self.origin) <= self.radius
                    && lit.lights(sight, p)
                    && self.in_line(sight, p)
            })
    }

    /// Has any part of tile `(x, y)` ever been visible?
    pub fn is_explored(&self, x: usize, y: usize) -> bool {
        self.explored[y * self.cols + x]
    }

    /// The tiles ever visible, to keep when the layer is made again (as
    /// another window onto a bigger level).
    pub fn explored_tiles(&self) -> impl Iterator<Item = [usize; 2]> + '_ {
        let cols = self.cols;
        (self.explored.iter().enumerate())
            .filter(|(_, e)| **e)
            .map(move |(i, _)| [i % cols, i / cols])
    }

    /// The outline edges of `level` (the layer it was made for) ever seen,
    /// likewise.
    pub fn seen_edges<'a>(&'a self, level: &'a Layer) -> impl Iterator<Item = &'a Edge> + 'a {
        (level.outline_edges().iter().zip(&self.edge_seen))
            .filter(|(_, seen)| **seen)
            .map(|(e, _)| e)
    }

    /// Remember as explored each tile `(x, y)` that `explored` says, and
    /// as seen each of `level`'s edges (the layer it was made for) that
    /// `seen` says: what was, before the layer was made again.
    pub fn remember(
        &mut self,
        level: &Layer,
        explored: impl Fn(usize, usize) -> bool,
        seen: impl Fn(&Edge) -> bool,
    ) {
        let cols = self.cols;
        for (i, e) in self.explored.iter_mut().enumerate() {
            *e |= explored(i % cols, i / cols);
        }
        for (e, s) in level.outline_edges().iter().zip(&mut self.edge_seen) {
            *s |= seen(e);
        }
    }

    fn rebuild_polygon(&mut self, level: &Layer) {
        let origin = self.origin;
        let mut rays: Vec<Ray> = Vec::new();
        // Angular zones around each corner (center, half-width) in which a
        // plain ray can't be trusted to land on the right side of it.
        let mut zones: Vec<(f32, f32)> = Vec::new();
        for i in level.corners_near(origin, self.reach) {
            let c = level.corners()[i];
            // Corners outside the radius can't shape the visible region.
            if distance(origin, c) > self.reach {
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
        // Each circle: along its sides (where its shadow starts and ends),
        // and across its near side, more the nearer (the wider) it is.
        for c in &self.blocking.circles {
            let (a, d) = (angle(origin, c.center), distance(origin, c.center));
            let half = (c.radius / d).asin();
            let near = (d * d - c.radius * c.radius).max(0.0).sqrt();
            for (side, starts) in [(-half, true), (half, false)] {
                let mut ray = Ray::new(a + side, vec![]);
                ray.tangent = Some((near, starts, c.center));
                rays.push(ray);
            }
            let across = ((2.0 * half / 0.15).ceil() as usize).max(3);
            rays.extend(
                (1..across)
                    .map(|i| Ray::new(a - half + 2.0 * half * i as f32 / across as f32, vec![])),
            );
        }
        if self.reach.is_finite() {
            rays.extend(
                (0..ARC_SAMPLES).map(|i| Ray::new(i as f32 * TAU / ARC_SAMPLES as f32, vec![])),
            );
            // Along a wall, the boundary can only switch between wall and
            // arc where the wall crosses the circle; elsewhere it switches at
            // corners. So sample either side of every such crossing, or the
            // chord between neighboring rays could cut off visible floor.
            let eps = corner_eps(self.reach);
            for k in level.edges_near(origin, self.reach) {
                let e = &level.outline_edges()[k];
                for p in circle_crossings(origin, self.reach, e.a, e.b) {
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
        // Zones differ in width, so check every one whose center is within
        // the widest half-width (looking across the wrap at TAU too).
        let widest = zones.iter().map(|z| z.1).fold(0.0, f32::max);
        let in_zone = |a: f32| {
            [a - TAU, a, a + TAU].iter().any(|&a| {
                let first = zones.partition_point(|z| z.0 < a - widest);
                zones[first..]
                    .iter()
                    .take_while(|z| z.0 <= a + widest)
                    .any(|&(center, half)| (a - center).abs() < half)
            })
        };
        rays.retain(|r| !r.corners.is_empty() || r.tangent.is_some() || !in_zone(r.angle));

        // Rays closer together than float precision can resolve may sort in
        // an order that disagrees with their actual directions, so merge
        // them (including across the wrap at TAU), pooling their corners.
        // Likewise a corner's ray passing within `SIDE_OFFSET` of a nearer
        // corner: too close to trust which side of it the ray lands on, so
        // the group aims at the nearer one.
        let band = |r: &Ray| {
            r.corners
                .iter()
                .map(|c| SIDE_OFFSET / distance(origin, *c))
                .fold(0.0, f32::max)
        };
        let close = |a: &Ray, b: &Ray, gap: f32| {
            gap < MERGE_EPS
                || (!a.corners.is_empty() && !b.corners.is_empty() && gap < band(a).max(band(b)))
        };
        rays.dedup_by(|next, prev| {
            let merge = close(prev, next, next.angle - prev.angle);
            if merge {
                prev.corners.append(&mut next.corners);
                prev.tangent = prev.tangent.or(next.tangent);
            }
            merge
        });
        if rays.len() > 1 {
            let gap = rays[0].angle + TAU - rays[rays.len() - 1].angle;
            if close(&rays[rays.len() - 1], &rays[0], gap) {
                let mut last = rays.pop().unwrap();
                rays[0].corners.append(&mut last.corners);
            }
        }

        let blocking = &self.blocking;
        let cast = |angle: f32| {
            let d = dir(angle);
            let walls = level.raycast(origin, d, self.reach);
            blocking.cast(angle, d, walls)
        };
        // Plain rays contribute their hit point. At a corner ray the
        // boundary can jump along the ray: it arrives from just before the
        // ray at some distance, runs out as far as the ray reaches, and
        // leaves for just after it at some distance. E.g. at a shadow edge it
        // goes corner -> far wall, or far wall -> corner; with collinear
        // corners, corner -> far -> corner. All three points lie on the ray
        // itself so they stay in angular order.
        self.polygon.clear();
        for ray in &rays {
            if let (Some((near, starts, center)), true) = (ray.tangent, ray.corners.is_empty()) {
                // Beyond the circle: as far as this ray goes, but for the
                // circle it grazes, and no further than a cast just past
                // it. Where its shadow starts the boundary comes in from
                // there to the circle; where it ends, it goes back out.
                let side = if starts { -1.0 } else { 1.0 };
                let d = dir(ray.angle);
                let walls = level.raycast(origin, d, self.reach);
                let exact = blocking.cast_except(ray.angle, d, walls, center);
                let far = exact.min(cast(ray.angle + side * corner_eps(near)));
                if far <= near + SAME_POINT {
                    self.polygon.push(at(origin, ray.angle, far));
                    continue;
                }
                let [first, then] = if starts { [far, near] } else { [near, far] };
                self.polygon.push(at(origin, ray.angle, first));
                self.polygon.push(at(origin, ray.angle, then));
                continue;
            }
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
            // Around the aim, not the group's angle, which may be that of a
            // farther corner merged in.
            let base = angle(origin, nearest);
            let (before, after) = (cast(base - side), cast(base + side));
            let reach = blocking
                .cast(base, aim, level.raycast(origin, aim, self.reach))
                .min(before.max(after));
            let along = |d: f32| [origin[0] + aim[0] * d, origin[1] + aim[1] * d];

            // Where the boundary arrives at (and leaves) this ray can only be
            // one of its corners, or its reach (a surface continuing across
            // it). The side casts say which: snap each to the nearest. Their
            // own distances aren't used directly, since against a surface
            // seen at a grazing angle they drift fast with angle.
            // Only corners actually on the aim line count: a corner merged
            // in from a nearer one's zone may lie a little to one side.
            let on_line: Vec<[f32; 2]> = ray
                .corners
                .iter()
                .copied()
                .filter(|c| {
                    let to = [c[0] - origin[0], c[1] - origin[1]];
                    (aim[0] * to[1] - aim[1] * to[0]).abs() < SIDE_OFFSET
                })
                .collect();
            let corner_dists: Vec<f32> = on_line
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
                on_line
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

    /// The visible stretches of wall are exactly the polygon's edges lying
    /// along outline edges. Mark those outline edges visible, extend each
    /// stretch one tile into solid, add the wedge behind every inside corner
    /// a stretch reaches (where the two extensions part), and clip it all to
    /// the solid parts of the tiles it covers.
    fn find_wall_band(&mut self, level: &Layer) {
        const ON: f32 = 1e-2;
        self.edge_visible.fill(false);
        self.wall_band.clear();
        self.stretches.clear();
        let mut band: Vec<Shape> = Vec::new();
        let mut reached: Vec<[f32; 2]> = Vec::new();

        let n = self.polygon.len();
        for i in 0..n {
            let (p, q) = (self.polygon[i], self.polygon[(i + 1) % n]);
            // Even tiny polygon edges count: rays aimed at other corners
            // can split a face into very short pieces.
            if distance(p, q) < 1e-6 {
                continue;
            }
            let candidates = Line::through(p, q, ON)
                .into_iter()
                .flat_map(|l| level.edges_on(l));
            for &k in candidates {
                let e = &level.outline_edges()[k];
                let len = distance(e.a, e.b);
                let u = [(e.b[0] - e.a[0]) / len, (e.b[1] - e.a[1]) / len];
                let along = |x: [f32; 2]| (x[0] - e.a[0]) * u[0] + (x[1] - e.a[1]) * u[1];
                let off = |x: [f32; 2]| (x[1] - e.a[1]) * u[0] - (x[0] - e.a[0]) * u[1];
                if off(p).abs() > ON || off(q).abs() > ON {
                    continue;
                }
                let (tp, tq) = (along(p), along(q));
                let (lo, hi) = (tp.min(tq).max(0.0), tp.max(tq).min(len));
                // Beyond the radius, only what's lit is seen: of the
                // stretch, what's within the radius, and what lamps light.
                let mut pieces = vec![(lo, hi)];
                if self.reach > self.radius {
                    let w = [e.a[0] - self.origin[0], e.a[1] - self.origin[1]];
                    let b = w[0] * u[0] + w[1] * u[1];
                    let disc = b * b - (w[0] * w[0] + w[1] * w[1] - self.radius * self.radius);
                    let s = disc.max(0.0).sqrt();
                    pieces[0] = (lo.max(-b - s), hi.min(-b + s));
                    let lit = self.lit.iter().flat_map(|l| l.lit_along(k));
                    pieces.extend(lit.map(|(a, b)| (lo.max(a), hi.min(b))));
                }
                for (lo, hi) in pieces {
                    if hi - lo <= 1e-6 {
                        continue;
                    }
                    // Seen from above, only faces showing through count.
                    if !self.in_shaft(e.solid) && self.shaft.is_some() {
                        continue;
                    }
                    self.edge_visible[k] = true;
                    self.edge_seen[k] = true;
                    self.stretches.push((k, lo, hi));
                    if !level.in_bounds(e.solid) {
                        continue; // The level's boundary: nothing to light.
                    }
                    let nrm = inward_normal(e.a, e.b);
                    let s0 = [e.a[0] + u[0] * lo, e.a[1] + u[1] * lo];
                    let s1 = [e.a[0] + u[0] * hi, e.a[1] + u[1] * hi];
                    let deep = |x: [f32; 2]| [x[0] + nrm[0] * TILE, x[1] + nrm[1] * TILE];
                    band.push(vec![s0, s1, deep(s1), deep(s0)]);
                    reached.extend([s0, s1]);
                }
            }
        }

        // Every stretch reached is within reach.
        let near = if reached.is_empty() {
            Vec::new()
        } else {
            level.concave_near(self.origin, self.reach + TILE)
        };
        for c in near.into_iter().map(|k| &level.concave_corners()[k]) {
            if reached.iter().any(|r| distance(*r, c.at) < 1e-3) {
                let [p, a, b] = [c.at, c.before, c.after];
                let k = TILE / (1.0 + a[0] * b[0] + a[1] * b[1]);
                band.push(vec![
                    p,
                    [p[0] + a[0] * TILE, p[1] + a[1] * TILE],
                    [p[0] + (a[0] + b[0]) * k, p[1] + (a[1] + b[1]) * k],
                    [p[0] + b[0] * TILE, p[1] + b[1] * TILE],
                ]);
            }
        }

        for piece in &band {
            let (lo, hi) = piece.iter().fold(
                ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
                |(lo, hi), p| {
                    (
                        [lo[0].min(p[0]), lo[1].min(p[1])],
                        [hi[0].max(p[0]), hi[1].max(p[1])],
                    )
                },
            );
            let tiles = |a: usize, max: usize| {
                let first = ((lo[a] / TILE).floor().max(0.0)) as usize;
                let last = ((hi[a] / TILE).ceil() as usize).min(max);
                first..last
            };
            for y in tiles(1, level.rows) {
                for x in tiles(0, level.cols) {
                    if !self.in_shaft([x as i32, y as i32]) {
                        continue;
                    }
                    let Some(solid) = level.solid(x as i32, y as i32).shape(x, y) else {
                        continue;
                    };
                    let clipped = clip_convex(piece, &solid);
                    // Clipping at shared corners leaves degenerate slivers.
                    if area(&clipped) > 1e-3 {
                        self.wall_band.push(clipped);
                    }
                }
            }
        }
    }

    /// A tile is explored once any of its sight samples (in its open part,
    /// or on an exposed face) is seen, through `sight`.
    fn mark_explored(&mut self, sight: &Layer, own: &Layer) {
        let reach = self.radius + TILE * std::f32::consts::SQRT_2;
        let (xs, ys) = self.tiles_within(own, reach);
        for y in ys {
            for x in xs.clone() {
                let i = y * self.cols + x;
                if self.explored[i] || !self.in_shaft([x as i32, y as i32]) {
                    continue;
                }
                let center = [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE];
                if distance(center, self.origin) > reach {
                    continue;
                }
                self.explored[i] = own
                    .sight_samples(x as i32, y as i32)
                    .into_iter()
                    .any(|p| self.can_see(sight, p));
            }
        }
        // And beyond, what's lit.
        for lit in self.lit.clone() {
            for &[x, y] in &lit.tiles {
                let i = y * self.cols + x;
                if self.explored[i] || !self.in_shaft([x as i32, y as i32]) {
                    continue;
                }
                self.explored[i] = own
                    .sight_samples(x as i32, y as i32)
                    .into_iter()
                    .any(|p| self.can_see(sight, p));
            }
        }
    }
}

/// Invariant checks and helpers for tests, here and in other crates.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::*;
    use crate::rng::Rng;

    pub trait OpenPoint {
        fn open_point(&mut self, level: &Layer) -> [f32; 2];
    }

    impl OpenPoint for Rng {
        fn open_point(&mut self, level: &Layer) -> [f32; 2] {
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

    /// Clear of solid space in every direction (unlike a zero-size
    /// `collides` check, which never fires exactly on a grid vertex).
    pub fn is_open_point(level: &Layer, p: [f32; 2]) -> bool {
        !level.collides([p[0] - 0.01, p[1] - 0.01], [0.02, 0.02])
    }

    /// Crossing-number point-in-polygon test.
    pub fn inside(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
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

    /// Visible, allowing a hair of rotation: points on a line that exactly
    /// grazes a corner are visible or not only up to rounding.
    pub fn visible_ish(level: &Layer, vision: &Vision, p: [f32; 2]) -> bool {
        let o = vision.origin();
        let (a, d) = (angle(o, p), distance(o, p));
        // Several sizes: a slit between corners can be narrower than the
        // largest, and a corner close by needs the largest to clear it.
        vision.can_see(level, p)
            || [1e-5, 3e-6, 1e-6]
                .iter()
                .flat_map(|r| [a - r, a + r])
                .any(|a| vision.can_see(level, at(o, a, d)))
    }

    /// Visible, and still visible a hair of rotation either way: points
    /// seen only within rounding of a grazing line needn't be in the polygon.
    pub fn visible_with_margin(level: &Layer, vision: &Vision, p: [f32; 2]) -> bool {
        let o = vision.origin();
        let (a, d) = (angle(o, p), distance(o, p));
        vision.can_see(level, p)
            && [a - 1e-5, a + 1e-5]
                .iter()
                .all(|a| vision.can_see(level, at(o, *a, d)))
    }

    pub fn check_polygon(level: &Layer, vision: &Vision, rng: &mut Rng) {
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
        // Inside a circle is the prop itself, seen whole when its near side
        // is (see `Vision::light_circles`), not by the polygon.
        let in_circle = |p: [f32; 2]| {
            (vision.blocking.circles.iter()).any(|c| distance(p, c.center) <= c.radius)
        };
        for _ in 0..200 {
            let p = rng.open_point(level);
            let d = (p[0] - origin[0]).hypot(p[1] - origin[1]);
            if d < limit && !in_circle(p) && visible_with_margin(level, vision, p) {
                let q = [
                    origin[0] + 0.995 * (p[0] - origin[0]),
                    origin[1] + 0.995 * (p[1] - origin[1]),
                ];
                assert!(inside(poly, q), "visible {p:?} outside polygon, {ctx}");
            }
        }
    }

    /// Total lit area, and whether point `p` is lit.
    pub fn band_area(v: &Vision) -> f32 {
        v.wall_band().iter().map(|s| area(s)).sum()
    }

    pub fn band_covers(v: &Vision, p: [f32; 2]) -> bool {
        // Pieces may wind either way.
        v.wall_band().iter().any(|s| {
            // Signed distance from each edge's line.
            let sides: Vec<f32> = (0..s.len())
                .map(|i| {
                    let (a, b) = (s[i], s[(i + 1) % s.len()]);
                    ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) / distance(a, b)
                })
                .collect();
            sides.iter().all(|d| *d >= -1e-3) || sides.iter().all(|d| *d <= 1e-3)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use crate::rng::Rng;

    /// Tile soups are dense with slopes in every arrangement: the hardest
    /// case. (Generated levels are checked the same way in the
    /// `goblin-nightclub-generate` crate's tests.)
    const SOUPS: u32 = 16;

    /// Tile soups, each with a suggested origin (which may be solid).
    fn levels() -> impl Iterator<Item = (Layer, [f32; 2])> {
        let spawn = [165.0, 105.0];
        (0..SOUPS).map(move |seed| (Layer::soup(seed, 32, 20), spawn))
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
        let level = Layer::from_ascii(&[
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
        let single = Layer::from_ascii(&["....", ".#..", "....", "...."]);
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

    /// Just `circles` around.
    fn circles(circles: &[Circle]) -> Around<'_> {
        Around {
            circles,
            ..Default::default()
        }
    }

    /// A circle around tile `t`'s center, radius `r`.
    fn circle_at(t: [usize; 2], r: f32) -> Circle {
        let [x, y] = t.map(|v| v as f32 * TILE);
        Circle {
            center: [x + TILE / 2.0, y + TILE / 2.0],
            radius: r,
        }
    }

    #[test]
    fn circles_cast_shadows_and_light_up_seen() {
        let rows: Vec<String> = (0..20).map(|_| ".".repeat(30)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let level = Layer::from_ascii(&rows);
        let rock = circle_at([15, 10], 4.0);
        let origin = [50.0, 105.0];
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update_through(&level, &level, None, None, origin, circles(&[rock]));
        // Behind it, hidden; beside it, and its own tile, seen.
        let (behind, beside) = ([250.0, 105.0], [250.0, 140.0]);
        assert!(!v.can_see(&level, behind));
        assert!(v.can_see(&level, beside));
        assert!(v.can_see(&level, rock.center));
        assert!(!inside(v.polygon(), behind));
        assert!(inside(v.polygon(), beside));
        // Its shadow widens with distance: just past it, a little off the
        // line through it is seen; far behind, hidden.
        assert!(v.can_see(&level, [170.0, 112.0]));
        assert!(!v.can_see(&level, [290.0, 112.0]));
        // Seen (to be shown whole), and explored.
        assert_eq!(v.props_seen(), &[[15, 10]]);
        assert!(v.is_explored(15, 10));
        // Without it, nothing's hidden.
        v.update(&level, origin);
        assert!(v.can_see(&level, behind));

        // Behind a wall it isn't seen.
        let mut rows: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
        rows[10].replace_range(10..11, "#");
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let walled = Layer::from_ascii(&rows);
        let mut v = Vision::new(&walled, f32::INFINITY);
        v.update_through(
            &walled,
            &walled,
            None,
            None,
            origin,
            circles(&[circle_at([12, 10], 2.0)]),
        );
        assert!(v.props_seen().is_empty());
    }

    /// Just `lamps` around.
    fn lamps(lamps: &[Lamp]) -> Around<'_> {
        Around {
            lamps,
            ..Default::default()
        }
    }

    #[test]
    fn lamps_show_what_they_light_beyond_the_radius() {
        let rows: Vec<String> = (0..20).map(|_| ".".repeat(40)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let level = Layer::from_ascii(&rows);
        let origin = [55.0, 105.0];
        let lamp = [Lamp::new([105.0, 105.0], 40.0, [1.0; 4])];
        let mut v = Vision::new(&level, 30.0);
        v.update_through(&level, &level, None, None, origin, self::lamps(&lamp));
        assert_eq!(v.lights().len(), 1, "it lights what's seen");
        assert_eq!(v.reach(), 50.0 + 40.0);
        // Past the radius, lit: seen. Past its light too: not.
        assert!(v.can_see(&level, [140.0, 105.0]));
        assert!(!v.can_see(&level, [150.0, 105.0]));
        assert!(!v.can_see(&level, [55.0, 150.0]), "unlit, past the radius");
        assert!(v.is_explored(13, 10));
        assert!(!v.is_explored(16, 10));
        // The polygon goes as far.
        assert!(inside(v.polygon(), [140.0, 105.0]));
        // Further off, its light no longer reaches into the radius.
        v.update_through(
            &level,
            &level,
            None,
            None,
            [15.0, 105.0],
            self::lamps(&lamp),
        );
        assert!(v.lights().is_empty());
        assert_eq!(v.reach(), 30.0);
        assert!(!v.can_see(&level, [100.0, 105.0]));
    }

    #[test]
    fn lamps_count_only_if_they_light_whats_seen() {
        // A corridor, and a room off it through a door, its lamp lighting
        // a little of the corridor below the door.
        let level = Layer::from_ascii(&[
            "##############################",
            "#############.....############",
            "#############.....############",
            "###############.##############",
            "#............................#",
            "##############################",
        ]);
        let lamp = [Lamp::new([155.0, 15.0], 80.0, [1.0; 4])];
        let lit = [159.0, 45.0];
        assert!(lamp[0].lit(&level, &[]).lights(&level, lit));
        let origin = [25.0, 45.0];
        // Its light and the viewer's sight overlap, but walls part them:
        // what it lights in view is all beyond the radius.
        let mut v = Vision::new(&level, 60.0);
        v.update_through(&level, &level, None, None, origin, self::lamps(&lamp));
        assert!(v.lights().is_empty());
        assert!(!v.can_see(&level, lit));
        assert!(!v.is_explored(15, 4));
        // Seeing a little further, the near edge of what it lights is in
        // the radius, and the rest of it beyond seen too.
        let mut v = Vision::new(&level, 128.0);
        v.update_through(&level, &level, None, None, origin, self::lamps(&lamp));
        assert_eq!(v.lights().len(), 1);
        assert!(distance(origin, lit) > 128.0);
        assert!(v.can_see(&level, lit));
        // Not what's past its light, nor the room, out of line of sight.
        assert!(!v.can_see(&level, [175.0, 45.0]));
        assert!(!v.can_see(&level, [155.0, 15.0]));
        // The wall it lights below the door is seen, as far as it's lit.
        assert!(band_covers(&v, [160.0, 52.0]));
        assert!(!band_covers(&v, [175.0, 52.0]));
    }

    #[test]
    fn polygon_matches_los_around_circles() {
        let mut rng = Rng::new(17);
        for (level, spawn) in levels() {
            // Circles of every size a prop may have, on open tiles.
            let circles: Vec<Circle> = (0..60)
                .filter_map(|_| {
                    let t = [rng.range(0, level.cols - 1), rng.range(0, level.rows - 1)];
                    let open = level.solid(t[0] as i32, t[1] as i32) == crate::layer::Solid::None;
                    open.then(|| circle_at(t, 1.0 + rng.unit() * 3.9))
                })
                .collect();
            let outside = |o: [f32; 2]| {
                circles
                    .iter()
                    .all(|c| distance(o, c.center) > c.radius + 0.5)
            };
            let mut origins = vec![spawn];
            origins.extend((0..5).map(|_| rng.open_point(&level)));
            for origin in origins
                .into_iter()
                .filter(|o| is_open_point(&level, *o) && outside(*o))
            {
                for radius in [f32::INFINITY, 80.0] {
                    let mut vision = Vision::new(&level, radius);
                    vision.update_through(
                        &level,
                        &level,
                        None,
                        None,
                        origin,
                        self::circles(&circles),
                    );
                    check_polygon(&level, &vision, &mut rng);
                }
            }
        }
    }

    #[test]
    fn radius_limits_sight() {
        let level = Layer::from_ascii(&["..........", "..........", ".........."]);
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
    fn band_is_one_tile_behind_visible_faces() {
        // A 3x3 block; from straight left only its left face (30px) shows.
        let level = Layer::from_ascii(&[".....", ".###.", ".###.", ".###.", "....."]);
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update(&level, [5.0, 25.0]);
        assert!((band_area(&v) - 300.0).abs() < 1e-2, "{}", band_area(&v));
        assert!(band_covers(&v, [15.0, 25.0]) && !band_covers(&v, [25.0, 25.0]));

        // Out of range: nothing.
        let mut short = Vision::new(&level, 3.0);
        short.update(&level, [5.0, 25.0]);
        assert!(short.wall_band().is_empty());
    }

    #[test]
    fn band_fills_inside_corners() {
        // Walls along the top and left meet at an inside corner at (10,10).
        let level = Layer::from_ascii(&["#####", "#....", "#...."]);
        let mut v = Vision::new(&level, f32::INFINITY);
        v.update(&level, [30.0, 20.0]);
        // Top face 40px + left face 20px, a tile deep, plus the corner tile.
        assert!((band_area(&v) - 700.0).abs() < 1e-2, "{}", band_area(&v));
        assert!(band_covers(&v, [5.0, 5.0]), "corner tile lit");
    }

    #[test]
    fn block_and_doorless_room_light_the_same() {
        let block = Layer::from_ascii(&[
            ".......", ".#####.", ".#####.", ".#####.", ".#####.", ".......",
        ]);
        let room = Layer::from_ascii(&[
            ".......", ".#####.", ".#...#.", ".#...#.", ".#####.", ".......",
        ]);
        // The room's inner corners add rays, so the band may be cut into
        // different pieces; compare what it covers.
        for origin in [[5.0, 5.0], [35.0, 55.0], [65.0, 30.0]] {
            let view = |level: &Layer| {
                let mut v = Vision::new(level, f32::INFINITY);
                v.update(level, origin);
                v
            };
            let (b, r) = (view(&block), view(&room));
            assert!(
                (band_area(&b) - band_area(&r)).abs() < 1e-2,
                "from {origin:?}"
            );
            for y in 0..60 {
                for x in 0..70 {
                    let p = [x as f32 + 0.5, y as f32 + 0.5];
                    assert_eq!(
                        band_covers(&b, p),
                        band_covers(&r, p),
                        "from {origin:?} at {p:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn band_never_covers_open_floor() {
        let mut rng = Rng::new(3);
        for (level, spawn) in levels().take(50) {
            if !is_open_point(&level, spawn) {
                continue;
            }
            let mut v = Vision::new(&level, 96.0);
            v.update(&level, spawn);
            for _ in 0..300 {
                let p = rng.open_point(&level);
                assert!(!band_covers(&v, p) || !is_open_point(&level, p), "{p:?}");
            }
        }
    }

    #[test]
    fn slope_faces_light_their_triangle() {
        let level = Layer::from_ascii(&["....#....", "..◸.#.◿..", "....#...."]);
        let mut vision = Vision::new(&level, f32::INFINITY);
        vision.update(&level, [35.0, 25.0]);
        // Inside the first slope, just behind its face; the second slope is
        // behind the wall.
        assert!(band_covers(&vision, [22.0, 12.0]));
        assert!(!band_covers(&vision, [68.0, 18.0]));
    }

    #[test]
    fn slopes_explored_by_face_side_or_open_half() {
        // Slopes at (2,1) and (6,1); a wall at column 4 hides the second
        // from a viewer on the left.
        let level = Layer::from_ascii(&[
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
    fn inner_faces_of_a_room_stay_unseen_from_outside() {
        // A doorless room with 1-tile walls, viewed from all around it.
        let level = Layer::from_ascii(&[
            ".......", //
            ".#####.", //
            ".#...#.", //
            ".#...#.", //
            ".#####.", //
            ".......", //
        ]);
        let inner: Vec<usize> = level
            .outline_edges()
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                (20.0..=50.0).contains(&e.a[0])
                    && (20.0..=40.0).contains(&e.a[1])
                    && (20.0..=50.0).contains(&e.b[0])
                    && (20.0..=40.0).contains(&e.b[1])
            })
            .map(|(i, _)| i)
            .collect();
        assert_eq!(inner.len(), 10, "the hole's outline");
        let mut vision = Vision::new(&level, f32::INFINITY);
        for origin in [
            [5.0, 5.0],
            [65.0, 5.0],
            [35.0, 55.0],
            [5.0, 35.0],
            [65.0, 35.0],
        ] {
            vision.update(&level, origin);
            assert!(inner.iter().all(|&i| !vision.edge_visible()[i]));
        }
        assert!(inner.iter().all(|&i| !vision.edge_seen()[i]));
        assert!(vision.edge_seen().iter().any(|s| *s), "outer faces seen");
    }

    #[test]
    fn wall_tops_seen_from_above_light_whole() {
        // Below: a block, then a longer one behind it.
        let below = Layer::from_ascii(&["......", ".#.##.", "......"]);
        let look = |deck: &Layer| {
            let mut v = Vision::new(&below, f32::INFINITY);
            let sight = deck_sight(&below, deck);
            let shaft = vec![true; 18];
            v.update_through(
                &sight,
                &below,
                Some(&shaft),
                Some(deck),
                [5.0, 15.0],
                Around::default(),
            );
            band_area(&v)
        };
        // Nothing above: all three wall tops, though the far block's faces
        // are hidden behind the near one.
        let open = Layer::from_ascii(&["~~~~~~", "~~~~~~", "~~~~~~"]);
        assert!((look(&open) - 300.0).abs() < 1e-2, "{}", look(&open));
        // A wall on the layer above, between the viewer and the far block,
        // hides its top.
        let walled = Layer::from_ascii(&["~~~~~~", "~~#~~~", "~~~~~~"]);
        assert!((look(&walled) - 100.0).abs() < 1e-2, "{}", look(&walled));
    }

    #[test]
    fn walls_under_walls_light_beside_void_in_view() {
        // Below, a wall at each end; above, walls over both, void beside
        // the left one only, and floor where the viewer stands.
        let below = Layer::from_ascii(&["#....#"]);
        let deck = Layer::from_ascii(&["#~~~.#"]);
        let mut v = Vision::new(&below, f32::INFINITY);
        let sight = deck_sight(&below, &deck);
        let shaft = [false, true, true, true, false, false];
        v.update_through(
            &sight,
            &below,
            Some(&shaft),
            Some(&deck),
            [45.0, 5.0],
            Around::default(),
        );
        // The left wall, next to the hole, lights up; the right one, next
        // to floor, doesn't.
        assert!((band_area(&v) - 100.0).abs() < 1e-2, "{}", band_area(&v));
        assert!(v.is_explored(0, 0));
        assert!(!v.is_explored(5, 0));
        // Out of range of the hole's edge, it stays dark.
        let mut near = Vision::new(&below, 20.0);
        near.update_through(
            &sight,
            &below,
            Some(&shaft),
            Some(&deck),
            [45.0, 5.0],
            Around::default(),
        );
        assert_eq!(band_area(&near), 0.0);
    }

    /// The walls of both layers together, as a level would combine them.
    fn deck_sight(below: &Layer, deck: &Layer) -> Layer {
        let tiles = (0..below.rows as i32)
            .flat_map(|y| (0..below.cols as i32).map(move |x| (x, y)))
            .map(|(x, y)| below.solid(x, y).union(deck.solid(x, y)))
            .collect();
        Layer::from_solids(below.cols, below.rows, tiles)
    }

    #[test]
    fn exploration_accumulates() {
        let level = Layer::from_ascii(&[
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
