//! A simple random level generator: rectangular blocks, some with their
//! corners cut at 45 degrees, plus a few diagonal bars.
//!
//! Every piece keeps a one-tile gap from every other piece and from the
//! level edge. Pieces are convex, so that's enough to keep all the open
//! floor connected; slopes only ever widen gaps.

use crate::level::{Corner, Level, Piece, TILE, Tile};
use crate::rng::Rng;

const BLOCK_COUNT: usize = 27;
const BLOCK_MAX_W: usize = 5;
const BLOCK_MAX_H: usize = 4;
/// Chance of cutting each corner of a block at least 2x2.
const CUT_CHANCE: f32 = 0.4;
/// Chance of a 1x1 block being a single slope instead.
const LONE_SLOPE_CHANCE: f32 = 0.5;

const BAR_COUNT: usize = 2;
const BAR_MIN_LEN: usize = 3;
const BAR_MAX_LEN: usize = 6;

/// Pieces may not come within this many tiles (Chebyshev) of the spawn.
const SPAWN_CLEARANCE: i32 = 2;

pub fn generate(seed: u32, cols: usize, rows: usize, spawn: [f32; 2]) -> Level {
    let mut rng = Rng::new(seed);
    let spawn_tile = [(spawn[0] / TILE) as i32, (spawn[1] / TILE) as i32];
    let mut occupied = vec![false; cols * rows];
    let mut pieces = Vec::new();

    // Bars first, since they're the hardest to fit.
    for (count, make) in [
        (BAR_COUNT, random_bar as fn(&mut Rng, usize, usize) -> Piece),
        (BLOCK_COUNT, random_block),
    ] {
        let mut placed = 0;
        for _ in 0..count * 50 {
            if placed == count {
                break;
            }
            let piece = make(&mut rng, cols, rows);
            if fits(&piece, &occupied, cols, rows, spawn_tile) {
                for ([x, y], _) in &piece.tiles {
                    occupied[*y as usize * cols + *x as usize] = true;
                }
                pieces.push(piece);
                placed += 1;
            }
        }
    }
    Level::from_pieces(cols, rows, &pieces)
}

fn random_block(rng: &mut Rng, cols: usize, rows: usize) -> Piece {
    let w = rng.range(1, BLOCK_MAX_W);
    let h = rng.range(1, BLOCK_MAX_H);
    let x = rng.range(1, cols - w - 1) as i32;
    let y = rng.range(1, rows - h - 1) as i32;
    if w == 1 && h == 1 && rng.unit() < LONE_SLOPE_CHANCE {
        let corner = Corner::CLOCKWISE[rng.range(0, 3)];
        return Piece {
            tiles: vec![([x, y], Tile::Slope(corner))],
        };
    }
    let cut = [(); 4].map(|_| rng.unit() < CUT_CHANCE);
    Piece::block(x, y, w as i32, h as i32, cut)
}

fn random_bar(rng: &mut Rng, cols: usize, rows: usize) -> Piece {
    let len = rng.range(BAR_MIN_LEN, BAR_MAX_LEN);
    let rising = rng.unit() < 0.5;
    // The bar spans `len` tiles each way; place that square inside the
    // one-tile margin.
    let x = rng.range(1, cols - len - 1) as i32;
    let top = rng.range(1, rows - len - 1) as i32;
    let y = if rising { top + len as i32 - 1 } else { top };
    Piece::bar(x, y, len as i32, rising)
}

