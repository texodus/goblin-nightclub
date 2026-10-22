//                                    ▗◡≝▔▔` ▔≝=◺▂             ∕        ▐▋  ▝▉
//                                  ╣▒◿▒≘≉◬_    _▒▆▒▀         ◞         ▐▋  ▐▌
//                                  ╔`     ▝◺◿▒▟▘╔╹ ▚◛                  ▐▌  ▐▏
//       G O B L I N               ▕▎_"▃▅≣P ▐▓▛▗▉▂_▂__◚◛_         __▂▂▂▂▟▙◓▀▀▀▀≝
//        N I G H T C L U B         ║`║║_▂▐╝ ▟◙██▛╬▔`_▂◙▒▖       ▕∏▃▃▃▃▂▐▌║▂▂▂▔▔
//                                  ▙⋡▜╥∺╝ ▗▟███▛Ƒ  ▐▛▔▔▜█▌      ▐╣ ▝▜███▋ ∭▔▔▔▔
//                                   ▁⋡▚▃▄▀▉Ƣ█▀▉╺▗▐∏▊   ╠█▎      ▐╣   ▁∭█▉ ▊ ∭▐█
//                          ▁▃◛◭◒≠◚▔▔╸ ╔▒∏▕▓▌ ▟▎ ▐▒██▖    ▜▎     ╠▎≕∏__  ◙∏▍ ╣█▌
//                     _◿▎`▂_ ▟▒║╣▂▂▂▂▞╜▔▜ ╬◙▏∏▎ ▐███◘⊠◚"` ▜     ╣ ▂▅▅▅_ ▐█▎▕▒█Ǩ
//              _▂_ _▂▂▒∭▒▒◛≝▔           ▕▶╣◚▔▔▔╠∏∏║▌ ┉━━≝▒▃▘   ▕▌▟█████▙ ∭ ▐▐▌▓
//    ▁▁▁▁_╓◛▋▝◠▬Ǟ◡▃╠◚≝≊■■■▆▆▤▀◚⊜⊛╥▄▃▃▂_║╳▞▗╝▟▛◚███████████▌    ▕▒████◚◚█▌▋ ∭▟▎╱
//   ▐▔▔"⋨∭◅▃╏◠≉╬▒▇▆▅┫ ▕▄▃∏◠━━◡▃▂║▃▃▃▄▙▄∭▟▟▙▓≝▬▬╩◘██████████    ▐∏██◘▜▁╔▕▒▎ ▒█▎╣
//   ▕╣◚◘▉▐▄▃▂▁▟▒╝◙▉◙Ů▒▒◚▜▛◙◘▒▒▌▍ ▔▔"▀▬ǡ∗▃∑◚▀▀◘◘▓████▀╣▟╢▒◙◙▙▖  ▐ ╬     ∭▐▏▐█▉▏▜
//    ĴĄ  ╲╲ ▔╠▛◘◘▓▍▅╣┇▐▒∭Ǭ▛╨◀∗▃▒▀Ň◘█▒⋯┳▂▃_▐█████████████████▌∑◚╣__%◺_⊋∛ ∭ ▐▐▉▌▝
//  ▔  ┗▌   ▔◚▜▍▝▀▀▜█▌▐▐▐██▙▄▃▂_ ▗╢▒▛◚▬∗▃▂◚▀╩╩╩◘◘◙╩◘Ů▔▔▔▔▔▔▔▔▔▝▜▒▒▒▒▒◘◘▣▤▌ ∭█▌▕▎
//  Copyright 2026, [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

//! The caves and dungeons below ground: caves grown by cellular automata,
//! rooms, pools, holes, stairs between the layers, and the entrances down
//! from the plains.

use super::*;
use crate::connect::reachable_from;
use crate::noise::{fbm, lattice};

/// Initial chance of each cave tile being rock.
const CAVE_FILL: f32 = 0.45;

/// Cellular automaton steps smoothing the cave.
const CAVE_STEPS: usize = 4;

/// Retry until at least this share of the layer is reachable floor.
const CAVE_MIN_OPEN: f32 = 0.35;

