//! Tile grid with randomly placed solid blocks, plus AABB-vs-tile collision.

use crate::palette;
use crate::rng::Rng;
use crate::sprite::Sprite;

/// Tile edge length in pixels.
pub const TILE: f32 = 10.0;

const BLOCK_COUNT: usize = 7;
const BLOCK_MIN: usize = 1;
const BLOCK_MAX_W: usize = 5;
const BLOCK_MAX_H: usize = 4;

/// An inclusive-exclusive rectangle of tiles.
#[derive(Clone, Copy)]
struct TileRect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl TileRect {
    /// True if the rects overlap after growing `self` by `margin` tiles.
    fn overlaps(&self, other: &TileRect, margin: usize) -> bool {
        self.x < other.x + other.w + margin
            && other.x < self.x + self.w + margin
            && self.y < other.y + other.h + margin
            && other.y < self.y + self.h + margin
    }
}

pub struct Level {
    pub cols: usize,
    pub rows: usize,
    solid: Vec<bool>,
    blocks: Vec<TileRect>,
}

impl Level {
    /// Generate a level, keeping a tile region around `spawn` (in pixels)
    /// free of blocks.
    pub fn generate(seed: u32, cols: usize, rows: usize, spawn: [f32; 2]) -> Self {
        let mut rng = Rng::new(seed);
        let spawn_tile = TileRect {
            x: (spawn[0] / TILE) as usize,
            y: (spawn[1] / TILE) as usize,
            w: 1,
            h: 1,
        };

        // Rejection-sample blocks, keeping a 1-tile gap from each other and
        // from the edge so the floor stays fully connected.
        let mut blocks: Vec<TileRect> = Vec::new();
        for _ in 0..BLOCK_COUNT * 50 {
            if blocks.len() == BLOCK_COUNT {
                break;
            }
            let w = rng.range(BLOCK_MIN, BLOCK_MAX_W);
            let h = rng.range(BLOCK_MIN, BLOCK_MAX_H);
            let candidate = TileRect {
                x: rng.range(1, cols - w - 1),
                y: rng.range(1, rows - h - 1),
                w,
                h,
            };
            if !candidate.overlaps(&spawn_tile, 2)
                && blocks.iter().all(|b| !candidate.overlaps(b, 1))
            {
                blocks.push(candidate);
            }
        }

        let mut solid = vec![false; cols * rows];
        for b in &blocks {
            for y in b.y..b.y + b.h {
                for x in b.x..b.x + b.w {
                    solid[y * cols + x] = true;
                }
            }
        }
        Self {
            cols,
            rows,
            solid,
            blocks,
        }
    }

