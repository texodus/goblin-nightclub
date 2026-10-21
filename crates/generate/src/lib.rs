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
//! its eight neighbors are wall or water, never floor or void. That holds
//! for lakes, rivers and pools alike.
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

mod chamfer;
mod connect;
mod noise;
mod overworld;
mod props;
mod regions;
mod underworld;
pub use chamfer::{Kind, RULES, Rule};
pub use connect::reachable_across;
pub use regions::{Generated, Plan, REGION, generated_meta, region_part};

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
            underworld_layers: 5,
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

/// Tiles (Chebyshev) around the spawn kept flat, dry and open.
const SPAWN_CLEARANCE: i32 = 2;

/// Materials, by name in the tile sheet. Some name the same material for
/// different uses: hills and cave floor are both dirt, rooms and bridges
/// both planks, mountains and canyon floors both stone. Tiles of these
/// can't be told apart afterward, so a check written for one matches the
/// other wherever both occur (see [`Floors`]).
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
    let world = World::new(seed, cols, rows, spawn, params, None);
    let (spawn, params) = (world.spawn, world.params.clone());
    let layers = world.build();
    let [top, ground] =
        [spawn_layer(&params), ground_layer(&params)].map(|k| k.min(layers.len() - 1));
    let all = [[0, 0], [cols as i32, rows as i32]];
    let props = props::place(
        seed,
        &layers,
        [cols, rows],
        [0, 0],
        all,
        ground,
        (top, spawn),
    );
    LevelFile::new(
        cols,
        rows,
        &layers,
        Spawn::at_tile(top, spawn),
        tileset().clone(),
        PLAYER_SPRITE,
        NPC_SPRITE,
    )
    .with_ground(ground)
    .with_props(props)
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
    /// For a region of a world too big to make whole (see [`regions`]):
    /// where this world (the region and the apron around it) is in the
    /// whole, and what's the same all over it. `None` for a world made
    /// whole.
    region: Option<regions::Frame>,
}

/// The floors the generator lays, by use. Three pairs are the same floor
/// (see [`PLAINS`]); the checks against them hold only because the pairs
/// keep to different layers (hills and mountains above the plains, caves
/// and canyon floors below), except rooms and bridges, which share the
/// caves: there a room's floor counts as a crossing, kept from being cut
/// into by a hole's rim, as a bridge's is.
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
    fn new(
        seed: u32,
        cols: usize,
        rows: usize,
        spawn: [i32; 2],
        params: &Params,
        region: Option<regions::Frame>,
    ) -> Self {
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
        // A region's spawn is the whole world's, wherever that is.
        let spawn = match &region {
            Some(_) => spawn,
            None => [
                spawn[0].clamp(SPAWN_CLEARANCE + 1, cols as i32 - SPAWN_CLEARANCE - 2),
                spawn[1].clamp(SPAWN_CLEARANCE + 1, rows as i32 - SPAWN_CLEARANCE - 2),
            ],
        };
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
            region,
        }
    }

    /// Build the world: its layers, bottom first, each row by row.
    fn build(mut self) -> Vec<Vec<Tile>> {
        self.terrain();
        self.lakes();
        self.rivers();
        self.raise_terrain();
        // The caves before the plains' holes, which may go down into
        // them (see `World::deepen`).
        self.underworld();
        self.plains_holes();
        self.ramps();
        self.entrances();
        if self.region.is_some() {
            self.join_stairs();
        }
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

    // What a pass needs of the part of the world it's making, a world made
    // whole or a region of a bigger one: asked here, so passes needn't ask
    // which it is unless they do something different for a region.

    /// The plan of the world this is a region of (what's the same all over
    /// it), or `None` for a world made whole.
    fn plan(&self) -> Option<&Plan> {
        self.region.as_ref().map(|r| &*r.plan)
    }

    /// The whole world's size, in tiles.
    fn world_size(&self) -> [usize; 2] {
        self.region
            .as_ref()
            .map_or([self.cols, self.rows], |r| r.world)
    }

    /// Where this one's first tile is in the whole world.
    fn origin(&self) -> [i32; 2] {
        self.region.as_ref().map_or([0, 0], |r| r.origin)
    }

    /// Its own tiles (from, and up to): a region's, without the apron
    /// around them; all of a world made whole.
    fn own_box(&self) -> [[i32; 2]; 2] {
        self.region
            .as_ref()
            .map_or([[0, 0], [self.cols as i32, self.rows as i32]], |r| r.core)
    }

    /// Where rooms and the stairs between caves may go (from, and up to):
    /// all but a border of the world; a region's own tiles, well inside
    /// (see [`World::placeable`]).
    fn fixtures_box(&self) -> [[i32; 2]; 2] {
        self.region.as_ref().map_or(
            [[2, 2], [self.cols as i32 - 2, self.rows as i32 - 2]],
            regions::Frame::placeable,
        )
    }

    /// Inside the one-tile border of rock (of the whole world, for a
    /// region).
    fn interior(&self, [x, y]: [i32; 2]) -> bool {
        let [cols, rows] = self.world_size();
        let [x, y] = self.absolute([x, y]);
        x >= 1 && y >= 1 && (x as usize) + 1 < cols && (y as usize) + 1 < rows
    }

    /// Tile `t` in the whole world.
    fn absolute(&self, t: [i32; 2]) -> [i32; 2] {
        let o = self.origin();
        [t[0] + o[0], t[1] + o[1]]
    }

    /// May something placed by chance (a hole, a room, stairs) be at `t`?
    /// Anywhere in a world made whole; in a region, only well inside its
    /// own tiles, so the regions around, seeing it only as their apron,
    /// never need to know of it.
    fn placeable(&self, [x, y]: [i32; 2]) -> bool {
        self.region.as_ref().is_none_or(|r| {
            let [lo, hi] = r.placeable();
            x >= lo[0] && y >= lo[1] && x < hi[0] && y < hi[1]
        })
    }

    /// `n` things per world the default size: the same per region, for as
    /// many to the tile it may place them on.
    fn scaled(&self, n: usize) -> usize {
        match &self.region {
            None => n,
            Some(r) => {
                let [lo, hi] = r.placeable();
                let area = ((hi[0] - lo[0]).max(0) * (hi[1] - lo[1]).max(0)) as f32;
                (n as f32 * area / regions::REFERENCE_AREA).round() as usize
            }
        }
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

    /// Open [`Params::holes`] holes and [`Params::canyons`] canyons as void
    /// on layer `k`, only where `allowed`: holes are rough discs of
    /// [`Params::hole_radius`], canyons winding rifts of
    /// [`Params::canyon_width`] and [`Params::canyon_length`]. Each goes
    /// [`Params::hole_depth`] layers down (see [`World::deepen`]).
    fn carve_voids(&mut self, k: usize, allowed: &dyn Fn(&World, [i32; 2]) -> bool) {
        let allowed = |w: &World, t: [i32; 2]| allowed(w, t) && w.placeable(t);
        let mut p = self.params.clone();
        (p.holes, p.canyons) = (self.scaled(p.holes), self.scaled(p.canyons));
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
                // (Stairs have a floor too, but stop it.)
                let tile = self.layers[layer][i];
                let soft = tile == Tile::Whole(self.rock) || tile == Tile::Whole(Part::Floor(cave));
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

    // --- The underworld --------------------------------------------------

    // --- Connectivity ----------------------------------------------------
}

#[cfg(test)]
#[path = "tests/generate_tests.rs"]
mod tests;