impl World {
    /// Caves with dungeon rooms on every underworld layer, pools in the
    /// caves, and stairs between the layers. A region's caves come from a
    /// hash of where each tile is (the same whichever region looks), and
    /// what's placed by chance only where placeable.
    pub(super) fn underworld(&mut self) {
        let m = self.params.underworld_layers;
        let seed = match self.plan() {
            Some(plan) => plan.cave_seed,
            None => self.rng.range(0, u16::MAX as usize) as u32,
        };
        for k in 0..m {
            let seed = seed ^ (k as u32).wrapping_mul(0x9E37_79B9);
            self.layers[k] = if self.region.is_some() {
                self.region_cave(seed)
            } else {
                // Grown around a seed tile, so there's floor near where the
                // stairs from above (or the entrances) will arrive.
                let around = [
                    self.rng.range(4, self.cols - 5) as i32,
                    self.rng.range(4, self.rows - 5) as i32,
                ];
                let floor = Part::Floor(self.floors.cave);
                cave(seed, self.cols, self.rows, around, self.rock, floor)
            };
            self.rooms(k);
            self.pools(k);
            self.cave_holes(k);
        }
        // A region has at least one way down from each, wherever it is.
        let stairs = match self.region {
            Some(_) => self.scaled(self.params.stairs).max(1),
            None => self.params.stairs,
        };
        for upper in (1..m).rev() {
            self.stairs_between(upper, stairs);
        }
    }

    /// Up to `count` stairs between cave layer `upper` and the one below,
    /// spread out, where both have floor all around; if there's nowhere, a
    /// pocket cleared on both, clear of water and undermining none.
    fn stairs_between(&mut self, upper: usize, count: usize) {
        let [lo, hi] = self.fixtures_box();
        let inside = |[x, y]: [i32; 2]| x >= lo[0] && y >= lo[1] && x < hi[0] && y < hi[1];
        let clear = |w: &World, k: usize, [x, y]: [i32; 2]| {
            (-1..=1).all(|dy| (-1..=1).all(|dx| w.layers[k][w.index([x + dx, y + dy])].is_floor()))
        };
        let mut candidates: Vec<[i32; 2]> = self
            .tiles()
            .filter(|&t| inside(t) && clear(self, upper, t) && clear(self, upper - 1, t))
            .collect();
        if candidates.is_empty() {
            let dry = |w: &World, t: [i32; 2]| {
                inside(t)
                    && !w.beside_water(upper, t)
                    && !w.pocket_undermines(upper - 1, t)
                    && !w.pocket_undermines(upper, t)
            };
            let pocket = if self.region.is_some() {
                let spots: Vec<[i32; 2]> = self.tiles().filter(|&t| dry(self, t)).collect();
                (!spots.is_empty()).then(|| spots[self.rng.range(0, spots.len() - 1)])
            } else {
                // A world made whole wants floor there on the upper layer
                // already (and draws, whether or not there's anywhere).
                let spots: Vec<[i32; 2]> = self
                    .tiles()
                    .filter(|&t| dry(self, t) && self.layers[upper][self.index(t)].is_floor())
                    .collect();
                spots
                    .get(self.rng.range(0, spots.len().max(1) - 1))
                    .copied()
            };
            if let Some(at) = pocket {
                for k in [upper - 1, upper] {
                    self.clear_pocket(k, at);
                }
                candidates.push(at);
            }
        }
        for _ in 0..count {
            if candidates.is_empty() {
                break;
            }
            let at = candidates.swap_remove(self.rng.range(0, candidates.len() - 1));
            let i = self.index(at);
            self.layers[upper][i] = Tile::Stairs(Dir::Down, self.floors.cave);
            self.layers[upper - 1][i] = Tile::Stairs(Dir::Up, self.floors.cave);
            candidates.retain(|c| (c[0] - at[0]).abs().max((c[1] - at[1]).abs()) > 4);
        }
    }