    /// Pixel-space `(pos, size)` of each solid block.
    pub fn blocks(&self) -> impl Iterator<Item = ([f32; 2], [f32; 2])> + '_ {
        self.blocks.iter().map(|b| {
            (
                [b.x as f32 * TILE, b.y as f32 * TILE],
                [b.w as f32 * TILE, b.h as f32 * TILE],
            )
        })
    }

    /// Build a level from rows of `.` (floor) and `#` (solid), for tests.
    /// Each solid tile becomes its own block.
    #[cfg(test)]
    pub fn from_ascii(rows: &[&str]) -> Self {
        let cols = rows[0].len();
        let solid: Vec<bool> = rows
            .iter()
            .flat_map(|r| r.chars().map(|c| c == '#'))
            .collect();
        let blocks = (0..solid.len())
            .filter(|&i| solid[i])
            .map(|i| TileRect {
                x: i % cols,
                y: i / cols,
                w: 1,
                h: 1,
            })
            .collect();
        Self {
            cols,
            rows: rows.len(),
            solid,
            blocks,
        }
    }

    /// Out-of-bounds tiles count as solid, so the level edge is a wall.
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.cols as i32 || y >= self.rows as i32 {
            return true;
        }
        self.solid[y as usize * self.cols + x as usize]
    }

    /// Distance from `from` along unit vector `dir` to the first solid tile,
    /// capped at `max` (which may be infinite; the level edge always stops
    /// the ray). Walks the grid exactly (Amanatides & Woo). A ray passing
    /// exactly through a grid vertex is blocked only if both tiles beside
    /// that vertex are solid, so light can't leak through a diagonal pinch
    /// but can graze a single corner.
    pub fn raycast(&self, from: [f32; 2], dir: [f32; 2], max: f32) -> f32 {
        let mut tile = [
            (from[0] / TILE).floor() as i32,
            (from[1] / TILE).floor() as i32,
        ];
        if self.is_solid(tile[0], tile[1]) {
            return 0.0;
        }

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

        loop {
            let t_max = [next_line(tile, 0), next_line(tile, 1)];
            let t = t_max[0].min(t_max[1]);
            if t >= max {
                return max;
            }
            if (t_max[0] - t_max[1]).abs() <= 1e-4 {
                // Exactly through a vertex: step diagonally.
                if self.is_solid(tile[0] + step[0], tile[1])
                    && self.is_solid(tile[0], tile[1] + step[1])
                {
                    return t;
                }
                tile = [tile[0] + step[0], tile[1] + step[1]];
            } else {
                let a = if t_max[0] < t_max[1] { 0 } else { 1 };
                tile[a] += step[a];
            }
            if self.is_solid(tile[0], tile[1]) {
                return t;
            }
        }
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
    /// the only places a visibility boundary can change direction.
    pub fn corners(&self) -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        for gy in 0..=self.rows as i32 {
            for gx in 0..=self.cols as i32 {
                let nw = self.is_solid(gx - 1, gy - 1);
                let ne = self.is_solid(gx, gy - 1);
                let sw = self.is_solid(gx - 1, gy);
                let se = self.is_solid(gx, gy);
                let count = [nw, ne, sw, se].iter().filter(|s| **s).count();
                // 1 or 3 solid: convex/concave corner. 2 diagonal: pinch.
                // 2 adjacent (straight edge), 0 or 4 (uniform): not a corner.
                if count == 1 || count == 3 || (count == 2 && nw == se) {
                    out.push([gx as f32 * TILE, gy as f32 * TILE]);
                }
            }
        }
        out
    }

    /// Does the pixel-space box `pos..pos+size` overlap any solid tile?
    pub fn collides(&self, pos: [f32; 2], size: [f32; 2]) -> bool {
        let x0 = (pos[0] / TILE).floor() as i32;
        let y0 = (pos[1] / TILE).floor() as i32;
        let x1 = ((pos[0] + size[0]) / TILE).ceil() as i32;
        let y1 = ((pos[1] + size[1]) / TILE).ceil() as i32;
        (y0..y1).any(|y| (x0..x1).any(|x| self.is_solid(x, y)))
    }

    /// Move a box by `delta`, one axis at a time so it slides along walls.
    /// On contact the box is snapped flush to the tile edge it hit.
    pub fn move_box(&self, mut pos: [f32; 2], size: [f32; 2], delta: [f32; 2]) -> [f32; 2] {
        for axis in 0..2 {
            let d = delta[axis];
            if d == 0.0 {
                continue;
            }
            let mut next = pos;
            next[axis] += d;
            if self.collides(next, size) {
                next[axis] = if d > 0.0 {
                    ((next[axis] + size[axis]) / TILE).floor() * TILE - size[axis]
                } else {
                    ((next[axis] / TILE).floor() + 1.0) * TILE
                };
                // A move bigger than a tile could snap into a different
                // wall; fall back to not moving on this axis.
                if self.collides(next, size) {
                    next[axis] = pos[axis];
                }
            }
            pos = next;
        }
        pos
    }

    pub fn sprites(&self, out: &mut Vec<Sprite>) {
        let w = self.cols as f32 * TILE;
        let h = self.rows as f32 * TILE;
        out.push(Sprite::rect([0.0, 0.0], [w, h], palette::BLACK));
        for x in 0..self.cols {
            out.push(Sprite::rect(
                [x as f32 * TILE, 0.0],
                [1.0, h],
                palette::BLUE,
            ));
        }
        for y in 0..self.rows {
            out.push(Sprite::rect(
                [0.0, y as f32 * TILE],
                [w, 1.0],
                palette::BLUE,
            ));
        }

        for b in &self.blocks {
            let pos = [b.x as f32 * TILE, b.y as f32 * TILE];
            let size = [b.w as f32 * TILE, b.h as f32 * TILE];
            out.push(Sprite::rect(pos, size, palette::BROWN));
            // Bevel: light top/left edge, dark bottom/right edge.
            out.push(Sprite::rect(pos, [size[0], 1.0], palette::YELLOW));
            out.push(Sprite::rect(pos, [1.0, size[1]], palette::YELLOW));
            out.push(Sprite::rect(
                [pos[0], pos[1] + size[1] - 1.0],
                [size[0], 1.0],
                palette::RED,
            ));
            out.push(Sprite::rect(
                [pos[0] + size[0] - 1.0, pos[1]],
                [1.0, size[1]],
                palette::RED,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: [f32; 2] = [8.0, 8.0];

    fn level_with(block: TileRect) -> Level {
        let (cols, rows) = (10, 10);
        let mut solid = vec![false; cols * rows];
        for y in block.y..block.y + block.h {
            for x in block.x..block.x + block.w {
                solid[y * cols + x] = true;
            }
        }
        Level {
            cols,
            rows,
            solid,
            blocks: vec![block],
        }
    }

    #[test]
    fn stops_flush_against_block() {
        let level = level_with(TileRect {
            x: 5,
            y: 0,
            w: 1,
            h: 10,
        });
        let pos = level.move_box([31.0, 1.0], SIZE, [100.0, 0.0]);
        // Fell back to not moving since the snap from a 100px move lands
        // past the wall; small steps reach flush contact.
        assert_eq!(pos, [31.0, 1.0]);
        let mut pos = [31.0, 1.0];
        for _ in 0..20 {
            pos = level.move_box(pos, SIZE, [1.5, 0.0]);
        }
        assert_eq!(pos, [42.0, 1.0]);
        assert!(!level.collides(pos, SIZE));
    }

    #[test]
    fn slides_along_wall() {
        let level = level_with(TileRect {
            x: 5,
            y: 0,
            w: 1,
            h: 10,
        });
        let pos = level.move_box([42.0, 10.0], SIZE, [1.0, 1.0]);
        assert_eq!(pos, [42.0, 11.0]);
    }

    #[test]
    fn edge_is_solid() {
        let level = level_with(TileRect {
            x: 9,
            y: 9,
            w: 1,
            h: 1,
        });
        assert_eq!(level.move_box([1.0, 1.0], SIZE, [-3.0, -3.0]), [0.0, 0.0]);
    }

    #[test]
    fn generated_blocks_avoid_spawn() {
        for seed in 0..200 {
            let spawn = [161.0, 101.0];
            let level = Level::generate(seed, 32, 20, spawn);
            assert!(!level.collides(spawn, SIZE), "seed {seed}");
            assert!(!level.blocks.is_empty());
        }
    }
}
