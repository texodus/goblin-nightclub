//! Random level generators, writing tiles:
//!
//! - [`Style::World`]: open floor with solid pieces stamped onto it,
//!   rectangular blocks (some with corners cut at 45 degrees) and diagonal
//!   bars, which may merge into composite shapes.
//! - [`Style::Cave`]: solid rock carved out by cellular automata, with
//!   stair-steps smoothed into slopes.
//! - [`Style::Layered`]: [`LAYERS`] layers, a cave below decks of floor
//!   with void openings and stairs down.
//!
//! Either way the area around the spawn is open, the level edge is solid
//! or open floor, and any open space not connected to the spawn is filled
//! in.

use crate::layer::{Corner, Dir, Layer, Side, TILE, Tile};
use crate::level::Level;
use crate::rng::Rng;

/// A solid piece to stamp into a level: tiles that together form one
/// convex shape.
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

    /// Union this piece into `tiles` (a `cols`-wide grid).
    fn stamp(&self, tiles: &mut [Tile], cols: usize) {
        for ([x, y], tile) in &self.tiles {
            let i = *y as usize * cols + *x as usize;
            tiles[i] = tiles[i].union(*tile);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    World,
    Cave,
    /// Only for whole levels; as a single layer, the same as `Cave`.
    Layered,
}

/// Layers in a layered level: a cave, and decks above it.
pub const LAYERS: usize = 5;
const _: () = assert!(LAYERS <= crate::level::MAX_LAYERS);

/// Void openings in each deck of a layered level.
const HOLE_COUNT: usize = 16;
const HOLE_MAX_W: usize = 8;
const HOLE_MAX_H: usize = 6;
/// Blocks on each deck.
const DECK_BLOCKS: usize = 8;
/// Staircases between each layer and the one below.
const STAIRS: usize = 2;

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

/// Chance of letting a piece touch or overlap another, merging into a
/// composite, rather than rejecting it.
const MERGE_CHANCE: f32 = 0.5;

/// Initial chance of each cave tile being rock.
const CAVE_FILL: f32 = 0.45;
/// Cellular automaton steps smoothing the cave.
const CAVE_STEPS: usize = 4;
/// Retry until at least this share of the level is reachable floor.
const CAVE_MIN_OPEN: f32 = 0.35;

/// Pieces may not come within this many tiles (Chebyshev) of the spawn.
const SPAWN_CLEARANCE: i32 = 2;

pub fn generate(style: Style, seed: u32, cols: usize, rows: usize, spawn: [f32; 2]) -> Layer {
    let spawn = [(spawn[0] / TILE) as i32, (spawn[1] / TILE) as i32];
    let tiles = match style {
        Style::World => world(seed, cols, rows, spawn),
        Style::Cave | Style::Layered => cave(seed, cols, rows, spawn),
    };
    Layer::from_tiles(cols, rows, tiles)
}

/// A whole level in `style`. World and Cave levels are a single layer.
pub fn generate_level(style: Style, seed: u32, cols: usize, rows: usize, spawn: [f32; 2]) -> Level {
    match style {
        Style::Layered => {
            let spawn = [(spawn[0] / TILE) as i32, (spawn[1] / TILE) as i32];
            layered(seed, cols, rows, spawn)
        }
        _ => Level::new(vec![generate(style, seed, cols, rows, spawn)]),
    }
}

/// A cave at the bottom, and above it [`LAYERS`]` - 1` decks: floor with
/// void openings (walkable where they open onto a wall below, pits where
/// onto anything else), some blocks, and stairs down to the layer below.
/// The player starts on the top deck.
fn layered(seed: u32, cols: usize, rows: usize, spawn: [i32; 2]) -> Level {
    let mut rng = Rng::new(seed ^ 0x5EED_1A7E);
    let index = |x: i32, y: i32| y as usize * cols + x as usize;
    let near_spawn = |x: i32, y: i32, margin: i32| {
        (x - spawn[0]).abs().max((y - spawn[1]).abs()) <= SPAWN_CLEARANCE + margin
    };

    let mut layers = vec![cave(seed, cols, rows, spawn)];
    for _ in 1..LAYERS {
        let mut deck = vec![Tile::Empty; cols * rows];
        for _ in 0..HOLE_COUNT {
            let (w, h) = (
                rng.range(3, HOLE_MAX_W) as i32,
                rng.range(2, HOLE_MAX_H) as i32,
            );
            let x0 = rng.range(1, cols - w as usize - 1) as i32;
            let y0 = rng.range(1, rows - h as usize - 1) as i32;
            for y in y0..y0 + h {
                for x in x0..x0 + w {
                    if !near_spawn(x, y, 1) {
                        deck[index(x, y)] = Tile::Void;
                    }
                }
            }
        }
        for _ in 0..DECK_BLOCKS {
            let piece = random_block(&mut rng, cols, rows);
            if piece.tiles.iter().all(|([x, y], _)| !near_spawn(*x, *y, 0)) {
                piece.stamp(&mut deck, cols);
            }
        }
        layers.push(deck);
    }

    // Stairs between each layer and the one below, where both have floor
    // all around, so arriving at either end never lands the player in a
    // wall, and where the player can get to them: placed from the top down,
    // so each layer's reachable floor includes the stairs arriving from
    // above. If there's no such spot, clear one around reachable floor:
    // every layer needs a way down, or everything below it is cut off.
    let clear = |tiles: &[Tile], x: i32, y: i32| {
        (-1..=1).all(|dy| (-1..=1).all(|dx| tiles[index(x + dx, y + dy)] == Tile::Empty))
    };
    let interior: Vec<[i32; 2]> = (2..rows as i32 - 2)
        .flat_map(|y| (2..cols as i32 - 2).map(move |x| [x, y]))
        .collect();
    for upper in (1..LAYERS).rev() {
        let level = Level::new(
            layers
                .iter()
                .map(|t| Layer::from_tiles(cols, rows, t.clone()))
                .collect(),
        );
        let reached = reachable_across(&level, LAYERS - 1, spawn);
        let reachable = |[x, y]: [i32; 2]| reached[upper][index(x, y)];
        let mut candidates: Vec<[i32; 2]> = interior
            .iter()
            .copied()
            .filter(|&[x, y]| {
                reachable([x, y])
                    && !near_spawn(x, y, 1)
                    && clear(&layers[upper], x, y)
                    && clear(&layers[upper - 1], x, y)
            })
            .collect();
        if candidates.is_empty() {
            // Plain floor only: not the stairs arriving from above.
            let floor = |t: [i32; 2]| {
                reachable(t)
                    && layers[upper][index(t[0], t[1])] == Tile::Empty
                    && !matches!(layers[upper - 1][index(t[0], t[1])], Tile::Stairs(_))
            };
            let away: Vec<[i32; 2]> = interior
                .iter()
                .copied()
                .filter(|&t| floor(t) && !near_spawn(t[0], t[1], 1))
                .collect();
            let near: Vec<[i32; 2]> = interior
                .iter()
                .copied()
                .filter(|&t| floor(t) && t != spawn)
                .collect();
            let pool = if away.is_empty() { near } else { away };
            let Some(&at) = pool.get(rng.range(0, pool.len().max(1) - 1)) else {
                continue;
            };
            for k in [upper - 1, upper] {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        // Leave stairs placed already.
                        let tile = &mut layers[k][index(at[0] + dx, at[1] + dy)];
                        if !matches!(tile, Tile::Stairs(_)) {
                            *tile = Tile::Empty;
                        }
                    }
                }
            }
            candidates.push(at);
        }
        for _ in 0..STAIRS {
            if candidates.is_empty() {
                break;
            }
            let [x, y] = candidates.swap_remove(rng.range(0, candidates.len() - 1));
            layers[upper][index(x, y)] = Tile::Stairs(Dir::Down);
            layers[upper - 1][index(x, y)] = Tile::Stairs(Dir::Up);
            // Keep staircases apart.
            candidates.retain(|c| (c[0] - x).abs().max((c[1] - y).abs()) > 4);
        }
    }

    // Fill in whatever the player can't reach. Filling a layer's floor can
    // turn void above it into a new wall top, so repeat until settled.
    for _ in 0..16 {
        let level = Level::new(
            layers
                .iter()
                .map(|t| Layer::from_tiles(cols, rows, t.clone()))
                .collect(),
        );
        let reached = reachable_across(&level, layers.len() - 1, spawn);
        let mut changed = false;
        for (k, tiles) in layers.iter_mut().enumerate() {
            let movement = level.movement(k);
            for (i, tile) in tiles.iter_mut().enumerate() {
                let open = movement.tile((i % cols) as i32, (i / cols) as i32) != Tile::Full;
                if open && !reached[k][i] {
                    *tile = Tile::Full;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    Level::new(
        layers
            .into_iter()
            .map(|t| Layer::from_tiles(cols, rows, t))
            .collect(),
    )
}

/// Which tiles of each layer the player can walk to from `start` on layer
/// `top`, across each layer's movement grid and up and down stairs.
pub fn reachable_across(level: &Level, top: usize, start: [i32; 2]) -> Vec<Vec<bool>> {
    let (cols, rows) = (level.cols(), level.rows());
    let grids: Vec<Vec<Tile>> = (0..level.depth())
        .map(|k| {
            let m = level.movement(k);
            (0..cols * rows)
                .map(|i| m.tile((i % cols) as i32, (i / cols) as i32))
                .collect()
        })
        .collect();
    let mut seeds: Vec<Vec<[i32; 2]>> = vec![Vec::new(); level.depth()];
    seeds[top].push(start);
    let mut reached = vec![vec![false; cols * rows]; level.depth()];
    loop {
        let mut grew = false;
        for k in 0..level.depth() {
            if seeds[k].is_empty() {
                continue;
            }
            let more = reachable_from(&grids[k], cols, rows, &std::mem::take(&mut seeds[k]));
            for i in 0..cols * rows {
                if more[i] && !reached[k][i] {
                    reached[k][i] = true;
                    grew = true;
                    let at = [(i % cols) as i32, (i / cols) as i32];
                    match level.layer(k).tile(at[0], at[1]) {
                        Tile::Stairs(Dir::Down) if k > 0 => seeds[k - 1].push(at),
                        Tile::Stairs(Dir::Up) if k + 1 < level.depth() => seeds[k + 1].push(at),
                        _ => {}
                    }
                }
            }
        }
        if !grew {
            return reached;
        }
    }
}

fn world(seed: u32, cols: usize, rows: usize, spawn: [i32; 2]) -> Vec<Tile> {
    let mut rng = Rng::new(seed);
    let mut occupied = vec![false; cols * rows];
    let mut tiles = vec![Tile::Empty; cols * rows];

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
            let merge = rng.unit() < MERGE_CHANCE;
            if fits(&piece, &occupied, cols, rows, spawn, merge) {
                for ([x, y], _) in &piece.tiles {
                    occupied[*y as usize * cols + *x as usize] = true;
                }
                piece.stamp(&mut tiles, cols);
                placed += 1;
            }
        }
    }
    // Merged pieces can enclose pockets.
    fill_unreachable(&mut tiles, cols, rows, spawn);
    tiles
}

fn cave(seed: u32, cols: usize, rows: usize, spawn: [i32; 2]) -> Vec<Tile> {
    let index = |x: i32, y: i32| y as usize * cols + x as usize;
    let edge = |x: i32, y: i32| x == 0 || y == 0 || x == cols as i32 - 1 || y == rows as i32 - 1;
    // The most open attempt so far, and how many open tiles it has.
    let mut best: (Vec<Tile>, usize) = (Vec::new(), 0);
    for attempt in 0..20u32 {
        let mut rng = Rng::new(seed ^ attempt.wrapping_mul(0x9E37_79B9));
        let mut rock: Vec<bool> = (0..cols * rows)
            .map(|i| edge((i % cols) as i32, (i / cols) as i32) || rng.unit() < CAVE_FILL)
            .collect();

        // Rock where most of the 3x3 neighborhood is rock (outside counts
        // as rock); the edge stays rock.
        for _ in 0..CAVE_STEPS {
            let solid = |x: i32, y: i32| {
                x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 || rock[index(x, y)]
            };
            rock = (0..cols * rows)
                .map(|i| {
                    let (x, y) = ((i % cols) as i32, (i / cols) as i32);
                    let around = (-1..=1)
                        .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                        .filter(|&(dx, dy)| solid(x + dx, y + dy))
                        .count();
                    edge(x, y) || around >= 5
                })
                .collect();
        }

        for y in spawn[1] - SPAWN_CLEARANCE..=spawn[1] + SPAWN_CLEARANCE {
            for x in spawn[0] - SPAWN_CLEARANCE..=spawn[0] + SPAWN_CLEARANCE {
                if !edge(x, y) {
                    rock[index(x, y)] = false;
                }
            }
        }
        let mut tiles: Vec<Tile> = rock
            .iter()
            .map(|r| if *r { Tile::Full } else { Tile::Empty })
            .collect();
        fill_unreachable(&mut tiles, cols, rows, spawn);
        let open = tiles.iter().filter(|t| **t == Tile::Empty).count();
        if open > best.1 {
            best = (tiles, open);
        }
        if best.1 as f32 >= CAVE_MIN_OPEN * (cols * rows) as f32 {
            break;
        }
    }
    let mut tiles = best.0;
    smooth(&mut tiles, cols, rows);
    tiles
}

/// Fill in (make full) every tile with open space that can't be reached
/// from `start`, moving between tiles across sides neither one covers.
fn fill_unreachable(tiles: &mut [Tile], cols: usize, rows: usize, start: [i32; 2]) {
    let reached = reachable(tiles, cols, rows, start);
    for (tile, reached) in tiles.iter_mut().zip(reached) {
        if !reached && *tile != Tile::Full {
            *tile = Tile::Full;
        }
    }
}

/// Which tiles can be reached from `start` across open sides.
pub fn reachable(tiles: &[Tile], cols: usize, rows: usize, start: [i32; 2]) -> Vec<bool> {
    reachable_from(tiles, cols, rows, &[start])
}

/// Which tiles can be reached from any of `starts` across open sides.
fn reachable_from(tiles: &[Tile], cols: usize, rows: usize, starts: &[[i32; 2]]) -> Vec<bool> {
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Tile::Full
        } else {
            tiles[y as usize * cols + x as usize]
        }
    };
    let mut seen = vec![false; cols * rows];
    let mut stack = starts.to_vec();
    while let Some([x, y]) = stack.pop() {
        if tile(x, y) == Tile::Full || seen[y as usize * cols + x as usize] {
            continue;
        }
        seen[y as usize * cols + x as usize] = true;
        for side in Side::ALL {
            let [dx, dy] = side.offset();
            let (nx, ny) = (x + dx, y + dy);
            if !tile(x, y).covers(side) && !tile(nx, ny).covers(side.opposite()) {
                stack.push([nx, ny]);
            }
        }
    }
    seen
}

