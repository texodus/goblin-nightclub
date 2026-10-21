//! Worlds too big to make whole, made a region at a time as they're
//! wanted, each the same however many times and in whatever order it's
//! made, and each meeting its neighbors without a seam.
//!
//! A region is [`REGION`] tiles a side, a block of chunks. It's made as a
//! small world of its own (the same passes as a world made whole) over
//! itself and an apron [`APRON`] tiles wide around it, and only its own
//! tiles are kept. What's decided by where things are comes out the same
//! wherever it's worked out: the terrain's noise and heights (evened out
//! within a few tiles), lakes, rivers (bands of a second noise), caves
//! (cellular automata from a hash of each tile, smoothed within a few
//! tiles), pools and ramps (one per square of the world, picked by hash).
//! What the whole world's statistics decide, like how high the hills
//! start, comes from its [`Plan`], from a sample of the noise. What's
//! placed by chance (holes, canyons, rooms, stairs, entrances) is only
//! placed [`MARGIN`] tiles or more inside a region's own tiles, in numbers
//! scaled to the region's size, so its neighbors, seeing that strip as
//! their apron, never need to know of it. Each region joins up what it
//! can reach, counting its apron's edge as reachable, since the world
//! goes on past it.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use goblin_nightclub_core::chunk::{CHUNK, Chunk, ChunkKey, Chunks};
use goblin_nightclub_core::layer::Tile;
use goblin_nightclub_core::level_file::{Character, LevelMeta, Spawn};
use goblin_nightclub_core::prop::Prop;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset::Tileset;
use goblin_nightclub_core::world::{ChunkSource, Roof, roofs};

use crate::noise::{fbm, quantiles};
use crate::overworld::terrain_lines;
use crate::props::place;
use crate::{NPC_SPRITE, PLAYER_SPRITE, Params, World, ground_layer, spawn_layer, tileset};

/// Tiles a side of a region, by default: a block of chunks.
pub const REGION: usize = 8 * CHUNK;
/// Tiles around a region made with it, to be thrown away: wide enough
/// that what's worked out from where things are is right across the
/// region's own tiles (heights even out within a few tiles, caves smooth
/// within a few more, ramps are picked per square of `ramp_spacing`).
const APRON: i32 = 24;
/// How far inside its own tiles a region places things by chance.
pub(crate) const MARGIN: i32 = 16;
/// Tiles in a world the default size: the parameters' counts (of holes,
/// rooms and so on) are per this many.
pub(crate) const REFERENCE_AREA: f32 = 96.0 * 60.0;
/// The noise sampled for the whole world's statistics: this many points a
/// side.
const SAMPLES: usize = 96;
/// Regions kept made, the most recently wanted.
const KEEP: usize = 16;

/// What's the same all over a world made a region at a time: its size,
/// spawn and parameters, the seeds of each noise, and where the terrain's
/// lines fall.
#[derive(Clone, Debug)]
pub struct Plan {
    pub seed: u32,
    pub params: Params,
    /// Tiles across and down.
    pub size: [usize; 2],
    /// The tile the player starts on, on the plains.
    pub spawn: [i32; 2],
    /// Tiles a side of a region ([`REGION`] unless changed): whole chunks,
    /// and more than twice the margin around each region (its apron).
    pub region: usize,
    pub(crate) terrain_seed: u32,
    pub(crate) river_seed: u32,
    pub(crate) cave_seed: u32,
    pub(crate) pool_seed: u32,
    pub(crate) ramp_seed: u32,
    /// The terrain noise above which it's hills, and mountains; at or
    /// below which lakes may be; and the pool noise below which pools may
    /// be.
    pub(crate) hills: f32,
    pub(crate) mountains: f32,
    pub(crate) lake_level: f32,
    pub(crate) pool_level: f32,
}

