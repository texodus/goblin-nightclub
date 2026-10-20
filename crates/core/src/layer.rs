//! One layer of a level: a tile grid, with rays, collision and its outline.
//!
//! The grid is the whole truth about solid space: rays, collision, outlines,
//! drawing and lighting all derive from it. Its outline is computed once,
//! when the layer is built. Void and stairs tiles are open, like floor;
//! what they mean between layers is up to [`crate::level`].

use std::sync::Arc;

use crate::camera::Rect;
use crate::chunk::{CHUNK, Chunk, Chunks};
use crate::draw::{Canvas, faces_light, inset_corner, inward_normal};
use crate::palette;
#[cfg(any(test, feature = "test-util"))]
use crate::rng::Rng;
use crate::tileset::{self, Tileset};

/// Tile edge length in pixels.
pub const TILE: f32 = 10.0;

/// A corner of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    NW,
    NE,
    SW,
    SE,
}

/// A side of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    N,
    S,
    W,
    E,
}

impl Corner {
    /// Clockwise on screen, starting top-left.
    pub const CLOCKWISE: [Corner; 4] = [Corner::NW, Corner::NE, Corner::SE, Corner::SW];

    /// Position within the tile, in tiles: 0 or 1 on each axis.
    pub fn offset(self) -> [i32; 2] {
        match self {
            Corner::NW => [0, 0],
            Corner::NE => [1, 0],
            Corner::SW => [0, 1],
            Corner::SE => [1, 1],
        }
    }

    fn at(offset: [i32; 2]) -> Corner {
        match offset {
            [0, 0] => Corner::NW,
            [1, 0] => Corner::NE,
            [0, 1] => Corner::SW,
            _ => Corner::SE,
        }
    }

    pub fn opposite(self) -> Corner {
        let [x, y] = self.offset();
        Corner::at([1 - x, 1 - y])
    }

    /// The two corners joined by the diagonal face of a slope solid here.
    fn face_ends(self) -> [Corner; 2] {
        match self {
            Corner::NW | Corner::SE => [Corner::NE, Corner::SW],
            Corner::NE | Corner::SW => [Corner::NW, Corner::SE],
        }
    }

    /// The two sides meeting at this corner.
    pub fn sides(self) -> [Side; 2] {
        match self {
            Corner::NW => [Side::N, Side::W],
            Corner::NE => [Side::N, Side::E],
            Corner::SW => [Side::S, Side::W],
            Corner::SE => [Side::S, Side::E],
        }
    }
}

impl Side {
    pub const ALL: [Side; 4] = [Side::N, Side::S, Side::W, Side::E];

    pub fn opposite(self) -> Side {
        match self {
            Side::N => Side::S,
            Side::S => Side::N,
            Side::W => Side::E,
            Side::E => Side::W,
        }
    }

    /// Offset to the neighboring tile across this side.
    pub fn offset(self) -> [i32; 2] {
        match self {
            Side::N => [0, -1],
            Side::S => [0, 1],
            Side::W => [-1, 0],
            Side::E => [1, 0],
        }
    }

    /// The corners at either end of this side.
    fn ends(self) -> [Corner; 2] {
        match self {
            Side::N => [Corner::NW, Corner::NE],
            Side::S => [Corner::SW, Corner::SE],
            Side::W => [Corner::NW, Corner::SW],
            Side::E => [Corner::NE, Corner::SE],
        }
    }
}

/// Which way a staircase leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

/// Index of a wall type in the level's tileset ([`Tileset::walls`]).
pub type WallId = u8;
/// Index of a floor type in the level's tileset ([`Tileset::floors`]).
pub type FloorId = u8;
/// Index of a liquid in the level's tileset ([`Tileset::liquids`]).
pub type LiquidId = u8;

/// What a part of a tile is made of. The engine knows only these kinds;
/// which material an id is (planks, rock, water) is the tileset's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Wall(WallId),
    Floor(FloorId),
    /// No floor: the layer below shows through.
    Void,
    /// A liquid: void to the game (impassable, open to sight, the layer
    /// below showing through), drawn as a translucent tile over what's
    /// beneath.
    Liquid(LiquidId),
}

/// The shape of some part of a tile: what rays, collision and outlines run
/// on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solid {
    None,
    Full,
    /// The right triangle at this corner, with the diagonal face running
    /// between the two neighboring corners.
    Slope(Corner),
}

/// One grid cell: wholly one part, or split along a diagonal into two.
/// Tiles are equal when made of the same parts in the same places (a
/// split can be given from either corner).
#[derive(Clone, Copy, Debug)]
pub enum Tile {
    Whole(Part),
    /// `corner`'s half is `at`, the other half `rest`. Never the same part
    /// on both sides (that's `Whole`).
    Split {
        corner: Corner,
        at: Part,
        rest: Part,
    },
    /// Floor leading to the layer above or below.
    Stairs(Dir, FloorId),
}

/// The tile point `p` (pixels) is in, and the quarter of it (as a bit;
/// see [`QUARTERS`]): the triangle between its diagonals.
pub fn quarter_at(p: [f32; 2]) -> ([i32; 2], u8) {
    let [x, y] = [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32];
    let (u, v) = (p[0] / TILE - x as f32, p[1] / TILE - y as f32);
    let quarter = if v < u.min(1.0 - u) {
        N
    } else if v > u.max(1.0 - u) {
        S
    } else if u < 0.5 {
        W
    } else {
        E
    };
    ([x, y], quarter)
}

/// The quarters of a tile, the triangles between its diagonals, as bits.
const N: u8 = 1;
const E: u8 = 2;
const S: u8 = 4;
const W: u8 = 8;
/// The quarters of a tile in bit order, as bits.
pub const QUARTERS: [u8; 4] = [N, E, S, W];

impl Corner {
    /// The quarters making up the half of a tile at this corner, as bits.
    pub fn quarters(self) -> u8 {
        match self {
            Corner::NW => N | W,
            Corner::NE => N | E,
            Corner::SE => S | E,
            Corner::SW => S | W,
        }
    }
}

impl Solid {
    /// Does it reach tile corner `c`, even just at a point?
    pub fn touches(self, c: Corner) -> bool {
        match self {
            Solid::None => false,
            Solid::Full => true,
            Solid::Slope(s) => c != s.opposite(),
        }
    }

    /// Does it cover the whole of side `side`?
    pub fn covers(self, side: Side) -> bool {
        match self {
            Solid::None => false,
            Solid::Full => true,
            Solid::Slope(s) => s.sides().contains(&side),
        }
    }

    /// For a slope, solidity as a linear function of tile-local `(u, v)` in
    /// `[0, 1]`: `k + g . (u, v)`, positive inside the solid, zero on the
    /// diagonal face. Returns `(k, g)`.
    fn slope_plane(c: Corner) -> (f32, [f32; 2]) {
        match c {
            Corner::NW => (1.0, [-1.0, -1.0]),
            Corner::NE => (0.0, [1.0, -1.0]),
            Corner::SW => (0.0, [-1.0, 1.0]),
            Corner::SE => (-1.0, [1.0, 1.0]),
        }
    }

    /// Does this shape, as tile `tile`'s, overlap the pixel-space box
    /// `pos..pos+size`? Only the part of the box over the tile counts, and
    /// touching a surface doesn't.
    pub fn hits_box(self, tile: [i32; 2], pos: [f32; 2], size: [f32; 2]) -> bool {
        match self {
            Solid::None => false,
            Solid::Full => {
                let over = |a: usize| {
                    let (lo, hi) = (tile[a] as f32 * TILE, (tile[a] + 1) as f32 * TILE);
                    pos[a] + size[a] > lo + COLLIDE_EPS && pos[a] < hi - COLLIDE_EPS
                };
                over(0) && over(1)
            }
            Solid::Slope(c) => {
                // The box clipped to the tile, in tile-local units; solidity
                // is linear, so its maximum is at one of the box's corners.
                let local = |p: f32, a: usize| (p / TILE - tile[a] as f32).clamp(0.0, 1.0);
                let (u0, u1) = (local(pos[0], 0), local(pos[0] + size[0], 0));
                let (v0, v1) = (local(pos[1], 1), local(pos[1] + size[1], 1));
                let (k, g) = Solid::slope_plane(c);
                let u = if g[0] > 0.0 { u1 } else { u0 };
                let v = if g[1] > 0.0 { v1 } else { v0 };
                k + g[0] * u + g[1] * v > COLLIDE_EPS / TILE
            }
        }
    }

    /// As a set of quarters: a slope is two neighboring quarters, a full
    /// tile all four.
    pub fn quarters(self) -> u8 {
        match self {
            Solid::None => 0,
            Solid::Full => N | E | S | W,
            Solid::Slope(c) => c.quarters(),
        }
    }

    /// The shape made of quarters `q`; any other combination becomes full
    /// (`round_up`) or none.
    pub fn from_quarters(q: u8, round_up: bool) -> Solid {
        [
            Solid::None,
            Solid::Full,
            Solid::Slope(Corner::NW),
            Solid::Slope(Corner::NE),
            Solid::Slope(Corner::SE),
            Solid::Slope(Corner::SW),
        ]
        .into_iter()
        .find(|t| t.quarters() == q)
        .unwrap_or(if round_up { Solid::Full } else { Solid::None })
    }

    /// Wherever either is. A result no shape can represent (two slopes at
    /// neighboring corners) becomes full, which keeps unions
    /// order-independent.
    pub fn union(self, other: Solid) -> Solid {
        Solid::from_quarters(self.quarters() | other.quarters(), true)
    }

    /// Where `self` is and `other` isn't: cutting a slope out of a full
    /// tile leaves the opposite slope. A result no shape can represent
    /// becomes none.
    pub fn carve(self, other: Solid) -> Solid {
        Solid::from_quarters(self.quarters() & !other.quarters(), false)
    }

    /// This shape in tile `(x, y)` as a convex polygon in pixels.
    pub fn shape(self, x: usize, y: usize) -> Option<Shape> {
        let point = |c: Corner| {
            let [dx, dy] = c.offset();
            [(x as i32 + dx) as f32 * TILE, (y as i32 + dy) as f32 * TILE]
        };
        match self {
            Solid::None => None,
            Solid::Full => Some(Corner::CLOCKWISE.map(point).to_vec()),
            Solid::Slope(s) => Some(
                Corner::CLOCKWISE
                    .into_iter()
                    .filter(|c| *c != s.opposite())
                    .map(point)
                    .collect(),
            ),
        }
    }
}

impl PartialEq for Tile {
    fn eq(&self, other: &Tile) -> bool {
        self.stairs() == other.stairs() && self.parts() == other.parts()
    }
}

impl Eq for Tile {}

impl Tile {
    /// Plain floor, wall, void and liquid, of the first type of each.
    pub const FLOOR: Tile = Tile::Whole(Part::Floor(0));
    pub const WALL: Tile = Tile::Whole(Part::Wall(0));
    pub const VOID: Tile = Tile::Whole(Part::Void);
    pub const LIQUID: Tile = Tile::Whole(Part::Liquid(0));

    /// Wall in `corner`'s half, floor in the rest.
    pub fn slope(corner: Corner) -> Tile {
        Tile::split(corner, Part::Wall(0), Part::Floor(0))
    }

    /// `at` in `corner`'s half and `rest` in the other, or wholly one part
    /// if they're the same.
    pub fn split(corner: Corner, at: Part, rest: Part) -> Tile {
        if at == rest {
            Tile::Whole(at)
        } else {
            Tile::Split { corner, at, rest }
        }
    }

    /// The default tile with shape `solid`: wall there, floor elsewhere.
    pub fn from_solid(solid: Solid) -> Tile {
        match solid {
            Solid::None => Tile::FLOOR,
            Solid::Full => Tile::WALL,
            Solid::Slope(c) => Tile::slope(c),
        }
    }

    /// The part in each quarter, in `N E S W` order. Stairs are floor.
    fn parts(self) -> [Part; 4] {
        match self {
            Tile::Whole(p) => [p; 4],
            Tile::Stairs(_, f) => [Part::Floor(f); 4],
            Tile::Split { corner, at, rest } => {
                QUARTERS.map(|q| if corner.quarters() & q != 0 { at } else { rest })
            }
        }
    }

    /// The tile with these quarters, if one can have them.
    fn from_parts(parts: [Part; 4]) -> Option<Tile> {
        if parts.iter().all(|p| *p == parts[0]) {
            return Some(Tile::Whole(parts[0]));
        }
        Corner::CLOCKWISE.into_iter().find_map(|c| {
            let at = |q: usize| c.quarters() & QUARTERS[q] != 0;
            let first = |inside: bool| (0..4).find(|&q| at(q) == inside).map(|q| parts[q]);
            let (a, b) = (first(true)?, first(false)?);
            (0..4)
                .all(|q| parts[q] == if at(q) { a } else { b })
                .then(|| Tile::split(c, a, b))
        })
    }

