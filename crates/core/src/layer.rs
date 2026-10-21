//! One layer of a level: a tile grid, with rays, collision and its outline.
//!
//! The grid is the whole truth about solid space: rays, collision, outlines,
//! drawing and lighting all derive from it. Its outline is computed once,
//! when the layer is built. Void and stairs tiles are open, like floor;
//! what they mean between layers is up to [`crate::level`].
//!
//! Split by concern: the algebra of a single tile (`tile.rs`), the outline
//! of solid space (`outline.rs`), and moving boxes against it
//! (`collide.rs`); this module holds the grid itself, rays and line of
//! sight. Tile art is drawn by [`crate::art`].

mod collide;
mod outline;
#[cfg(any(test, feature = "test-util"))]
pub mod testing;
mod tile;

pub use outline::{ConcaveCorner, Edge, Line, Outline, Piece};
pub use tile::*;

use std::sync::Arc;

use crate::chunk::{CHUNK, Chunk, Chunks};
use crate::geom::{Rect, bounds, centroid, inward_normal, length, toward};
use crate::prop::{Prop, PropId};

use outline::{Buckets, Lines, corner_at, edges_at};

/// Tolerance for slope solidity (tile-local units), matching `raycast`'s
/// 1e-4 px vertex tolerance.
const SLOPE_EPS: f32 = 1e-5;

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

    /// A layer of default tiles with shapes `solids`, row by row: wall
    /// where solid, floor elsewhere.
    pub fn from_solids(cols: usize, rows: usize, solids: Vec<Solid>) -> Self {
        Self::from_tiles(
            cols,
            rows,
            solids.into_iter().map(Tile::from_solid).collect(),
        )
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

    /// The prop on tile `(x, y)`, if there's one (see [`crate::prop`]).
    pub fn prop(&self, x: i32, y: i32) -> Option<PropId> {
        self.in_bounds([x, y])
            .then(|| self.tiles.prop(x as usize, y as usize))
            .flatten()
    }

    /// Every prop on it.
    pub fn props(&self) -> impl Iterator<Item = Prop> + '_ {
        self.tiles.props()
    }

    /// The same layer with `props` on its tiles instead. Props don't
    /// change its shape (what's in the way of walking or seeing is
    /// worked out from them elsewhere: see [`crate::level`]).
    pub fn with_props(mut self, props: &[Prop]) -> Self {
        self.tiles = self.tiles.with_props(props);
        self
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
                        let e = end.of_tile(tile);
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
        let len = length(d);
        if len < 1e-6 {
            return true;
        }
        self.raycast(a, [d[0] / len, d[1] / len], len) >= len - 1e-3
    }

    /// Points which, if seen, mean part of tile `(x, y)` has been seen: in
    /// its open part (the center, and its corners pulled slightly in), and
    /// along each exposed face of its solid part (straight sides the
    /// neighbor doesn't cover, and a slope's diagonal face).
    pub fn sight_samples(&self, x: i32, y: i32) -> Vec<[f32; 2]> {
        const INSET: f32 = 0.5;
        const ALONG: [f32; 3] = [0.1, 0.5, 0.9];
        let tile = self.solid(x, y);
        let corner = |c: Corner| c.of_tile([x, y]);
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
            let center = centroid(&open);
            out.push(center);
            out.extend(open.iter().map(|p| toward(*p, center, INSET)));
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

    pub fn in_bounds(&self, t: [i32; 2]) -> bool {
        t[0] >= 0 && t[1] >= 0 && t[0] < self.cols as i32 && t[1] < self.rows as i32
    }
}

#[cfg(test)]
#[path = "tests/layer_tests.rs"]
mod tests;
