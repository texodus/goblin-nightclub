//! The world generator for the engine in `goblin_nightclub_core`: one kind of level,
//! built in two groups of layers, bottom to top:
//!
//! - **The underworld**, [`Params::underworld_layers`] layers of caves and
//!   dungeon rooms carved out of solid rock, joined by stairs, with pools
//!   of water in the caves. Enclosed on every side.
//! - **The overworld**, [`Params::overworld_layers`] layers of terrain from
//!   a heightmap: each column is rock up to its height, floor at it, and
//!   void above, so from above you look down onto plains, hills and
//!   mountains, and a step in height is a cliff. Cliffs are climbed by
//!   ramps (a staircase in the cliff face), placed along them every so
//!   often if [`Params::ramp_spacing`] allows. Lakes fill the lowest
//!   ground and rivers run down to them, both water over a sandy bed, with
//!   fords where a river would otherwise cut the land in two.
//!
//! The two meet at entrances: a cellar under the plains with a staircase
//! down from the surface and another down into the top cave.
//!
//! Everything the player can walk to from the spawn is kept; floor in the
//! underworld that can't be reached is filled with rock (ground in the
//! overworld stays, reachable or not: whether every hilltop has a ramp is
//! up to the parameters). Water never blocks the way between two pieces of
//! land that nothing else joins.
//!
//! The materials are named in the tile sheet (see [`goblin_nightclub_core::tileset`]).

use serde::{Deserialize, Serialize};
use goblin_nightclub_core::layer::{Corner, Dir, FloorId, Layer, Part, Side, Solid, TILE, Tile, WallId};
use goblin_nightclub_core::level::{Level, MAX_LAYERS};
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset;

/// What to generate. Every field has a default; see [`Params::default`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// Layers of terrain. Heights run from 1 (plains) to this minus 1, so
    /// 2 gives flat plains only, 3 adds hills, 4 adds mountains, and more
    /// stack higher peaks. The lowest layer holds the lake beds.
    pub overworld_layers: usize,
    /// Layers of caves and dungeons below the terrain.
    pub underworld_layers: usize,
    /// Size of terrain features, in tiles: the wavelength of the largest
    /// hills.
    pub terrain_scale: f32,
    /// Share of the land that is hills or higher, and of that, the share
    /// that is mountains (the highest heights), both 0 to 1.
    pub hill_share: f32,
    pub mountain_share: f32,
    /// Share of the surface under lakes, 0 to 1.
    pub water_share: f32,
    /// Rivers to run from the hills to the lakes or the edge.
    pub rivers: usize,
    /// Tiles between ramps along a cliff. 0 for no ramps at all, so the
    /// hills can't be climbed; otherwise every piece of high ground gets at
    /// least one, spacing or not.
    pub ramp_spacing: usize,
    /// Tiles between fords along a river.
    pub ford_spacing: usize,
    /// Entrances from the surface to the underworld.
    pub entrances: usize,
    /// Dungeon rooms on each underworld layer, and staircases between each
    /// pair of underworld layers.
    pub rooms: usize,
    pub stairs: usize,
    /// Share of each cave's open floor under pools of water, 0 to 1.
    pub pool_share: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            overworld_layers: 4,
            underworld_layers: 3,
            terrain_scale: 14.0,
            hill_share: 0.4,
            mountain_share: 0.25,
            water_share: 0.1,
            rivers: 2,
            ramp_spacing: 7,
            ford_spacing: 9,
            entrances: 2,
            rooms: 3,
            stairs: 2,
            pool_share: 0.06,
        }
    }
}

/// Initial chance of each cave tile being rock.
const CAVE_FILL: f32 = 0.45;
/// Cellular automaton steps smoothing the cave.
const CAVE_STEPS: usize = 4;
/// Retry until at least this share of the layer is reachable floor.
const CAVE_MIN_OPEN: f32 = 0.35;

/// Tiles (Chebyshev) around the spawn kept flat, dry and open.
const SPAWN_CLEARANCE: i32 = 2;

/// Materials, by name in the tile sheet.
const PLAINS: &str = "grass";
const HILLS: &str = "dirt";
const MOUNTAINS: &str = "stone";
const BED: &str = "sand";
const ROCK: &str = "rock";
const CAVE_FLOOR: &str = "dirt";
const ROOM_FLOOR: &str = "planks";
const ROOM_WALL: &str = "brick";

/// Generate a level from `seed`, `cols` x `rows` tiles a layer, the player
/// starting on the plains at `spawn` (pixels).
pub fn generate_level(
    seed: u32,
    cols: usize,
    rows: usize,
    spawn: [f32; 2],
    params: &Params,
) -> Level {
    let spawn = [(spawn[0] / TILE) as i32, (spawn[1] / TILE) as i32];
    World::new(seed, cols, rows, spawn, params).build()
}

/// The spawn's layer: the plains, the second overworld layer up.
pub fn spawn_layer(params: &Params) -> usize {
    params.underworld_layers.max(1) + 1
}

struct World {
    rng: Rng,
    cols: usize,
    rows: usize,
    spawn: [i32; 2],
    params: Params,
    /// Underworld layers, then overworld, each row by row.
    layers: Vec<Vec<Tile>>,
    /// Terrain height per column, 1 and up (the overworld layer its floor
    /// is on).
    height: Vec<usize>,
    /// Terrain noise per column, in [0, 1).
    noise: Vec<f32>,
    rock: Part,
    floors: Floors,
}

struct Floors {
    plains: FloorId,
    hills: FloorId,
    mountains: FloorId,
    bed: FloorId,
    cave: FloorId,
    room: FloorId,
}