    /// The quarters where `f` holds of the part.
    fn quarters_where(self, f: impl Fn(Part) -> bool) -> u8 {
        let parts = self.parts();
        (0..4)
            .filter(|&q| f(parts[q]))
            .fold(0, |m, q| m | QUARTERS[q])
    }

    /// Where it's wall.
    pub fn wall(self) -> Solid {
        Solid::from_quarters(self.quarters_where(|p| matches!(p, Part::Wall(_))), true)
    }

    /// Where it's void (or liquid), open to the layer below.
    pub fn void(self) -> Solid {
        Solid::from_quarters(self.void_quarters(), true)
    }

    /// The quarters that are void or liquid, as bits: open to the layer
    /// below, impassable, and no block to sight.
    pub fn void_quarters(self) -> u8 {
        self.quarters_where(|p| matches!(p, Part::Void | Part::Liquid(_)))
    }

    /// The quarters that are liquid, as bits.
    pub fn liquid_quarters(self) -> u8 {
        self.quarters_where(|p| matches!(p, Part::Liquid(_)))
    }

    /// Its liquid, if any of it is liquid.
    pub fn liquid_id(self) -> Option<LiquidId> {
        self.parts().into_iter().find_map(|p| match p {
            Part::Liquid(l) => Some(l),
            _ => None,
        })
    }

    /// Its wall type, if any of it is wall.
    pub fn wall_id(self) -> Option<WallId> {
        self.parts().into_iter().find_map(|p| match p {
            Part::Wall(w) => Some(w),
            _ => None,
        })
    }

    /// Its floor type, if any of it is floor.
    pub fn floor_id(self) -> Option<FloorId> {
        self.parts().into_iter().find_map(|p| match p {
            Part::Floor(f) => Some(f),
            _ => None,
        })
    }

    /// The same tile with its wall, if any, of type `wall`.
    pub fn with_wall(self, wall: WallId) -> Tile {
        let parts = self.parts().map(|p| match p {
            Part::Wall(_) => Part::Wall(wall),
            p => p,
        });
        match self {
            Tile::Stairs(..) => self,
            _ => Tile::from_parts(parts).unwrap_or(self),
        }
    }

    /// Wholly floor (not stairs), of any type.
    pub fn is_floor(self) -> bool {
        matches!(self, Tile::Whole(Part::Floor(_)))
    }

    /// A whole tile with `corner`'s half made `part` (any kind): a 45-degree
    /// edge between the two. Stairs and tiles already split are left as
    /// they are.
    pub fn chamfer(self, corner: Corner, part: Part) -> Tile {
        match self {
            Tile::Whole(own) => Tile::split(corner, part, own),
            _ => self,
        }
    }

    pub fn stairs(self) -> Option<Dir> {
        match self {
            Tile::Stairs(d, _) => Some(d),
            _ => None,
        }
    }

    /// The tile with `piece`'s walls stamped on: wherever `piece` is wall,
    /// that wall; elsewhere unchanged (so a slope stamped over void keeps
    /// the void beside it). A result no tile can have becomes wholly
    /// `piece`'s wall, as solid shapes round up.
    pub fn stamp(self, piece: Tile) -> Tile {
        let (mine, theirs) = (self.parts(), piece.parts());
        let Some(wall) = theirs.iter().copied().find(|p| matches!(p, Part::Wall(_))) else {
            return self;
        };
        let parts = [0, 1, 2, 3].map(|q| match theirs[q] {
            Part::Wall(_) => theirs[q],
            _ => mine[q],
        });
        Tile::from_parts(parts).unwrap_or(Tile::Whole(wall))
    }

    /// The tile with `cut` replaced by `with` (floor or void). A result no
    /// tile can have becomes wholly `with`.
    pub fn carve(self, cut: Solid, with: Part) -> Tile {
        if cut == Solid::None {
            return self;
        }
        let mine = self.parts();
        let parts = [0, 1, 2, 3].map(|q| {
            if cut.quarters() & QUARTERS[q] != 0 {
                with
            } else {
                mine[q]
            }
        });
        Tile::from_parts(parts).unwrap_or(Tile::Whole(with))
    }

    /// The part each half of the tile is, for drawing: the half at
    /// `corner` and the rest, or the whole tile.
    pub fn halves(self) -> Halves {
        match self {
            Tile::Whole(p) => Halves::Whole(p),
            Tile::Stairs(_, f) => Halves::Whole(Part::Floor(f)),
            Tile::Split { corner, at, rest } => Halves::Split(corner, at, rest),
        }
    }
}

/// Quarters `q` (bits) of tile `(x, y)` as convex polygons in pixels: one
/// if they make a whole or half tile, else a triangle each.
pub fn quarter_shapes(x: usize, y: usize, q: u8) -> Vec<Shape> {
    let solid = Solid::from_quarters(q, false);
    if solid.quarters() == q {
        return solid.shape(x, y).into_iter().collect();
    }
    let point = |c: Corner| {
        let [dx, dy] = c.offset();
        [(x as i32 + dx) as f32 * TILE, (y as i32 + dy) as f32 * TILE]
    };
    let center = [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE];
    [
        (N, Corner::NW, Corner::NE),
        (E, Corner::NE, Corner::SE),
        (S, Corner::SE, Corner::SW),
        (W, Corner::SW, Corner::NW),
    ]
    .into_iter()
    .filter(|(bit, _, _)| q & bit != 0)
    .map(|(_, a, b)| vec![point(a), point(b), center])
    .collect()
}

/// How a tile divides into parts, for drawing.
pub enum Halves {
    Whole(Part),
    Split(Corner, Part, Part),
}

/// Tolerance for slope solidity (tile-local units), matching `raycast`'s
/// 1e-4 px vertex tolerance.
const SLOPE_EPS: f32 = 1e-5;

/// Overlap (pixels) below which a box merely touches solid space.
const COLLIDE_EPS: f32 = 1e-3;

/// Longest substep (pixels) when moving a box: under half a tile and under
/// half the smallest box, so nothing passes through a wall.
const MAX_STEP: f32 = 2.0;

/// Rounds of pushing a box out of overlapping tiles per substep.
const PUSH_ROUNDS: usize = 4;

/// A convex polygon in pixel space, in increasing-angle order (clockwise on
/// screen, since y points down).
pub type Shape = Vec<[f32; 2]>;

/// A piece of the outline of solid space, oriented with solid on its right
/// (so outlines run clockwise on screen, since y points down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// The tile whose solid part this edge bounds; outside the level for
    /// the level's own boundary.
    pub solid: [i32; 2],
}

/// A closed loop of outline, with solid on its right: clockwise on screen
/// around solid, counter-clockwise around holes and the level's boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// Where the loop turns, in order; it closes from the last back to the
    /// first.
    pub points: Vec<[f32; 2]>,
    /// For each segment `points[i]` to `points[i + 1]`, the level's outline
    /// edges (indices into [`Layer::outline_edges`]) it's made of, in order.
    pub runs: Vec<Vec<usize>>,
}

/// A corner where the outline turns away from solid (an inside corner,
/// seen from open space), with the inward normals of the faces before and
/// after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConcaveCorner {
    pub at: [f32; 2],
    pub before: [f32; 2],
    pub after: [f32; 2],
}

/// A grid line outline edges can lie on, in tiles: horizontal (`y`),
/// vertical (`x`), or diagonal (`x + y` rising to the right, `x - y`
/// falling).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Line {
    Horizontal(i32),
    Vertical(i32),
    Rising(i32),
    Falling(i32),
}

