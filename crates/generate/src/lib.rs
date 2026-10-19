//! The world generator for the engine in `goblin_nightclub_core`: one kind of level,
//! built in two groups of layers, bottom to top:
//!
//! - **The underworld**, [`Params::underworld_layers`] layers of caves and
//!   dungeon rooms carved out of solid rock, joined by stairs, with pools
//!   of water and holes down to the cave below. Enclosed on every side.
//! - **The overworld**, [`Params::overworld_layers`] layers of terrain from
//!   a heightmap: each column is rock up to its height, floor at it, and
//!   void above, so from above you look down onto plains, hills and
//!   mountains, and a step in height is a cliff. Cliffs are climbed by
//!   ramps (a staircase in the cliff face), placed along them every so
//!   often if [`Params::ramp_spacing`] allows. Lakes fill the lowest
//!   ground and rivers run down to them, with fords where a river would
//!   otherwise cut the land in two. Holes and canyons open onto a stony
//!   floor one layer down.
//!
//! Water rests on solid ground: under every water tile, the tile below and
//! its eight neighbors are wall or water, never floor or void (see
//! [`World::rests`]). That holds for lakes, rivers and pools alike.
//!
//! The two meet at entrances: a cellar under the plains with a staircase
//! down from the surface and another down into the top cave.
//!
//! Everything the player can walk to from the spawn is kept; floor in the
//! underworld that can't be reached is filled with rock (ground in the
//! overworld stays, reachable or not: whether every hilltop has a ramp is
//! up to the parameters). Water and holes never block the way between two
//! pieces of land that nothing else joins: fords cross water, and bridges
//! a tile wide cross holes. Holes keep a tile clear of water.
//!
//! The materials are named in the generator's tileset, `assets/tileset.json`
//! (see [`tileset`]), which every generated level carries along with the
//! sprites the player and NPCs wear.

use std::sync::OnceLock;

use goblin_nightclub_core::layer::{Corner, Dir, FloorId, Part, Side, Solid, TILE, Tile};
use goblin_nightclub_core::level::{Level, MAX_LAYERS};
use goblin_nightclub_core::level_file::{LevelFile, Spawn};
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset::Tileset;
use serde::{Deserialize, Serialize};

/// What to generate. Every field has a default; see [`Params::default`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// Layers of terrain. Heights run from 1 (plains) to this minus 1, so
    /// 2 gives flat plains only, 3 adds hills, 4 adds mountains, and more
    /// stack higher peaks. The lowest layer is rock under the lakes.
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
    /// Holes (round pits) and canyons (long winding rifts) opened in the
    /// plains and in each cave, as void. Land they cut off is joined up
    /// again by bridges a tile wide.
    pub holes: usize,
    pub canyons: usize,
    /// A hole's radius in tiles, and a canyon's width and length, each
    /// chosen between the two bounds (inclusive) for every hole or canyon.
    pub hole_radius: [usize; 2],
    pub canyon_width: [usize; 2],
    pub canyon_length: [usize; 2],
    /// How many layers down a hole or canyon goes, chosen between the
    /// bounds for each: 1 opens onto the layer below, 2 through it onto
    /// the next, and so on. It stops short where the layer below has
    /// something other than rock or cave floor (a room, stairs), or water
    /// within a tile.
    pub hole_depth: [usize; 2],
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
            entrances: 4,
            rooms: 3,
            stairs: 2,
            pool_share: 0.06,
            holes: 40,
            canyons: 5,
            hole_radius: [2, 4],
            canyon_width: [1, 2],
            canyon_length: [10, 24],
            hole_depth: [1, 4],
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
/// Fords across water.
const BED: &str = "sand";
const ROCK: &str = "rock";
const CAVE_FLOOR: &str = "dirt";
const ROOM_FLOOR: &str = "planks";
const ROOM_WALL: &str = "brick";
const BRIDGE: &str = "planks";
/// The floor of a canyon, under a hole in the plains.
const CANYON_FLOOR: &str = "stone";
const WATER: &str = "water";
/// Sprites, for the level file.
const PLAYER_SPRITE: &str = "player";
const NPC_SPRITE: &str = "npc";

/// A blank level for the editor to start from: `depth` layers of plain
/// floor with the generator's tileset and sprites.
pub fn template(cols: usize, rows: usize, depth: usize) -> LevelFile {
    LevelFile::blank(
        cols,
        rows,
        depth,
        tileset().clone(),
        PLAYER_SPRITE,
        NPC_SPRITE,
    )
}

/// The tileset generated levels use: the placeholder sheet's floors,
/// walls, water and sprites, from `assets/tileset.json`.
pub fn tileset() -> &'static Tileset {
    static TILESET: OnceLock<Tileset> = OnceLock::new();
    TILESET.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/tileset.json"))
            .expect("assets/tileset.json is a valid tileset")
    })
}

/// Generate a level from `seed`, `cols` x `rows` tiles a layer, the player
/// starting on the plains at `spawn` (pixels).
pub fn generate_level(
    seed: u32,
    cols: usize,
    rows: usize,
    spawn: [f32; 2],
    params: &Params,
) -> Level {
    world(seed, cols, rows, spawn, params).level()
}

/// Generate a level as a complete level file: layers, spawn, ground level,
/// tileset and sprites, the same as [`generate_level`] gives in memory. The
/// spawn is the tile under `spawn` (pixels), moved inside the level if
/// need be.
pub fn world(seed: u32, cols: usize, rows: usize, spawn: [f32; 2], params: &Params) -> LevelFile {
    let spawn = [(spawn[0] / TILE) as i32, (spawn[1] / TILE) as i32];
    let world = World::new(seed, cols, rows, spawn, params);
    let (spawn, params) = (world.spawn, world.params.clone());
    let layers = world.build();
    LevelFile::new(
        cols,
        rows,
        &layers,
        Spawn::at_tile(spawn_layer(&params).min(layers.len() - 1), spawn),
        tileset().clone(),
        PLAYER_SPRITE,
        NPC_SPRITE,
    )
    .with_ground(ground_layer(&params).min(layers.len() - 1))
}