impl Plan {
    /// A world from `seed`, `cols` x `rows` tiles a layer, the player
    /// starting in its middle.
    pub fn new(seed: u32, cols: usize, rows: usize, params: &Params) -> Self {
        let mut params = params.clone();
        // As a world made whole has them.
        let most = goblin_nightclub_core::level::MAX_LAYERS;
        params.underworld_layers = params.underworld_layers.clamp(1, most - 2);
        params.overworld_layers = params
            .overworld_layers
            .clamp(2, most - params.underworld_layers);
        let mut rng = Rng::new(seed ^ 0x5EED_5EED);
        let mut next = || rng.range(0, u16::MAX as usize) as u32;
        let (terrain_seed, river_seed, cave_seed, pool_seed, ramp_seed) =
            (next(), next(), next(), next(), next());
        // The noise at points all over the world, a little off a grid.
        let sample = |seed: u32, scale: f32| {
            let mut jitter = Rng::new(seed);
            quantiles(
                (0..SAMPLES * SAMPLES)
                    .map(|i| {
                        let x =
                            ((i % SAMPLES) as f32 + jitter.unit()) * cols as f32 / SAMPLES as f32;
                        let y =
                            ((i / SAMPLES) as f32 + jitter.unit()) * rows as f32 / SAMPLES as f32;
                        fbm(seed, x / scale, y / scale)
                    })
                    .collect(),
            )
        };
        let terrain = sample(terrain_seed, params.terrain_scale.max(2.0));
        let ([hills_from, mountains_from], lakes) = terrain_lines(&params);
        let pools = sample(pool_seed, 6.0);
        Self {
            seed,
            size: [cols, rows],
            spawn: [cols as i32 / 2, rows as i32 / 2],
            region: REGION,
            terrain_seed,
            river_seed,
            cave_seed,
            pool_seed,
            ramp_seed,
            hills: terrain(hills_from),
            mountains: terrain(mountains_from),
            lake_level: terrain(lakes),
            pool_level: pools(params.pool_share.clamp(0.0, 0.5)),
            params,
        }
    }

    /// Layers.
    pub fn depth(&self) -> usize {
        self.params.underworld_layers + self.params.overworld_layers
    }

    /// Regions across and down.
    pub fn regions(&self) -> [usize; 2] {
        self.size.map(|n| n.div_ceil(self.region))
    }

    /// The same world, in regions `region` tiles a side (see
    /// [`Plan::region`]): smaller ones make quicker tests.
    pub fn with_region(mut self, region: usize) -> Self {
        assert!(region.is_multiple_of(CHUNK) && region as i32 > 2 * MARGIN);
        self.region = region;
        self
    }
}

/// Where a region's world is in the whole: see [`World`]'s `region`.
pub(crate) struct Frame {
    /// Its top-left tile, and the whole world's size.
    pub origin: [i32; 2],
    pub world: [usize; 2],
    pub plan: Arc<Plan>,
    /// The region's own tiles in it: from, and up to.
    pub core: [[i32; 2]; 2],
}

impl Frame {
    /// Where things may be placed by chance (from, and up to): the
    /// region's own tiles, [`MARGIN`] in from each side with another region
    /// beyond it, and from the world's edge, 2 (as a world made whole
    /// keeps its rooms in).
    pub fn placeable(&self) -> [[i32; 2]; 2] {
        let [lo, hi] = self.core;
        let inset = |axis: usize, from: bool| {
            let at = self.origin[axis] + if from { lo[axis] } else { hi[axis] };
            let edge = if from {
                at == 0
            } else {
                at == self.world[axis] as i32
            };
            if edge { 2 } else { MARGIN }
        };
        [
            [lo[0] + inset(0, true), lo[1] + inset(1, true)],
            [hi[0] - inset(0, false), hi[1] - inset(1, false)],
        ]
    }
}

/// The level file's worth of a world made a region at a time: everything
/// but the tiles.
pub fn generated_meta(plan: &Plan) -> LevelMeta {
    let depth = plan.depth();
    LevelMeta {
        cols: plan.size[0],
        rows: plan.size[1],
        depth,
        spawn: Spawn::at_tile(spawn_layer(&plan.params).min(depth - 1), plan.spawn),
        ground: ground_layer(&plan.params).min(depth - 1),
        player: Character {
            sprite: PLAYER_SPRITE.into(),
        },
        npc: Character {
            sprite: NPC_SPRITE.into(),
        },
        tileset: tileset().clone(),
    }
}

/// Region `(rx, ry)` made, apron and all: where its world starts in the
/// whole, how many tiles across and down, where its own tiles are in it
/// (from, and up to), and each layer's tiles, row by row.
fn region_box(plan: &Arc<Plan>, rx: usize, ry: usize) -> Boxed {
    let [cols, rows] = plan.size;
    let span = |r: usize, n: usize| {
        let from = (r * plan.region) as i32;
        let to = ((r + 1) * plan.region).min(n) as i32;
        let (lo, hi) = ((from - APRON).max(0), (to + APRON).min(n as i32));
        (from, to, lo, hi)
    };
    let (x0, x1, bx0, bx1) = span(rx, cols);
    let (y0, y1, by0, by1) = span(ry, rows);
    let core = [[x0 - bx0, y0 - by0], [x1 - bx0, y1 - by0]];
    let frame = Frame {
        origin: [bx0, by0],
        world: [cols, rows],
        plan: plan.clone(),
        core,
    };
    let (w, h) = ((bx1 - bx0) as usize, (by1 - by0) as usize);
    let spawn = [plan.spawn[0] - bx0, plan.spawn[1] - by0];
    let mix = [rx as u32, ry as u32]
        .into_iter()
        .fold(plan.seed ^ 0x2545_F491, |h, n| {
            (h ^ n).wrapping_mul(0x0100_0193)
        });
    let layers = World::new(mix, w, h, spawn, &plan.params, Some(frame)).build();
    Boxed {
        origin: [bx0, by0],
        size: [w, h],
        core,
        layers,
    }
}

