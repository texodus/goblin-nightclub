//! Playing a level too big to keep whole, a window at a time (see
//! [`goblin_nightclub_core::world`]). What's kept between windows, in the
//! level's own coordinates: the tiles explored and the outline edges seen
//! on each layer, and the NPCs outside the window, asleep where they were
//! (only those in the window move).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use goblin_nightclub_core::chunk::{CHUNK, ChunkKey};
use goblin_nightclub_core::entity::{Entity, EntityId};
use goblin_nightclub_core::layer::{Edge, TILE};
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::vision::Vision;
use goblin_nightclub_core::world::{Build, ChunkSource, Pieces, Roofs, Window};

/// Words of bits for a chunk's tiles.
const WORDS: usize = CHUNK * CHUNK / 64;

pub(crate) struct Stream {
    pub source: Rc<dyn ChunkSource>,
    /// The window the game's level is, and the roofs over it.
    pub window: Window,
    pub roofs: Roofs,
    /// The next one, being made.
    pub next: Option<Build>,
    /// Pieces of the layers' geometry, kept from one window to the next.
    pub pieces: Arc<Mutex<Pieces>>,
    /// Per chunk, its tiles ever explored, a bit each, row by row.
    explored: HashMap<ChunkKey, Box<[u64; WORDS]>>,
    /// Outline edges ever seen: the layer, and the ends in eighths of a
    /// tile.
    seen: HashSet<(usize, [i64; 4])>,
    /// NPCs outside the window, by the chunk they're in.
    asleep: HashMap<ChunkKey, Vec<Entity>>,
    /// Chunks whose NPCs have been made.
    peopled: HashSet<ChunkKey>,
    next_id: EntityId,
    seed: u32,
}

impl Stream {
    pub fn new(
        source: Rc<dyn ChunkSource>,
        window: Window,
        roofs: Roofs,
        pieces: Arc<Mutex<Pieces>>,
        seed: u32,
    ) -> Self {
        Self {
            source,
            window,
            roofs,
            next: None,
            pieces,
            explored: HashMap::new(),
            seen: HashSet::new(),
            asleep: HashMap::new(),
            peopled: HashSet::new(),
            next_id: 0,
            seed,
        }
    }

    /// Tiles across and down, and layers, of the whole level.
    pub fn size(&self) -> [usize; 3] {
        self.source.size()
    }

    /// The level's tile at the window's tile `t`.
    pub fn absolute(&self, t: [i32; 2]) -> [i32; 2] {
        let o = self.window.tile_origin();
        [t[0] + o[0], t[1] + o[1]]
    }

    /// The chunk, and the bit in it, of the window's tile `(x, y)` on its
    /// layer `k`.
    fn bit(&self, k: usize, x: usize, y: usize) -> (ChunkKey, usize) {
        let [ax, ay] = self.absolute([x as i32, y as i32]);
        let c = CHUNK as i32;
        let key = (self.window.base + k, ax.div_euclid(c), ay.div_euclid(c));
        (key, (ay.rem_euclid(c) * c + ax.rem_euclid(c)) as usize)
    }

    /// An edge of the window's layer `k`, in the level.
    fn edge(&self, k: usize, e: &Edge) -> (usize, [i64; 4]) {
        let o = self.window.tile_origin();
        let at = |v: f32, axis: usize| ((v / TILE + o[axis] as f32) * 8.0).round() as i64;
        (
            self.window.base + k,
            [at(e.a[0], 0), at(e.a[1], 1), at(e.b[0], 0), at(e.b[1], 1)],
        )
    }

    /// Keep what `visions` (one a layer of `level`, the window's) have
    /// explored and seen.
    pub fn keep(&mut self, level: &Level, visions: &[Vision]) {
        for (k, v) in visions.iter().enumerate() {
            for [x, y] in v.explored_tiles() {
                let (key, bit) = self.bit(k, x, y);
                let words = self
                    .explored
                    .entry(key)
                    .or_insert_with(|| Box::new([0; WORDS]));
                words[bit / 64] |= 1 << (bit % 64);
            }
            let seen: Vec<_> = v
                .seen_edges(level.layer(k))
                .map(|e| self.edge(k, e))
                .collect();
            self.seen.extend(seen);
        }
    }