/// The spawn's layer: the plains, the second overworld layer up.
pub fn spawn_layer(params: &Params) -> usize {
    params.underworld_layers.max(1) + 1
}

/// The ground level: the plains, the layer the player starts on. The
/// canyon floors and cellars below it count as underground.
pub fn ground_layer(params: &Params) -> usize {
    spawn_layer(params)
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
    /// A tile of water.
    water: Tile,
    floors: Floors,
}

struct Floors {
    plains: FloorId,
    hills: FloorId,
    mountains: FloorId,
    bed: FloorId,
    cave: FloorId,
    room: FloorId,
    bridge: FloorId,
    canyon: FloorId,
}

impl World {
    fn new(seed: u32, cols: usize, rows: usize, spawn: [i32; 2], params: &Params) -> Self {
        let mut params = params.clone();
        params.underworld_layers = params.underworld_layers.clamp(1, MAX_LAYERS - 2);
        params.overworld_layers = params
            .overworld_layers
            .clamp(2, MAX_LAYERS - params.underworld_layers);
        let depth = params.underworld_layers + params.overworld_layers;
        let t = tileset();
        let wall = |name| t.wall(name).unwrap_or_else(|| panic!("no wall {name:?}"));
        let floor = |name| t.floor(name).unwrap_or_else(|| panic!("no floor {name:?}"));
        let liquid = |name| {
            t.liquid(name)
                .unwrap_or_else(|| panic!("no liquid {name:?}"))
        };
        let rock = Part::Wall(wall(ROCK));
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
            water: Tile::Whole(Part::Liquid(liquid(WATER))),
            floors: Floors {
                plains: floor(PLAINS),
                hills: floor(HILLS),
                mountains: floor(MOUNTAINS),
                bed: floor(BED),
                cave: floor(CAVE_FLOOR),
                room: floor(ROOM_FLOOR),
                bridge: floor(BRIDGE),
                canyon: floor(CANYON_FLOOR),
            },
        }
    }

    /// Build the world: its layers, bottom first, each row by row.
    fn build(mut self) -> Vec<Vec<Tile>> {
        self.terrain();
        self.lakes();
        self.rivers();
        self.raise_terrain();
        self.plains_holes();
        self.ramps();
        self.underworld();
        self.entrances();
        self.connect();
        self.chamfer();
        self.layers
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

    /// Lakes where the noise is lowest: water on the plains' layer, over
    /// the rock of the layer below. Not at the spawn, and not where the
    /// relaxed terrain is still high.
    fn lakes(&mut self) {
        let mut sorted = self.noise.clone();
        sorted.sort_by(f32::total_cmp);
        let share = self.params.water_share.clamp(0.0, 0.9);
        let level = sorted[(share * (sorted.len() - 1) as f32) as usize];
        let plains = self.over(1);
        for t in self.tiles() {
            let i = self.index(t);
            let low = share > 0.0 && self.noise[i] <= level && self.height[i] == 1;
            if low && self.interior(t) && !self.near_spawn(t, 2) && self.rests(plains, t) {
                self.layers[plains][i] = self.water;
            }
        }
    }

    /// Rivers: from a high tile, downhill by the noise (with a little
    /// wandering) to a lake, the edge, or exhaustion, carving the terrain
    /// down to the plains along the way. Every `ford_spacing` tiles a ford
    /// is left as land.
    fn rivers(&mut self) {
        let plains = self.over(1);
        let water = |w: &World, t: [i32; 2]| w.layers[plains][w.index(t)] == w.water;
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
                if !ford && self.rests(plains, t) {
                    self.layers[plains][i] = self.water;
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
            if self.layers[plains][i] == self.water || !self.interior(t) || self.near_spawn(t, 2) {
                continue;
            }
            let landlocked = Side::ALL.into_iter().all(|side| {
                let [dx, dy] = side.offset();
                water(self, [t[0] + dx, t[1] + dy])
            });
            if landlocked && self.height[i] == 1 && self.rests(plains, t) {
                self.layers[plains][i] = self.water;
            }
        }
    }

    /// Write the terrain into the overworld layers: for each column, rock
    /// below its height, floor at it, void above. Water placed already is
    /// kept. The lowest layer is all rock.
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
                if self.layers[layer][i] == self.water {
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

    /// Holes and canyons in the plains, opening onto a stony canyon floor
    /// below. Never on or beside water, and not at the spawn.
    fn plains_holes(&mut self) {
        let plains = self.over(1);
        let plains_floor = self.floors.plains;
        let allowed = move |w: &World, t: [i32; 2]| {
            w.interior(t)
                && !w.near_spawn(t, 2)
                && w.height[w.index(t)] == 1
                && w.layers[plains][w.index(t)].floor_id() == Some(plains_floor)
                && !w.beside_water(plains, t)
        };
        self.carve_voids(plains, &allowed);
    }

    /// Is any tile within one of `t` (itself included) water, in any part,
    /// on layer `k`?
    fn beside_water(&self, k: usize, t: [i32; 2]) -> bool {
        self.beside(k, t, |tile| tile.liquid_quarters() != 0)
    }

    /// Is any tile within one of `t` (itself included) void, in any part,
    /// on layer `k`?
    fn beside_void(&self, k: usize, t: [i32; 2]) -> bool {
        self.beside(k, t, |tile| {
            tile.void_quarters() & !tile.liquid_quarters() != 0
        })
    }

    fn beside(&self, k: usize, t: [i32; 2], f: impl Fn(Tile) -> bool) -> bool {
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                let n = [t[0] + dx, t[1] + dy];
                self.in_bounds(n) && f(self.layers[k][self.index(n)])
            })
        })
    }

    /// Would water on layer `k` at `t` rest on solid ground? The tile under
    /// it and that tile's eight neighbors, on the layer below, must all be
    /// wall or water: nothing open beneath. Below the lowest layer is
    /// bedrock, and so is anything off the edge. Every water tile is
    /// placed only where this holds, and nothing is dug under water
    /// afterward (holes, cellars and stairs keep a tile clear of it).
    fn rests(&self, k: usize, t: [i32; 2]) -> bool {
        let Some(below) = k.checked_sub(1) else {
            return true;
        };
        (-1..=1).all(|dy| {
            (-1..=1).all(|dx| {
                let n = [t[0] + dx, t[1] + dy];
                if !self.in_bounds(n) {
                    return true;
                }
                let tile = self.layers[below][self.index(n)];
                tile.wall().quarters() | tile.liquid_quarters() == 0b1111
            })
        })
    }

    /// Put 45-degree edges where two kinds of tile meet in a stair step
    /// (see [`chamfer_layer`]), on every layer, bottom up: walls into
    /// floor as ever, and now hole rims and shores both ways, and rock into
    /// pools. Never where it would break a rule: not at the spawn, not on
    /// bridges or fords, water only onto solid ground and away from holes,
    /// holes away from water, and the column under a new piece of hole
    /// gets a floor like the rest of the hole's bottom.
    fn chamfer(&mut self) {
        let rock = Tile::Whole(self.rock);
        let canyon = Tile::Whole(Part::Floor(self.floors.canyon));
        let plains = self.over(1);
        // Top down, so the floor put under a new piece of hole is there
        // before the layer below is looked at. Candidates come from the
        // layer as it was; each is checked against the level as it is when
        // its turn comes, so two chamfers can't together break a rule
        // neither breaks alone.
        for k in (0..self.layers.len()).rev() {
            for c in chamfer_layer(&self.layers[k], self.cols, self.rows) {
                let i = c.index;
                let t = [(i % self.cols) as i32, (i / self.cols) as i32];
                if !self.chamfer_allowed(k, t, c.rule)
                    || !apply(&mut self.layers[k], self.cols, self.rows, c)
                {
                    continue;
                }
                // Under a new piece of hole in the plains, the floor the
                // rest of the hole's bottom has (in the caves, the rock
                // stays: a floor there would be cut off).
                if c.rule.from == Kind::Void && k == plains && self.layers[k - 1][i] == rock {
                    self.layers[k - 1][i] = canyon;
                }
            }
        }
    }

    /// May tile `t` on layer `k` be chamfered by `rule` (see
    /// [`World::chamfer`])?
    fn chamfer_allowed(&self, k: usize, t: [i32; 2], rule: Rule) -> bool {
        if self.near_spawn(t, 0) {
            return false;
        }
        let i = self.index(t);
        let floor = self.layers[k][i].floor_id();
        let crossing = floor == Some(self.floors.bridge) || floor == Some(self.floors.bed);
        match rule {
            Rule {
                into: Kind::Floor,
                from: Kind::Void,
            } => {
                // Not over a room or stairs, and clear of water (which a
                // hole keeps a tile from).
                let soft = k == 0 || {
                    let below = self.layers[k - 1][i];
                    below == Tile::Whole(self.rock) || below.is_floor()
                };
                !crossing && soft && !self.beside_water(k, t)
            }
            Rule {
                into: Kind::Floor,
                from: Kind::Liquid,
            } => !crossing && self.rests(k, t) && !self.beside_void(k, t),
            Rule {
                into: Kind::Liquid,
                from: Kind::Floor,
            } => k + 1 >= self.layers.len() || !self.beside_water(k + 1, t),
            _ => true,
        }
    }

    /// Open [`Params::holes`] holes and [`Params::canyons`] canyons as void
    /// on layer `k`, only where `allowed`: holes are rough discs of
    /// [`Params::hole_radius`], canyons winding rifts of
    /// [`Params::canyon_width`] and [`Params::canyon_length`]. Each goes
    /// [`Params::hole_depth`] layers down (see [`World::deepen`]).
    fn carve_voids(&mut self, k: usize, allowed: &dyn Fn(&World, [i32; 2]) -> bool) {
        let p = self.params.clone();
        let between = |w: &mut World, [lo, hi]: [usize; 2]| w.rng.range(lo.min(hi), lo.max(hi));
        let spots: Vec<[i32; 2]> = self.tiles().filter(|&t| allowed(self, t)).collect();
        if spots.is_empty() {
            return;
        }
        let mut features: Vec<(Vec<[i32; 2]>, usize)> = Vec::new();
        for _ in 0..p.holes {
            let c = spots[self.rng.range(0, spots.len() - 1)];
            let r = between(self, p.hole_radius) as i32;
            let mut tiles = Vec::new();
            for dy in -r..=r {
                for dx in -r..=r {
                    // A rough disc: the corners of the square left out.
                    if dx * dx + dy * dy <= r * r + 1 {
                        tiles.push([c[0] + dx, c[1] + dy]);
                    }
                }
            }
            let depth = between(self, p.hole_depth);
            features.push((tiles, depth));
        }
        for _ in 0..p.canyons {
            let mut at = spots[self.rng.range(0, spots.len() - 1)];
            let mut angle = self.rng.unit() * std::f32::consts::TAU;
            let length = between(self, p.canyon_length);
            let (mut fx, mut fy) = (at[0] as f32 + 0.5, at[1] as f32 + 0.5);
            let mut tiles = Vec::new();
            for _ in 0..length {
                // Widened across the direction of travel.
                let width = between(self, p.canyon_width) as i32;
                let side = if angle.cos().abs() > angle.sin().abs() {
                    [0, 1]
                } else {
                    [1, 0]
                };
                for i in 0..width {
                    tiles.push([at[0] + side[0] * i, at[1] + side[1] * i]);
                }
                angle += (self.rng.unit() - 0.5) * 0.8;
                fx += angle.cos();
                fy += angle.sin();
                at = [fx.floor() as i32, fy.floor() as i32];
                if !self.in_bounds(at) || !allowed(self, at) {
                    break;
                }
            }
            let depth = between(self, p.hole_depth);
            features.push((tiles, depth));
        }
        for (tiles, depth) in features {
            let opened: Vec<[i32; 2]> = tiles.into_iter().filter(|&t| allowed(self, t)).collect();
            for &t in &opened {
                let i = self.index(t);
                self.layers[k][i] = Tile::VOID;
            }
            self.deepen(k, &opened, depth);
        }
    }

    /// Take a hole on layer `k` at `tiles` down `depth` layers: the layers
    /// below become void too, as far as they're rock or cave floor with no
    /// water within a tile (a room, stairs or a pool stops it), and the
    /// layer under the last gets a stony floor where it was rock.
    fn deepen(&mut self, k: usize, tiles: &[[i32; 2]], depth: usize) {
        let cave = self.floors.cave;
        for &t in tiles {
            let i = self.index(t);
            let mut bottom = k;
            for j in 1..depth {
                let Some(layer) = k.checked_sub(j) else {
                    break;
                };
                let tile = self.layers[layer][i];
                let soft = tile == Tile::Whole(self.rock) || tile.floor_id() == Some(cave);
                if !soft || self.beside_water(layer, t) {
                    break;
                }
                self.layers[layer][i] = Tile::VOID;
                bottom = layer;
            }
            if let Some(floor) = bottom.checked_sub(1)
                && self.layers[floor][i] == Tile::Whole(self.rock)
            {
                self.layers[floor][i] = Tile::Whole(Part::Floor(self.floors.canyon));
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
            self.cave_holes(k);
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
                // (Clear of water above, so the pocket below undermines
                // none.)
                let pool: Vec<[i32; 2]> = interior
                    .iter()
                    .copied()
                    .filter(|&t| {
                        self.layers[upper][self.index(t)].is_floor() && !self.beside_water(upper, t)
                    })
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
            Part::Wall(tileset().wall(ROOM_WALL).expect("a brick wall")),
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
        let seed = self.rng.range(0, u16::MAX as usize) as u32;
        let cave = self.floors.cave;
        let floor = self.layers[k]
            .iter()
            .filter(|t| t.floor_id() == Some(cave))
            .count();
        let mut open: Vec<([i32; 2], f32)> = self
            .tiles()
            .filter(|&t| {
                self.layers[k][self.index(t)].floor_id() == Some(cave)
                    && (-1..=1).all(|dy| {
                        (-1..=1).all(|dx| {
                            let n = [t[0] + dx, t[1] + dy];
                            self.in_bounds(n) && self.layers[k][self.index(n)].is_floor()
                        })
                    })
                    && self.rests(k, t)
            })
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
    fn entrances(&mut self) {
        let (plains, cellar, cave) = (self.over(1), self.over(0), self.top_cave());
        let plains_floor = self.floors.plains;
        for _ in 0..self.params.entrances {
            let spots: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| {
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

    /// Lay `path` on layer `k`: water becomes a ford (a sandy floor), void
    /// a bridge (planks, over whatever's below).
    fn lay_crossing(&mut self, k: usize, path: Vec<[i32; 2]>) {
        for t in path {
            let i = self.index(t);
            if self.layers[k][i] == self.water {
                self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.bed));
            } else {
                self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.bridge));
            }
        }
    }

    /// The connected piece of floor on layer `k` that `start` is on
    /// (across sides).
    fn region(&self, k: usize, start: [i32; 2]) -> Vec<[i32; 2]> {
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
                    && self.layers[k][self.index(n)].is_floor()
                {
                    seen[self.index(n)] = true;
                    stack.push(n);
                }
            }
        }
        out
    }

    /// The water and void tiles of the shortest way on layer `k` from
    /// `start` (cut-off land) to land that's `reached`, within `reach`
    /// tiles: a ford across water, a bridge across a hole, or both. Water
    /// with water resting on it (on the layer above) can't be forded.
    fn crossing(
        &self,
        k: usize,
        start: [i32; 2],
        reached: &[bool],
        reach: usize,
    ) -> Option<Vec<[i32; 2]>> {
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
                    let tile = self.layers[k][self.index(n)];
                    seen[self.index(n)] = true;
                    from[self.index(n)] = Some(t);
                    if tile.is_floor() && reached[self.index(n)] {
                        // Walk back, collecting what was crossed.
                        let mut path = Vec::new();
                        let mut at = t;
                        while at != start {
                            path.push(at);
                            at = from[self.index(at)].unwrap();
                        }
                        return Some(path);
                    }
                    // Water can be forded unless water on the layer above
                    // rests on it (a ford is floor: open ground beneath).
                    let fordable = tile == self.water
                        && (k + 1 >= self.layers.len() || !self.beside_water(k + 1, n));
                    if fordable || tile == Tile::VOID {
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
                let region = self.region(plains, start);
                let path = self
                    .crossing(plains, start, &reached[plains], 8)
                    .or_else(|| {
                        (region.len() > ISLET)
                            .then(|| self.crossing(plains, start, &reached[plains], far))
                            .flatten()
                    });
                // An islet sinks only into water: never next to a hole,
                // which must stay clear of water, and only where the
                // water would rest on solid ground (not over a cellar or
                // canyon floor).
                let in_water = region.iter().all(|&t| {
                    self.interior(t)
                        && !self.near_spawn(t, 2)
                        && self.rests(plains, t)
                        && Side::ALL.into_iter().all(|side| {
                            let [dx, dy] = side.offset();
                            let n = [t[0] + dx, t[1] + dy];
                            self.layers[plains][self.index(n)] != Tile::VOID
                        })
                });
                if let Some(path) = path {
                    self.lay_crossing(plains, path);
                    changed = true;
                } else if region.len() <= ISLET && in_water {
                    for t in region {
                        let i = self.index(t);
                        self.layers[plains][i] = self.water;
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
                                .filter(|n| self.layers[plains][self.index(*n)] == self.water)
                        })
                        .map(|n| (t, n))
                });
                if let Some((t, n)) = shore {
                    let far = self.cols + self.rows;
                    match self.crossing(plains, n, &reached[plains], far) {
                        Some(path) => {
                            self.lay_crossing(plains, path.into_iter().chain([n]).collect());
                        }
                        None => {
                            skipped[hills][self.index(t)] = true;
                        }
                    }
                    changed = true;
                }
            }
            // Bridges in the caves: cut-off cave floor joined across a
            // hole (or pool) to floor that's reached, if it's near.
            if !changed {
                for k in 0..m {
                    let cut_off = self.tiles().find(|&t| {
                        let i = self.index(t);
                        self.layers[k][i].is_floor() && !reached[k][i] && !skipped[k][i]
                    });
                    let Some(start) = cut_off else {
                        continue;
                    };
                    match self.crossing(k, start, &reached[k], 8) {
                        Some(path) => self.lay_crossing(k, path),
                        None => {
                            for t in self.region(k, start) {
                                skipped[k][self.index(t)] = true;
                            }
                        }
                    }
                    changed = true;
                    break;
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
                if open && !reached[i] && *tile != self.water {
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

/// The kinds of tile the chamfer tells apart (materials aside).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Floor,
    Wall,
    Liquid,
    Void,
}

impl Kind {
    fn of(part: Part) -> Kind {
        match part {
            Part::Floor(_) => Kind::Floor,
            Part::Wall(_) => Kind::Wall,
            Part::Liquid(_) => Kind::Liquid,
            Part::Void => Kind::Void,
        }
    }
}

/// A chamfer: a tile of kind `into` whose corner is pinched by `from` on
/// both sides gets a 45-degree edge, that corner becoming `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub into: Kind,
    pub from: Kind,
}

/// The chamfers made, in order; a tile gets at most one, the first that
/// fits. Walls into floor (the old smoothing), hole rims both ways,
/// shores both ways, rock into pools.
pub const RULES: [Rule; 6] = [
    Rule {
        into: Kind::Floor,
        from: Kind::Wall,
    },
    Rule {
        into: Kind::Floor,
        from: Kind::Void,
    },
    Rule {
        into: Kind::Void,
        from: Kind::Floor,
    },
    Rule {
        into: Kind::Floor,
        from: Kind::Liquid,
    },
    Rule {
        into: Kind::Liquid,
        from: Kind::Floor,
    },
    Rule {
        into: Kind::Liquid,
        from: Kind::Wall,
    },
];

/// The part of `tile` along its side `s`: of a split tile, the half that
/// side belongs to.
fn part_at_side(tile: Tile, s: Side) -> Part {
    match tile {
        Tile::Whole(p) => p,
        Tile::Stairs(_, f) => Part::Floor(f),
        Tile::Split { corner, at, rest } => {
            if corner.sides().contains(&s) {
                at
            } else {
                rest
            }
        }
    }
}

/// The part of `tile` at its corner `c`: of a split tile, the half that
/// corner is in, or none if the split's diagonal runs through it.
fn part_at_corner(tile: Tile, c: Corner) -> Option<Part> {
    match tile {
        Tile::Whole(p) => Some(p),
        Tile::Stairs(_, f) => Some(Part::Floor(f)),
        Tile::Split { corner, at, .. } if corner == c => Some(at),
        Tile::Split { corner, rest, .. } if corner == c.opposite() => Some(rest),
        Tile::Split { .. } => None,
    }
}

/// A chamfer to make: tile `index`'s `corner`, by `rule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chamfer {
    pub index: usize,
    pub corner: Corner,
    pub rule: Rule,
}