impl Line {
    /// The grid lines running through both pixel-space points (within
    /// `tolerance` pixels).
    pub fn through(p: [f32; 2], q: [f32; 2], tolerance: f32) -> Vec<Line> {
        let on = |f: fn([f32; 2]) -> f32, make: fn(i32) -> Line| {
            let (a, b) = (f(p), f(q));
            let k = (a / TILE).round();
            ((a - b).abs() < tolerance && (a - k * TILE).abs() < tolerance).then(|| make(k as i32))
        };
        [
            on(|p| p[1], Line::Horizontal),
            on(|p| p[0], Line::Vertical),
            on(|p| p[0] + p[1], Line::Rising),
            on(|p| p[0] - p[1], Line::Falling),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Indices of a layer's outline edges by the grid line each lies on: a
/// slot per line through a `cols` x `rows` layer, the horizontal ones top
/// to bottom, then the vertical, rising and falling ones, by their number.
#[derive(Default)]
struct Lines {
    cols: i32,
    rows: i32,
    /// Where each slot's indices start in `items`, then the end.
    starts: Vec<u32>,
    items: Vec<usize>,
}

impl Lines {
    fn new(cols: usize, rows: usize, edges: &[Edge]) -> Self {
        let mut lines = Self {
            cols: cols as i32,
            rows: rows as i32,
            starts: Vec::new(),
            items: vec![0; edges.len()],
        };
        let slots = 3 * (cols + rows) + 4;
        let slot = |e: &Edge| lines.slot(line_of(e)).expect("edges lie inside");
        let slots_of: Vec<usize> = edges.iter().map(slot).collect();
        let mut starts = vec![0u32; slots + 1];
        for &k in &slots_of {
            starts[k + 1] += 1;
        }
        for k in 1..starts.len() {
            starts[k] += starts[k - 1];
        }
        let mut next = starts.clone();
        for (i, &k) in slots_of.iter().enumerate() {
            lines.items[next[k] as usize] = i;
            next[k] += 1;
        }
        lines.starts = starts;
        lines
    }

    /// `line`'s slot, if it runs through the layer.
    fn slot(&self, line: Line) -> Option<usize> {
        let (c, r) = (self.cols, self.rows);
        let (at, lo, hi, before) = match line {
            Line::Horizontal(y) => (y, 0, r, 0),
            Line::Vertical(x) => (x, 0, c, r + 1),
            Line::Rising(s) => (s, 0, c + r, r + c + 2),
            Line::Falling(d) => (d, -r, c, 2 * (r + c) + 3),
        };
        (lo..=hi).contains(&at).then(|| (before + at - lo) as usize)
    }

    fn on(&self, line: Line) -> &[usize] {
        let Some(k) = self.slot(line).filter(|&k| k + 1 < self.starts.len()) else {
            return &[];
        };
        &self.items[self.starts[k] as usize..self.starts[k + 1] as usize]
    }
}

/// The grid point at pixel position `p`, in tiles.
fn vertex(p: [f32; 2]) -> [i32; 2] {
    [(p[0] / TILE).round() as i32, (p[1] / TILE).round() as i32]
}

/// Which way an outline edge heads, in eighths of a turn clockwise on
/// screen from east.
fn heading(e: &Edge) -> i32 {
    let ([ax, ay], [bx, by]) = (vertex(e.a), vertex(e.b));
    match [(bx - ax).signum(), (by - ay).signum()] {
        [1, 0] => 0,
        [1, 1] => 1,
        [0, 1] => 2,
        [-1, 1] => 3,
        [-1, 0] => 4,
        [-1, -1] => 5,
        [0, -1] => 6,
        _ => 7,
    }
}

/// How far clockwise a turn from heading `from` to `to` is, in eighths of
/// a turn: from -3, sharply left, through 0, straight on, to 4, back.
fn turn(from: i32, to: i32) -> i32 {
    (to - from + 11).rem_euclid(8) - 3
}

/// The grid line an outline edge (a tile long) lies on.
fn line_of(e: &Edge) -> Line {
    let ([ax, ay], [bx, by]) = (vertex(e.a), vertex(e.b));
    if ay == by {
        Line::Horizontal(ay)
    } else if ax == bx {
        Line::Vertical(ax)
    } else if ax + ay == bx + by {
        Line::Rising(ax + ay)
    } else {
        Line::Falling(ax - ay)
    }
}

/// Grid point `(gx, gy)` in pixels, if the outline of a grid shaped
/// `solid(x, y)` turns there (or ends, or meets itself).
fn corner_at(solid: &impl Fn(i32, i32) -> Solid, gx: i32, gy: i32) -> Option<[f32; 2]> {
    let nw = solid(gx - 1, gy - 1);
    let ne = solid(gx, gy - 1);
    let sw = solid(gx - 1, gy);
    let se = solid(gx, gy);

    // The ways outline runs from this vertex.
    let mut dirs = [[0i32; 2]; 8];
    let mut n = 0;
    // An axis edge is outline where exactly one side covers it.
    for (d, outline) in [
        ([0, -1], nw.covers(Side::E) != ne.covers(Side::W)),
        ([0, 1], sw.covers(Side::E) != se.covers(Side::W)),
        ([-1, 0], nw.covers(Side::S) != sw.covers(Side::N)),
        ([1, 0], ne.covers(Side::S) != se.covers(Side::N)),
    ] {
        if outline {
            dirs[n] = d;
            n += 1;
        }
    }
    // A slope's face is outline, running from this vertex across its tile
    // when the vertex is one of the face's ends.
    for (tile, d, at) in [
        (nw, [-1, -1], Corner::SE),
        (ne, [1, -1], Corner::SW),
        (sw, [-1, 1], Corner::NE),
        (se, [1, 1], Corner::NW),
    ] {
        if let Solid::Slope(s) = tile
            && at != s
            && at != s.opposite()
        {
            dirs[n] = d;
            n += 1;
        }
    }

    let straight = n == 2 && dirs[0][0] == -dirs[1][0] && dirs[0][1] == -dirs[1][1];
    (n > 0 && !straight).then_some([gx as f32 * TILE, gy as f32 * TILE])
}

/// Onto `out`, the outline edges of a grid shaped `solid(x, y)` along the
/// top and left sides of tile `(x, y)` (all of them, over every tile and
/// one more row and column), and its slope's face if it's `inside` the
/// grid; oriented with the solid side on the right.
fn edges_at(solid: &impl Fn(i32, i32) -> Solid, x: i32, y: i32, inside: bool, out: &mut Vec<Edge>) {
    let px = |x: i32, y: i32| [x as f32 * TILE, y as f32 * TILE];
    let here = solid(x, y);
    let (above, left) = (solid(x, y - 1), solid(x - 1, y));
    match (here.covers(Side::N), above.covers(Side::S)) {
        (true, false) => out.push(Edge {
            a: px(x, y),
            b: px(x + 1, y),
            solid: [x, y],
        }),
        (false, true) => out.push(Edge {
            a: px(x + 1, y),
            b: px(x, y),
            solid: [x, y - 1],
        }),
        _ => {}
    }
    match (here.covers(Side::W), left.covers(Side::E)) {
        (true, false) => out.push(Edge {
            a: px(x, y + 1),
            b: px(x, y),
            solid: [x, y],
        }),
        (false, true) => out.push(Edge {
            a: px(x, y),
            b: px(x, y + 1),
            solid: [x - 1, y],
        }),
        _ => {}
    }
    if let Solid::Slope(c) = here
        && inside
    {
        // The face joins the two corners beside the solid one; orient it so
        // the solid corner is on its right.
        let corner = |k: Corner| {
            let [dx, dy] = k.offset();
            px(x + dx, y + dy)
        };
        let [mut a, mut b] = c.face_ends().map(corner);
        let s = corner(c);
        let (e, to_solid) = ([b[0] - a[0], b[1] - a[1]], [s[0] - a[0], s[1] - a[1]]);
        if -e[1] * to_solid[0] + e[0] * to_solid[1] < 0.0 {
            std::mem::swap(&mut a, &mut b);
        }
        out.push(Edge {
            a,
            b,
            solid: [x, y],
        });
    }
}

/// A chunk's own part of a layer's geometry, the same whichever window
/// it's seen through, so made once it can be reused by each (see
/// [`Layer::from_pieces`]): its tiles, their shapes, and the corners and
/// edges of the outline at its grid points and tile sides; all but those
/// along its top and left, which depend on the chunks beside it.
pub struct Piece {
    tiles: Arc<Chunk>,
    /// Its tiles across and down in the level: fewer than [`CHUNK`] at the
    /// level's right and bottom edges.
    size: [usize; 2],
    /// Row by row.
    solids: Vec<Solid>,
    /// In pixels from its top-left corner, row by row (as a layer's are),
    /// and where each row's start in them, then the end.
    corners: Vec<[f32; 2]>,
    corner_rows: Vec<u32>,
    edges: Vec<Edge>,
    edge_rows: Vec<u32>,
}

impl Piece {
    /// The piece of a layer that's chunk `tiles`, `size` of it in the
    /// level.
    pub fn new(tiles: Arc<Chunk>, size: [usize; 2]) -> Self {
        let solids = (0..size[0] * size[1])
            .map(|i| tiles.get(i % size[0], i / size[0]).wall())
            .collect();
        Self::shaped(tiles, size, solids)
    }

    /// The piece of a layer of default tiles shaped `solids`, `size` of
    /// them row by row (see [`Layer::from_solids`]).
    pub fn from_solids(size: [usize; 2], solids: Vec<Solid>) -> Self {
        let [w, h] = size;
        let tiles =
            Chunk::from_fn(|x, y| Tile::from_solid(solids[y.min(h - 1) * w + x.min(w - 1)]));
        Self::shaped(Arc::new(tiles), size, solids)
    }

    fn shaped(tiles: Arc<Chunk>, size: [usize; 2], solids: Vec<Solid>) -> Self {
        let [w, h] = size.map(|n| n as i32);
        let solid = |x: i32, y: i32| solids[(y * w + x) as usize];
        let (mut corners, mut edges) = (Vec::new(), Vec::new());
        // The first row, along its top, has none.
        let (mut corner_rows, mut edge_rows) = (vec![0, 0], vec![0, 0]);
        for y in 1..h {
            for x in 1..w {
                corners.extend(corner_at(&solid, x, y));
                edges_at(&solid, x, y, true, &mut edges);
            }
            corner_rows.push(corners.len() as u32);
            edge_rows.push(edges.len() as u32);
        }
        Self {
            tiles,
            size,
            solids,
            corners,
            corner_rows,
            edges,
            edge_rows,
        }
    }
}

pub struct Layer {
    pub cols: usize,
    pub rows: usize,
    /// Where its tile (0, 0) is in the whole level, in tiles: for a layer
    /// that's a window onto a bigger one. Only how tiles are drawn depends
    /// on it (which variant of its art each shows), so it looks the same
    /// whichever window it's seen through.
    origin: [i32; 2],
    tiles: Chunks,
    /// Where each tile is wall: all the geometry runs on these.
    solids: Vec<Solid>,
    corners: Vec<[f32; 2]>,
    edges: Vec<Edge>,
    edges_by_line: Lines,
    /// For each edge, the next along its outline loop, and the one before
    /// (see [`Layer::link_edges`]).
    next: Vec<u32>,
    prev: Vec<u32>,
    concave: Vec<ConcaveCorner>,
    /// `corners`, `edges` and `concave` by where they are, so sight can
    /// look at just those within its radius.
    corner_buckets: Buckets,
    edge_buckets: Buckets,
    concave_buckets: Buckets,
}

/// Edge of the square blocks [`Buckets`] sorts things into, in world units.
const BUCKET: f32 = 8.0 * TILE;

/// Indices of things in a level (corners, edges), by which square blocks
/// of the level their bounding boxes overlap.
#[derive(Default)]
struct Buckets {
    /// Things in all.
    len: usize,
    /// Blocks across and down.
    blocks: [usize; 2],
    /// Where each block's indices start in `items`, row by row, then the
    /// end.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Buckets {
    /// Sort things into the blocks of a level `cols` x `rows` tiles, by
    /// their bounding boxes.
    fn new(cols: usize, rows: usize, boxes: &[Rect]) -> Self {
        let blocks = [cols, rows].map(|n| ((n as f32 * TILE / BUCKET).ceil() as usize).max(1));
        let mut buckets = Self {
            len: boxes.len(),
            blocks,
            starts: vec![0; blocks[0] * blocks[1] + 1],
            items: Vec::new(),
        };
        // Count each block's things, then place them.
        for &b in boxes {
            for k in buckets.blocks_over(b, 0.0) {
                buckets.starts[k + 1] += 1;
            }
        }
        for k in 1..buckets.starts.len() {
            buckets.starts[k] += buckets.starts[k - 1];
        }
        let mut next = buckets.starts.clone();
        buckets.items = vec![0; *buckets.starts.last().unwrap() as usize];
        for (i, &b) in boxes.iter().enumerate() {
            for k in buckets.blocks_over(b, 0.0) {
                buckets.items[next[k] as usize] = i as u32;
                next[k] += 1;
            }
        }
        buckets
    }

    /// Indices of the blocks that `rect`, grown by `margin`, overlaps.
    fn blocks_over(&self, rect: Rect, margin: f32) -> impl Iterator<Item = usize> + use<> {
        let span = |axis: usize| {
            let block =
                |v: f32| ((v / BUCKET).floor().max(0.0) as usize).min(self.blocks[axis] - 1);
            block(rect[0][axis] - margin)..=block(rect[1][axis] + margin)
        };
        let (xs, ys) = (span(0), span(1));
        let across = self.blocks[0];
        ys.flat_map(move |y| xs.clone().map(move |x| y * across + x))
    }

    /// Indices of the things that may come within `radius` of `p` (all of
    /// them for an infinite radius), in increasing order, each once.
    fn near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.over([p, p], radius)
    }

    /// Indices of the things that may reach into `rect` grown by `margin`
    /// (all of them for an infinite one), in increasing order, each once.
    fn over(&self, rect: Rect, margin: f32) -> Vec<usize> {
        let finite = rect.iter().flatten().all(|v| v.is_finite()) && margin.is_finite();
        if !finite || self.starts.is_empty() {
            return (0..self.len).collect();
        }
        let mut found: Vec<usize> = self
            .blocks_over(rect, margin)
            .flat_map(|k| &self.items[self.starts[k] as usize..self.starts[k + 1] as usize])
            .map(|&i| i as usize)
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }
}

/// The bounding box of some points.
fn bounds(points: &[[f32; 2]]) -> Rect {
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

impl Layer {
    /// A level from its tiles, row by row.
    pub fn from_tiles(cols: usize, rows: usize, tiles: Vec<Tile>) -> Self {
        assert_eq!(tiles.len(), cols * rows);
        Self::from_chunks(Chunks::from_tiles(cols, rows, &tiles))
    }

    /// A level from its tiles in chunks.
    pub fn from_chunks(tiles: Chunks) -> Self {
        let (cols, rows) = (tiles.cols(), tiles.rows());
        let solids = (0..cols * rows)
            .map(|i| tiles.get(i % cols, i / cols).wall())
            .collect();
        let mut level = Self::bare(tiles, solids);
        level.corners = level.find_corners();
        level.edges = level.find_edges();
        level.link()
    }

    /// A level `cols` x `rows` made of `pieces`, a chunk each, row by row:
    /// the same as one made from all their tiles at once (see
    /// [`Layer::from_chunks`]), but only the outline along the seams
    /// between them, and around its edge, is left to work out.
    pub fn from_pieces(cols: usize, rows: usize, pieces: &[Arc<Piece>]) -> Self {
        let across = cols.div_ceil(CHUNK);
        assert_eq!(pieces.len(), across * rows.div_ceil(CHUNK));
        for (i, p) in pieces.iter().enumerate() {
            let [cx, cy] = [i % across, i / across];
            let size = [CHUNK.min(cols - cx * CHUNK), CHUNK.min(rows - cy * CHUNK)];
            assert_eq!(p.size, size, "piece ({cx}, {cy})");
        }
        let piece = |cx: usize, y: usize| &pieces[y / CHUNK * across + cx];
        let mut solids = Vec::with_capacity(cols * rows);
        for y in 0..rows {
            for cx in 0..across {
                let p = piece(cx, y);
                let row = y % CHUNK * p.size[0];
                solids.extend_from_slice(&p.solids[row..row + p.size[0]]);
            }
        }
        let tiles = pieces.iter().map(|p| p.tiles.clone()).collect();
        let mut level = Self::bare(Chunks::from_chunks(cols, rows, tiles), solids);

        // Row by row, as the whole layer's are: along a seam, worked out;
        // between them, each piece's row and the seam before it.
        let (mut corners, mut edges) = (Vec::new(), Vec::new());
        let solid = |x: i32, y: i32| level.solid(x, y);
        let ([c, r], k) = ([cols as i32, rows as i32], CHUNK as i32);
        for y in 0..=r {
            if y % k == 0 || y == r {
                for x in 0..=c {
                    corners.extend(corner_at(&solid, x, y));
                    edges_at(&solid, x, y, x < c && y < r, &mut edges);
                }
                continue;
            }
            let row = (y % k) as usize;
            for cx in 0..across {
                let p = piece(cx, y as usize);
                let at = [cx as i32 * k, y - row as i32];
                corners.extend(corner_at(&solid, at[0], y));
                edges_at(&solid, at[0], y, true, &mut edges);
                let px = at.map(|t| t as f32 * TILE);
                let shift = |q: [f32; 2]| [q[0] + px[0], q[1] + px[1]];
                let span = |starts: &[u32]| starts[row] as usize..starts[row + 1] as usize;
                corners.extend(p.corners[span(&p.corner_rows)].iter().map(|&q| shift(q)));
                edges.extend(p.edges[span(&p.edge_rows)].iter().map(|e| Edge {
                    a: shift(e.a),
                    b: shift(e.b),
                    solid: [e.solid[0] + at[0], e.solid[1] + at[1]],
                }));
            }
            corners.extend(corner_at(&solid, c, y));
            edges_at(&solid, c, y, false, &mut edges);
        }
        level.corners = corners;
        level.edges = edges;
        level.link()
    }

    /// A level of `tiles`, shaped `solids`, its outline not yet found.
    fn bare(tiles: Chunks, solids: Vec<Solid>) -> Self {
        Self {
            cols: tiles.cols(),
            rows: tiles.rows(),
            origin: [0, 0],
            tiles,
            solids,
            corners: Vec::new(),
            edges: Vec::new(),
            edges_by_line: Default::default(),
            next: Vec::new(),
            prev: Vec::new(),
            concave: Vec::new(),
            corner_buckets: Buckets::default(),
            edge_buckets: Buckets::default(),
            concave_buckets: Buckets::default(),
        }
    }

    /// The rest of a level made from its outline's corners and edges: the
    /// edges linked into loops, the concave corners, and all three by
    /// where they are.
    fn link(mut self) -> Self {
        let (cols, rows) = (self.cols, self.rows);
        self.edges_by_line = Lines::new(cols, rows, &self.edges);
        self.next = self.link_edges();
        self.prev = vec![0; self.next.len()];
        for (i, &n) in self.next.iter().enumerate() {
            self.prev[n as usize] = i as u32;
        }
        self.concave = self.find_concave_corners();
        let corners: Vec<Rect> = self.corners.iter().map(|&c| [c, c]).collect();
        let edges: Vec<Rect> = self.edges.iter().map(|e| bounds(&[e.a, e.b])).collect();
        let concave: Vec<Rect> = self.concave.iter().map(|c| [c.at, c.at]).collect();
        self.corner_buckets = Buckets::new(cols, rows, &corners);
        self.edge_buckets = Buckets::new(cols, rows, &edges);
        self.concave_buckets = Buckets::new(cols, rows, &concave);
        self
    }

    /// A layer of default tiles with shapes `solids`, row by row: wall
    /// where solid, floor elsewhere.
    pub fn from_solids(cols: usize, rows: usize, solids: Vec<Solid>) -> Self {
        Self::from_tiles(
            cols,
            rows,
            solids.into_iter().map(Tile::from_solid).collect(),
        )
    }

    /// Build a layer from rows of `.` (floor), `#` (wall), `◸ ◹ ◺ ◿`
    /// (wall in that corner, floor in the rest), `◤ ◥ ◣ ◢` (wall in that
    /// corner over void), `▛ ▜ ▙ ▟` (floor in that corner, void in the
    /// rest), `~` (void), `≈` (liquid), `◜ ◝ ◟ ◞` (liquid in that corner
    /// over floor) and `<` `>` (stairs up and down), for tests. Materials
    /// are the first of each kind in the tileset.
    #[cfg(any(test, feature = "test-util"))]
    pub fn from_ascii(rows: &[&str]) -> Self {
        use Corner::*;
        let cols = rows[0].chars().count();
        let (wall, floor, liquid) = (Part::Wall(0), Part::Floor(0), Part::Liquid(0));
        let tiles = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| match c {
                '.' => Tile::FLOOR,
                '~' => Tile::VOID,
                '≈' => Tile::LIQUID,
                '<' => Tile::Stairs(Dir::Up, 0),
                '>' => Tile::Stairs(Dir::Down, 0),
                '#' => Tile::WALL,
                '◸' => Tile::slope(NW),
                '◹' => Tile::slope(NE),
                '◺' => Tile::slope(SW),
                '◿' => Tile::slope(SE),
                '◤' => Tile::split(NW, wall, Part::Void),
                '◥' => Tile::split(NE, wall, Part::Void),
                '◣' => Tile::split(SW, wall, Part::Void),
                '◢' => Tile::split(SE, wall, Part::Void),
                '▛' => Tile::split(NW, floor, Part::Void),
                '▜' => Tile::split(NE, floor, Part::Void),
                '▙' => Tile::split(SW, floor, Part::Void),
                '▟' => Tile::split(SE, floor, Part::Void),
                '◜' => Tile::split(NW, liquid, floor),
                '◝' => Tile::split(NE, liquid, floor),
                '◟' => Tile::split(SW, liquid, floor),
                '◞' => Tile::split(SE, liquid, floor),
                _ => panic!("unknown tile {c:?}"),
            })
            .collect();
        Self::from_tiles(cols, rows.len(), tiles)
    }

    /// Random tiles with plenty of slopes in every arrangement, for
    /// stress-testing queries.
    #[cfg(any(test, feature = "test-util"))]
    pub fn soup(seed: u32, cols: usize, rows: usize) -> Self {
        let mut rng = Rng::new(seed);
        let tiles = (0..cols * rows)
            .map(|_| match rng.unit() {
                r if r < 0.10 => Solid::Full,
                r if r < 0.30 => Solid::Slope(Corner::CLOCKWISE[rng.range(0, 3)]),
                _ => Solid::None,
            })
            .collect();
        Self::from_solids(cols, rows, tiles)
    }

    /// Its chunk `(cx, cy)` alone, as a layer of its own at its place in
    /// the whole level, without its outline: for drawing.
    pub fn chunk_alone(&self, cx: usize, cy: usize) -> Layer {
        let [w, h] = [
            CHUNK.min(self.cols - cx * CHUNK),
            CHUNK.min(self.rows - cy * CHUNK),
        ];
        let tiles = Chunks::from_chunks(w, h, vec![self.tiles.chunk(cx, cy).clone()]);
        let solids = (0..w * h).map(|i| tiles.get(i % w, i / w).wall()).collect();
        let at = [cx, cy].map(|c| (c * CHUNK) as i32);
        Self::bare(tiles, solids).at([self.origin[0] + at[0], self.origin[1] + at[1]])
    }

    /// The same, its tile (0, 0) at `origin` in the whole level.
    pub fn at(mut self, origin: [i32; 2]) -> Self {
        self.origin = origin;
        self
    }

    /// Where its tile (0, 0) is in the whole level, in tiles.
    pub fn origin(&self) -> [i32; 2] {
        self.origin
    }

    /// Its tile `(x, y)` in the whole level, for picking what it looks
    /// like.
    pub fn absolute(&self, x: usize, y: usize) -> [usize; 2] {
        [
            (x as i64 + self.origin[0] as i64) as usize,
            (y as i64 + self.origin[1] as i64) as usize,
        ]
    }

    /// The tile at `(x, y)`. Out-of-bounds tiles are wall, so the level
    /// edge is a wall.
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if !self.in_bounds([x, y]) {
            return Tile::WALL;
        }
        self.tiles.get(x as usize, y as usize)
    }

    /// Where tile `(x, y)` is wall; out of bounds, all of it.
    pub fn solid(&self, x: i32, y: i32) -> Solid {
        if !self.in_bounds([x, y]) {
            return Solid::Full;
        }
        self.solids[y as usize * self.cols + x as usize]
    }

    /// Is point `p` (pixels) over void (or liquid): in a void quarter of
    /// its tile? Outside the layer, no.
    pub fn is_void_at(&self, p: [f32; 2]) -> bool {
        let ([x, y], quarter) = quarter_at(p);
        self.in_bounds([x, y]) && self.tile(x, y).void_quarters() & quarter != 0
    }

    /// Is tile `(x, y)` entirely free of solid?
    pub fn is_open(&self, x: i32, y: i32) -> bool {
        self.solid(x, y) == Solid::None
    }

    /// Distance from `from` along unit vector `dir` to the first solid
    /// surface, capped at `max` (which may be infinite; the level edge always
    /// stops the ray). Walks the grid exactly (Amanatides & Woo), testing the
    /// diagonal face inside slope tiles. A ray passing exactly through a grid
    /// vertex is blocked only if both tiles beside it reach that vertex, so
    /// light can't leak through a pinch but can graze a single corner. A ray
    /// running exactly along a face, straight or diagonal, grazes it.
    pub fn raycast(&self, from: [f32; 2], dir: [f32; 2], max: f32) -> f32 {
        for across in 0..2 {
            let line = (from[across] / TILE).round();
            if dir[across] == 0.0 && (from[across] - line * TILE).abs() < 1e-4 {
                return self.raycast_along_line(
                    from,
                    1 - across,
                    dir[1 - across],
                    line as i32,
                    max,
                );
            }
        }

        let mut tile = [
            (from[0] / TILE).floor() as i32,
            (from[1] / TILE).floor() as i32,
        ];
        let step = dir.map(|d| {
            if d > 0.0 {
                1
            } else if d < 0.0 {
                -1
            } else {
                0
            }
        });
        // Distance to the next grid line on each axis, computed fresh from
        // the tile index each step rather than accumulated, so rounding
        // stays well inside the vertex tolerance even on long rays.
        let next_line = |tile: [i32; 2], a: usize| match step[a] {
            1 => ((tile[a] + 1) as f32 * TILE - from[a]) / dir[a],
            -1 => (tile[a] as f32 * TILE - from[a]) / dir[a],
            _ => f32::INFINITY,
        };

        let mut t_enter = 0.0;
        loop {
            let t_max = [next_line(tile, 0), next_line(tile, 1)];
            let t_exit = t_max[0].min(t_max[1]);
            if let Some(t) = self.hit_in_tile(tile, from, dir, t_enter, t_exit) {
                return t.min(max);
            }
            if t_exit >= max {
                return max;
            }
            if (t_max[0] - t_max[1]).abs() <= 1e-4 {
                // Exactly through a vertex: blocked by a pinch, else step
                // diagonally.
                let vertex = [
                    tile[0] + (step[0] > 0) as i32,
                    tile[1] + (step[1] > 0) as i32,
                ];
                let beside = [[tile[0] + step[0], tile[1]], [tile[0], tile[1] + step[1]]];
                if beside.iter().all(|&t| self.touches_vertex(t, vertex)) {
                    return t_exit;
                }
                tile = [tile[0] + step[0], tile[1] + step[1]];
            } else {
                let a = if t_max[0] < t_max[1] { 0 } else { 1 };
                tile[a] += step[a];
            }
            t_enter = t_exit;
        }
    }

    /// A ray lying exactly on grid line `line` (perpendicular to axis
    /// `1 - along`), moving along axis `along` in direction `sign`. It runs
    /// between two rows (or columns) of tiles: blocked where both cover the
    /// shared edge, grazing where one does, and blocked at a vertex where
    /// tiles diagonally across the line pinch it.
    fn raycast_along_line(
        &self,
        from: [f32; 2],
        along: usize,
        sign: f32,
        line: i32,
        max: f32,
    ) -> f32 {
        let step = if sign > 0.0 { 1 } else { -1 };
        // Tile position (along axis) of the stretch just ahead of `from`.
        let mut i = if step > 0 {
            (from[along] / TILE).floor() as i32
        } else {
            (from[along] / TILE).ceil() as i32 - 1
        };
        // The tiles before (`a`) and after (`b`) the line at position `i`,
        // and the sides of each facing the line.
        let pair = |i: i32| -> ([i32; 2], [i32; 2]) {
            if along == 0 {
                ([i, line - 1], [i, line])
            } else {
                ([line - 1, i], [line, i])
            }
        };
        let (side_a, side_b) = if along == 0 {
            (Side::S, Side::N)
        } else {
            (Side::E, Side::W)
        };
        let tile = |t: [i32; 2]| self.solid(t[0], t[1]);

        let mut t: f32 = 0.0;
        loop {
            let (a, b) = pair(i);
            if tile(a).covers(side_a) && tile(b).covers(side_b) {
                return t.min(max);
            }
            // Distance to the vertex at the far end of this stretch.
            let edge = if step > 0 { i + 1 } else { i };
            t = (edge as f32 * TILE - from[along]) * sign;
            if t >= max {
                return max;
            }
            let vertex = if along == 0 {
                [edge, line]
            } else {
                [line, edge]
            };
            let (na, nb) = pair(i + step);
            if (self.touches_vertex(a, vertex) && self.touches_vertex(nb, vertex))
                || (self.touches_vertex(b, vertex) && self.touches_vertex(na, vertex))
            {
                return t;
            }
            i += step;
        }
    }

    /// Where a ray crossing `tile` between `t0` and `t1` first enters solid,
    /// if it does before leaving the tile.
    fn hit_in_tile(
        &self,
        tile: [i32; 2],
        from: [f32; 2],
        dir: [f32; 2],
        t0: f32,
        t1: f32,
    ) -> Option<f32> {
        match self.solid(tile[0], tile[1]) {
            Solid::None => None,
            Solid::Full => Some(t0),
            Solid::Slope(c) => {
                let (k, g) = Solid::slope_plane(c);
                // Entry point in tile-local units, kept small for precision.
                let u = (from[0] + dir[0] * t0) / TILE - tile[0] as f32;
                let v = (from[1] + dir[1] * t0) / TILE - tile[1] as f32;
                let f = k + g[0] * u + g[1] * v;
                let df = (g[0] * dir[0] + g[1] * dir[1]) / TILE;
                if f > SLOPE_EPS {
                    // Came in through a solid side.
                    Some(t0)
                } else if df <= 0.0 {
                    // Moving away from the face, or exactly along it.
                    None
                } else {
                    // Reaching the face exactly at the tile corner where the
                    // ray leaves is a vertex crossing, which `raycast`
                    // handles. Judge "exactly" by how closely the ray passes
                    // the face's end: where the ray meets the face along its
                    // length is ill-conditioned at a shallow angle.
                    let t = t0 + (-f).max(0.0) / df;
                    let at_end = c.face_ends().iter().any(|end| {
                        let [dx, dy] = end.offset();
                        let e = [(tile[0] + dx) as f32 * TILE, (tile[1] + dy) as f32 * TILE];
                        let to_end = [e[0] - from[0], e[1] - from[1]];
                        let perp = (dir[0] * to_end[1] - dir[1] * to_end[0]).abs();
                        let along = dir[0] * to_end[0] + dir[1] * to_end[1];
                        // Only where the ray leaves the tile; entering at the
                        // face's end heading into solid is a real hit.
                        perp < 1e-4 && (along - t1).abs() < 0.05 && (along - t).abs() < 0.05
                    });
                    (t < t1 - 1e-4 && !at_end).then_some(t)
                }
            }
        }
    }

    /// Does tile `t` reach grid vertex `vertex` (in tile units)?
    fn touches_vertex(&self, t: [i32; 2], vertex: [i32; 2]) -> bool {
        self.solid(t[0], t[1])
            .touches(Corner::at([vertex[0] - t[0], vertex[1] - t[1]]))
    }

    /// Unobstructed line of sight between two pixel-space points. Symmetric,
    /// and a point lying on a solid tile's face counts as visible.
    pub fn los(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = crate::length(d);
        if len < 1e-6 {
            return true;
        }
        self.raycast(a, [d[0] / len, d[1] / len], len) >= len - 1e-3
    }

    /// Grid vertices (in pixels) where the outline of solid space turns:
    /// the only places a visibility boundary can change direction. Gathers
    /// the outline edges meeting at each vertex (4 axis directions plus 4
    /// diagonals from slope faces); it's a corner unless exactly two meet
    /// and they continue straight.
    pub fn corners(&self) -> &[[f32; 2]] {
        &self.corners
    }

    fn find_corners(&self) -> Vec<[f32; 2]> {
        let solid = |x: i32, y: i32| self.solid(x, y);
        let mut out = Vec::new();
        for gy in 0..=self.rows as i32 {
            for gx in 0..=self.cols as i32 {
                out.extend(corner_at(&solid, gx, gy));
            }
        }
        out
    }

    /// Every piece of the outline of solid space: tile sides with solid on
    /// exactly one side, and slope faces, each one tile long.
    pub fn outline_edges(&self) -> &[Edge] {
        &self.edges
    }

    fn find_edges(&self) -> Vec<Edge> {
        let solid = |x: i32, y: i32| self.solid(x, y);
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        let mut out = Vec::new();
        for y in 0..=rows {
            for x in 0..=cols {
                edges_at(&solid, x, y, x < cols && y < rows, &mut out);
            }
        }
        out
    }

    /// The outline of solid space as closed loops, straight runs merged.
    pub fn outlines(&self) -> Vec<Outline> {
        let mut used = vec![false; self.edges.len()];
        let mut out = Vec::new();
        for first in 0..self.edges.len() {
            if used[first] {
                continue;
            }
            // Walk the loop edge by edge.
            let mut chain = vec![first];
            used[first] = true;
            let mut at = self.next[first] as usize;
            while at != first {
                used[at] = true;
                chain.push(at);
                at = self.next[at] as usize;
            }

            // Start at a turn, then merge runs of edges going the same way.
            let dir = |k: usize| heading(&self.edges[chain[k]]);
            let n = chain.len();
            let start = (0..n).find(|&k| dir(k) != dir((k + n - 1) % n));
            chain.rotate_left(start.unwrap_or(0));
            let mut outline = Outline {
                points: Vec::new(),
                runs: Vec::new(),
            };
            for (k, &i) in chain.iter().enumerate() {
                if k == 0 || heading(&self.edges[i]) != heading(&self.edges[chain[k - 1]]) {
                    outline.points.push(self.edges[i].a);
                    outline.runs.push(Vec::new());
                }
                outline.runs.last_mut().unwrap().push(i);
            }
            out.push(outline);
        }
        out
    }

    /// Indices into [`Layer::outline_edges`] of every edge reaching
    /// within `radius` of `p`, and maybe a few more, in order.
    pub fn edges_near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.edge_buckets.near(p, radius)
    }