/// Fill stair-step corners with slopes: an open tile with solid on two
/// neighboring sides (say north and west) and open space on the other two
/// and the diagonal between them gets a slope in that corner. Only ever
/// adds solid, and leaves a way around, so it never thins a wall or cuts
/// a passage.
fn smooth(tiles: &mut [Tile], cols: usize, rows: usize) {
    let snapshot = tiles.to_vec();
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Tile::Full
        } else {
            snapshot[y as usize * cols + x as usize]
        }
    };
    for y in 0..rows as i32 {
        for x in 0..cols as i32 {
            if tile(x, y) != Tile::Empty {
                continue;
            }
            let solid = |side: Side| {
                let [dx, dy] = side.offset();
                tile(x + dx, y + dy).covers(side.opposite())
            };
            for corner in Corner::CLOCKWISE {
                let [a, b] = corner.sides();
                let [c, d] = corner.opposite().sides();
                let [ox, oy] = [c, d]
                    .map(|s| s.offset())
                    .into_iter()
                    .fold([0, 0], |acc, o| [acc[0] + o[0], acc[1] + o[1]]);
                if solid(a)
                    && solid(b)
                    && !solid(c)
                    && !solid(d)
                    && tile(x + ox, y + oy) == Tile::Empty
                {
                    tiles[y as usize * cols + x as usize] = Tile::Slope(corner);
                    break;
                }
            }
        }
    }
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