/// Inside the margin, clear of the spawn, and not touching (even
/// diagonally) any occupied tile.
fn fits(piece: &Piece, occupied: &[bool], cols: usize, rows: usize, spawn: [i32; 2]) -> bool {
    let (cols, rows) = (cols as i32, rows as i32);
    piece.tiles.iter().all(|([x, y], _)| {
        let in_margin = *x >= 1 && *y >= 1 && *x < cols - 1 && *y < rows - 1;
        let near_spawn = (x - spawn[0]).abs().max((y - spawn[1]).abs()) <= SPAWN_CLEARANCE;
        let crowded = (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let (nx, ny) = (x + dx, y + dy);
                nx >= 0 && ny >= 0 && nx < cols && ny < rows && occupied[(ny * cols + nx) as usize]
            })
        });
        in_margin && !near_spawn && !crowded
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAWN: [f32; 2] = [165.0, 105.0];

    #[test]
    fn shapes_match_tiles() {
        // A point well inside a piece's shape is solid; a point well
        // outside every shape is open.
        let mut rng = Rng::new(1);
        for seed in 0..100 {
            let level = generate(seed, 32, 20, SPAWN);
            for _ in 0..2000 {
                // Clear of the level edge, which is solid but not a piece.
                let p = [0.5 + rng.unit() * 319.0, 0.5 + rng.unit() * 199.0];
                let depth = level
                    .solids()
                    .iter()
                    .map(|s| inside_depth(s, p))
                    .fold(f32::NEG_INFINITY, f32::max);
                if depth.abs() < 0.01 {
                    continue; // On an outline.
                }
                let solid = level.collides([p[0] - 0.005, p[1] - 0.005], [0.01, 0.01]);
                assert_eq!(solid, depth > 0.0, "seed {seed} at {p:?}");
            }
        }
    }

    /// Signed distance inside a convex shape (negative outside).
    fn inside_depth(shape: &[[f32; 2]], p: [f32; 2]) -> f32 {
        (0..shape.len())
            .map(|i| {
                let (a, b) = (shape[i], shape[(i + 1) % shape.len()]);
                let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                // Shape order is clockwise on screen, so the inside is to
                // the right of each edge in y-down coordinates.
                ((p[1] - a[1]) * ex - (p[0] - a[0]) * ey) / ex.hypot(ey)
            })
            .fold(f32::INFINITY, f32::min)
    }

    #[test]
    fn open_floor_is_connected() {
        for seed in 0..200 {
            let level = generate(seed, 32, 20, SPAWN);
            let open = |x: i32, y: i32| level.tile(x, y) == Tile::Empty;
            let start = [(SPAWN[0] / TILE) as i32, (SPAWN[1] / TILE) as i32];
            let mut seen = vec![false; 32 * 20];
            let mut stack = vec![start];
            while let Some([x, y]) = stack.pop() {
                if !open(x, y) || seen[(y * 32 + x) as usize] {
                    continue;
                }
                seen[(y * 32 + x) as usize] = true;
                stack.extend([[x + 1, y], [x - 1, y], [x, y + 1], [x, y - 1]]);
            }
            for y in 0..20 {
                for x in 0..32 {
                    assert!(
                        !open(x, y) || seen[(y * 32 + x) as usize],
                        "seed {seed}: ({x},{y}) cut off"
                    );
                }
            }
        }
    }

    #[test]
    fn levels_have_variety_and_clear_spawn() {
        let (mut slopes, mut bars) = (0, 0);
        for seed in 0..100 {
            let level = generate(seed, 32, 20, SPAWN);
            assert!(!level.collides([SPAWN[0] - 4.0, SPAWN[1] - 4.0], [8.0, 8.0]));
            for y in 0..20 {
                for x in 0..32 {
                    if matches!(level.tile(x, y), Tile::Slope(_)) {
                        slopes += 1;
                    }
                }
            }
            // A bar's shape has a 45-degree edge longer than one tile.
            bars += level
                .solids()
                .iter()
                .filter(|s| {
                    (0..s.len()).any(|i| {
                        let (a, b) = (s[i], s[(i + 1) % s.len()]);
                        let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
                        dx > TILE * 1.5 && (dx - dy).abs() < 1e-3
                    })
                })
                .count();
        }
        assert!(slopes > 500, "{slopes} slope tiles in 100 levels");
        assert!(bars > 100, "{bars} bars in 100 levels");
    }
}