    /// Indices into [`Layer::corners`] of every corner within `radius` of
    /// `p`, and maybe a few more, in order.
    pub fn corners_near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.corner_buckets.near(p, radius)
    }

    /// Indices into [`Layer::concave_corners`] of every one within
    /// `radius` of `p`, and maybe a few more, in order.
    pub fn concave_near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.concave_buckets.near(p, radius)
    }

    /// Indices of the outline edges lying on `line`.
    pub fn edges_on(&self, line: Line) -> &[usize] {
        self.edges_by_line.on(line)
    }

    /// Inside corners of the outline.
    pub fn concave_corners(&self) -> &[ConcaveCorner] {
        &self.concave
    }

    /// With solid on the right, a left turn (counter-clockwise on screen)
    /// bends away from the solid.
    fn find_concave_corners(&self) -> Vec<ConcaveCorner> {
        let normal = |e: &Edge| inward_normal(e.a, e.b);
        (0..self.edges.len())
            .filter_map(|i| {
                let (before, here) = (&self.edges[self.prev[i] as usize], &self.edges[i]);
                (turn(heading(before), heading(here)) < 0).then(|| ConcaveCorner {
                    at: here.a,
                    before: normal(before),
                    after: normal(here),
                })
            })
            .collect()
    }

    /// For each outline edge, the next along its loop: each edge's end is
    /// the start of another; where several start at one vertex (solids
    /// touching only at a corner), the sharpest right turn, which keeps the
    /// solid on the right the same and so keeps each solid's loop to
    /// itself.
    fn link_edges(&self) -> Vec<u32> {
        // The edges by the vertex they start at.
        let across = self.cols + 1;
        let at = |p: [f32; 2]| {
            let [x, y] = vertex(p);
            y as usize * across + x as usize
        };
        let mut starts = vec![0u32; across * (self.rows + 1) + 1];
        for e in &self.edges {
            starts[at(e.a) + 1] += 1;
        }
        for k in 1..starts.len() {
            starts[k] += starts[k - 1];
        }
        let mut next = starts.clone();
        let mut items = vec![0u32; self.edges.len()];
        for (i, e) in self.edges.iter().enumerate() {
            let v = at(e.a);
            items[next[v] as usize] = i as u32;
            next[v] += 1;
        }
        let headings: Vec<i32> = self.edges.iter().map(heading).collect();
        self.edges
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let v = at(e.b);
                let from = &items[starts[v] as usize..starts[v + 1] as usize];
                // An outline is closed: something starts where each ends.
                *from
                    .iter()
                    .max_by_key(|&&j| turn(headings[i], headings[j as usize]))
                    .unwrap()
            })
            .collect()
    }

    /// Points which, if seen, mean part of tile `(x, y)` has been seen: in
    /// its open part (the center, and its corners pulled slightly in), and
    /// along each exposed face of its solid part (straight sides the
    /// neighbor doesn't cover, and a slope's diagonal face).
    pub fn sight_samples(&self, x: i32, y: i32) -> Vec<[f32; 2]> {
        const INSET: f32 = 0.5;
        const ALONG: [f32; 3] = [0.1, 0.5, 0.9];
        let tile = self.solid(x, y);
        let corner = |c: Corner| {
            let [dx, dy] = c.offset();
            [(x + dx) as f32 * TILE, (y + dy) as f32 * TILE]
        };
        let mut out = Vec::new();

        let open: Vec<[f32; 2]> = match tile {
            Solid::None => Corner::CLOCKWISE.map(corner).to_vec(),
            Solid::Full => Vec::new(),
            Solid::Slope(c) => Corner::CLOCKWISE
                .into_iter()
                .filter(|k| *k != c)
                .map(corner)
                .collect(),
        };
        if !open.is_empty() {
            let n = open.len() as f32;
            let center = open
                .iter()
                .fold([0.0; 2], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
            out.push(center);
            out.extend(open.iter().map(|p| {
                let d = [center[0] - p[0], center[1] - p[1]];
                let len = crate::length(d);
                [p[0] + d[0] / len * INSET, p[1] + d[1] / len * INSET]
            }));
        }

        let mut faces: Vec<[Corner; 2]> = Side::ALL
            .into_iter()
            .filter(|side| {
                let [dx, dy] = side.offset();
                tile.covers(*side) && !self.solid(x + dx, y + dy).covers(side.opposite())
            })
            .map(Side::ends)
            .collect();
        if let Solid::Slope(c) = tile {
            faces.push(c.face_ends());
        }
        for [a, b] in faces.into_iter().map(|f| f.map(corner)) {
            out.extend(ALONG.map(|s| [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s]));
        }
        out
    }

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
        let steps = (crate::length(delta) / MAX_STEP).ceil().max(1.0);
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

    /// Draw the art of tile `(x, y)` from `tileset`: each part's floor,
    /// wall or liquid (translucent), nothing where void, stairs over the
    /// whole tile.
    pub fn draw_tile(&self, tileset: &Tileset, canvas: &mut Canvas, x: usize, y: usize) {
        let tile = self.tiles.get(x, y);
        let origin = [x as f32 * TILE, y as f32 * TILE];
        let [ax, ay] = self.absolute(x, y);
        let mut part = |shape: Option<Shape>, part: Part| {
            let (cell, color) = match part {
                Part::Floor(f) => (
                    tileset::variant(&tileset.floors[f as usize].cells, ax, ay),
                    palette::WHITE,
                ),
                Part::Wall(w) => (
                    tileset::variant(&tileset.walls[w as usize].cells, ax, ay),
                    palette::WHITE,
                ),
                // Translucent, so what's beneath shows through.
                Part::Liquid(l) => {
                    let liquid = &tileset.liquids[l as usize];
                    (
                        tileset::variant(&liquid.cells, ax, ay),
                        [1.0, 1.0, 1.0, liquid.alpha],
                    )
                }
                Part::Void => return,
            };
            if let Some(shape) = shape {
                canvas.textured(&shape, origin, cell, color);
            }
        };
        if let Some(dir) = tile.stairs() {
            let cell = match dir {
                Dir::Up => tileset.stairs[0],
                Dir::Down => tileset.stairs[1],
            };
            canvas.textured(
                &Solid::Full.shape(x, y).unwrap(),
                origin,
                cell,
                palette::WHITE,
            );
            return;
        }
        match tile.halves() {
            Halves::Whole(p) => part(Solid::Full.shape(x, y), p),
            Halves::Split(c, at, rest) => {
                part(Solid::Slope(c).shape(x, y), at);
                part(Solid::Slope(c.opposite()).shape(x, y), rest);
            }
        }
    }

    /// Draw every tile's art (see [`Layer::draw_tile`]).
    pub fn draw_tiles(&self, tileset: &Tileset, canvas: &mut Canvas) {
        for y in 0..self.rows {
            for x in 0..self.cols {
                self.draw_tile(tileset, canvas, x, y);
            }
        }
    }

    /// Draw a bevel `width` wide along the outline edges `bevel(edge
    /// index)` picks that reach into `within`: the wall's light color on
    /// faces facing up or left, its dark color on faces facing down or
    /// right. Strips along each loop are mitered at its corners so they
    /// meet exactly. Walls with a fringe (see [`crate::fringe`]) have none:
    /// their fringe's rim takes its place.
    pub fn draw_bevels(
        &self,
        tileset: &Tileset,
        canvas: &mut Canvas,
        bevel: impl Fn(usize) -> bool,
        width: f32,
        within: Rect,
    ) {
        let [lo, hi] = within;
        let drawn = |i: usize| {
            let e = &self.edges[i];
            let reaches = |axis: usize| {
                e.a[axis].max(e.b[axis]) + width >= lo[axis]
                    && e.a[axis].min(e.b[axis]) - width <= hi[axis]
            };
            let fringed = || {
                let wall = self.tile(e.solid[0], e.solid[1]).wall_id().unwrap_or(0);
                tileset.walls[wall as usize].edge.width > 0.0
            };
            reaches(0) && reaches(1) && self.in_bounds(e.solid) && !fringed() && bevel(i)
        };
        let normal = |i: usize| inward_normal(self.edges[i].a, self.edges[i].b);
        for i in self.edge_buckets.over(within, width) {
            if !drawn(i) {
                continue;
            }
            let e = self.edges[i];
            let n = normal(i);
            // Along a straight run the strip's inner edge is just `width`
            // in; where the loop turns it's mitered with the next run's.
            let inner = |p: [f32; 2], other: usize, before: bool| {
                if heading(&self.edges[other]) == heading(&e) {
                    [p[0] + n[0] * width, p[1] + n[1] * width]
                } else if before {
                    inset_corner(p, normal(other), n, width)
                } else {
                    inset_corner(p, n, normal(other), width)
                }
            };
            let a = inner(e.a, self.prev[i] as usize, true);
            let b = inner(e.b, self.next[i] as usize, false);
            let wall = self.tile(e.solid[0], e.solid[1]).wall_id().unwrap_or(0);
            let wall = &tileset.walls[wall as usize];
            let color = if faces_light(n) {
                wall.light
            } else {
                wall.dark
            };
            canvas.poly(&[e.a, e.b, b, a], color);
        }
    }

    pub fn in_bounds(&self, t: [i32; 2]) -> bool {
        t[0] >= 0 && t[1] >= 0 && t[0] < self.cols as i32 && t[1] < self.rows as i32
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
        .min_by(|x, y| crate::length(*x).total_cmp(&crate::length(*y)))
}

/// Invariant checks for tests, here and in other crates.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::Layer;

    /// Each outline edge is in exactly one run, and each loop is closed:
    /// every run starts where the previous one ended.
    pub fn check_outlines(level: &Layer) {
        let mut seen = vec![0; level.outline_edges().len()];
        for o in level.outlines() {
            assert_eq!(o.points.len(), o.runs.len());
            for (k, run) in o.runs.iter().enumerate() {
                let edges: Vec<_> = run.iter().map(|&i| level.outline_edges()[i]).collect();
                assert_eq!(edges[0].a, o.points[k]);
                assert_eq!(edges.last().unwrap().b, o.points[(k + 1) % o.points.len()]);
                for w in edges.windows(2) {
                    assert_eq!(w[0].b, w[1].a);
                }
                for &i in run {
                    seen[i] += 1;
                }
            }
        }
        assert!(seen.iter().all(|&n| n == 1), "{seen:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::testing::check_outlines;
    use super::*;

    const SIZE: [f32; 2] = [8.0, 8.0];

    #[track_caller]
    fn assert_near(a: [f32; 2], b: [f32; 2]) {
        assert!(
            (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn slides_along_tiled_wall_without_snagging() {
        // A floor of separate tiles along row 5 (y 50..60).
        let level = Layer::from_ascii(&[
            "....................",
            "....................",
            "....................",
            "....................",
            "....................",
            "####################",
        ]);
        let mut pos = [5.0, 41.0];
        for _ in 0..100 {
            pos = level.move_box(pos, SIZE, [1.0, 1.0]);
            assert!(!level.collides(pos, SIZE));
        }
        assert_near(pos, [105.0, 42.0]);
    }

    #[test]
    fn slides_up_a_slope() {
        // Solid below the "/" face x + y = 70.
        let level = Layer::from_ascii(&[
            "......◿#", //
            ".....◿##", //
            "....◿###", //
            "...◿####", //
            "..◿#####", //
            ".◿######", //
            "◿#######", //
        ]);
        let size = [4.0, 4.0];
        let mut pos = [20.0, 30.0];
        for _ in 0..30 {
            pos = level.move_box(pos, size, [1.0, 0.0]);
            assert!(!level.collides(pos, size), "{pos:?}");
        }
        // Resting on the face: bottom-right corner on x + y = 70.
        assert!((pos[0] + pos[1] + 8.0 - 70.0).abs() < 1e-2, "{pos:?}");
        assert!(pos[1] < 30.0 && pos[0] > 24.0, "slid up and along: {pos:?}");
    }

    #[test]
    fn slopes_slide_in_every_direction() {
        // A lone slope of each kind; push a box into it from all 8 directions.
        for tile in ["◸", "◹", "◺", "◿"] {
            let rows = [".....", ".....", &format!("..{tile}.."), ".....", "....."];
            let level = Layer::from_ascii(&rows);
            for d in [
                [1., 0.],
                [-1., 0.],
                [0., 1.],
                [0., -1.],
                [1., 1.],
                [1., -1.],
                [-1., 1.],
                [-1., -1.],
            ] {
                let start = [25.0 - d[0] * 12.0 - 2.0, 25.0 - d[1] * 12.0 - 2.0];
                let mut pos = start;
                for _ in 0..40 {
                    pos = level.move_box(pos, [4.0, 4.0], [d[0] * 0.7, d[1] * 0.7]);
                    assert!(!level.collides(pos, [4.0, 4.0]), "{tile} {d:?} {pos:?}");
                }
                assert!(pos != start, "{tile} {d:?}: stuck at start");
            }
        }
    }

    #[test]
    fn settles_in_a_wedge() {
        // A 135-degree corner where the face x + y = 60 meets the wall x = 40.
        let level = Layer::from_ascii(&[
            "....#", //
            "....#", //
            "...◿#", //
            "..◿##", //
            ".◿###", //
        ]);
        let size = [6.0, 6.0];
        // Pushing right and a little down: onto the face, then along it.
        let mut pos = [10.0, 5.0];
        let mut trail = Vec::new();
        for _ in 0..80 {
            pos = level.move_box(pos, size, [1.0, 0.5]);
            assert!(!level.collides(pos, size), "{pos:?}");
            trail.push(pos);
        }
        // Wedged against both: right edge on the wall, corner on the face,
        // and holding still rather than jittering.
        assert_near(pos, [34.0, 14.0]);
        assert!(
            trail[trail.len() - 10..].iter().all(|p| *p == pos),
            "{:?}",
            &trail[trail.len() - 10..]
        );
    }

    #[test]
    fn fast_moves_dont_tunnel() {
        // One-tile walls at x 20..30: full tiles, and stacked slopes whose
        // left sides form a continuous face.
        for rows in [["..#..", "..#..", "..#.."], ["..◸..", "..◸..", "..◸.."]] {
            let level = Layer::from_ascii(&rows);
            let size = [3.0, 3.0];
            for y in [1.0, 8.5, 13.0, 26.0] {
                let mut pos = [2.0, y];
                for _ in 0..10 {
                    pos = level.move_box(pos, size, [9.0, 0.0]);
                }
                assert!(
                    pos[0] + size[0] <= 20.0 + 1e-3,
                    "through {rows:?} at y={y}: {pos:?}"
                );
            }
        }
    }

    #[test]
    fn random_walks_never_enter_solid() {
        let mut rng = Rng::new(5);
        for seed in 0..30 {
            let level = Layer::soup(seed, 32, 20);
            let size = [6.0, 6.0];
            let Some(mut pos) = (0..1000)
                .map(|_| [rng.unit() * 310.0, rng.unit() * 190.0])
                .find(|p| !level.collides(*p, size))
            else {
                continue;
            };
            for _ in 0..500 {
                let a = rng.unit() * std::f32::consts::TAU;
                let len = rng.unit() * 9.0;
                pos = level.move_box(pos, size, [a.cos() * len, a.sin() * len]);
                assert!(!level.collides(pos, size), "seed {seed} at {pos:?}");
            }
        }
    }

    fn level_with(x: i32, y: i32, w: i32, h: i32) -> Layer {
        let mut tiles = vec![Tile::FLOOR; 100];
        for ty in y..y + h {
            for tx in x..x + w {
                tiles[(ty * 10 + tx) as usize] = Tile::WALL;
            }
        }
        Layer::from_tiles(10, 10, tiles)
    }

    #[test]
    fn stops_flush_against_block() {
        let level = level_with(5, 0, 1, 10);
        // One big move and many small ones both end flush at x = 50 - 8.
        let pos = level.move_box([31.0, 1.0], SIZE, [100.0, 0.0]);
        assert_near(pos, [42.0, 1.0]);
        let mut pos = [31.0, 1.0];
        for _ in 0..20 {
            pos = level.move_box(pos, SIZE, [1.5, 0.0]);
        }
        assert_near(pos, [42.0, 1.0]);
        assert!(!level.collides(pos, SIZE));
    }

    #[test]
    fn slides_along_wall() {
        let level = level_with(5, 0, 1, 10);
        let pos = level.move_box([42.0, 10.0], SIZE, [1.0, 1.0]);
        assert_near(pos, [42.0, 11.0]);
    }

    #[test]
    fn edge_is_solid() {
        let level = level_with(9, 9, 1, 1);
        assert_near(level.move_box([1.0, 1.0], SIZE, [-3.0, -3.0]), [0.0, 0.0]);
    }

    /// Shorthand: does the segment from `a` to `b` have line of sight?
    fn sees(level: &Layer, a: [f32; 2], b: [f32; 2]) -> bool {
        level.los(a, b)
    }

    #[test]
    fn rays_hit_slope_sides_and_face() {
        // Slope solid in the NW half of tile (1,1): x 10..20, y 10..20.
        let level = Layer::from_ascii(&["...", ".◸.", "..."]);
        let left = level.raycast([5.0, 12.5], [1.0, 0.0], f32::INFINITY);
        assert!(
            (left - 5.0).abs() < 1e-4,
            "hits the left side at x=10: {left}"
        );
        let right = level.raycast([25.0, 17.5], [-1.0, 0.0], f32::INFINITY);
        assert!(
            (right - 12.5).abs() < 1e-4,
            "hits the face at x=12.5: {right}"
        );
        // From below-right, the face is visible but not what's behind it.
        assert!(sees(&level, [25.0, 25.0], [15.0, 15.0]));
        assert!(!sees(&level, [25.0, 25.0], [12.0, 12.0]));
    }

    #[test]
    fn rays_graze_slope_faces_and_corners() {
        let level = Layer::from_ascii(&["...", ".◸.", "..."]);
        // Exactly along the face from corner (20,10) to corner (10,20).
        assert!(sees(&level, [25.0, 5.0], [5.0, 25.0]));
        // A hair inside the solid side of it.
        assert!(!sees(&level, [24.9, 5.0], [4.9, 25.0]));
        // Through the empty corner (20,20) only.
        assert!(sees(&level, [25.0, 15.0], [15.0, 25.0]));
    }

    #[test]
    fn slopes_pinch_at_shared_vertex() {
        // Both reach vertex (10,10): a ray through it is blocked.
        let pinch = Layer::from_ascii(&["◿.", ".◸"]);
        assert!(!sees(&pinch, [15.0, 5.0], [5.0, 15.0]));
        // Only the first reaches it: the ray grazes past.
        let graze = Layer::from_ascii(&["◿.", ".◿"]);
        assert!(sees(&graze, [15.0, 5.0], [5.0, 15.0]));
    }

    #[test]
    fn rays_graze_along_straight_faces() {
        // Along y = 10, the top face of the block, with open floor above.
        let level = Layer::from_ascii(&["....", ".##.", "...."]);
        assert!(sees(&level, [0.0, 10.0], [40.0, 10.0]));
        assert!(sees(&level, [40.0, 10.0], [0.0, 10.0]));
        // Along x = 20, through the middle of the block: blocked.
        assert!(!sees(&level, [20.0, 0.0], [20.0, 30.0]));
        // Along y = 10 between diagonally touching blocks: pinched.
        let pinch = Layer::from_ascii(&["#...", ".#..", "...."]);
        assert!(!sees(&pinch, [0.0, 10.0], [40.0, 10.0]));
    }

    #[test]
    fn rays_start_inside_slope_tiles() {
        let level = Layer::from_ascii(&["◸.."]);
        // Empty half of the slope tile, looking right: open to the far edge.
        let open = level.raycast([8.0, 8.0], [1.0, 0.0], f32::INFINITY);
        assert!((open - 22.0).abs() < 1e-4, "{open}");
        // Solid half: blocked immediately.
        assert_eq!(level.raycast([2.0, 2.0], [1.0, 0.0], f32::INFINITY), 0.0);
    }

    #[test]
    fn corners_follow_diagonal_outline() {
        // Solid below the diagonal from (30,0) to (0,30).
        let level = Layer::from_ascii(&["..◿", ".◿#", "◿##"]);
        let mut corners = level.corners().to_vec();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(corners, vec![[0.0, 0.0], [0.0, 30.0], [30.0, 0.0]]);

        // A lone slope: its three corners.
        let lone = Layer::from_ascii(&["...", ".◹.", "..."]);
        let c = lone.corners();
        for v in [[10.0, 10.0], [20.0, 10.0], [20.0, 20.0]] {
            assert!(c.contains(&v), "{v:?} in {c:?}");
        }
        assert!(!c.contains(&[10.0, 20.0]));
    }

    #[test]
    fn outline_edges_cover_sides_and_faces() {
        let level = Layer::from_ascii(&["◸"]);
        let edges: Vec<_> = level
            .outline_edges()
            .iter()
            .map(|e| (e.a, e.b, e.solid))
            .collect();
        // Solid on the right of each: the face (solid toward the NW corner),
        // and the empty half's right and bottom sides against the
        // out-of-bounds wall.
        assert!(
            edges.contains(&([10.0, 0.0], [0.0, 10.0], [0, 0])),
            "{edges:?}"
        );
        assert!(
            edges.contains(&([10.0, 10.0], [10.0, 0.0], [1, 0])),
            "{edges:?}"
        );
        assert!(
            edges.contains(&([0.0, 10.0], [10.0, 10.0], [0, 1])),
            "{edges:?}"
        );
        assert_eq!(edges.len(), 3, "{edges:?}");
    }

    /// Twice the signed area: positive clockwise on screen (around solid).
    fn area2(points: &[[f32; 2]]) -> f32 {
        (0..points.len())
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum()
    }

    #[test]
    fn outlines_of_a_block_and_the_boundary() {
        let level = Layer::from_ascii(&["....", ".##.", ".##.", "...."]);
        check_outlines(&level);
        let outlines = level.outlines();
        assert_eq!(outlines.len(), 2);
        let block = outlines.iter().find(|o| area2(&o.points) > 0.0).unwrap();
        assert_eq!(block.points.len(), 4);
        assert!(
            block.runs.iter().all(|r| r.len() == 2),
            "straight runs merged"
        );
        assert!((area2(&block.points) - 2.0 * 400.0).abs() < 1e-3);
        let boundary = outlines.iter().find(|o| area2(&o.points) < 0.0).unwrap();
        assert!((area2(&boundary.points) + 2.0 * 1600.0).abs() < 1e-3);
    }

    #[test]
    fn hollow_room_has_a_hole_loop() {
        let level = Layer::from_ascii(&[
            ".....", //
            ".###.", //
            ".#.#.", //
            ".###.", //
            ".....", //
        ]);
        check_outlines(&level);
        let mut areas: Vec<f32> = level
            .outlines()
            .into_iter()
            .map(|o| area2(&o.points) / 2.0)
            .collect();
        areas.sort_by(f32::total_cmp);
        // Layer boundary, the hole (counter-clockwise), the room's outside.
        assert_eq!(areas, vec![-2500.0, -100.0, 900.0]);
    }

    #[test]
    fn touching_blocks_merge_and_pinches_split() {
        // An L of two blocks sharing a side: one loop, no internal edges.
        let l = Layer::from_ascii(&["....", ".#..", ".##.", "...."]);
        check_outlines(&l);
        let solid: Vec<_> = l
            .outlines()
            .into_iter()
            .filter(|o| area2(&o.points) > 0.0)
            .collect();
        assert_eq!(solid.len(), 1);
        assert_eq!(solid[0].points.len(), 6);

        // Two blocks touching only at a corner: two loops.
        let pinch = Layer::from_ascii(&["....", ".#..", "..#.", "...."]);
        check_outlines(&pinch);
        let solid: Vec<_> = pinch
            .outlines()
            .into_iter()
            .filter(|o| area2(&o.points) > 0.0)
            .collect();
        assert_eq!(solid.len(), 2);
        assert!(solid.iter().all(|o| o.points.len() == 4));
    }

    #[test]
    fn slanted_outlines_merge_diagonal_runs() {
        // A rising bar: two full tiles with slopes filling both steps.
        let bar = Layer::from_ascii(&[".....", ".◿#..", ".#◸..", "....."]);
        check_outlines(&bar);
        let solid: Vec<_> = bar
            .outlines()
            .into_iter()
            .filter(|o| area2(&o.points) > 0.0)
            .collect();
        assert_eq!(solid.len(), 1);
        // Flat ends and two long diagonal faces: a hexagon.
        assert_eq!(solid[0].points.len(), 6, "{:?}", solid[0].points);

        for seed in 0..30 {
            check_outlines(&Layer::soup(seed, 32, 20));
        }
    }

    /// How outlines were found before edges were linked: chained edge by
    /// edge, each the sharpest right turn among those not yet used.
    fn traced_outlines(level: &Layer) -> Vec<Outline> {
        let key = |p: [f32; 2]| [(p[0] / TILE).round() as i32, (p[1] / TILE).round() as i32];
        let edges = level.outline_edges();
        let dir = |e: &Edge| {
            let [a, b] = [key(e.a), key(e.b)];
            [b[0] - a[0], b[1] - a[1]]
        };
        let mut starting: std::collections::HashMap<[i32; 2], Vec<usize>> = Default::default();
        for (i, e) in edges.iter().enumerate() {
            starting.entry(key(e.a)).or_default().push(i);
        }
        let mut used = vec![false; edges.len()];
        let mut out = Vec::new();
        for first in 0..edges.len() {
            if used[first] {
                continue;
            }
            let mut chain = vec![first];
            used[first] = true;
            loop {
                let last = *chain.last().unwrap();
                let d = dir(&edges[last]);
                let next = starting[&key(edges[last].b)]
                    .iter()
                    .copied()
                    .filter(|&i| !used[i] || i == first)
                    .max_by(|&i, &j| {
                        let turn = |k: usize| {
                            let n = dir(&edges[k]);
                            let cross = (d[0] * n[1] - d[1] * n[0]) as f32;
                            let dot = (d[0] * n[0] + d[1] * n[1]) as f32;
                            cross.atan2(dot)
                        };
                        turn(i).total_cmp(&turn(j))
                    });
                match next {
                    Some(i) if i == first => break,
                    Some(i) => {
                        used[i] = true;
                        chain.push(i);
                    }
                    None => panic!("an open outline"),
                }
            }
            let turns_at = |k: usize| {
                dir(&edges[chain[k]]) != dir(&edges[chain[(k + chain.len() - 1) % chain.len()]])
            };
            let start = (0..chain.len()).find(|&k| turns_at(k)).unwrap_or(0);
            chain.rotate_left(start);
            let mut outline = Outline {
                points: Vec::new(),
                runs: Vec::new(),
            };
            for (k, &i) in chain.iter().enumerate() {
                if k == 0 || dir(&edges[i]) != dir(&edges[chain[k - 1]]) {
                    outline.points.push(edges[i].a);
                    outline.runs.push(Vec::new());
                }
                outline.runs.last_mut().unwrap().push(i);
            }
            out.push(outline);
        }
        out
    }

    /// Bevel strips as they were drawn from whole loops, each a quad of
    /// points.
    fn traced_bevels(level: &Layer, width: f32) -> Vec<[[f32; 2]; 4]> {
        let mut out = Vec::new();
        for o in traced_outlines(level) {
            let n = o.points.len();
            let normals: Vec<[f32; 2]> = (0..n)
                .map(|k| inward_normal(o.points[k], o.points[(k + 1) % n]))
                .collect();
            let inset: Vec<[f32; 2]> = (0..n)
                .map(|k| inset_corner(o.points[k], normals[(k + n - 1) % n], normals[k], width))
                .collect();
            for (k, run) in o.runs.iter().enumerate() {
                let normal = [normals[k][0] * width, normals[k][1] * width];
                let (start, end) = (o.points[k], o.points[(k + 1) % n]);
                let inner = |p: [f32; 2], corner: [f32; 2], mitered: [f32; 2]| {
                    if p == corner {
                        mitered
                    } else {
                        [p[0] + normal[0], p[1] + normal[1]]
                    }
                };
                for &i in run {
                    let e = level.outline_edges()[i];
                    if level.in_bounds(e.solid) {
                        let a = inner(e.a, start, inset[k]);
                        let b = inner(e.b, end, inset[(k + 1) % n]);
                        out.push([e.a, e.b, b, a]);
                    }
                }
            }
        }
        out
    }

    #[test]
    fn linked_edges_make_the_loops_tracing_did() {
        let close =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        let mut levels: Vec<Layer> = (0..40).map(|seed| Layer::soup(seed, 40, 30)).collect();
        // Pinches: lone full tiles touching at corners, a checkerboard.
        levels.push(Layer::from_ascii(&["#.#.", ".#.#", "#.#.", ".#.#"]));
        levels.push(Layer::from_ascii(&["....", ".#..", "..#.", "...."]));
        for level in &levels {
            check_outlines(level);
            let traced = traced_outlines(level);
            assert_eq!(level.outlines(), traced);
            // Concave corners: where a loop turns left.
            let mut concave = Vec::new();
            for o in &traced {
                let n = o.points.len();
                for k in 0..n {
                    let (prev, here, next) = (
                        o.points[(k + n - 1) % n],
                        o.points[k],
                        o.points[(k + 1) % n],
                    );
                    let (d0, d1) = (
                        [here[0] - prev[0], here[1] - prev[1]],
                        [next[0] - here[0], next[1] - here[1]],
                    );
                    if d0[0] * d1[1] - d0[1] * d1[0] < 0.0 {
                        concave.push((here, inward_normal(prev, here), inward_normal(here, next)));
                    }
                }
            }
            assert_eq!(level.concave_corners().len(), concave.len());
            for c in level.concave_corners() {
                assert!(
                    concave.iter().any(|&(at, b, a)| close(at, c.at)
                        && close(b, c.before)
                        && close(a, c.after)),
                    "{c:?}"
                );
            }
            // Bevels: the same strips.
            let tileset = crate::tileset::testing::tileset();
            let mut canvas = Canvas::new(&tileset);
            level.draw_bevels(
                &tileset,
                &mut canvas,
                |_| true,
                1.5,
                crate::camera::EVERYWHERE,
            );
            let traced = traced_bevels(level, 1.5);
            let drawn = canvas.vertices.len();
            assert_eq!(drawn, traced.len() * 6, "two triangles a strip");
            let strips: Vec<[[f32; 2]; 3]> = canvas
                .vertices
                .chunks(3)
                .map(|t| [t[0].pos, t[1].pos, t[2].pos])
                .collect();
            for quad in &traced {
                let [a, b, c, d] = *quad;
                let has = |t: [[f32; 2]; 3]| {
                    strips
                        .iter()
                        .any(|s| s.iter().zip(&t).all(|(p, q)| close(*p, *q)))
                };
                assert!(has([a, b, c]) && has([a, c, d]), "{quad:?}");
            }
        }
    }

    /// Everything two layers know about their shape, compared.
    #[track_caller]
    fn assert_same_shape(a: &Layer, b: &Layer) {
        assert_eq!((a.cols, a.rows), (b.cols, b.rows));
        assert!(a.solids == b.solids, "solids");
        assert_eq!(a.corners, b.corners);
        assert_eq!(a.edges, b.edges);
        assert_eq!((&a.next, &a.prev), (&b.next, &b.prev));
        assert_eq!(a.concave, b.concave);
        for y in 0..a.rows as i32 {
            for x in 0..a.cols as i32 {
                assert!(identical_tile(a.tile(x, y), b.tile(x, y)), "({x}, {y})");
            }
        }
    }

    fn identical_tile(a: Tile, b: Tile) -> bool {
        crate::chunk::identical(a, b)
    }

    #[test]
    fn layers_from_pieces_are_layers_from_tiles() {
        // Not whole chunks: the last are cut short, one to a single column.
        for (seed, [cols, rows]) in [(1, [100, 70]), (2, [64, 64]), (3, [97, 33]), (4, [20, 9])] {
            let soup = Layer::soup(seed, cols, rows);
            let tiles: Vec<Tile> = (0..cols * rows)
                .map(|i| soup.tile((i % cols) as i32, (i / cols) as i32))
                .collect();
            let chunks = Chunks::from_tiles(cols, rows, &tiles);
            let [across, down] = chunks.size();
            let size =
                |cx: usize, cy: usize| [CHUNK.min(cols - cx * CHUNK), CHUNK.min(rows - cy * CHUNK)];
            let pieces: Vec<Arc<Piece>> = (0..across * down)
                .map(|c| {
                    let [cx, cy] = [c % across, c / across];
                    Arc::new(Piece::new(chunks.chunk(cx, cy).clone(), size(cx, cy)))
                })
                .collect();
            let whole = Layer::from_chunks(chunks.clone());
            assert_same_shape(&Layer::from_pieces(cols, rows, &pieces), &whole);
            // And shaped by solids alone.
            let solids: Vec<Solid> = tiles.iter().map(|t| t.wall()).collect();
            let pieces: Vec<Arc<Piece>> = (0..across * down)
                .map(|c| {
                    let [cx, cy] = [c % across, c / across];
                    let [w, h] = size(cx, cy);
                    let solids = (0..w * h)
                        .map(|i| solids[(cy * CHUNK + i / w) * cols + cx * CHUNK + i % w])
                        .collect();
                    Arc::new(Piece::from_solids([w, h], solids))
                })
                .collect();
            assert_same_shape(
                &Layer::from_pieces(cols, rows, &pieces),
                &Layer::from_solids(cols, rows, solids),
            );
        }
    }

    /// Area of the bevel strips drawn.
    fn bevel_area(level: &Layer, bevel: impl Fn(usize) -> bool) -> f32 {
        let tileset = tileset::testing::tileset();
        let mut canvas = Canvas::new(&tileset);
        level.draw_bevels(&tileset, &mut canvas, bevel, 1.0, crate::camera::EVERYWHERE);
        canvas
            .vertices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn bevels_out_of_view_are_left_out() {
        // Blocks at the left and right ends.
        let level = Layer::from_ascii(&["........", ".#....#.", "........"]);
        let tileset = tileset::testing::tileset();
        let area = |within: Rect| {
            let mut canvas = Canvas::new(&tileset);
            level.draw_bevels(&tileset, &mut canvas, |_| true, 1.0, within);
            canvas.vertices.len()
        };
        let all = area(crate::camera::EVERYWHERE);
        let left = area([[0.0, 0.0], [30.0, 30.0]]);
        assert!(left > 0 && 2 * left == all, "{left} of {all}");
        assert_eq!(area([[25.0, 0.0], [55.0, 30.0]]), 0, "between them");
    }

    #[test]
    fn fringed_walls_have_no_bevels() {
        let level = Layer::from_ascii(&["....", ".#..", "...."]);
        let mut tileset = tileset::testing::tileset();
        let count = |tileset: &Tileset| {
            let mut canvas = Canvas::new(tileset);
            level.draw_bevels(
                tileset,
                &mut canvas,
                |_| true,
                1.0,
                crate::camera::EVERYWHERE,
            );
            canvas.vertices.len()
        };
        assert!(count(&tileset) > 0);
        tileset.walls[0].edge.width = 2.0;
        assert_eq!(count(&tileset), 0, "the fringe's rim instead");
    }

    #[test]
    fn near_finds_everything_within_reach() {
        let mut rng = Rng::new(5);
        for seed in 0..10 {
            let level = Layer::soup(seed, 60, 40);
            for _ in 0..10 {
                let p = [rng.unit() * 600.0, rng.unit() * 400.0];
                let radius = rng.unit() * 200.0;
                let dist = |q: [f32; 2]| crate::length([q[0] - p[0], q[1] - p[1]]);
                let near = level.corners_near(p, radius);
                assert!(near.windows(2).all(|w| w[0] < w[1]), "in order, once each");
                for (i, c) in level.corners().iter().enumerate() {
                    if dist(*c) <= radius {
                        assert!(near.contains(&i), "corner {c:?} near {p:?}");
                    }
                }
                let near = level.edges_near(p, radius);
                for (i, e) in level.outline_edges().iter().enumerate() {
                    // Its nearest point to `p`.
                    let d = [e.b[0] - e.a[0], e.b[1] - e.a[1]];
                    let t = (((p[0] - e.a[0]) * d[0] + (p[1] - e.a[1]) * d[1])
                        / (d[0] * d[0] + d[1] * d[1]))
                        .clamp(0.0, 1.0);
                    if dist([e.a[0] + t * d[0], e.a[1] + t * d[1]]) <= radius {
                        assert!(near.contains(&i), "edge {i} near {p:?}");
                    }
                }
                let near = level.concave_near(p, radius);
                for (i, c) in level.concave_corners().iter().enumerate() {
                    if dist(c.at) <= radius {
                        assert!(near.contains(&i));
                    }
                }
            }
            let all = level.corners_near([0.0; 2], f32::INFINITY);
            assert_eq!(all.len(), level.corners().len());
        }
    }

    #[test]
    fn bevels_tile_the_border_exactly() {
        // A 20x20 block: strips cover its border, 400 - 18 * 18.
        let block = Layer::from_ascii(&["....", ".##.", ".##.", "...."]);
        assert!((bevel_area(&block, |_| true) - 76.0).abs() < 1e-3);

        // An L (concave corner): 300 minus its 1px inset, 18 * 18 - 10 * 10.
        let l = Layer::from_ascii(&["....", ".#..", ".##.", "...."]);
        assert!((bevel_area(&l, |_| true) - 76.0).abs() < 1e-3);

        // A lone slope with two 45-degree corners: half of 100 minus its
        // inset triangle, whose legs are 10 - 2 - sqrt(2).
        let slope = Layer::from_ascii(&["...", ".◸.", "..."]);
        let leg = 10.0 - 2.0 - std::f32::consts::SQRT_2;
        assert!((bevel_area(&slope, |_| true) - (50.0 - leg * leg / 2.0)).abs() < 1e-3);
    }

    #[test]
    fn bevels_only_where_picked() {
        let block = Layer::from_ascii(&["....", ".##.", ".##.", "...."]);
        assert_eq!(bevel_area(&block, |_| false), 0.0);
        // Just the top face's two edges: each 10x1 strip, mitered at the
        // outer ends (each loses a half-pixel triangle).
        let top: Vec<usize> = (0..block.outline_edges().len())
            .filter(|&i| {
                let e = block.outline_edges()[i];
                e.a[1] == 10.0 && e.b[1] == 10.0 && block.in_bounds(e.solid)
            })
            .collect();
        assert_eq!(top.len(), 2);
        let area = bevel_area(&block, |i| top.contains(&i));
        assert!((area - 19.0).abs() < 1e-3, "{area}");
    }

    #[test]
    fn union_and_carve() {
        use Corner::*;
        use Solid::*;
        assert_eq!(Slope(NW).union(Slope(SE)), Full, "opposite slopes fill");
        assert_eq!(Slope(NW).union(Slope(NW)), Slope(NW));
        assert_eq!(
            Slope(NW).union(Slope(NE)),
            Full,
            "unrepresentable rounds up"
        );
        assert_eq!(None.union(Slope(SW)), Slope(SW));
        assert_eq!(
            Full.carve(Slope(NW)),
            Slope(SE),
            "leaves the opposite slope"
        );
        assert_eq!(
            Slope(NW).carve(Slope(NE)),
            None,
            "unrepresentable rounds down"
        );
        assert_eq!(Slope(NW).carve(Slope(SE)), Slope(NW));
        assert_eq!(Full.carve(Full), None);
    }

    #[test]
    fn stamping_and_carving_keep_materials() {
        use Corner::*;
        let (brick, stone) = (Part::Wall(1), Part::Wall(2));
        let (planks, dirt) = (Part::Floor(1), Part::Floor(2));
        let slope = Tile::split(NW, brick, planks);
        // The stamped wall where the piece is wall; what was there before
        // elsewhere, void included. The piece's floor doesn't matter.
        assert_eq!(Tile::VOID.stamp(slope), Tile::split(NW, brick, Part::Void));
        assert_eq!(Tile::Whole(dirt).stamp(slope), Tile::split(NW, brick, dirt));
        let filled = Tile::split(SE, stone, dirt).stamp(slope);
        assert_eq!(filled, Tile::split(NW, brick, stone));
        assert_eq!(filled.wall(), Solid::Full);
        // Halves no tile can have round up to the stamped wall.
        assert_eq!(
            Tile::split(NE, stone, dirt).stamp(slope),
            Tile::Whole(brick)
        );
        assert_eq!(Tile::Whole(dirt).stamp(Tile::VOID), Tile::Whole(dirt));
        assert_eq!(
            Tile::Stairs(Dir::Up, 1).stamp(Tile::VOID),
            Tile::Stairs(Dir::Up, 1)
        );

        assert_eq!(
            Tile::Whole(brick).carve(Solid::Slope(NW), planks),
            Tile::split(SE, brick, planks)
        );
        assert_eq!(
            Tile::split(NW, brick, Part::Void).carve(Solid::Slope(NW), planks),
            Tile::split(NW, planks, Part::Void)
        );
        assert_eq!(
            Tile::Whole(brick).carve(Solid::Full, Part::Void),
            Tile::VOID
        );
        assert_eq!(Tile::split(NE, dirt, dirt), Tile::Whole(dirt), "normalized");
    }

    #[test]
    fn liquid_is_void_to_the_game() {
        let t = Layer::from_ascii(&["≈"]).tile(0, 0);
        assert_eq!(t, Tile::LIQUID);
        assert_eq!(t.void(), Solid::Full);
        assert_eq!(t.liquid_quarters(), 0b1111);
        assert_eq!(t.liquid_id(), Some(0));
        assert_eq!(Tile::VOID.liquid_quarters(), 0);
        assert_eq!(t.wall(), Solid::None, "no block to sight");
        let half = Tile::split(Corner::NW, Part::Floor(0), Part::Liquid(1));
        assert_eq!(half.void(), Solid::Slope(Corner::SE));
        assert_eq!(half.liquid_quarters(), half.void_quarters());
        assert_eq!(half.liquid_id(), Some(1));
    }

    #[test]
    fn points_over_void_by_quarter() {
        let l = Layer::from_ascii(&["▛~.", "≈#."]);
        // Floor in the NW half, void in the SE: the diagonal decides.
        assert!(!l.is_void_at([2.0, 3.0]));
        assert!(l.is_void_at([8.0, 7.0]));
        assert!(l.is_void_at([15.0, 5.0]), "void");
        assert!(l.is_void_at([5.0, 15.0]), "liquid counts");
        assert!(!l.is_void_at([25.0, 5.0]), "floor");
        assert!(!l.is_void_at([15.0, 15.0]), "wall");
        assert!(!l.is_void_at([-1.0, 5.0]), "outside");
    }

    #[test]
    fn tiles_have_wall_and_void_shapes() {
        use Corner::*;
        let t = Tile::split(NW, Part::Wall(0), Part::Void);
        assert_eq!((t.wall(), t.void()), (Solid::Slope(NW), Solid::Slope(SE)));
        let t = Tile::split(SW, Part::Floor(0), Part::Void);
        assert_eq!((t.wall(), t.void()), (Solid::None, Solid::Slope(NE)));
        assert_eq!(Tile::Stairs(Dir::Up, 0).void(), Solid::None);
        let l = Layer::from_ascii(&["◤▟◿"]);
        assert_eq!(l.solid(0, 0), Solid::Slope(NW));
        assert!(l.is_open(1, 0), "floor and void, no wall");
        assert_eq!(l.solid(2, 0), Solid::Slope(SE));
    }

    #[test]
    fn quarter_shapes_cover_their_quarters() {
        let area = |q: u8| -> f32 {
            quarter_shapes(1, 1, q)
                .iter()
                .map(|s| area2(s).abs() / 2.0)
                .sum()
        };
        assert_eq!(quarter_shapes(1, 1, N | E | S | W).len(), 1);
        assert_eq!(quarter_shapes(1, 1, S | E).len(), 1, "a half is one shape");
        assert!((area(N | E | S | W) - 100.0).abs() < 1e-3);
        assert!((area(S | E) - 50.0).abs() < 1e-3);
        assert!((area(N) - 25.0).abs() < 1e-3);
        assert!((area(N | S) - 50.0).abs() < 1e-3);
        assert!((area(N | E | S) - 75.0).abs() < 1e-3);
        assert!(quarter_shapes(1, 1, 0).is_empty());
    }

    #[test]
    fn stamping_is_order_independent() {
        let all = [
            Solid::None,
            Solid::Full,
            Solid::Slope(Corner::NW),
            Solid::Slope(Corner::NE),
            Solid::Slope(Corner::SE),
            Solid::Slope(Corner::SW),
        ];
        for a in all {
            for b in all {
                for c in all {
                    assert_eq!(
                        a.union(b).union(c),
                        c.union(a).union(b),
                        "{a:?} {b:?} {c:?}"
                    );
                    assert_eq!(a.union(b.union(c)), b.union(c).union(a));
                }
            }
        }
    }

    #[test]
    fn boxes_collide_with_slope_solid_half_only() {
        let level = Layer::from_ascii(&["...", ".◸.", "..."]);
        // In the empty half, clear of the face.
        assert!(!level.collides([16.0, 16.0], [3.0, 3.0]));
        // Corner (16,14) pokes past the face u + v = 1, i.e. x + y = 30.
        assert!(level.collides([16.0, 13.0], [3.0, 3.0]));
        // Touching the face at a single point doesn't count.
        assert!(!level.collides([15.0, 15.0], [3.0, 3.0]));
    }
}