struct Boxed {
    /// Where its first tile is in the whole world.
    origin: [i32; 2],
    size: [usize; 2],
    core: [[i32; 2]; 2],
    layers: Vec<Vec<Tile>>,
}

/// Region `(rx, ry)`'s own tiles, made: how many across and down (fewer
/// at the world's far edges), and each layer's, row by row.
#[cfg(test)]
pub fn region_tiles(plan: &Arc<Plan>, rx: usize, ry: usize) -> ([usize; 2], Vec<Vec<Tile>>) {
    let (size, tiles, _) = region_made(plan, rx, ry);
    (size, tiles)
}

/// The same, and each layer's props (see [`crate::props`]), where they
/// are in the region.
pub fn region_made(
    plan: &Arc<Plan>,
    rx: usize,
    ry: usize,
) -> ([usize; 2], Vec<Vec<Tile>>, Vec<Vec<Prop>>) {
    let Boxed {
        origin,
        size,
        core,
        layers,
    } = region_box(plan, rx, ry);
    let props = place(
        plan.seed,
        &layers,
        size,
        origin,
        core,
        ground_layer(&plan.params),
        (spawn_layer(&plan.params), plan.spawn),
    );
    let ([w, _], [lo, hi]) = (size, core);
    let (cw, ch) = ((hi[0] - lo[0]) as usize, (hi[1] - lo[1]) as usize);
    let (ox, oy) = (lo[0] as usize, lo[1] as usize);
    let own = layers
        .into_iter()
        .map(|tiles| {
            (0..cw * ch)
                .map(|i| tiles[(oy + i / cw) * w + ox + i % cw])
                .collect()
        })
        .collect();
    ([cw, ch], own, props)
}

/// Region `(rx, ry)` made and packed (see
/// [`goblin_nightclub_core::packed::pack_part`]): its chunks and the roofs
/// over them, by where they are in the whole world. To hand over from
/// wherever it's made (a worker) to the game.
pub fn region_part(plan: &Arc<Plan>, rx: usize, ry: usize) -> Vec<u8> {
    let ([cols, rows], tiles, props) = region_made(plan, rx, ry);
    let per = (plan.region / CHUNK) as i32;
    let at = |x: usize, y: usize| (rx as i32 * per + x as i32, ry as i32 * per + y as i32);
    let mut chunks = Vec::new();
    for (k, layer) in tiles.iter().enumerate() {
        let grid = Chunks::from_tiles(cols, rows, layer).with_props(&props[k]);
        let [across, down] = grid.size();
        for (x, y) in (0..down).flat_map(|y| (0..across).map(move |x| (x, y))) {
            let (cx, cy) = at(x, y);
            chunks.push(((k, cx, cy), grid.chunk(x, y).clone()));
        }
    }
    let across = cols.div_ceil(CHUNK);
    let roofs = roofs(cols, rows, &tiles)
        .into_iter()
        .enumerate()
        .map(|(i, roof)| (at(i % across, i / across), roof))
        .collect();
    goblin_nightclub_core::packed::pack_part(&generated_meta(plan), chunks, roofs)
}

/// A region made: its chunks, by layer, row by row, and the roofs over
/// them.
struct Made {
    chunks: Vec<Chunks>,
    roofs: Vec<Roof>,
}

/// A world made a region at a time, as the chunks of it are asked for:
/// right away, on the asking thread (see the app's worker for the browser,
/// which makes them on another).
pub struct Generated {
    plan: Arc<Plan>,
    tileset: Tileset,
    made: Mutex<Kept>,
}

/// Regions made, and the order they were wanted in.
type Kept = (HashMap<[usize; 2], Arc<Made>>, VecDeque<[usize; 2]>);