    /// A region's cave: as [`cave`] makes one, by cellular automata, but
    /// starting from a hash of where each tile is in the whole world, so
    /// it comes out the same wherever it's made (but within the steps of
    /// the region's edge, past which counts as rock). The world's border
    /// stays rock.
    fn region_cave(&self, seed: u32) -> Vec<Tile> {
        let rock: Vec<bool> = self
            .tiles()
            .map(|t| {
                let [x, y] = self.absolute(t);
                !self.interior(t) || lattice(seed, x, y) < CAVE_FILL
            })
            .collect();
        let rock = smooth_cave(rock, self.cols, self.rows, |x, y| !self.interior([x, y]));
        let (wall, floor) = (self.rock, Part::Floor(self.floors.cave));
        rock.into_iter()
            .map(|r| Tile::Whole(if r { wall } else { floor }))
            .collect()
    }

    /// Would a pocket cleared around `at` on layer `k` (see
    /// [`World::clear_pocket`]) undermine water on the layer above? Water
    /// rests on the 3x3 under it (see [`World::rests`]), so any within two
    /// tiles of `at` would.
    fn pocket_undermines(&self, k: usize, at: [i32; 2]) -> bool {
        k + 1 < self.layers.len()
            && (-2..=2).any(|dy| {
                (-2..=2).any(|dx| {
                    let n = [at[0] + dx, at[1] + dy];
                    self.in_bounds(n) && self.layers[k + 1][self.index(n)].liquid_quarters() != 0
                })
            })
    }