/// Does `rule` fit tile `(x, y)` of `tiles` at `corner` right now: a whole
/// tile of the rule's kind, the other kind across the corner's two sides,
/// its own kind across the other two and on the diagonal between them
/// (where that diagonal tile touches this one)? If so, the part the
/// corner becomes (the material of the neighbor it continues).
fn fits(
    tiles: &[Tile],
    cols: usize,
    rows: usize,
    x: i32,
    y: i32,
    corner: Corner,
    rule: Rule,
) -> Option<Part> {
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Tile::WALL
        } else {
            tiles[y as usize * cols + x as usize]
        }
    };
    let Tile::Whole(own) = tile(x, y) else {
        return None;
    };
    if Kind::of(own) != rule.into {
        return None;
    }
    // What's across each side, as far as this tile is concerned.
    let across = |side: Side| {
        let [dx, dy] = side.offset();
        part_at_side(tile(x + dx, y + dy), side.opposite())
    };
    let [a, b] = corner.sides();
    let [c, d] = corner.opposite().sides();
    let [ox, oy] = [c, d]
        .map(|s| s.offset())
        .into_iter()
        .fold([0, 0], |acc, o| [acc[0] + o[0], acc[1] + o[1]]);
    // The diagonal tile touches this one at the corner opposite the cut.
    let diagonal = part_at_corner(tile(x + ox, y + oy), corner);
    let ok = Kind::of(across(a)) == rule.from
        && Kind::of(across(b)) == rule.from
        && Kind::of(across(c)) == rule.into
        && Kind::of(across(d)) == rule.into
        && diagonal.is_some_and(|p| Kind::of(p) == rule.into);
    ok.then(|| across(a))
}