impl Generated {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan: Arc::new(plan),
            tileset: tileset().clone(),
            made: Default::default(),
        }
    }

    /// Region `(rx, ry)`, made if it isn't (and kept, with the others most
    /// recently wanted).
    fn region(&self, at: [usize; 2]) -> Arc<Made> {
        if let Some(made) = self.made.lock().unwrap().0.get(&at) {
            return made.clone();
        }
        let ([cols, rows], tiles, props) = region_made(&self.plan, at[0], at[1]);
        let made = Arc::new(Made {
            chunks: tiles
                .iter()
                .zip(&props)
                .map(|(t, props)| Chunks::from_tiles(cols, rows, t).with_props(props))
                .collect(),
            roofs: roofs(cols, rows, &tiles),
        });
        let mut kept = self.made.lock().unwrap();
        kept.0.insert(at, made.clone());
        kept.1.push_back(at);
        if kept.1.len() > KEEP {
            let gone = kept.1.pop_front().unwrap();
            kept.0.remove(&gone);
        }
        made
    }

    /// Make every region, on as many threads as there are cores, and pack
    /// the lot (see [`goblin_nightclub_core::packed`]).
    pub fn bake(&self) -> Vec<u8> {
        let [across, down] = self.plan.regions();
        let all: Vec<[usize; 2]> = (0..down)
            .flat_map(|y| (0..across).map(move |x| [x, y]))
            .collect();
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        let made: Vec<(usize, Arc<Made>)> = std::thread::scope(|s| {
            let jobs: Vec<_> = (0..threads)
                .map(|t| {
                    let all = &all;
                    s.spawn(move || {
                        (t..all.len())
                            .step_by(threads)
                            .map(|i| {
                                let ([cols, rows], tiles, props) =
                                    region_made(&self.plan, all[i][0], all[i][1]);
                                let made = Made {
                                    chunks: tiles
                                        .iter()
                                        .zip(&props)
                                        .map(|(t, p)| {
                                            Chunks::from_tiles(cols, rows, t).with_props(p)
                                        })
                                        .collect(),
                                    roofs: roofs(cols, rows, &tiles),
                                };
                                (i, Arc::new(made))
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap()).collect()
        });
        let baked = Baked {
            plan: self.plan.clone(),
            tileset: &self.tileset,
            made: made.into_iter().map(|(i, m)| (all[i], m)).collect(),
        };
        goblin_nightclub_core::packed::pack(&generated_meta(&self.plan), &baked)
    }
}

/// Which region (`region` tiles a side) a chunk's in, and which chunk of
/// it.
fn locate(region: usize, cx: i32, cy: i32) -> ([usize; 2], [usize; 2]) {
    let per = (region / CHUNK) as i32;
    let at = [cx.div_euclid(per) as usize, cy.div_euclid(per) as usize];
    (
        at,
        [cx.rem_euclid(per) as usize, cy.rem_euclid(per) as usize],
    )
}

impl ChunkSource for Generated {
    fn size(&self) -> [usize; 3] {
        [self.plan.size[0], self.plan.size[1], self.plan.depth()]
    }

    fn tileset(&self) -> &Tileset {
        &self.tileset
    }

    fn chunk(&self, (k, cx, cy): ChunkKey) -> Option<Arc<Chunk>> {
        let [across, down] = self.plan.size.map(|n| n.div_ceil(CHUNK) as i32);
        if k >= self.plan.depth() || !(0..across).contains(&cx) || !(0..down).contains(&cy) {
            return None;
        }
        let (at, [x, y]) = locate(self.plan.region, cx, cy);
        Some(self.region(at).chunks[k].chunk(x, y).clone())
    }

    fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
        let [across, down] = self.plan.size.map(|n| n.div_ceil(CHUNK) as i32);
        if !(0..across).contains(&cx) || !(0..down).contains(&cy) {
            return None;
        }
        let (at, [x, y]) = locate(self.plan.region, cx, cy);
        let made = self.region(at);
        let per = made.chunks[0].size()[0];
        Some(made.roofs[y * per + x].clone())
    }
}

/// Every region made, for packing.
struct Baked<'a> {
    plan: Arc<Plan>,
    tileset: &'a Tileset,
    made: HashMap<[usize; 2], Arc<Made>>,
}

impl ChunkSource for Baked<'_> {
    fn size(&self) -> [usize; 3] {
        [self.plan.size[0], self.plan.size[1], self.plan.depth()]
    }

    fn tileset(&self) -> &Tileset {
        self.tileset
    }

    fn chunk(&self, (k, cx, cy): ChunkKey) -> Option<Arc<Chunk>> {
        let (at, [x, y]) = locate(self.plan.region, cx, cy);
        Some(self.made.get(&at)?.chunks.get(k)?.chunk(x, y).clone())
    }

    fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
        let (at, [x, y]) = locate(self.plan.region, cx, cy);
        let made = self.made.get(&at)?;
        let per = made.chunks[0].size()[0];
        Some(made.roofs[y * per + x].clone())
    }
}

#[cfg(test)]
#[path = "tests/regions_tests.rs"]
mod tests;