    /// A 3x3 of cave floor around `at` on layer `k`, leaving stairs.
    fn clear_pocket(&mut self, k: usize, at: [i32; 2]) {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let t = [at[0] + dx, at[1] + dy];
                if self.interior(t) {
                    let i = self.index(t);
                    if self.layers[k][i].stairs().is_none() {
                        self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.cave));
                    }
                }
            }
        }
    }

    /// Dungeon rooms on cave layer `k`: rectangles of plank floor walled
    /// in brick, each with a door and a corridor dug toward the cave.
    fn rooms(&mut self, k: usize) {
        let (brick, planks) = (
            Part::Wall(tileset().wall(ROOM_WALL).expect("a brick wall")),
            Part::Floor(self.floors.room),
        );
        let mut placed: Vec<[i32; 4]> = Vec::new();
        let rooms = self.scaled(self.params.rooms);
        for _ in 0..rooms * 10 {
            if placed.len() >= rooms {
                break;
            }
            let (w, h) = (self.rng.range(5, 9) as i32, self.rng.range(4, 7) as i32);
            let [lo, hi] = self.fixtures_box();
            if lo[0] + w >= hi[0] || lo[1] + h >= hi[1] {
                continue;
            }
            let x = self.rng.range(lo[0] as usize, (hi[0] - w) as usize) as i32;
            let y = self.rng.range(lo[1] as usize, (hi[1] - h) as usize) as i32;
            let overlaps = placed.iter().any(|r| {
                x < r[0] + r[2] + 1 && x + w + 1 > r[0] && y < r[1] + r[3] + 1 && y + h + 1 > r[1]
            });
            if overlaps {
                continue;
            }
            placed.push([x, y, w, h]);
            for ty in y..y + h {
                for tx in x..x + w {
                    let edge = tx == x || tx == x + w - 1 || ty == y || ty == y + h - 1;
                    let i = self.index([tx, ty]);
                    self.layers[k][i] = Tile::Whole(if edge { brick } else { planks });
                }
            }
            // A door on a random side, and a corridor out from it until it
            // meets cave floor.
            let side = Side::ALL[self.rng.range(0, 3)];
            let door = match side {
                Side::N => [x + 1 + self.rng.range(0, (w - 3) as usize) as i32, y],
                Side::S => [
                    x + 1 + self.rng.range(0, (w - 3) as usize) as i32,
                    y + h - 1,
                ],
                Side::W => [x, y + 1 + self.rng.range(0, (h - 3) as usize) as i32],
                Side::E => [
                    x + w - 1,
                    y + 1 + self.rng.range(0, (h - 3) as usize) as i32,
                ],
            };
            let door_index = self.index(door);
            self.layers[k][door_index] = Tile::Whole(planks);
            let [dx, dy] = side.offset();
            let mut at = door;
            for _ in 0..(self.cols + self.rows) {
                at = [at[0] + dx, at[1] + dy];
                if !self.interior(at) || !self.placeable(at) {
                    break;
                }
                let i = self.index(at);
                if self.layers[k][i].is_floor() || self.layers[k][i] == self.water {
                    break;
                }
                self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.cave));
            }
        }
    }

    /// Pools: the lowest-noise cave floor, where all its neighbors are
    /// floor too (so nothing narrow is blocked) and the cave below is
    /// solid underneath (see [`World::rests`]), up to `pool_share` of the
    /// floor.
    fn pools(&mut self, k: usize) {
        let share = self.params.pool_share.clamp(0.0, 0.5);
        if share <= 0.0 {
            return;
        }
        // Cave floor with floor all around (so nothing narrow is blocked),
        // over solid ground.
        let cave = self.floors.cave;
        let spot = |w: &World, t: [i32; 2]| {
            w.layers[k][w.index(t)].floor_id() == Some(cave)
                && (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| {
                        let n = [t[0] + dx, t[1] + dy];
                        w.in_bounds(n) && w.layers[k][w.index(n)].is_floor()
                    })
                })
                && w.rests(k, t)
        };
        if let Some(plan) = self.plan() {
            // Where the pool noise is lowest, by the whole world's line.
            let (seed, level) = (plan.pool_seed ^ k as u32, plan.pool_level);
            let pools: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| {
                    let [x, y] = self.absolute(t);
                    spot(self, t) && fbm(seed, x as f32 / 6.0, y as f32 / 6.0) < level
                })
                .collect();
            for t in pools {
                let i = self.index(t);
                self.layers[k][i] = self.water;
            }
            return;
        }
        let seed = self.rng.range(0, u16::MAX as usize) as u32;
        let floor = self.layers[k]
            .iter()
            .filter(|t| t.floor_id() == Some(cave))
            .count();
        let mut open: Vec<([i32; 2], f32)> = self
            .tiles()
            .filter(|&t| spot(self, t))
            .map(|t| (t, fbm(seed, t[0] as f32 / 6.0, t[1] as f32 / 6.0)))
            .collect();
        open.sort_by(|a, b| a.1.total_cmp(&b.1));
        let count = ((floor as f32 * share) as usize).min(open.len());
        for (t, _) in open.into_iter().take(count) {
            let i = self.index(t);
            self.layers[k][i] = self.water;
        }
    }

    /// Holes and canyons in cave layer `k`, down to the cave below (or
    /// into the dark, on the lowest). Only in cave floor, clear of pools.
    fn cave_holes(&mut self, k: usize) {
        let cave_floor = self.floors.cave;
        let allowed = move |w: &World, t: [i32; 2]| {
            w.interior(t)
                && w.layers[k][w.index(t)].floor_id() == Some(cave_floor)
                && !w.beside_water(k, t)
        };
        self.carve_voids(k, &allowed);
    }

    /// Entrances: a cellar in the lowest overworld layer under a plains
    /// tile, with stairs down from the surface and, beside them, stairs on
    /// down into the top cave (cleared around where they arrive).
    pub(super) fn entrances(&mut self) {
        let (plains, cellar, cave) = (self.over(1), self.over(0), self.top_cave());
        let plains_floor = self.floors.plains;
        for _ in 0..self.scaled(self.params.entrances) {
            let spots: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| {
                    self.placeable(t) &&
                    // The cellar's footprint: plains all over it, rock
                    // under all of it, and no water on the plains within a
                    // tile of it (water needs solid ground beneath).
                    t[0] >= 3
                        && t[1] >= 3
                        && (t[0] as usize) + 4 < self.cols
                        && (t[1] as usize) + 3 < self.rows
                        && !self.near_spawn(t, 3)
                        && (-1..=1).all(|dy| {
                            (-1..=2).all(|dx| {
                                let i = self.index([t[0] + dx, t[1] + dy]);
                                self.layers[plains][i].floor_id() == Some(plains_floor)
                                    && self.layers[cellar][i] == Tile::Whole(self.rock)
                            })
                        })
                        && (-2..=2).all(|dy| {
                            (-2..=3).all(|dx| {
                                let i = self.index([t[0] + dx, t[1] + dy]);
                                self.layers[plains][i] != self.water
                            })
                        })
                })
                .collect();
            if spots.is_empty() {
                break;
            }
            let at = spots[self.rng.range(0, spots.len() - 1)];
            let beside = [at[0] + 1, at[1]];
            let (ia, ib) = (self.index(at), self.index(beside));
            let cave_floor = self.floors.cave;
            self.clear_pocket(cellar, at);
            self.clear_pocket(cellar, beside);
            self.clear_pocket(cave, beside);
            self.layers[plains][ia] = Tile::Stairs(Dir::Down, plains_floor);
            self.layers[cellar][ia] = Tile::Stairs(Dir::Up, cave_floor);
            self.layers[cellar][ib] = Tile::Stairs(Dir::Down, cave_floor);
            self.layers[cave][ib] = Tile::Stairs(Dir::Up, cave_floor);
        }
    }
}