/// Inside the margin, clear of the spawn, and (unless `merge`) not
/// touching, even diagonally, any occupied tile.
fn fits(
    piece: &Piece,
    occupied: &[bool],
    cols: usize,
    rows: usize,
    spawn: [i32; 2],
    merge: bool,
) -> bool {
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
        in_margin && !near_spawn && (merge || !crowded)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPAWN: [f32; 2] = [165.0, 105.0];
    const STYLES: [Style; 2] = [Style::World, Style::Cave];

    fn spawn_tile() -> [i32; 2] {
        [(SPAWN[0] / TILE) as i32, (SPAWN[1] / TILE) as i32]
    }

    fn tiles_of(level: &Layer) -> Vec<Tile> {
        (0..20)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .map(|(x, y)| level.tile(x, y))
            .collect()
    }

    #[test]
    fn open_space_is_connected() {
        for style in STYLES {
            for seed in 0..100 {
                let level = generate(style, seed, 32, 20, SPAWN);
                let tiles = tiles_of(&level);
                let reached = reachable(&tiles, 32, 20, spawn_tile());
                for (i, t) in tiles.iter().enumerate() {
                    assert!(
                        *t == Tile::Full || reached[i],
                        "{style:?} seed {seed}: tile {i} cut off"
                    );
                }
            }
        }
    }

    #[test]
    fn spawn_is_clear() {
        for style in STYLES {
            for seed in 0..100 {
                let level = generate(style, seed, 32, 20, SPAWN);
                assert!(
                    !level.collides([SPAWN[0] - 4.0, SPAWN[1] - 4.0], [8.0, 8.0]),
                    "{style:?} {seed}"
                );
            }
        }
    }

    #[test]
    fn worlds_have_variety_and_composites() {
        let (mut slopes, mut bars, mut merged) = (0, 0, 0);
        for seed in 0..100 {
            let level = generate(Style::World, seed, 32, 20, SPAWN);
            for y in 0..20 {
                for x in 0..32 {
                    let tile = level.tile(x, y);
                    slopes += matches!(tile, Tile::Slope(_)) as usize;
                    // A bar's signature: full tiles diagonally adjacent with
                    // a slope filling the step between them.
                    let full = |dx, dy| level.tile(x + dx, y + dy) == Tile::Full;
                    let step = level.tile(x + 1, y);
                    let rising = full(0, 0) && full(1, -1) && step == Tile::Slope(Corner::NW);
                    let falling = full(0, 0) && full(1, 1) && step == Tile::Slope(Corner::SW);
                    bars += (rising || falling) as usize;
                }
            }
            // An outline loop with a concave corner is a composite (pieces
            // are convex on their own), or a filled-in pocket.
            merged += level
                .concave_corners()
                .iter()
                .any(|c| level.in_bounds([(c.at[0] / TILE) as i32, (c.at[1] / TILE) as i32]))
                as usize;
        }
        assert!(slopes > 500, "{slopes} slope tiles in 100 levels");
        assert!(bars > 100, "{bars} bar steps in 100 levels");
        assert!(merged > 50, "{merged} of 100 levels with composites");
    }

    #[test]
    fn caves_are_mostly_rock_with_open_halls() {
        for seed in 0..100 {
            let level = generate(Style::Cave, seed, 32, 20, SPAWN);
            let tiles = tiles_of(&level);
            // Slopes (from smoothing) are half open.
            let open = tiles
                .iter()
                .map(|t| match t {
                    Tile::Empty | Tile::Void | Tile::Stairs(_) => 1.0,
                    Tile::Slope(_) => 0.5,
                    Tile::Full => 0.0,
                })
                .sum::<f32>()
                / tiles.len() as f32;
            assert!(
                (CAVE_MIN_OPEN - 0.05..0.8).contains(&open),
                "seed {seed}: {open} open"
            );
            for x in 0..32 {
                assert_eq!(level.tile(x, 0), Tile::Full);
                assert_eq!(level.tile(x, 19), Tile::Full);
            }
            assert!(
                tiles.iter().any(|t| matches!(t, Tile::Slope(_))),
                "seed {seed}: no slopes"
            );
        }
    }

    #[test]
    fn layered_levels_connect_through_stairs() {
        let mut sees_below = 0;
        for seed in 0..50 {
            let level = generate_level(Style::Layered, seed, 32, 20, SPAWN);
            assert_eq!(level.depth(), LAYERS);
            let top = level.depth() - 1;
            assert!(
                !level
                    .movement(top)
                    .collides([SPAWN[0] - 4.0, SPAWN[1] - 4.0], [8.0, 8.0]),
                "seed {seed}: spawn blocked"
            );

            // Every walkable tile on every layer is reachable.
            let reached = reachable_across(&level, top, spawn_tile());
            for (k, reached) in reached.iter().enumerate() {
                let m = level.movement(k);
                for i in 0..32 * 20 {
                    let open = m.tile(i % 32, i / 32) != Tile::Full;
                    assert!(
                        !open || reached[i as usize],
                        "seed {seed}: layer {k} tile {i} cut off"
                    );
                }
            }

            // Each layer has stairs down to the one below, paired with
            // stairs up, with floor all around on both.
            for upper in 1..level.depth() {
                let mut pairs = 0;
                for y in 0..20 {
                    for x in 0..32 {
                        if level.layer(upper).tile(x, y) == Tile::Stairs(Dir::Down) {
                            pairs += 1;
                            assert_eq!(
                                level.layer(upper - 1).tile(x, y),
                                Tile::Stairs(Dir::Up),
                                "seed {seed}"
                            );
                            for k in [upper - 1, upper] {
                                let around = [(x - 1) as f32 * TILE, (y - 1) as f32 * TILE];
                                assert!(
                                    !level.movement(k).collides(around, [3.0 * TILE, 3.0 * TILE]),
                                    "seed {seed}"
                                );
                            }
                        }
                    }
                }
                assert!(pairs > 0, "seed {seed}: no stairs from layer {upper}");
            }
            sees_below += level.visible_layers(top).contains(&(top - 1)) as usize;
        }
        assert!(
            sees_below > 40,
            "{sees_below} of 50 levels show the layer below"
        );
    }

    #[test]
    fn bigger_levels_stay_connected() {
        let spawn = [325.0, 205.0];
        let start = [32, 20];
        for seed in 0..10 {
            let level = generate_level(Style::Layered, seed, 64, 40, spawn);
            let top = level.depth() - 1;
            assert!(
                !level
                    .movement(top)
                    .collides([spawn[0] - 4.0, spawn[1] - 4.0], [8.0, 8.0])
            );
            let reached = reachable_across(&level, top, start);
            for (k, reached) in reached.iter().enumerate() {
                let m = level.movement(k);
                for i in 0..64 * 40 {
                    let open = m.tile(i % 64, i / 64) != Tile::Full;
                    assert!(
                        !open || reached[i as usize],
                        "seed {seed}: layer {k} tile {i} cut off"
                    );
                }
            }
            for style in [Style::World, Style::Cave] {
                let layer = generate(style, seed, 64, 40, spawn);
                let tiles: Vec<Tile> = (0..40)
                    .flat_map(|y| (0..64).map(move |x| (x, y)))
                    .map(|(x, y)| layer.tile(x, y))
                    .collect();
                let reached = reachable(&tiles, 64, 40, start);
                assert!(
                    tiles
                        .iter()
                        .zip(&reached)
                        .all(|(t, r)| *t == Tile::Full || *r),
                    "{style:?} {seed}"
                );
            }
        }
    }

    #[test]
    fn smoothing_fills_stair_steps_only() {
        // An inside corner with room around it gets a slope; a one-tile
        // corridor bend doesn't, since that would pinch it shut.
        let rows = ["#####", "#....", "#....", "#...."];
        let mut tiles: Vec<Tile> = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| if c == '#' { Tile::Full } else { Tile::Empty })
            .collect();
        smooth(&mut tiles, 5, 4);
        assert_eq!(tiles[5 + 1], Tile::Slope(Corner::NW));
        assert_eq!(tiles[2 * 5 + 2], Tile::Empty);

        let bend = ["#####", "#..##", "##.##", "#####"];
        let mut tiles: Vec<Tile> = bend
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| if c == '#' { Tile::Full } else { Tile::Empty })
            .collect();
        let before = tiles.clone();
        smooth(&mut tiles, 5, 4);
        assert_eq!(tiles, before);
    }
}