/// Where to put 45-degree edges in stair-step corners of `tiles`: a whole
/// tile of one kind with another kind on two neighboring sides (say north
/// and west), its own kind on the other two and on the diagonal between
/// them, gets that corner cut to the other kind (see [`RULES`] and
/// [`fits`]). Each tile keeps a way through to its own region, so this
/// never thins a wall or cuts a passage. At most one per tile, the first
/// rule and corner that fit, decided from `tiles` as they are; the caller
/// applies them one by one, checking each still [`fits`] when its turn
/// comes (see [`apply`]), so no two together strand a piece of ground.
pub fn chamfer_layer(tiles: &[Tile], cols: usize, rows: usize) -> Vec<Chamfer> {
    let mut found = Vec::new();
    for y in 0..rows as i32 {
        for x in 0..cols as i32 {
            let found_here = RULES.iter().find_map(|&rule| {
                Corner::CLOCKWISE
                    .into_iter()
                    .find(|&corner| fits(tiles, cols, rows, x, y, corner, rule).is_some())
                    .map(|corner| Chamfer {
                        index: y as usize * cols + x as usize,
                        corner,
                        rule,
                    })
            });
            found.extend(found_here);
        }
    }
    found
}

/// Make chamfer `c` in `tiles` if it still fits; whether it was made.
pub fn apply(tiles: &mut [Tile], cols: usize, rows: usize, c: Chamfer) -> bool {
    let (x, y) = ((c.index % cols) as i32, (c.index / cols) as i32);
    match fits(tiles, cols, rows, x, y, c.corner, c.rule) {
        Some(part) => {
            tiles[c.index] = tiles[c.index].chamfer(c.corner, part);
            true
        }
        None => false,
    }
}