impl World {
    fn new(seed: u32, cols: usize, rows: usize, spawn: [i32; 2], params: &Params) -> Self {
        let mut params = params.clone();
        params.underworld_layers = params.underworld_layers.clamp(1, MAX_LAYERS - 2);
        params.overworld_layers = params
            .overworld_layers
            .clamp(2, MAX_LAYERS - params.underworld_layers);
        let depth = params.underworld_layers + params.overworld_layers;
        let rock = Part::Wall(tileset::wall(ROCK));
        let spawn = [
            spawn[0].clamp(SPAWN_CLEARANCE + 1, cols as i32 - SPAWN_CLEARANCE - 2),
            spawn[1].clamp(SPAWN_CLEARANCE + 1, rows as i32 - SPAWN_CLEARANCE - 2),
        ];
        Self {
            rng: Rng::new(seed),
            cols,
            rows,
            spawn,
            params,
            layers: vec![vec![Tile::Whole(rock); cols * rows]; depth],
            height: vec![1; cols * rows],
            noise: vec![0.0; cols * rows],
            rock,
            floors: Floors {
                plains: tileset::floor(PLAINS),
                hills: tileset::floor(HILLS),
                mountains: tileset::floor(MOUNTAINS),
                bed: tileset::floor(BED),
                cave: tileset::floor(CAVE_FLOOR),
                room: tileset::floor(ROOM_FLOOR),
            },
        }
    }

    fn build(mut self) -> Level {
        self.terrain();
        self.lakes();
        self.rivers();
        self.raise_terrain();
        self.ramps();
        self.underworld();
        self.entrances();
        self.connect();
        for k in 0..self.layers.len() {
            smooth(&mut self.layers[k], self.cols, self.rows);
        }
        self.level()
    }

    fn level(&self) -> Level {
        Level::new(
            self.layers
                .iter()
                .map(|t| Layer::from_tiles(self.cols, self.rows, t.clone()))
                .collect(),
        )
    }

    fn index(&self, [x, y]: [i32; 2]) -> usize {
        y as usize * self.cols + x as usize
    }