/// A cave of `floor` carved out of `wall` by cellular automata, grown
/// until at least [`CAVE_MIN_OPEN`] of the layer is floor connected to
/// `around` (which is cleared), the border left solid.
fn cave(
    seed: u32,
    cols: usize,
    rows: usize,
    around: [i32; 2],
    wall: Part,
    floor: Part,
) -> Vec<Tile> {
    let index = |x: i32, y: i32| y as usize * cols + x as usize;
    let edge = |x: i32, y: i32| x == 0 || y == 0 || x == cols as i32 - 1 || y == rows as i32 - 1;
    let mut best: (Vec<Tile>, usize) = (Vec::new(), 0);
    for attempt in 0..20u32 {
        let mut rng = Rng::new(seed ^ attempt.wrapping_mul(0x9E37_79B9));
        let rock: Vec<bool> = (0..cols * rows)
            .map(|i| edge((i % cols) as i32, (i / cols) as i32) || rng.unit() < CAVE_FILL)
            .collect();
        // The edge stays rock.
        let mut rock = smooth_cave(rock, cols, rows, edge);
        for y in around[1] - SPAWN_CLEARANCE..=around[1] + SPAWN_CLEARANCE {
            for x in around[0] - SPAWN_CLEARANCE..=around[0] + SPAWN_CLEARANCE {
                if !edge(x, y) {
                    rock[index(x, y)] = false;
                }
            }
        }
        let mut tiles: Vec<Tile> = rock
            .iter()
            .map(|r| Tile::Whole(if *r { wall } else { floor }))
            .collect();
        let solids: Vec<Solid> = tiles.iter().map(|t| t.wall()).collect();
        let reached = reachable_from(&solids, cols, rows, &[around]);
        for (tile, reached) in tiles.iter_mut().zip(reached) {
            if !reached {
                *tile = Tile::Whole(wall);
            }
        }
        let open = tiles.iter().filter(|t| t.is_floor()).count();
        if open > best.1 {
            best = (tiles, open);
        }
        if best.1 as f32 >= CAVE_MIN_OPEN * (cols * rows) as f32 {
            break;
        }
    }
    best.0
}

/// A cave's rock (`rock`, `cols` x `rows`, row by row) smoothed by
/// cellular automaton: rock where most of the 3x3 neighborhood is rock
/// (outside counts as rock), `fixed(x, y)` tiles rock whatever.
fn smooth_cave(
    mut rock: Vec<bool>,
    cols: usize,
    rows: usize,
    fixed: impl Fn(i32, i32) -> bool,
) -> Vec<bool> {
    let (c, r) = (cols as i32, rows as i32);
    for _ in 0..CAVE_STEPS {
        let solid =
            |x: i32, y: i32| x < 0 || y < 0 || x >= c || y >= r || rock[(y * c + x) as usize];
        rock = (0..cols * rows)
            .map(|i| {
                let (x, y) = ((i % cols) as i32, (i / cols) as i32);
                let count = (-1..=1)
                    .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                    .filter(|&(dx, dy)| solid(x + dx, y + dy))
                    .count();
                fixed(x, y) || count >= 5
            })
            .collect();
    }
    rock
}
