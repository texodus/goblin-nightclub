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

use crate::geom::{
    Shape, area, bounds, centroid, circle_crossings, clip_convex, distance, inward_normal, toward,
};
use crate::layer::{Edge, Layer, Line, TILE};
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
                    let c = centroid(&shape);
                    std::iter::once(c)
                        .chain(shape.iter().map(|p| toward(*p, c, 0.5)))
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
            let [lo, hi] = bounds(piece);
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
    fn visible_ish(level: &Layer, vision: &Vision, p: [f32; 2]) -> bool {
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
    fn visible_with_margin(level: &Layer, vision: &Vision, p: [f32; 2]) -> bool {
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
            let d = distance(origin, p);
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