    fn in_bounds(&self, [x, y]: [i32; 2]) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.cols && (y as usize) < self.rows
    }

    /// Inside the one-tile border of rock.
    fn interior(&self, [x, y]: [i32; 2]) -> bool {
        x >= 1 && y >= 1 && (x as usize) + 1 < self.cols && (y as usize) + 1 < self.rows
    }

    fn near_spawn(&self, [x, y]: [i32; 2], margin: i32) -> bool {
        (x - self.spawn[0]).abs().max((y - self.spawn[1]).abs()) <= SPAWN_CLEARANCE + margin
    }

    /// The overworld layer `h` above the lowest (height `h`'s floor).
    fn over(&self, h: usize) -> usize {
        self.params.underworld_layers + h
    }

    fn top_cave(&self) -> usize {
        self.params.underworld_layers - 1
    }

    fn tiles(&self) -> impl Iterator<Item = [i32; 2]> + use<> {
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        (0..rows).flat_map(move |y| (0..cols).map(move |x| [x, y]))
    }

    // --- The overworld ---------------------------------------------------

    /// Heights from noise: the lowest `water_share` of the surface is left
    /// at the plains' height for lakes; of the rest, `hill_share` is hills
    /// and above, and of that `mountain_share` is the mountains, spread
    /// over the heights above the hills. Steps between neighbors are
    /// limited to one, so every cliff is one layer tall. The spawn and its
    /// surroundings are plains.
    fn terrain(&mut self) {
        let p = &self.params;
        let top = p.overworld_layers - 1;
        let seed = self.rng.range(0, u16::MAX as usize) as u32;
        let scale = p.terrain_scale.max(2.0);
        let noise: Vec<f32> = self
            .tiles()
            .map(|[x, y]| fbm(seed, x as f32 / scale, y as f32 / scale))
            .collect();
        let mut sorted = noise.clone();
        sorted.sort_by(f32::total_cmp);
        let at = |share: f32| sorted[(share.clamp(0.0, 1.0) * (sorted.len() - 1) as f32) as usize];
        let land_from = p.water_share.clamp(0.0, 0.9);
        let hills_from = land_from + (1.0 - land_from) * (1.0 - p.hill_share.clamp(0.0, 1.0));
        let mountains_from =
            hills_from + (1.0 - hills_from) * (1.0 - p.mountain_share.clamp(0.0, 1.0));
        let (hills, mountains) = (at(hills_from), at(mountains_from));
        let peaks = top.saturating_sub(2).max(1) as f32;
        for (i, &n) in noise.iter().enumerate() {
            self.height[i] = if top < 2 || n < hills {
                1
            } else if top < 3 || n < mountains {
                2
            } else {
                // The mountains' heights, 3 and up, by how far above the
                // mountain line.
                let t = (n - mountains) / (1.0 - mountains).max(1e-6);
                3 + ((t * peaks) as usize).min(top - 3)
            };
        }
        self.noise = noise;
        // No step of more than one between neighbors: lower the high side.
        loop {
            let mut changed = false;
            for t in self.tiles() {
                let i = self.index(t);
                for side in Side::ALL {
                    let [dx, dy] = side.offset();
                    let n = [t[0] + dx, t[1] + dy];
                    if self.in_bounds(n) && self.height[i] > self.height[self.index(n)] + 1 {
                        self.height[i] = self.height[self.index(n)] + 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        for t in self.tiles() {
            if self.near_spawn(t, 2) {
                let i = self.index(t);
                self.height[i] = 1;
            }
        }
        // Relax again around the flattened spawn.
        loop {
            let mut changed = false;
            for t in self.tiles() {
                let i = self.index(t);
                for side in Side::ALL {
                    let [dx, dy] = side.offset();
                    let n = [t[0] + dx, t[1] + dy];
                    if self.in_bounds(n) && self.height[i] > self.height[self.index(n)] + 1 {
                        self.height[i] = self.height[self.index(n)] + 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// Lakes where the noise is lowest: water on the plains' layer over a
    /// sandy bed on the layer below. Not at the spawn, and not where the
    /// relaxed terrain is still high.
    fn lakes(&mut self) {
        let mut sorted = self.noise.clone();
        sorted.sort_by(f32::total_cmp);
        let share = self.params.water_share.clamp(0.0, 0.9);
        let level = sorted[(share * (sorted.len() - 1) as f32) as usize];
        let (bed, plains) = (self.over(0), self.over(1));
        for t in self.tiles() {
            let i = self.index(t);
            let low = share > 0.0 && self.noise[i] <= level && self.height[i] == 1;
            if low && self.interior(t) && !self.near_spawn(t, 2) {
                self.layers[bed][i] = Tile::Whole(Part::Floor(self.floors.bed));
                self.layers[plains][i] = Tile::WATER;
            }
        }
    }

    /// Rivers: from a high tile, downhill by the noise (with a little
    /// wandering) to a lake, the edge, or exhaustion, carving the terrain
    /// down to the plains along the way. Every `ford_spacing` tiles a ford
    /// is left as land.
    fn rivers(&mut self) {
        let plains = self.over(1);
        let bed = self.over(0);
        let water = |w: &World, t: [i32; 2]| w.layers[plains][w.index(t)] == Tile::WATER;
        for _ in 0..self.params.rivers {
            let highs: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| {
                    self.interior(t) && self.height[self.index(t)] >= 2 && !self.near_spawn(t, 4)
                })
                .collect();
            if highs.is_empty() {
                break;
            }
            let mut at = highs[self.rng.range(0, highs.len() - 1)];
            let mut path = vec![at];
            let mut visited = vec![false; self.cols * self.rows];
            visited[self.index(at)] = true;
            for _ in 0..(self.cols + self.rows) * 2 {
                // Lowest neighbor by noise, jittered so rivers meander.
                let mut best: Option<([i32; 2], f32)> = None;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = [at[0] + dx, at[1] + dy];
                        if (dx, dy) == (0, 0) || !self.in_bounds(n) || visited[self.index(n)] {
                            continue;
                        }
                        let score = self.noise[self.index(n)] + self.rng.unit() * 0.08;
                        if best.is_none_or(|(_, s)| score < s) {
                            best = Some((n, score));
                        }
                    }
                }
                let Some((next, _)) = best else {
                    break;
                };
                at = next;
                visited[self.index(at)] = true;
                if !self.interior(at) || water(self, at) {
                    break;
                }
                path.push(at);
            }
            // Carve: the river's columns become plains with water on them,
            // and their neighbors are lowered to keep steps to one.
            for (n, &t) in path.iter().enumerate() {
                if self.near_spawn(t, 2) {
                    continue;
                }
                let i = self.index(t);
                self.height[i] = 1;
                let ford = self.params.ford_spacing > 0
                    && n % self.params.ford_spacing == self.params.ford_spacing / 2;
                if !ford {
                    self.layers[bed][i] = Tile::Whole(Part::Floor(self.floors.bed));
                    self.layers[plains][i] = Tile::WATER;
                }
            }
            loop {
                let mut changed = false;
                for t in self.tiles() {
                    let i = self.index(t);
                    for side in Side::ALL {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        if self.in_bounds(n) && self.height[i] > self.height[self.index(n)] + 1 {
                            self.height[i] = self.height[self.index(n)] + 1;
                            changed = true;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        // A lone tile of land with water on every side becomes water too.
        for t in self.tiles() {
            let i = self.index(t);
            if self.layers[plains][i] == Tile::WATER || !self.interior(t) || self.near_spawn(t, 2) {
                continue;
            }
            let landlocked = Side::ALL.into_iter().all(|side| {
                let [dx, dy] = side.offset();
                water(self, [t[0] + dx, t[1] + dy])
            });
            if landlocked && self.height[i] == 1 {
                self.layers[bed][i] = Tile::Whole(Part::Floor(self.floors.bed));
                self.layers[plains][i] = Tile::WATER;
            }
        }
    }

    /// Write the terrain into the overworld layers: for each column, rock
    /// below its height, floor at it, void above. Water placed already is
    /// kept. The lowest layer is the lake beds and rock.
    fn raise_terrain(&mut self) {
        let n = self.params.overworld_layers;
        for t in self.tiles() {
            let i = self.index(t);
            let h = self.height[i];
            let floor = match h {
                1 => self.floors.plains,
                2 => self.floors.hills,
                _ => self.floors.mountains,
            };
            for k in 0..n {
                let layer = self.over(k);
                if self.layers[layer][i] == Tile::WATER
                    || self.layers[layer][i].floor_id() == Some(self.floors.bed)
                {
                    continue;
                }
                self.layers[layer][i] = if k < h {
                    Tile::Whole(self.rock)
                } else if k == h {
                    Tile::Whole(Part::Floor(floor))
                } else {
                    Tile::VOID
                };
            }
            if !self.interior(t) {
                // The border is rock all the way up.
                for k in 0..n {
                    let layer = self.over(k);
                    self.layers[layer][i] = Tile::Whole(self.rock);
                }
            }
        }
    }

    /// Can a ramp go at column `t`, down from its height `h` to a neighbor
    /// at `h - 1`? Land on both ends.
    fn ramp_fits(&self, t: [i32; 2]) -> bool {
        let i = self.index(t);
        let h = self.height[i];
        if h < 2 || !self.interior(t) || self.near_spawn(t, 0) {
            return false;
        }
        if !self.layers[self.over(h)][i].is_floor() {
            return false;
        }
        Side::ALL.into_iter().any(|side| {
            let [dx, dy] = side.offset();
            let n = [t[0] + dx, t[1] + dy];
            self.in_bounds(n)
                && self.height[self.index(n)] == h - 1
                && self.layers[self.over(h - 1)][self.index(n)].is_floor()
        })
    }

    /// A ramp at column `t`: stairs down on its own layer, stairs up in the
    /// cliff face below, where the lower ground reaches it.
    fn place_ramp(&mut self, t: [i32; 2]) {
        let i = self.index(t);
        let h = self.height[i];
        let floor = |w: &World, k: usize| match k {
            1 => w.floors.plains,
            2 => w.floors.hills,
            _ => w.floors.mountains,
        };
        let (upper, lower) = (self.over(h), self.over(h - 1));
        let (up_floor, down_floor) = (floor(self, h), floor(self, h - 1));
        self.layers[upper][i] = Tile::Stairs(Dir::Down, up_floor);
        self.layers[lower][i] = Tile::Stairs(Dir::Up, down_floor);
    }

    /// Ramps along every cliff, `ramp_spacing` apart (none if that's 0).
    fn ramps(&mut self) {
        let spacing = self.params.ramp_spacing as i32;
        if spacing == 0 {
            return;
        }
        let mut placed: Vec<[i32; 2]> = Vec::new();
        for t in self.tiles() {
            if !self.ramp_fits(t) {
                continue;
            }
            let h = self.height[self.index(t)];
            let crowded = placed.iter().any(|p| {
                (p[0] - t[0]).abs().max((p[1] - t[1]).abs()) < spacing
                    && self.height[self.index(*p)] == h
            });
            if !crowded {
                self.place_ramp(t);
                placed.push(t);
            }
        }
    }

    // --- The underworld --------------------------------------------------

    /// Caves with dungeon rooms on every underworld layer, pools in the
    /// caves, and stairs between the layers.
    fn underworld(&mut self) {
        let m = self.params.underworld_layers;
        let seed = self.rng.range(0, u16::MAX as usize) as u32;
        // Each cave is grown around a seed tile, so there's floor near
        // where the stairs from above (or the entrances) will arrive.
        for k in 0..m {
            let around = [
                self.rng.range(4, self.cols - 5) as i32,
                self.rng.range(4, self.rows - 5) as i32,
            ];
            self.layers[k] = cave(
                seed ^ (k as u32).wrapping_mul(0x9E37_79B9),
                self.cols,
                self.rows,
                around,
                self.rock,
                Part::Floor(self.floors.cave),
            );
            self.rooms(k);
            self.pools(k);
        }
        // Stairs between each underworld layer and the one below, where
        // both have floor all around; cleared if there's no such spot.
        let interior: Vec<[i32; 2]> = self
            .tiles()
            .filter(|&[x, y]| {
                x >= 2 && y >= 2 && (x as usize) + 2 < self.cols && (y as usize) + 2 < self.rows
            })
            .collect();
        for upper in (1..m).rev() {
            let clear = |w: &World, k: usize, [x, y]: [i32; 2]| {
                (-1..=1)
                    .all(|dy| (-1..=1).all(|dx| w.layers[k][w.index([x + dx, y + dy])].is_floor()))
            };
            let mut candidates: Vec<[i32; 2]> = interior
                .iter()
                .copied()
                .filter(|&t| clear(self, upper, t) && clear(self, upper - 1, t))
                .collect();
            if candidates.is_empty() {
                let pool: Vec<[i32; 2]> = interior
                    .iter()
                    .copied()
                    .filter(|&t| self.layers[upper][self.index(t)].is_floor())
                    .collect();
                if let Some(&at) = pool.get(self.rng.range(0, pool.len().max(1) - 1)) {
                    for k in [upper - 1, upper] {
                        self.clear_pocket(k, at);
                    }
                    candidates.push(at);
                }
            }
            for _ in 0..self.params.stairs {
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
            Part::Wall(tileset::wall(ROOM_WALL)),
            Part::Floor(self.floors.room),
        );
        let mut placed: Vec<[i32; 4]> = Vec::new();
        for _ in 0..self.params.rooms * 10 {
            if placed.len() >= self.params.rooms {
                break;
            }
            let (w, h) = (self.rng.range(5, 9) as i32, self.rng.range(4, 7) as i32);
            if w as usize + 4 >= self.cols || h as usize + 4 >= self.rows {
                continue;
            }
            let x = self.rng.range(2, self.cols - w as usize - 2) as i32;
            let y = self.rng.range(2, self.rows - h as usize - 2) as i32;
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
                if !self.interior(at) {
                    break;
                }
                let i = self.index(at);
                if self.layers[k][i].is_floor() || self.layers[k][i] == Tile::WATER {
                    break;
                }
                self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.cave));
            }
        }
    }

    /// Pools: the lowest-noise cave floor, where all its neighbors are
    /// floor too (so nothing narrow is blocked), up to `pool_share` of the
    /// floor.
    fn pools(&mut self, k: usize) {
        let share = self.params.pool_share.clamp(0.0, 0.5);
        if share <= 0.0 {
            return;
        }
        let seed = self.rng.range(0, u16::MAX as usize) as u32;
        let mut open: Vec<([i32; 2], f32)> = self
            .tiles()
            .filter(|&t| {
                let cave = self.floors.cave;
                self.layers[k][self.index(t)].floor_id() == Some(cave)
                    && (-1..=1).all(|dy| {
                        (-1..=1).all(|dx| {
                            let n = [t[0] + dx, t[1] + dy];
                            self.in_bounds(n) && self.layers[k][self.index(n)].is_floor()
                        })
                    })
            })
            .map(|t| (t, fbm(seed, t[0] as f32 / 6.0, t[1] as f32 / 6.0)))
            .collect();
        open.sort_by(|a, b| a.1.total_cmp(&b.1));
        let count = (open.len() as f32 * share) as usize;
        for (t, _) in open.into_iter().take(count) {
            let i = self.index(t);
            self.layers[k][i] = Tile::WATER;
        }
    }

    /// Entrances: a cellar in the lowest overworld layer under a plains
    /// tile, with stairs down from the surface and, beside them, stairs on
    /// down into the top cave (cleared around where they arrive).
    fn entrances(&mut self) {
        let (plains, cellar, cave) = (self.over(1), self.over(0), self.top_cave());
        let plains_floor = self.floors.plains;
        for _ in 0..self.params.entrances {
            let spots: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| {
                    // The cellar's footprint: plains all over it, rock
                    // under all of it.
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

    // --- Connectivity ----------------------------------------------------

    /// Which tiles of each layer the player can walk to from the spawn,
    /// straight from the tiles (no [`Level`] needed): walls and void
    /// block, stairs join layers. Also each layer's movement grid.
    fn reachable(&self) -> (Vec<Vec<bool>>, Vec<Vec<Solid>>) {
        let (cols, rows) = (self.cols, self.rows);
        let grids: Vec<Vec<Solid>> = self
            .layers
            .iter()
            .map(|tiles| {
                tiles
                    .iter()
                    .map(|t| Solid::from_quarters(t.wall().quarters() | t.void_quarters(), true))
                    .collect()
            })
            .collect();
        let depth = self.layers.len();
        let mut seeds: Vec<Vec<[i32; 2]>> = vec![Vec::new(); depth];
        seeds[self.over(1)].push(self.spawn);
        let mut reached = vec![vec![false; cols * rows]; depth];
        loop {
            let mut grew = false;
            for k in 0..depth {
                if seeds[k].is_empty() {
                    continue;
                }
                let more = reachable_from(&grids[k], cols, rows, &std::mem::take(&mut seeds[k]));
                for i in 0..cols * rows {
                    if more[i] && !reached[k][i] {
                        reached[k][i] = true;
                        grew = true;
                        let at = [(i % cols) as i32, (i / cols) as i32];
                        match self.layers[k][i].stairs() {
                            Some(Dir::Down) if k > 0 => seeds[k - 1].push(at),
                            Some(Dir::Up) if k + 1 < depth => seeds[k + 1].push(at),
                            _ => {}
                        }
                    }
                }
            }
            if !grew {
                return (reached, grids);
            }
        }
    }

    /// The connected piece of plains floor `start` is on (across sides).
    fn plains_region(&self, start: [i32; 2]) -> Vec<[i32; 2]> {
        let plains = self.over(1);
        let mut seen = vec![false; self.cols * self.rows];
        let mut stack = vec![start];
        let mut out = Vec::new();
        seen[self.index(start)] = true;
        while let Some(t) = stack.pop() {
            out.push(t);
            for side in Side::ALL {
                let [dx, dy] = side.offset();
                let n = [t[0] + dx, t[1] + dy];
                if self.in_bounds(n)
                    && !seen[self.index(n)]
                    && self.layers[plains][self.index(n)].is_floor()
                {
                    seen[self.index(n)] = true;
                    stack.push(n);
                }
            }
        }
        out
    }

    /// The water tiles of the shortest way from `start` (cut-off land)
    /// across water to land that's `reached`, within `reach` tiles.
    fn ford_path(&self, start: [i32; 2], reached: &[bool], reach: usize) -> Option<Vec<[i32; 2]>> {
        let plains = self.over(1);
        let mut from: Vec<Option<[i32; 2]>> = vec![None; self.cols * self.rows];
        let mut seen = vec![false; self.cols * self.rows];
        let mut frontier = vec![start];
        seen[self.index(start)] = true;
        for _ in 0..reach {
            let mut next = Vec::new();
            for t in frontier {
                for side in Side::ALL {
                    let [dx, dy] = side.offset();
                    let n = [t[0] + dx, t[1] + dy];
                    if !self.in_bounds(n) || seen[self.index(n)] {
                        continue;
                    }
                    let tile = self.layers[plains][self.index(n)];
                    seen[self.index(n)] = true;
                    from[self.index(n)] = Some(t);
                    if tile.is_floor() && reached[self.index(n)] {
                        // Walk back, collecting the water crossed.
                        let mut path = Vec::new();
                        let mut at = t;
                        while at != start {
                            path.push(at);
                            at = from[self.index(at)].unwrap();
                        }
                        return Some(path);
                    }
                    if tile == Tile::WATER {
                        next.push(n);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        None
    }

    /// Join what the player can't reach from the spawn: fords across water
    /// between separated land, ramps up to unreached ground (if ramps are
    /// allowed), and rock filling unreached underworld floor.
    fn connect(&mut self) {
        /// A cut-off piece of plains this small sinks into the lake around
        /// it rather than getting a causeway.
        const ISLET: usize = 16;
        let m = self.params.underworld_layers;
        let plains = self.over(1);
        let bed = self.over(0);
        // Tiles given up on, per layer: nothing could join them.
        let mut skipped = vec![vec![false; self.cols * self.rows]; self.layers.len()];
        for _ in 0..256 {
            let (reached, _) = self.reachable();
            let mut changed = false;

            // A ford: from a piece of cut-off plains, the shortest way
            // through water to plains that are reached. A short one if
            // there is one; else a small piece becomes lake, a large one
            // gets a causeway however long.
            let cut_off = self.tiles().find(|&t| {
                let i = self.index(t);
                self.layers[plains][i].is_floor()
                    && !reached[plains][i]
                    && self.height[i] == 1
                    && !skipped[plains][i]
            });
            if let Some(start) = cut_off {
                let far = self.cols + self.rows;
                let region = self.plains_region(start);
                let path = self.ford_path(start, &reached[plains], 8).or_else(|| {
                    (region.len() > ISLET)
                        .then(|| self.ford_path(start, &reached[plains], far))
                        .flatten()
                });
                if let Some(path) = path {
                    for t in path {
                        let i = self.index(t);
                        self.layers[plains][i] = Tile::Whole(Part::Floor(self.floors.bed));
                        self.layers[bed][i] = Tile::Whole(self.rock);
                    }
                    changed = true;
                } else if region.len() <= ISLET
                    && region
                        .iter()
                        .all(|t| self.interior(*t) && !self.near_spawn(*t, 2))
                {
                    for t in region {
                        let i = self.index(t);
                        self.layers[plains][i] = Tile::WATER;
                        self.layers[bed][i] = Tile::Whole(Part::Floor(self.floors.bed));
                    }
                    changed = true;
                } else {
                    for t in region {
                        skipped[plains][self.index(t)] = true;
                    }
                    changed = true;
                }
            }

            // Ramps: an unreached column that could ramp down to reached
            // ground, or reached ground above unreached ground.
            if self.params.ramp_spacing > 0 && !changed {
                let candidate = self.tiles().find(|&t| {
                    if !self.ramp_fits(t) {
                        return false;
                    }
                    let i = self.index(t);
                    let h = self.height[i];
                    let here = reached[self.over(h)][i];
                    Side::ALL.into_iter().any(|side| {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        self.in_bounds(n)
                            && self.height[self.index(n)] == h - 1
                            && self.layers[self.over(h - 1)][self.index(n)].is_floor()
                            && reached[self.over(h - 1)][self.index(n)] != here
                    })
                });
                if let Some(t) = candidate {
                    self.place_ramp(t);
                    changed = true;
                }
            }
            // High ground rising straight out of water, with no land below
            // to ramp down onto: give it a shore, one tile of water made
            // bed, and a causeway from there to land that's reached; or
            // give that hill up.
            if self.params.ramp_spacing > 0 && !changed {
                let hills = self.over(2);
                let shore = self.tiles().find_map(|t| {
                    let i = self.index(t);
                    let h = self.height[i];
                    if h != 2
                        || !self.interior(t)
                        || reached[hills][i]
                        || skipped[hills][i]
                        || !self.layers[hills][i].is_floor()
                    {
                        return None;
                    }
                    let lower = |w: &World, side: Side| {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        (w.in_bounds(n) && w.height[w.index(n)] == 1).then_some(n)
                    };
                    let has_land = Side::ALL.into_iter().any(|side| {
                        lower(self, side)
                            .is_some_and(|n| self.layers[plains][self.index(n)].is_floor())
                    });
                    if has_land {
                        return None;
                    }
                    Side::ALL
                        .into_iter()
                        .find_map(|side| {
                            lower(self, side)
                                .filter(|n| self.layers[plains][self.index(*n)] == Tile::WATER)
                        })
                        .map(|n| (t, n))
                });
                if let Some((t, n)) = shore {
                    let far = self.cols + self.rows;
                    match self.ford_path(n, &reached[plains], far) {
                        Some(path) => {
                            for t in path.into_iter().chain([n]) {
                                let i = self.index(t);
                                self.layers[plains][i] = Tile::Whole(Part::Floor(self.floors.bed));
                                self.layers[bed][i] = Tile::Whole(self.rock);
                            }
                        }
                        None => {
                            skipped[hills][self.index(t)] = true;
                        }
                    }
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        // Underworld floor nobody can reach becomes rock.
        let (reached, grids) = self.reachable();
        let rock = self.rock;
        for (k, reached) in reached.iter().enumerate().take(m) {
            for (i, tile) in self.layers[k].iter_mut().enumerate() {
                let open = grids[k][i] != Solid::Full;
                if open && !reached[i] && *tile != Tile::WATER {
                    *tile = Tile::Whole(rock);
                }
            }
        }
    }
}

/// Value noise with three octaves, in [0, 1): smooth hills of wavelength
/// about one unit of `(x, y)`.
fn fbm(seed: u32, x: f32, y: f32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let (mut fx, mut fy) = (x, y);
    for octave in 0..3u32 {
        sum += value_noise(seed.wrapping_add(octave.wrapping_mul(0x51ED_270B)), fx, fy) * amp;
        norm += amp;
        amp *= 0.5;
        fx *= 2.0;
        fy *= 2.0;
    }
    (sum / norm).clamp(0.0, 0.999_99)
}

/// Bilinear value noise: a hashed value at each lattice point, smoothly
/// interpolated between.
fn value_noise(seed: u32, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (smooth(tx), smooth(ty));
    let at = |dx: f32, dy: f32| lattice(seed, (x0 + dx) as i32, (y0 + dy) as i32);
    let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * sx;
    let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * sx;
    top + (bottom - top) * sy
}

fn lattice(seed: u32, x: i32, y: i32) -> f32 {
    let mut h = seed ^ (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
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
                    let count = (-1..=1)
                        .flat_map(|dy| (-1..=1).map(move |dx| (dx, dy)))
                        .filter(|&(dx, dy)| solid(x + dx, y + dy))
                        .count();
                    edge(x, y) || count >= 5
                })
                .collect();
        }
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

/// Which tiles of each layer the player can walk to from `start` on layer
/// `top`, across each layer's movement grid and up and down stairs.
pub fn reachable_across(level: &Level, top: usize, start: [i32; 2]) -> Vec<Vec<bool>> {
    let (cols, rows) = (level.cols(), level.rows());
    let grids: Vec<Vec<Solid>> = (0..level.depth())
        .map(|k| {
            let m = level.movement(k);
            (0..cols * rows)
                .map(|i| m.solid((i % cols) as i32, (i / cols) as i32))
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
                    match level.layer(k).tile(at[0], at[1]).stairs() {
                        Some(Dir::Down) if k > 0 => seeds[k - 1].push(at),
                        Some(Dir::Up) if k + 1 < level.depth() => seeds[k + 1].push(at),
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

/// Which tiles can be reached from any of `starts` across open sides,
/// given where each tile is solid.
pub fn reachable_from(tiles: &[Solid], cols: usize, rows: usize, starts: &[[i32; 2]]) -> Vec<bool> {
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Solid::Full
        } else {
            tiles[y as usize * cols + x as usize]
        }
    };
    let mut seen = vec![false; cols * rows];
    let mut stack = starts.to_vec();
    while let Some([x, y]) = stack.pop() {
        if tile(x, y) == Solid::Full || seen[y as usize * cols + x as usize] {
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
pub fn smooth(tiles: &mut [Tile], cols: usize, rows: usize) {
    let snapshot = tiles.to_vec();
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Tile::WALL
        } else {
            snapshot[y as usize * cols + x as usize]
        }
    };
    for y in 0..rows as i32 {
        for x in 0..cols as i32 {
            let here = tile(x, y);
            if !here.is_floor() {
                continue;
            }
            let solid = |side: Side| {
                let [dx, dy] = side.offset();
                tile(x + dx, y + dy).wall().covers(side.opposite())
            };
            for corner in Corner::CLOCKWISE {
                let [a, b] = corner.sides();
                let [c, d] = corner.opposite().sides();
                let [ox, oy] = [c, d]
                    .map(|s| s.offset())
                    .into_iter()
                    .fold([0, 0], |acc, o| [acc[0] + o[0], acc[1] + o[1]]);
                if solid(a) && solid(b) && !solid(c) && !solid(d) && tile(x + ox, y + oy).is_floor()
                {
                    // The wall of the neighbor it continues, over this floor.
                    let [ax, ay] = a.offset();
                    let wall: WallId = tile(x + ax, y + ay).wall_id().unwrap_or(0);
                    let floor = here.floor_id().unwrap_or(0);
                    tiles[y as usize * cols + x as usize] =
                        Tile::split(corner, Part::Wall(wall), Part::Floor(floor));
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::needless_range_loop)]

    use super::*;

    const SPAWN: [f32; 2] = [325.0, 205.0];
    const COLS: usize = 64;
    const ROWS: usize = 40;

    fn world(seed: u32) -> Level {
        generate_level(seed, COLS, ROWS, SPAWN, &Params::default())
    }

    fn spawn_tile() -> [i32; 2] {
        [(SPAWN[0] / TILE) as i32, (SPAWN[1] / TILE) as i32]
    }

    /// Each overworld column is rock, then one floor (or water on the
    /// plains, with a bed beneath), then void; and the underworld is
    /// enclosed.
    #[test]
    fn columns_follow_the_heightmap() {
        let p = Params::default();
        for seed in 0..20 {
            let level = world(seed);
            assert_eq!(level.depth(), p.underworld_layers + p.overworld_layers);
            let under = p.underworld_layers;
            for y in 1..ROWS as i32 - 1 {
                for x in 1..COLS as i32 - 1 {
                    let column: Vec<Tile> = (under..level.depth())
                        .map(|k| level.layer(k).tile(x, y))
                        .collect();
                    // Ramps (stairs in a cliff) and entrance cellars (floor
                    // under the plains) are the exceptions to the rule.
                    let cellar = column[0]
                        .floor_id()
                        .is_some_and(|f| f != tileset::floor(BED));
                    if column.iter().any(|t| t.stairs().is_some()) || cellar {
                        continue;
                    }
                    let surface = column
                        .iter()
                        .position(|t| t.wall() != Solid::Full)
                        .unwrap_or_else(|| panic!("seed {seed}: ({x}, {y}) is rock to the top"));
                    for (k, t) in column.iter().enumerate() {
                        match k.cmp(&surface) {
                            std::cmp::Ordering::Less => assert_eq!(
                                t.wall(),
                                Solid::Full,
                                "seed {seed} ({x}, {y}) layer {k}"
                            ),
                            std::cmp::Ordering::Equal => {
                                let ok = t.floor_id().is_some()
                                    || *t == Tile::WATER
                                    || t.stairs().is_some();
                                assert!(ok, "seed {seed} ({x}, {y}): {t:?} at the surface");
                            }
                            std::cmp::Ordering::Greater => {
                                // Above the surface: void, or the bed's water.
                                let ok = *t == Tile::VOID
                                    || *t == Tile::WATER
                                    || (t.void_quarters() != 0);
                                assert!(
                                    ok,
                                    "seed {seed} ({x}, {y}) layer {k}: {t:?} above the surface"
                                );
                            }
                        }
                    }
                }
            }
            // The underworld's border is rock, as is the overworld's.
            for k in 0..level.depth() {
                for x in 0..COLS as i32 {
                    assert_eq!(level.layer(k).solid(x, 0), Solid::Full, "seed {seed}");
                    assert_eq!(level.layer(k).solid(x, ROWS as i32 - 1), Solid::Full);
                }
            }
        }
    }

    /// Every cave and dungeon floor is reachable (the rest is filled in),
    /// and so is nearly all the ground above: a hill ringed by rivers
    /// with no way onto it is left as scenery.
    #[test]
    fn everything_walkable_is_reachable_with_ramps() {
        let p = Params::default();
        let (mut ground, mut cut_off) = (0, 0);
        for seed in 0..20 {
            let level = world(seed);
            let top = spawn_layer(&p);
            assert!(
                !level
                    .movement(top)
                    .collides([SPAWN[0] - 4.0, SPAWN[1] - 4.0], [8.0, 8.0]),
                "seed {seed}"
            );
            let reached = reachable_across(&level, top, spawn_tile());
            for k in 0..level.depth() {
                let m = level.movement(k);
                for i in 0..COLS * ROWS {
                    let (x, y) = ((i % COLS) as i32, (i / COLS) as i32);
                    let open = m.solid(x, y) != Solid::Full;
                    // A lake bed is floor, but under water: nobody walks there.
                    let bed = k + 1 < level.depth() && level.layer(k + 1).tile(x, y) == Tile::WATER;
                    if !open || bed {
                        continue;
                    }
                    if k < p.underworld_layers {
                        assert!(
                            reached[k][i],
                            "seed {seed}: layer {k} tile {i} cut off underground"
                        );
                    } else {
                        ground += 1;
                        cut_off += !reached[k][i] as usize;
                    }
                }
            }
        }
        assert!(
            cut_off * 100 < ground,
            "{cut_off} of {ground} ground tiles cut off"
        );
    }

    #[test]
    fn without_ramps_the_hills_are_out_of_reach() {
        let params = Params {
            ramp_spacing: 0,
            ..Params::default()
        };
        let mut cut_off = 0;
        for seed in 0..10 {
            let level = generate_level(seed, COLS, ROWS, SPAWN, &params);
            let top = spawn_layer(&params);
            let reached = reachable_across(&level, top, spawn_tile());
            // No ramps anywhere: no stairs on the overworld but the entrances.
            let hills = top + 1;
            for i in 0..COLS * ROWS {
                let t = level
                    .layer(hills)
                    .tile((i % COLS) as i32, (i / COLS) as i32);
                assert!(t.stairs().is_none(), "seed {seed}: a ramp");
                if t.is_floor() && !reached[hills][i] {
                    cut_off += 1;
                }
            }
        }
        assert!(cut_off > 0, "some hilltop should be unreachable");
    }

    #[test]
    fn water_lies_on_the_plains_over_sand_and_never_at_the_spawn() {
        let p = Params::default();
        let (bed, plains) = (p.underworld_layers, p.underworld_layers + 1);
        let mut water = 0;
        for seed in 0..20 {
            let level = world(seed);
            for y in 0..ROWS as i32 {
                for x in 0..COLS as i32 {
                    if level.layer(plains).tile(x, y) == Tile::WATER {
                        water += 1;
                        assert_eq!(
                            level.layer(bed).tile(x, y).floor_id(),
                            Some(tileset::floor(BED)),
                            "seed {seed}"
                        );
                        let s = spawn_tile();
                        assert!((x - s[0]).abs().max((y - s[1]).abs()) > SPAWN_CLEARANCE);
                    }
                }
            }
        }
        assert!(water > 20 * 100, "{water} water tiles in 20 levels");
    }

    #[test]
    fn stairs_come_in_pairs() {
        for seed in 0..20 {
            let level = world(seed);
            let mut pairs = 0;
            for k in 1..level.depth() {
                for y in 0..ROWS as i32 {
                    for x in 0..COLS as i32 {
                        if level.layer(k).tile(x, y).stairs() == Some(Dir::Down) {
                            pairs += 1;
                            assert_eq!(
                                level.layer(k - 1).tile(x, y).stairs(),
                                Some(Dir::Up),
                                "seed {seed} ({x}, {y}) layer {k}"
                            );
                        }
                    }
                }
            }
            assert!(pairs >= 2, "seed {seed}: {pairs} staircases");
        }
    }

    #[test]
    fn parameters_round_trip_and_default_when_missing() {
        let p: Params = serde_json::from_str(r#"{"rivers": 5, "water_share": 0.3}"#).unwrap();
        assert_eq!(p.rivers, 5);
        assert_eq!(p.overworld_layers, Params::default().overworld_layers);
        let back: Params = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        // Out-of-range layer counts are clamped, not fatal.
        let many = Params {
            overworld_layers: 20,
            underworld_layers: 20,
            ..Params::default()
        };
        let level = generate_level(1, 32, 20, [165.0, 105.0], &many);
        assert!(level.depth() <= MAX_LAYERS);
    }

    #[test]
    fn smoothing_fills_stair_steps_only() {
        let rows = ["#####", "#....", "#....", "#...."];
        let mut tiles: Vec<Tile> = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| if c == '#' { Tile::WALL } else { Tile::FLOOR })
            .collect();
        smooth(&mut tiles, 5, 4);
        assert_eq!(tiles[5 + 1], Tile::slope(Corner::NW));
        assert_eq!(tiles[2 * 5 + 2], Tile::FLOOR);

        let bend = ["#####", "#..##", "##.##", "#####"];
        let mut tiles: Vec<Tile> = bend
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| if c == '#' { Tile::WALL } else { Tile::FLOOR })
            .collect();
        let before = tiles.clone();
        smooth(&mut tiles, 5, 4);
        assert_eq!(tiles, before);
    }
}
