//! Playing a level too big to keep whole, a window at a time (see
//! [`goblin_nightclub_core::world`]): making the next window as the player
//! moves on, and swapping it in. What's kept between windows, in the
//! level's own coordinates: the tiles explored on each layer (as the map,
//! shown beyond the window) and the outline edges seen, and the NPCs
//! outside the window, asleep where they were (only those in the window
//! move).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use goblin_nightclub_core::chunk::{CHUNK, ChunkKey};
use goblin_nightclub_core::entity::{Entity, EntityId};
use goblin_nightclub_core::layer::{Edge, TILE};
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::map::{Map, UNEXPLORED};
use goblin_nightclub_core::pieces::Pieces;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::vision::Vision;
use goblin_nightclub_core::world::{Build, ChunkSource, Roofs, Window};

use super::*;

pub(crate) struct Stream {
    pub source: Rc<dyn ChunkSource>,
    /// The window the game's level is, and the roofs over it.
    pub window: Window,
    pub roofs: Roofs,
    /// The next one, being made.
    pub next: Option<Build>,
    /// Pieces of the layers' geometry, kept from one window to the next.
    pub pieces: Arc<Mutex<Pieces>>,
    /// The tiles ever explored, as they are.
    pub map: Arc<Map>,
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
            map: Arc::new(Map::new(source.tileset())),
            source,
            window,
            roofs,
            next: None,
            pieces,
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
        let (o, base) = (self.window.tile_origin(), self.window.base);
        for (k, v) in visions.iter().enumerate() {
            let (layer, map) = (level.layer(k), Arc::make_mut(&mut self.map));
            for [x, y] in v.explored_tiles() {
                let t = [x as i32, y as i32];
                map.explore(base + k, [t[0] + o[0], t[1] + o[1]], layer.tile(t[0], t[1]));
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
                        let Some(chunk) = self.map.chunk(key) else {
                            continue;
                        };
                        for (i, code) in chunk.tiles.iter().enumerate() {
                            let [x, y] = [cx * CHUNK + i % CHUNK, cy * CHUNK + i / CHUNK];
                            if x < w.cols && y < w.rows && code[0] != UNEXPLORED {
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

impl Game {
    /// What's drawn of the next window, as it's made: to have ready for
    /// when it's swapped in.
    pub fn upcoming(&self) -> impl Iterator<Item = &Mesh> {
        let next = self.stream.iter().flat_map(|s| &s.next);
        next.flat_map(|b| b.meshes())
    }

    /// For a level played a window at a time: once the player has moved
    /// on from the middle of the window, make the next one around them, a
    /// piece each update, and swap it in when it's made.
    pub(super) fn advance_stream(&mut self) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let tile = stream.absolute(tile_of(center(self.player, PLAYER_SIZE)));
        let top = stream.window.base + self.top;
        let build = match &mut stream.next {
            Some(build) => build,
            None => {
                let want = Window::around(stream.size(), tile, top);
                if want == stream.window {
                    return;
                }
                stream.next.insert(
                    Build::new(stream.source.clone(), want, top).with_pieces(stream.pieces.clone()),
                )
            }
        };
        if !build.step() {
            return;
        }
        let build = stream.next.take().unwrap();
        // Unless the player's left it already (and the next is due).
        let chunk = tile.map(|t| t.div_euclid(CHUNK as i32));
        if build.window().local((top, chunk[0], chunk[1])).is_some() {
            self.swap(build);
        }
    }

    /// Move the game onto the window `build` makes: everything moves with
    /// the level under it, and what's kept between windows is kept.
    pub(super) fn swap(&mut self, build: Build) {
        let stream = self.stream.as_mut().unwrap();
        let (old, new) = (stream.window, build.window());
        stream.keep(&self.level, &self.visions);
        stream.sleep(std::mem::take(&mut self.npcs));
        let (level, roofs) = build.finish();
        stream.roofs = roofs;
        let [ox, oy] = old.tile_origin();
        let [nx, ny] = new.tile_origin();
        let tiles = [ox - nx, oy - ny];
        let by = tiles.map(|t| t as f32 * TILE);
        let shift = |p: [f32; 2]| [p[0] + by[0], p[1] + by[1]];
        self.player = shift(self.player);
        self.camera.pos = shift(self.camera.pos);
        self.top = old.base + self.top - new.base;
        if let Some(route) = &mut self.route {
            for t in &mut route.tiles {
                *t = [t[0] + tiles[0], t[1] + tiles[1]];
            }
            for w in &mut route.waypoints {
                *w = shift(*w);
            }
        }
        // Each layer's memories go with it, if it's still in the window.
        let size = [new.cols as f32 * TILE, new.rows as f32 * TILE];
        let mut memories: Vec<Memory> = (0..new.depth).map(|_| Memory::default()).collect();
        for (k, mut memory) in std::mem::take(&mut self.memories).into_iter().enumerate() {
            let at = (old.base + k)
                .checked_sub(new.base)
                .filter(|&k| k < new.depth);
            if let Some(k) = at {
                memory.shift(by, [[0.0; 2], size]);
                memories[k] = memory;
            }
        }
        self.memories = memories;
        stream.window = new;
        self.visions = stream.visions(&level, VIEW_RADIUS);
        let avoid = center(self.player, PLAYER_SIZE);
        self.npcs = stream.wake(&level, NPC_PER_CHUNK, avoid, NPC_SPAWN_DIST);
        let old = std::mem::replace(&mut self.level, level);
        self.litter.extend(old.take_apart());
        self.observed = None;
        self.observe();
    }

    /// The ground level, as a layer of the game's level: 0 if it's below
    /// the window, its depth if it's above.
    pub(super) fn local_ground(&self) -> usize {
        let base = self.stream.as_ref().map_or(0, |s| s.window.base);
        self.ground.saturating_sub(base).min(self.level.depth())
    }
}