    /// Sight on each layer of `level` (the window's), `radius` far,
    /// knowing what's been explored and seen there before.
    pub fn visions(&self, level: &Level, radius: f32) -> Vec<Vision> {
        let w = self.window;
        (0..level.depth())
            .map(|k| {
                let layer = level.layer(k);
                // What's explored, a chunk at a time.
                let mut explored = vec![false; w.cols * w.rows];
                for cy in 0..w.chunks[1] {
                    for cx in 0..w.chunks[0] {
                        let key = (w.base + k, w.origin[0] + cx as i32, w.origin[1] + cy as i32);
                        let Some(words) = self.explored.get(&key) else {
                            continue;
                        };
                        for bit in 0..CHUNK * CHUNK {
                            let [x, y] = [cx * CHUNK + bit % CHUNK, cy * CHUNK + bit / CHUNK];
                            if x < w.cols && y < w.rows && words[bit / 64] & 1 << (bit % 64) != 0 {
                                explored[y * w.cols + x] = true;
                            }
                        }
                    }
                }
                let mut v = Vision::new(layer, radius);
                let seen = |e: &Edge| self.seen.contains(&self.edge(k, e));
                v.remember(layer, |x, y| explored[y * w.cols + x], seen);
                v
            })
            .collect()
    }

    /// Put `npcs` (on the window's layers, at its positions) to sleep,
    /// where they are in the level.
    pub fn sleep(&mut self, npcs: Vec<Entity>) {
        let o = self.window.tile_origin().map(|t| t as f32 * TILE);
        for mut npc in npcs {
            npc.layer += self.window.base;
            npc.pos = [npc.pos[0] + o[0], npc.pos[1] + o[1]];
            let c = npc
                .center()
                .map(|v| (v / TILE / CHUNK as f32).floor() as i32);
            self.asleep
                .entry((npc.layer, c[0], c[1]))
                .or_default()
                .push(npc);
        }
    }

    /// The NPCs in the window's chunks, at its positions: those asleep
    /// there woken, and `per_chunk` made in each chunk that's never had
    /// any, by chance (but the same chance every time for a level and
    /// seed), on open ground in `level` (the window's), at least `clear`
    /// from `avoid`.
    pub fn wake(
        &mut self,
        level: &Level,
        per_chunk: usize,
        avoid: [f32; 2],
        clear: f32,
    ) -> Vec<Entity> {
        let w = self.window;
        let o = w.tile_origin().map(|t| t as f32 * TILE);
        let mut npcs = Vec::new();
        for key in w.keys() {
            let (k, x, y) = w.local(key).unwrap();
            if let Some(asleep) = self.asleep.remove(&key) {
                npcs.extend(asleep.into_iter().map(|mut npc| {
                    npc.layer = k;
                    npc.pos = [npc.pos[0] - o[0], npc.pos[1] - o[1]];
                    npc
                }));
            } else if self.peopled.insert(key) {
                let (layer, cx, cy) = key;
                let mix = [layer as u32, cx as u32, cy as u32]
                    .into_iter()
                    .fold(self.seed ^ 0x9E37_79B9, |h, n| {
                        (h ^ n).wrapping_mul(0x0100_0193)
                    });
                let mut rng = Rng::new(mix);
                let edge = CHUNK as f32 * TILE;
                let at = [x as f32 * edge, y as f32 * edge];
                let size = [
                    edge.min(w.cols as f32 * TILE - at[0]),
                    edge.min(w.rows as f32 * TILE - at[1]),
                ];
                let made = Entity::spawn_npcs_in(
                    level.movement(k),
                    [at, size],
                    &mut rng,
                    per_chunk,
                    self.next_id,
                    avoid,
                    clear,
                );
                self.next_id += made.len() as EntityId;
                npcs.extend(made.into_iter().map(|mut npc| {
                    npc.layer = k;
                    npc
                }));
            }
        }
        npcs
    }
}
