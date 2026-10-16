//! Tile grid with randomly placed solid blocks, plus AABB-vs-tile collision.
//!
//! The grid answers spatial queries (rays, collision); alongside it the level
//! keeps the same solid space as a list of convex shapes, for drawing and
//! lighting.

use crate::draw::Layer;
use crate::palette;
#[cfg(test)]
use crate::rng::Rng;

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
    fn offset(self) -> [i32; 2] {
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

    fn opposite(self) -> Corner {
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
    fn sides(self) -> [Side; 2] {
        match self {
            Corner::NW => [Side::N, Side::W],
            Corner::NE => [Side::N, Side::E],
            Corner::SW => [Side::S, Side::W],
            Corner::SE => [Side::S, Side::E],
        }
    }
}

impl Side {
    const ALL: [Side; 4] = [Side::N, Side::S, Side::W, Side::E];

    fn opposite(self) -> Side {
        match self {
            Side::N => Side::S,
            Side::S => Side::N,
            Side::W => Side::E,
            Side::E => Side::W,
        }
    }

    /// Offset to the neighboring tile across this side.
    fn offset(self) -> [i32; 2] {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tile {
    Empty,
    Full,
    /// Solid in the right triangle at this corner, with the diagonal face
    /// running between the two neighboring corners.
    Slope(Corner),
}

impl Tile {
    /// Does the solid part reach tile corner `c`, even just at a point?
    pub fn touches(self, c: Corner) -> bool {
        match self {
            Tile::Empty => false,
            Tile::Full => true,
            Tile::Slope(s) => c != s.opposite(),
        }
    }

    /// Does the solid part cover the whole of side `side`?
    pub fn covers(self, side: Side) -> bool {
        match self {
            Tile::Empty => false,
            Tile::Full => true,
            Tile::Slope(s) => s.sides().contains(&side),
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

    /// The solid part of tile `(x, y)` as a convex shape in pixels.
    pub fn shape(self, x: usize, y: usize) -> Option<Shape> {
        let point = |c: Corner| {
            let [dx, dy] = c.offset();
            [(x as i32 + dx) as f32 * TILE, (y as i32 + dy) as f32 * TILE]
        };
        match self {
            Tile::Empty => None,
            Tile::Full => Some(Corner::CLOCKWISE.map(point).to_vec()),
            Tile::Slope(s) => Some(
                Corner::CLOCKWISE
                    .into_iter()
                    .filter(|c| *c != s.opposite())
                    .map(point)
                    .collect(),
            ),
        }
    }
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

/// A solid piece of a level: tiles that together form one convex shape.
pub struct Piece {
    pub tiles: Vec<([i32; 2], Tile)>,
}

impl Piece {
    /// A `w` x `h` block of full tiles at `(x, y)`, with each corner (in
    /// `Corner::CLOCKWISE` order) optionally cut off by a slope. Corners are
    /// only cut on blocks at least 2x2.
    pub fn block(x: i32, y: i32, w: i32, h: i32, cut: [bool; 4]) -> Self {
        let mut tiles = Vec::new();
        for ty in y..y + h {
            for tx in x..x + w {
                tiles.push(([tx, ty], Tile::Full));
            }
        }
        if w >= 2 && h >= 2 {
            for (corner, cut) in Corner::CLOCKWISE.into_iter().zip(cut) {
                if cut {
                    let [dx, dy] = corner.offset();
                    let at = [x + dx * (w - 1), y + dy * (h - 1)];
                    // Keep the half facing into the block.
                    let tile = Tile::Slope(corner.opposite());
                    tiles.iter_mut().find(|(t, _)| *t == at).unwrap().1 = tile;
                }
            }
        }
        Self { tiles }
    }

    /// A 45-degree bar of `len` full tiles starting at `(x, y)` and running
    /// up-right (`rising`) or down-right, with slopes filling the stair steps
    /// on both sides so its long faces are straight diagonals.
    pub fn bar(x: i32, y: i32, len: i32, rising: bool) -> Self {
        let dy = if rising { -1 } else { 1 };
        // Slopes in the steps beside and above/below each diagonal neighbor.
        let (beside, across) = if rising {
            (Corner::NW, Corner::SE)
        } else {
            (Corner::SW, Corner::NE)
        };
        let mut tiles = Vec::new();
        for i in 0..len {
            tiles.push(([x + i, y + dy * i], Tile::Full));
            if i + 1 < len {
                tiles.push(([x + i + 1, y + dy * i], Tile::Slope(beside)));
                tiles.push(([x + i, y + dy * (i + 1)], Tile::Slope(across)));
            }
        }
        Self { tiles }
    }

    /// The piece's outline: the convex hull of its tiles' solid parts.
    pub fn shape(&self) -> Shape {
        let mut points: Vec<[f32; 2]> = self
            .tiles
            .iter()
            .filter_map(|([x, y], tile)| tile.shape(*x as usize, *y as usize))
            .flatten()
            .collect();
        convex_hull(&mut points)
    }
}

/// Convex hull (Andrew's monotone chain), in `Shape` order, without
/// collinear points.
fn convex_hull(points: &mut Vec<[f32; 2]>) -> Shape {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    if points.len() < 3 {
        return points.clone();
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut hull: Vec<[f32; 2]> = Vec::new();
    for pass in [points.clone(), points.iter().rev().copied().collect()] {
        let start = hull.len();
        for p in pass {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

pub struct Level {
    pub cols: usize,
    pub rows: usize,
    tiles: Vec<Tile>,
    /// The solid tiles again, as convex shapes.
    shapes: Vec<Shape>,
}

impl Level {
    /// Generate a random level, keeping the area around `spawn` (in pixels)
    /// clear. See [`crate::generate`].
    pub fn generate(seed: u32, cols: usize, rows: usize, spawn: [f32; 2]) -> Self {
        crate::generate::generate(seed, cols, rows, spawn)
    }

    /// A level made of non-overlapping pieces on open floor.
    pub fn from_pieces(cols: usize, rows: usize, pieces: &[Piece]) -> Self {
        let mut tiles = vec![Tile::Empty; cols * rows];
        for piece in pieces {
            for ([x, y], tile) in &piece.tiles {
                tiles[*y as usize * cols + *x as usize] = *tile;
            }
        }
        Self {
            cols,
            rows,
            tiles,
            shapes: pieces.iter().map(Piece::shape).collect(),
        }
    }

    /// Solid space as convex shapes, for drawing and lighting.
    pub fn solids(&self) -> &[Shape] {
        &self.shapes
    }

    /// A level from explicit tiles, each solid tile its own shape.
    #[cfg(test)]
    fn from_tiles(cols: usize, rows: usize, tiles: Vec<Tile>) -> Self {
        let shapes = (0..tiles.len())
            .filter_map(|i| tiles[i].shape(i % cols, i / cols))
            .collect();
        Self {
            cols,
            rows,
            tiles,
            shapes,
        }
    }

    /// Build a level from rows of `.` (floor), `#` (solid) and `◸ ◹ ◺ ◿`
    /// (slopes solid in that corner), for tests.
    #[cfg(test)]
    pub fn from_ascii(rows: &[&str]) -> Self {
        let cols = rows[0].chars().count();
        let tiles = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| match c {
                '.' => Tile::Empty,
                '#' => Tile::Full,
                '◸' => Tile::Slope(Corner::NW),
                '◹' => Tile::Slope(Corner::NE),
                '◺' => Tile::Slope(Corner::SW),
                '◿' => Tile::Slope(Corner::SE),
                _ => panic!("unknown tile {c:?}"),
            })
            .collect();
        Self::from_tiles(cols, rows.len(), tiles)
    }

    /// Random tiles with plenty of slopes in every arrangement, for
    /// stress-testing queries.
    #[cfg(test)]
    pub fn soup(seed: u32, cols: usize, rows: usize) -> Self {
        let mut rng = Rng::new(seed);
        let tiles = (0..cols * rows)
            .map(|_| match rng.unit() {
                r if r < 0.10 => Tile::Full,
                r if r < 0.30 => Tile::Slope(Corner::CLOCKWISE[rng.range(0, 3)]),
                _ => Tile::Empty,
            })
            .collect();
        Self::from_tiles(cols, rows, tiles)
    }

    /// The tile at `(x, y)`. Out-of-bounds tiles are `Full`, so the level
    /// edge is a wall.
    pub fn tile(&self, x: i32, y: i32) -> Tile {
        if x < 0 || y < 0 || x >= self.cols as i32 || y >= self.rows as i32 {
            return Tile::Full;
        }
        self.tiles[y as usize * self.cols + x as usize]
    }

    /// Is tile `(x, y)` entirely open floor?
    pub fn is_open(&self, x: i32, y: i32) -> bool {
        self.tile(x, y) == Tile::Empty
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
        let tile = |t: [i32; 2]| self.tile(t[0], t[1]);

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
        match self.tile(tile[0], tile[1]) {
            Tile::Empty => None,
            Tile::Full => Some(t0),
            Tile::Slope(c) => {
                let (k, g) = Tile::slope_plane(c);
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
                    // Reaching the face exactly at a tile corner is a vertex
                    // crossing, which `raycast` handles.
                    let t = t0 + (-f).max(0.0) / df;
                    (t < t1 - 1e-4).then_some(t)
                }
            }
        }
    }

    /// Does tile `t` reach grid vertex `vertex` (in tile units)?
    fn touches_vertex(&self, t: [i32; 2], vertex: [i32; 2]) -> bool {
        self.tile(t[0], t[1])
            .touches(Corner::at([vertex[0] - t[0], vertex[1] - t[1]]))
    }

    /// Unobstructed line of sight between two pixel-space points. Symmetric,
    /// and a point lying on a solid tile's face counts as visible.
    pub fn los(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = d[0].hypot(d[1]);
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
    pub fn corners(&self) -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        for gy in 0..=self.rows as i32 {
            for gx in 0..=self.cols as i32 {
                let nw = self.tile(gx - 1, gy - 1);
                let ne = self.tile(gx, gy - 1);
                let sw = self.tile(gx - 1, gy);
                let se = self.tile(gx, gy);

                // An axis edge is outline where exactly one side covers it.
                let mut dirs: Vec<[i32; 2]> = [
                    ([0, -1], nw.covers(Side::E) != ne.covers(Side::W)),
                    ([0, 1], sw.covers(Side::E) != se.covers(Side::W)),
                    ([-1, 0], nw.covers(Side::S) != sw.covers(Side::N)),
                    ([1, 0], ne.covers(Side::S) != se.covers(Side::N)),
                ]
                .into_iter()
                .filter_map(|(d, outline)| outline.then_some(d))
                .collect();
                // A slope's face is outline, running from this vertex across
                // its tile when the vertex is one of the face's ends.
                for (tile, d, at) in [
                    (nw, [-1, -1], Corner::SE),
                    (ne, [1, -1], Corner::SW),
                    (sw, [-1, 1], Corner::NE),
                    (se, [1, 1], Corner::NW),
                ] {
                    if let Tile::Slope(s) = tile
                        && at != s
                        && at != s.opposite()
                    {
                        dirs.push(d);
                    }
                }

                let straight =
                    dirs.len() == 2 && dirs[0][0] == -dirs[1][0] && dirs[0][1] == -dirs[1][1];
                if !dirs.is_empty() && !straight {
                    out.push([gx as f32 * TILE, gy as f32 * TILE]);
                }
            }
        }
        out
    }

    /// Every piece of the outline of solid space, as pixel-space segments:
    /// tile sides with solid on exactly one side, and slope faces.
    pub fn outline_edges(&self) -> Vec<([f32; 2], [f32; 2])> {
        let px = |x: i32, y: i32| [x as f32 * TILE, y as f32 * TILE];
        let mut out = Vec::new();
        for y in 0..=self.rows as i32 {
            for x in 0..=self.cols as i32 {
                let here = self.tile(x, y);
                // Top and left sides of each position (covering the bottom
                // and right of the level via the extra row and column).
                if here.covers(Side::N) != self.tile(x, y - 1).covers(Side::S) {
                    out.push((px(x, y), px(x + 1, y)));
                }
                if here.covers(Side::W) != self.tile(x - 1, y).covers(Side::E) {
                    out.push((px(x, y), px(x, y + 1)));
                }
                if let Tile::Slope(c) = here
                    && x < self.cols as i32
                    && y < self.rows as i32
                {
                    // The face joins the two corners beside the solid one.
                    let [c1, c2] = c.face_ends().map(|k| {
                        let [dx, dy] = k.offset();
                        px(x + dx, y + dy)
                    });
                    out.push((c1, c2));
                }
            }
        }
        out
    }

    /// Points which, if seen, mean part of tile `(x, y)` has been seen: in
    /// its open part (the center, and its corners pulled slightly in), and
    /// along each exposed face of its solid part (straight sides the
    /// neighbor doesn't cover, and a slope's diagonal face).
    pub fn sight_samples(&self, x: i32, y: i32) -> Vec<[f32; 2]> {
        const INSET: f32 = 0.5;
        const ALONG: [f32; 3] = [0.1, 0.5, 0.9];
        let tile = self.tile(x, y);
        let corner = |c: Corner| {
            let [dx, dy] = c.offset();
            [(x + dx) as f32 * TILE, (y + dy) as f32 * TILE]
        };
        let mut out = Vec::new();

        let open: Vec<[f32; 2]> = match tile {
            Tile::Empty => Corner::CLOCKWISE.map(corner).to_vec(),
            Tile::Full => Vec::new(),
            Tile::Slope(c) => Corner::CLOCKWISE
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
                let len = d[0].hypot(d[1]);
                [p[0] + d[0] / len * INSET, p[1] + d[1] / len * INSET]
            }));
        }

        let mut faces: Vec<[Corner; 2]> = Side::ALL
            .into_iter()
            .filter(|side| {
                let [dx, dy] = side.offset();
                tile.covers(*side) && !self.tile(x + dx, y + dy).covers(side.opposite())
            })
            .map(Side::ends)
            .collect();
        if let Tile::Slope(c) = tile {
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
        match self.tile(tile[0], tile[1]) {
            Tile::Empty => false,
            Tile::Full => true,
            Tile::Slope(c) => {
                // The box clipped to the tile, in tile-local units; solidity
                // is linear, so its maximum is at one of the box's corners.
                let local = |p: f32, a: usize| (p / TILE - tile[a] as f32).clamp(0.0, 1.0);
                let (u0, u1) = (local(pos[0], 0), local(pos[0] + size[0], 0));
                let (v0, v1) = (local(pos[1], 1), local(pos[1] + size[1], 1));
                let (k, g) = Tile::slope_plane(c);
                let u = if g[0] > 0.0 { u1 } else { u0 };
                let v = if g[1] > 0.0 { v1 } else { v0 };
                k + g[0] * u + g[1] * v > COLLIDE_EPS / TILE
            }
        }
    }

    /// Move a box by `delta`, sliding along anything it runs into. Moves in
    /// substeps short enough not to skip through a wall, pushing the box
    /// back out of solid space after each.
    pub fn move_box(&self, mut pos: [f32; 2], size: [f32; 2], delta: [f32; 2]) -> [f32; 2] {
        let steps = (delta[0].hypot(delta[1]) / MAX_STEP).ceil().max(1.0);
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
        let tile = self.tile(t[0], t[1]);
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
        let exposed = |dx: i32, dy: i32, side: Side| !self.tile(t[0] + dx, t[1] + dy).covers(side);
        let mut pushes = vec![
            ([1.0, 0.0], exposed(1, 0, Side::W)),
            ([-1.0, 0.0], exposed(-1, 0, Side::E)),
            ([0.0, 1.0], exposed(0, 1, Side::N)),
            ([0.0, -1.0], exposed(0, -1, Side::S)),
        ];
        if let Tile::Slope(c) = tile {
            // Away from the solid corner; the face always borders open space.
            let (_, g) = Tile::slope_plane(c);
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

    /// Draw the floor, its grid, and every solid shape with a bevel.
    pub fn draw(&self, layer: &mut Layer) {
        let w = self.cols as f32 * TILE;
        let h = self.rows as f32 * TILE;
        layer.rect([0.0, 0.0], [w, h], palette::BLACK);
        for x in 0..self.cols {
            layer.rect([x as f32 * TILE, 0.0], [1.0, h], palette::BLUE);
        }
        for y in 0..self.rows {
            layer.rect([0.0, y as f32 * TILE], [w, 1.0], palette::BLUE);
        }
        for shape in &self.shapes {
            layer.beveled(shape, palette::BROWN, palette::YELLOW, palette::RED);
        }
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
        .min_by(|x, y| x[0].hypot(x[1]).total_cmp(&y[0].hypot(y[1])))
}

#[cfg(test)]
mod tests {
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
        let level = Level::from_ascii(&[
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
        let level = Level::from_ascii(&[
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
            let level = Level::from_ascii(&rows);
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
        let level = Level::from_ascii(&[
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
            let level = Level::from_ascii(&rows);
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
            let level = Level::soup(seed, 32, 20);
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

    fn level_with(x: i32, y: i32, w: i32, h: i32) -> Level {
        Level::from_pieces(10, 10, &[Piece::block(x, y, w, h, [false; 4])])
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
    fn sees(level: &Level, a: [f32; 2], b: [f32; 2]) -> bool {
        level.los(a, b)
    }

    #[test]
    fn rays_hit_slope_sides_and_face() {
        // Slope solid in the NW half of tile (1,1): x 10..20, y 10..20.
        let level = Level::from_ascii(&["...", ".◸.", "..."]);
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
        let level = Level::from_ascii(&["...", ".◸.", "..."]);
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
        let pinch = Level::from_ascii(&["◿.", ".◸"]);
        assert!(!sees(&pinch, [15.0, 5.0], [5.0, 15.0]));
        // Only the first reaches it: the ray grazes past.
        let graze = Level::from_ascii(&["◿.", ".◿"]);
        assert!(sees(&graze, [15.0, 5.0], [5.0, 15.0]));
    }

    #[test]
    fn rays_graze_along_straight_faces() {
        // Along y = 10, the top face of the block, with open floor above.
        let level = Level::from_ascii(&["....", ".##.", "...."]);
        assert!(sees(&level, [0.0, 10.0], [40.0, 10.0]));
        assert!(sees(&level, [40.0, 10.0], [0.0, 10.0]));
        // Along x = 20, through the middle of the block: blocked.
        assert!(!sees(&level, [20.0, 0.0], [20.0, 30.0]));
        // Along y = 10 between diagonally touching blocks: pinched.
        let pinch = Level::from_ascii(&["#...", ".#..", "...."]);
        assert!(!sees(&pinch, [0.0, 10.0], [40.0, 10.0]));
    }

    #[test]
    fn rays_start_inside_slope_tiles() {
        let level = Level::from_ascii(&["◸.."]);
        // Empty half of the slope tile, looking right: open to the far edge.
        let open = level.raycast([8.0, 8.0], [1.0, 0.0], f32::INFINITY);
        assert!((open - 22.0).abs() < 1e-4, "{open}");
        // Solid half: blocked immediately.
        assert_eq!(level.raycast([2.0, 2.0], [1.0, 0.0], f32::INFINITY), 0.0);
    }

    #[test]
    fn corners_follow_diagonal_outline() {
        // Solid below the diagonal from (30,0) to (0,30).
        let level = Level::from_ascii(&["..◿", ".◿#", "◿##"]);
        let mut corners = level.corners();
        corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(corners, vec![[0.0, 0.0], [0.0, 30.0], [30.0, 0.0]]);

        // A lone slope: its three corners.
        let lone = Level::from_ascii(&["...", ".◹.", "..."]);
        let c = lone.corners();
        for v in [[10.0, 10.0], [20.0, 10.0], [20.0, 20.0]] {
            assert!(c.contains(&v), "{v:?} in {c:?}");
        }
        assert!(!c.contains(&[10.0, 20.0]));
    }

    #[test]
    fn outline_edges_cover_sides_and_faces() {
        let level = Level::from_ascii(&["◸"]);
        let edges = level.outline_edges();
        // The face from (10,0) to (0,10), plus the empty half's two sides
        // against the out-of-bounds wall: right and bottom.
        assert!(edges.contains(&([10.0, 0.0], [0.0, 10.0])));
        assert!(edges.contains(&([10.0, 0.0], [10.0, 10.0])));
        assert!(edges.contains(&([0.0, 10.0], [10.0, 10.0])));
        assert_eq!(edges.len(), 3, "{edges:?}");
    }

    #[test]
    fn boxes_collide_with_slope_solid_half_only() {
        let level = Level::from_ascii(&["...", ".◸.", "..."]);
        // In the empty half, clear of the face.
        assert!(!level.collides([16.0, 16.0], [3.0, 3.0]));
        // Corner (16,14) pokes past the face u + v = 1, i.e. x + y = 30.
        assert!(level.collides([16.0, 13.0], [3.0, 3.0]));
        // Touching the face at a single point doesn't count.
        assert!(!level.collides([15.0, 15.0], [3.0, 3.0]));
    }

    #[test]
    fn generated_blocks_avoid_spawn() {
        for seed in 0..200 {
            let spawn = [161.0, 101.0];
            let level = Level::generate(seed, 32, 20, spawn);
            assert!(!level.collides(spawn, SIZE), "seed {seed}");
            assert!(!level.solids().is_empty());
        }
    }
}