/// [`chamfer_layer`], applied.
pub fn chamfer(tiles: &mut [Tile], cols: usize, rows: usize) {
    for c in chamfer_layer(tiles, cols, rows) {
        apply(tiles, cols, rows, c);
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

    fn water() -> Tile {
        Tile::Whole(Part::Liquid(tileset().liquid(WATER).unwrap()))
    }

    fn spawn_tile() -> [i32; 2] {
        [(SPAWN[0] / TILE) as i32, (SPAWN[1] / TILE) as i32]
    }

    /// Each overworld column is rock, then one floor (or water on the
    /// plains), then void; and the underworld is enclosed.
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
                    let cellar = column[0].floor_id().is_some();
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
                                    || t.liquid_quarters() != 0
                                    || t.stairs().is_some();
                                assert!(ok, "seed {seed} ({x}, {y}): {t:?} at the surface");
                            }
                            std::cmp::Ordering::Greater => {
                                // Above the surface: void.
                                let ok = *t == Tile::VOID || (t.void_quarters() != 0);
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
                    // A canyon floor (the layer under the plains) is floor,
                    // but under a hole: nobody walks there.
                    let covered = k + 1 == top && level.layer(top).tile(x, y).void_quarters() != 0;
                    if !open || covered {
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

    /// Under every water tile (lake, river or pool), the tile below and
    /// its eight neighbors are wall or water.
    #[test]
    fn water_rests_on_solid_ground_and_never_at_the_spawn() {
        let p = Params::default();
        let plains = p.underworld_layers + 1;
        let (mut lakes, mut pools) = (0, 0);
        for seed in 0..20 {
            let level = world(seed);
            for k in 0..level.depth() {
                for y in 0..ROWS as i32 {
                    for x in 0..COLS as i32 {
                        if level.layer(k).tile(x, y).liquid_quarters() == 0 {
                            continue;
                        }
                        if k == plains {
                            lakes += 1;
                            let s = spawn_tile();
                            assert!((x - s[0]).abs().max((y - s[1]).abs()) > SPAWN_CLEARANCE);
                        } else {
                            assert!(k < p.underworld_layers, "seed {seed}: water on layer {k}");
                            pools += 1;
                        }
                        let Some(below) = k.checked_sub(1) else {
                            continue;
                        };
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                let under = level.layer(below).tile(x + dx, y + dy);
                                assert!(
                                    under.wall().quarters() | under.liquid_quarters() == 0b1111,
                                    "seed {seed}: water at ({x}, {y}) on layer {k} over {under:?} at ({}, {})",
                                    x + dx,
                                    y + dy
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(lakes > 20 * 100, "{lakes} lake tiles in 20 levels");
        assert!(pools > 20 * 10, "{pools} pool tiles in 20 levels");
    }

    #[test]
    fn holes_open_onto_the_layer_below_and_keep_clear_of_water() {
        let p = Params::default();
        let plains = p.underworld_layers + 1;
        let (mut plains_holes, mut cave_holes, mut bridges) = (0, 0, 0);
        for seed in 0..20 {
            let level = world(seed);
            for k in 0..level.depth() {
                for y in 1..ROWS as i32 - 1 {
                    for x in 1..COLS as i32 - 1 {
                        let t = level.layer(k).tile(x, y);
                        if k == plains && t.floor_id() == tileset().floor(BRIDGE) {
                            bridges += 1;
                        }
                        if t != Tile::VOID || k > plains {
                            continue;
                        }
                        // A hole: on the plains over a canyon floor; in a
                        // cave over whatever's below. Never beside water.
                        if k == plains {
                            plains_holes += 1;
                            // (Smoothing may bevel the canyon floor's corners.)
                            let under = level.layer(k - 1).tile(x, y);
                            assert!(
                                under.floor_id().is_some() || under.stairs().is_some(),
                                "seed {seed} ({x}, {y}): {under:?}"
                            );
                        } else if k < p.underworld_layers {
                            cave_holes += 1;
                        }
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                let n = level.layer(k).tile(x + dx, y + dy);
                                assert_eq!(
                                    n.liquid_quarters(),
                                    0,
                                    "seed {seed}: hole at ({x}, {y}) beside water"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(
            plains_holes > 20 * 10,
            "{plains_holes} plains holes in 20 levels"
        );
        assert!(cave_holes > 20 * 10, "{cave_holes} cave holes in 20 levels");
        assert!(bridges > 0, "no bridges in 20 levels");
    }

    #[test]
    fn hole_sizes_and_depth_follow_the_parameters() {
        // On flat, dry plains, so the holes aren't trimmed by hills or water.
        let flat = Params {
            hill_share: 0.0,
            water_share: 0.0,
            rivers: 0,
            ..Params::default()
        };
        let p = Params {
            holes: 2,
            canyons: 1,
            hole_radius: [3, 3],
            canyon_width: [3, 3],
            canyon_length: [30, 30],
            hole_depth: [2, 2],
            ..flat.clone()
        };
        let plains = p.underworld_layers + 1;
        // Hole tiles on the plains over `seeds`, with `params`.
        let count = |params: &Params| {
            (0..10u32)
                .map(|seed| {
                    let level = generate_level(seed, COLS, ROWS, SPAWN, params);
                    (1..ROWS as i32 - 1)
                        .flat_map(|y| (1..COLS as i32 - 1).map(move |x| (x, y)))
                        .filter(|&(x, y)| level.layer(plains).tile(x, y) == Tile::VOID)
                        .count()
                })
                .sum::<usize>()
        };
        let (big, usual) = (count(&p), count(&flat));
        assert!(
            big > 2 * usual,
            "{big} hole tiles with big holes, {usual} with the defaults"
        );
        // Two deep: through the layer below too, onto a floor in the top
        // cave where that was rock.
        let (mut deep, mut floors_below) = (0, 0);
        for seed in 0..10 {
            let level = generate_level(seed, COLS, ROWS, SPAWN, &p);
            for y in 1..ROWS as i32 - 1 {
                for x in 1..COLS as i32 - 1 {
                    if level.layer(plains).tile(x, y) == Tile::VOID
                        && level.layer(plains - 1).tile(x, y) == Tile::VOID
                    {
                        deep += 1;
                        floors_below +=
                            level.layer(plains - 2).tile(x, y).floor_id().is_some() as usize;
                    }
                }
            }
        }
        assert!(deep * 2 > big, "{deep} of {big} go two deep");
        assert!(floors_below > 0);
        // Depth 1: nothing opened on the layer below.
        let shallow = Params {
            hole_depth: [1, 1],
            ..p
        };
        let level = generate_level(1, COLS, ROWS, SPAWN, &shallow);
        for y in 1..ROWS as i32 - 1 {
            for x in 1..COLS as i32 - 1 {
                assert_ne!(level.layer(plains - 1).tile(x, y), Tile::VOID);
            }
        }
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

    /// A generated level file has everything the game needs: the tileset,
    /// the characters' sprites, the spawn on the plains and the ground
    /// level there.
    #[test]
    fn level_files_carry_the_tileset_and_sprites() {
        let p = Params::default();
        let file = super::world(7, COLS, ROWS, SPAWN, &p);
        assert_eq!(file.tileset, *tileset());
        assert_eq!(file.spawn.layer, spawn_layer(&p));
        assert_eq!(file.spawn, Spawn::at_tile(spawn_layer(&p), spawn_tile()));
        assert_eq!(file.ground, ground_layer(&p));
        assert!(file.tileset.sprite(&file.player.sprite).is_some());
        assert!(file.tileset.sprite(&file.npc.sprite).is_some());
        let back = LevelFile::from_json(&file.to_json()).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.level().depth(), world(7).depth());
    }

    /// Every cell the tileset names is in the placeholder sheet, and its
    /// white cell is white (so plain shapes draw right).
    #[test]
    fn the_tileset_fits_the_placeholder_sheet() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../www/assets/tiles.png");
        let file = std::fs::File::open(path).expect("www/assets/tiles.png (run `npm run tiles`)");
        let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
        let mut reader = decoder.read_info().unwrap();
        let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut rgba).unwrap();
        let t = tileset();
        t.check_sheet([info.width, info.height]).unwrap();
        let [cx, cy] = t.white;
        let i = ((cy as u32 * t.cell) * info.width + cx as u32 * t.cell) as usize * 4;
        assert_eq!(&rgba[i..i + 4], &[255, 255, 255, 255], "white cell");
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

    /// Tiles from rows of `#` wall, `.` floor, `~` void, `≈` water.
    fn tiles(rows: &[&str]) -> Vec<Tile> {
        rows.iter()
            .flat_map(|r| r.chars())
            .map(|c| match c {
                '#' => Tile::WALL,
                '~' => Tile::VOID,
                '≈' => water(),
                _ => Tile::FLOOR,
            })
            .collect()
    }

    #[test]
    fn chamfer_fills_stair_steps_only() {
        use Corner::*;
        let (floor, void, liquid) = (Part::Floor(0), Part::Void, water().liquid_id().unwrap());
        let mut t = tiles(&["#####", "#....", "#....", "#...."]);
        chamfer(&mut t, 5, 4);
        assert_eq!(t[5 + 1], Tile::slope(NW), "wall into floor");
        assert_eq!(t[2 * 5 + 2], Tile::FLOOR);

        let bend = tiles(&["#####", "#..##", "##.##", "#####"]);
        let mut t = bend.clone();
        chamfer(&mut t, 5, 4);
        assert_eq!(t, bend, "a bend in a passage is left alone");

        // A hole's rim: the void at its convex corners gets floor (a 2x2
        // hole becomes a diamond), the floor at a concave corner opens.
        let mut t = tiles(&[".....", ".~~..", ".~~..", "....."]);
        chamfer(&mut t, 5, 4);
        assert_eq!(t[5 + 1], Tile::split(NW, floor, void), "convex corner");
        assert_eq!(t[2 * 5 + 2], Tile::split(SE, floor, void));
        assert_eq!(t[5 + 2], Tile::split(NE, floor, void));
        assert_eq!(t[5 + 3], Tile::FLOOR, "a straight edge stays");
        let mut t = tiles(&["~~~..", "~~~..", "~~...", "~~..."]);
        chamfer(&mut t, 5, 4);
        assert_eq!(
            t[5 + 2],
            Tile::split(SE, floor, void),
            "void's convex corner"
        );
        // The floor's concave corner just below would zigzag against it:
        // the first chamfer made wins, the other no longer fits.
        assert_eq!(t[2 * 5 + 2], Tile::FLOOR, "vetoed by its neighbor");
        assert_eq!(t[2 * 5 + 3], Tile::FLOOR, "a straight edge stays");
        let mut split = t.clone();
        chamfer(&mut split, 5, 4);
        assert_eq!(split, t, "nothing cascades: split tiles are left alone");
        // On its own, the concave corner opens.
        let mut t = tiles(&["~~~~~", "~~~~~", "~~...", "~~..."]);
        chamfer(&mut t, 5, 4);
        assert_eq!(
            t[2 * 5 + 2],
            Tile::split(NW, void, floor),
            "floor's concave corner"
        );
        assert_eq!(t[5 + 2], Tile::VOID, "a straight void edge stays");

        // A shore, both ways; and rock into a pool.
        let mut t = tiles(&["..≈≈≈", "..≈≈≈", "....≈", "....."]);
        chamfer(&mut t, 5, 4);
        assert_eq!(t[5 + 1], Tile::FLOOR, "straight shore");
        assert_eq!(
            t[2 * 5 + 3],
            Tile::split(NE, Part::Liquid(liquid), floor),
            "water into the floor's corner"
        );
        assert_eq!(
            t[5 + 2],
            Tile::split(SW, floor, Part::Liquid(liquid)),
            "floor into the water's corner"
        );
        let mut t = tiles(&["#####", "#≈≈≈≈", "#≈≈≈≈", "#≈≈≈≈"]);
        chamfer(&mut t, 5, 4);
        assert_eq!(
            t[5 + 1],
            Tile::split(NW, Part::Wall(0), Part::Liquid(liquid))
        );
    }

    /// Generated levels have 45-degree hole rims and shores, each obeying
    /// the rules: the right kinds either side, nothing under water but
    /// solid ground, floor under every piece of hole on the plains, and
    /// never on stairs, bridges, fords or at the spawn.
    #[test]
    fn shores_and_hole_rims_are_chamfered() {
        let p = Params::default();
        let plains = p.underworld_layers + 1;
        let t = tileset();
        let (bridge, ford) = (t.floor(BRIDGE), t.floor(BED));
        let mut counts = std::collections::HashMap::new();
        for seed in 0..20 {
            let level = world(seed);
            for k in 0..level.depth() {
                for y in 0..ROWS as i32 {
                    for x in 0..COLS as i32 {
                        let Tile::Split { corner, at, rest } = level.layer(k).tile(x, y) else {
                            continue;
                        };
                        let (from, into) = (Kind::of(at), Kind::of(rest));
                        *counts.entry((into, from)).or_insert(0) += 1;
                        assert!(
                            RULES.contains(&Rule { into, from }),
                            "seed {seed} ({x}, {y}) layer {k}: {at:?} over {rest:?}"
                        );
                        let s = spawn_tile();
                        assert!((x - s[0]).abs().max((y - s[1]).abs()) > SPAWN_CLEARANCE);
                        // Holes and water keep off bridges and fords (a
                        // wall's corner in a plank room is fine).
                        if let (Part::Floor(f), Kind::Void | Kind::Liquid) = (rest, from) {
                            assert!(
                                Some(f) != bridge && Some(f) != ford,
                                "seed {seed}: on a crossing"
                            );
                        }
                        // The neighbors across the corner's sides are what
                        // the corner became (unless chamfered too, later).
                        for side in corner.sides() {
                            let [dx, dy] = side.offset();
                            let n = level.layer(k).tile(x + dx, y + dy);
                            assert!(
                                Kind::of(part_at_side(n, side.opposite())) == from
                                    || matches!(n, Tile::Split { .. }),
                                "seed {seed} ({x}, {y}) layer {k}: {n:?} across {side:?}"
                            );
                        }
                        if from == Kind::Void && k == plains {
                            let under = level.layer(k - 1).tile(x, y);
                            assert!(
                                under.floor_id().is_some(),
                                "seed {seed} ({x}, {y}): {under:?}"
                            );
                        }
                    }
                }
            }
        }
        for (into, from) in [
            (Kind::Floor, Kind::Void),
            (Kind::Void, Kind::Floor),
            (Kind::Floor, Kind::Liquid),
            (Kind::Liquid, Kind::Floor),
        ] {
            assert!(
                counts.get(&(into, from)).is_some_and(|n| *n > 20),
                "{from:?} into {into:?}: {counts:?}"
            );
        }
    }
}
