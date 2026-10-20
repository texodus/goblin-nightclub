//! Levels too big to keep whole. A level's tiles come from a
//! [`ChunkSource`], a chunk at a time, and the game plays on a [`Window`]
//! of them: a block of chunks around the player, on the layers around
//! theirs, made into an ordinary [`Level`] (so movement, sight and drawing
//! run on it unchanged). As the player moves on, the next window is made
//! in the background, a piece a frame ([`Build`]), and swapped in when
//! it's done; the window is wide enough that the player never gets near
//! its edge (which is wall, like any level's) before then. Only the tiles
//! right along its edge may look different from one window to the next
//! (their fringes and trims meet that wall), and those are always well
//! out of sight. A level that fits in one window is played whole, and
//! never moves.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::chunk::{CHUNK, Chunk, ChunkKey, Chunks};
use crate::layer::{Layer, Piece, QUARTERS, Tile};
use crate::level::{LayerMeshes, Level, MAX_VISIBLE, MeshPiece, movement_solid};
use crate::level_file::LevelFile;
use crate::mesh::Mesh;
use crate::tileset::Tileset;

/// Chunks a window reaches either side of the chunk it's centered on.
pub const WINDOW_REACH: usize = 3;
/// Layers a window reaches below and above the player's: enough below to
/// see all that's drawn ([`MAX_VISIBLE`]) with a layer to spare for the
/// stairs, and above for the stairs.
pub const WINDOW_BELOW: usize = MAX_VISIBLE;
pub const WINDOW_ABOVE: usize = 2;

/// [`Piece`]s kept for the next window: about two windows' worth.
pub const PIECES_KEPT: usize = 3000;
/// [`MeshPiece`]s kept for the next window: a window's worth (one a chunk
/// of each layer seen) and a column more, as they're big.
pub const MESH_PIECES_KEPT: usize = 450;

/// What a [`Piece`] is of: a layer, where it blocks movement, or where it
/// blocks the view of it from layer `top` (see [`Level::view`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceOf {
    Tiles,
    Movement,
    View { top: usize },
}

/// Pieces of layers made for a window, kept for the next one, which
/// shares most of its chunks: of their geometry ([`Piece`]s) and what's
/// drawn of them ([`MeshPiece`]s), those used most recently. Each is of a
/// chunk of the level, the same in any window.
#[derive(Default)]
pub struct Pieces {
    geometry: Kept<(PieceOf, ChunkKey), Piece>,
    meshes: Kept<ChunkKey, MeshPiece>,
}

impl Pieces {
    /// The piece of `of` over chunk `key`, made by `make` unless it's kept.
    pub fn get(&mut self, of: PieceOf, key: ChunkKey, make: impl FnOnce() -> Piece) -> Arc<Piece> {
        self.geometry.get((of, key), PIECES_KEPT, make)
    }

    /// What's drawn of chunk `key`, made by `make` unless it's kept.
    pub fn meshes(&mut self, key: ChunkKey, make: impl FnOnce() -> MeshPiece) -> Arc<MeshPiece> {
        self.meshes.get(key, MESH_PIECES_KEPT, make)
    }

    /// How many pieces of geometry have been made, all told.
    pub fn made(&self) -> usize {
        self.geometry.made
    }

    /// How many [`MeshPiece`]s have been made, all told.
    pub fn meshes_made(&self) -> usize {
        self.meshes.made
    }
}

/// Things kept by key, the most recently used.
struct Kept<K, T> {
    kept: HashMap<K, (Arc<T>, u64)>,
    /// Counts uses, to tell the most recent.
    clock: u64,
    /// How many have been made.
    made: usize,
}

impl<K, T> Default for Kept<K, T> {
    fn default() -> Self {
        Self {
            kept: HashMap::new(),
            clock: 0,
            made: 0,
        }
    }
}

impl<K: std::hash::Hash + Eq, T> Kept<K, T> {
    /// The thing kept by `key`, made by `make` if there's none, keeping
    /// about `most`.
    fn get(&mut self, key: K, most: usize, make: impl FnOnce() -> T) -> Arc<T> {
        self.clock += 1;
        let clock = self.clock;
        if let Some((thing, used)) = self.kept.get_mut(&key) {
            *used = clock;
            return thing.clone();
        }
        let thing = Arc::new(make());
        self.made += 1;
        self.kept.insert(key, (thing.clone(), clock));
        // Down to the most recent, a batch at a time.
        if self.kept.len() > most + most / 4 {
            let mut uses: Vec<u64> = self.kept.values().map(|&(_, used)| used).collect();
            let gone = self.kept.len() - most;
            let cut = *uses.select_nth_unstable(gone).1;
            self.kept.retain(|_, &mut (_, used)| used >= cut);
        }
        thing
    }
}

/// Where a level's chunks come from: a file, a generator, a server.
pub trait ChunkSource {
    /// Tiles across and down, and layers.
    fn size(&self) -> [usize; 3];

    /// The art its tiles refer to.
    fn tileset(&self) -> &Tileset;

    /// Chunk `key`, if it's ready. A slow source starts getting a chunk
    /// when it's first asked for, and has it for a later ask.
    fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>>;

    /// The roof over the chunk column `(cx, cy)` (see [`Roof`]), if it's
    /// ready, likewise.
    fn roof(&self, cx: i32, cy: i32) -> Option<Roof>;
}

/// Over each tile of a chunk column (row by row, [`CHUNK`] squared; those
/// past the level's edge 0), for each quarter of it (as in [`QUARTERS`]):
/// the lowest layer from which it's open, void (liquid included) or
/// stairs, on every layer above, up to the top; 0 when it's open all the
/// way down. Someone at `top` is under the open sky there when it's at
/// most `top + 1`.
pub type Roof = Arc<[[u16; 4]]>;

/// The roof over each tile of a level `tiles` (each layer's, row by row),
/// `cols` x `rows`, per chunk column, row by row.
pub fn roofs(cols: usize, rows: usize, tiles: &[Vec<Tile>]) -> Vec<Roof> {
    let (across, down) = (cols.div_ceil(CHUNK), rows.div_ceil(CHUNK));
    (0..across * down)
        .map(|c| {
            let [x0, y0] = [c % across * CHUNK, c / across * CHUNK];
            (0..CHUNK * CHUNK)
                .map(|i| {
                    let [x, y] = [x0 + i % CHUNK, y0 + i / CHUNK];
                    if x >= cols || y >= rows {
                        return [0; 4];
                    }
                    QUARTERS.map(|bit| {
                        let shut = |t: Tile| t.stairs().is_none() && t.void_quarters() & bit == 0;
                        (0..tiles.len())
                            .rev()
                            .find(|&k| shut(tiles[k][y * cols + x]))
                            .map_or(0, |k| k as u16 + 1)
                    })
                })
                .collect()
        })
        .collect()
}

/// A level file's tiles as chunks: all there from the start.
pub struct FileSource {
    size: [usize; 3],
    tileset: Tileset,
    layers: Vec<Chunks>,
    /// Per chunk column, row by row.
    roofs: Vec<Roof>,
}

impl FileSource {
    pub fn new(file: &LevelFile) -> Self {
        let (cols, rows) = (file.cols, file.rows);
        let tiles = file.tiles();
        Self {
            size: [cols, rows, tiles.len()],
            tileset: file.tileset.clone(),
            layers: tiles
                .iter()
                .zip(&file.props)
                .map(|(t, props)| Chunks::from_tiles(cols, rows, t).with_props(props))
                .collect(),
            roofs: roofs(cols, rows, &tiles),
        }
    }
}

impl ChunkSource for FileSource {
    fn size(&self) -> [usize; 3] {
        self.size
    }

    fn tileset(&self) -> &Tileset {
        &self.tileset
    }

    fn chunk(&self, (k, cx, cy): ChunkKey) -> Option<Arc<Chunk>> {
        let [across, down] = self.layers.get(k)?.size();
        let inside = (0..across as i32).contains(&cx) && (0..down as i32).contains(&cy);
        inside.then(|| self.layers[k].chunk(cx as usize, cy as usize).clone())
    }

    fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
        let across = self.size[0].div_ceil(CHUNK) as i32;
        let down = self.size[1].div_ceil(CHUNK) as i32;
        let inside = (0..across).contains(&cx) && (0..down).contains(&cy);
        inside.then(|| self.roofs[(cy * across + cx) as usize].clone())
    }
}

/// The roofs over a window's chunk columns.
pub struct Roofs {
    window: Window,
    /// Row by row.
    roofs: Vec<Roof>,
}

impl Roofs {
    /// Per quarter, over the level's tile `tile`, which must be in the
    /// window: see [`Roof`].
    pub fn at(&self, tile: [i32; 2]) -> [u16; 4] {
        let c = CHUNK as i32;
        let [cx, cy] = [
            tile[0].div_euclid(c) - self.window.origin[0],
            tile[1].div_euclid(c) - self.window.origin[1],
        ];
        let roof = &self.roofs[cy as usize * self.window.chunks[0] + cx as usize];
        roof[(tile[1].rem_euclid(c) * c + tile[0].rem_euclid(c)) as usize]
    }
}

/// Part of a level: whole chunks across and down, and layers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// Its top-left chunk.
    pub origin: [i32; 2],
    /// Chunks across and down.
    pub chunks: [usize; 2],
    /// Its tiles across and down (the last chunks may run past the level's
    /// edge).
    pub cols: usize,
    pub rows: usize,
    /// Its lowest layer, and how many.
    pub base: usize,
    pub depth: usize,
}

impl Window {
    /// The window of a level `size` (tiles across, down, and layers) for
    /// someone at tile `tile` on layer `top`: around them, but inside the
    /// level, so near its edges they're off center.
    pub fn around(size: [usize; 3], tile: [i32; 2], top: usize) -> Self {
        let span = |axis: usize| {
            let chunks = size[axis].div_ceil(CHUNK);
            let wide = (2 * WINDOW_REACH + 1).min(chunks);
            let center = tile[axis].div_euclid(CHUNK as i32);
            let first = (center - WINDOW_REACH as i32).clamp(0, (chunks - wide) as i32);
            let tiles = (wide * CHUNK).min(size[axis] - first as usize * CHUNK);
            (first, wide, tiles)
        };
        let ((x, across, cols), (y, down, rows)) = (span(0), span(1));
        let depth = (WINDOW_BELOW + WINDOW_ABOVE + 1).min(size[2]);
        let base = top.saturating_sub(WINDOW_BELOW).min(size[2] - depth);
        Self {
            origin: [x, y],
            chunks: [across, down],
            cols,
            rows,
            base,
            depth,
        }
    }

    /// Tiles across and down of its chunk `(x, y)` (from its top left)
    /// that are in the level.
    pub fn chunk_size(&self, x: usize, y: usize) -> [usize; 2] {
        [
            CHUNK.min(self.cols - x * CHUNK),
            CHUNK.min(self.rows - y * CHUNK),
        ]
    }

    /// Its top-left tile in the level.
    pub fn tile_origin(&self) -> [i32; 2] {
        self.origin.map(|c| c * CHUNK as i32)
    }

    /// Whether it's all of a level `size`.
    pub fn is_whole(&self, size: [usize; 3]) -> bool {
        [self.cols, self.rows, self.depth] == size
    }

    /// The level's chunk `key` holds, if it's in the window: as a local
    /// layer and chunk.
    pub fn local(&self, (k, cx, cy): ChunkKey) -> Option<(usize, usize, usize)> {
        let [x, y] = [cx - self.origin[0], cy - self.origin[1]];
        let inside = (self.base..self.base + self.depth).contains(&k)
            && (0..self.chunks[0] as i32).contains(&x)
            && (0..self.chunks[1] as i32).contains(&y);
        inside.then_some((k - self.base, x as usize, y as usize))
    }

    /// Its chunks, layer by layer, row by row.
    pub fn keys(&self) -> impl Iterator<Item = ChunkKey> + '_ {
        (self.base..self.base + self.depth).flat_map(move |k| {
            (0..self.chunks[1] as i32).flat_map(move |y| {
                (0..self.chunks[0] as i32).map(move |x| (k, self.origin[0] + x, self.origin[1] + y))
            })
        })
    }
}

/// A window being made into a [`Level`], a step at a time: the roofs over
/// it, each layer, then what's drawn of the layers seen from the
/// player's, then what they're seen through, so swapping it in costs
/// nothing more. Each layer's geometry is made of [`Piece`]s, those of
/// chunks a window before had reused.
pub struct Build {
    source: Rc<dyn ChunkSource>,
    pieces: Arc<Mutex<Pieces>>,
    window: Window,
    roofs: Option<Vec<Roof>>,
    /// The layer it's for, local to the window.
    top: usize,
    layers: Vec<Layer>,
    movement: Vec<Layer>,
    level: Option<Level>,
    /// How many of the pieces after the level are done.
    done: usize,
    /// What's drawn of the layers seen, as made, and where they show
    /// through.
    made: Vec<Arc<LayerMeshes>>,
    shafts: Vec<Arc<Mesh>>,
}

impl Build {
    /// Start making `window` from `source`, for someone on layer `top` of
    /// the level.
    pub fn new(source: Rc<dyn ChunkSource>, window: Window, top: usize) -> Self {
        Self {
            source,
            pieces: Default::default(),
            window,
            roofs: None,
            top: top.clamp(window.base, window.base + window.depth - 1) - window.base,
            layers: Vec::new(),
            movement: Vec::new(),
            level: None,
            done: 0,
            made: Vec::new(),
            shafts: Vec::new(),
        }
    }

    /// Reusing the pieces in `pieces` (those of the window before), and
    /// keeping those it makes there for the next.
    pub fn with_pieces(mut self, pieces: Arc<Mutex<Pieces>>) -> Self {
        self.pieces = pieces;
        self
    }

    /// The pieces it reuses and keeps.
    pub fn pieces(&self) -> Arc<Mutex<Pieces>> {
        self.pieces.clone()
    }

    pub fn window(&self) -> Window {
        self.window
    }

    /// The meshes of the layers seen made so far, which a renderer can
    /// have ready by the time the level's done.
    pub fn meshes(&self) -> impl Iterator<Item = &Mesh> {
        let shafts = self.shafts.iter().map(|m| &**m);
        self.made.iter().flat_map(|m| m.all()).chain(shafts)
    }

    /// Make one more piece: whether it's all made. A layer whose chunks
    /// aren't all ready waits for them.
    pub fn step(&mut self) -> bool {
        let w = self.window;
        if self.roofs.is_none() {
            let columns = (0..w.chunks[1] as i32).flat_map(|y| {
                (0..w.chunks[0] as i32).map(move |x| (w.origin[0] + x, w.origin[1] + y))
            });
            // Asking for them all, so a slow source gets them all at once.
            let roofs: Vec<_> = columns.map(|(x, y)| self.source.roof(x, y)).collect();
            self.roofs = roofs.into_iter().collect();
            return false;
        }
        let k = self.layers.len();
        if self.level.is_none() && k < w.depth {
            let keys: Vec<ChunkKey> = (0..w.chunks[1] as i32)
                .flat_map(|y| {
                    (0..w.chunks[0] as i32)
                        .map(move |x| (w.base + k, w.origin[0] + x, w.origin[1] + y))
                })
                .collect();
            // Likewise.
            let chunks: Vec<_> = keys.iter().map(|&key| self.source.chunk(key)).collect();
            let Some(chunks) = chunks.into_iter().collect::<Option<Vec<_>>>() else {
                return false;
            };
            let mut kept = self.pieces.lock().unwrap();
            let (mut tiles, mut movement) = (Vec::new(), Vec::new());
            for (i, (key, chunk)) in keys.into_iter().zip(chunks).enumerate() {
                let [w, h] = w.chunk_size(i % w.chunks[0], i / w.chunks[0]);
                let props = &self.source.tileset().props;
                movement.push(kept.get(PieceOf::Movement, key, || {
                    let at = |t: usize| {
                        let (x, y) = (t % w, t / w);
                        let blocked = chunk.prop(x, y).is_some_and(|k| props[k as usize].blocks);
                        movement_solid(chunk.get(x, y), blocked)
                    };
                    Piece::from_solids([w, h], (0..w * h).map(at).collect())
                }));
                tiles.push(kept.get(PieceOf::Tiles, key, || Piece::new(chunk, [w, h])));
            }
            drop(kept);
            let layer = Layer::from_pieces(w.cols, w.rows, &tiles).at(w.tile_origin());
            self.movement
                .push(Layer::from_pieces(w.cols, w.rows, &movement));
            self.layers.push(layer);
            return false;
        }
        let Some(level) = &self.level else {
            let (layers, movement) = (
                std::mem::take(&mut self.layers),
                std::mem::take(&mut self.movement),
            );
            let level = Level::from_parts(layers, movement, self.source.tileset().clone());
            self.level = Some(level.placed(w.base, w.origin, self.pieces.clone()));
            return false;
        };
        // Then a layer seen from `top` a piece: its art, then what it's
        // seen through.
        let seen = level.visible_layers(self.top);
        let Some(&k) = seen.get(self.done) else {
            return true;
        };
        self.made.push(level.meshes(k).clone());
        level.view(self.top, k);
        if k < self.top {
            self.shafts.push(level.shaft_mesh(self.top, k));
        }
        self.done += 1;
        self.done == seen.len()
    }

    /// The level, made the rest of the way at once, and the roofs over it:
    /// only when every chunk is ready (else it would wait forever).
    pub fn finish(mut self) -> (Level, Roofs) {
        while !self.step() {}
        let roofs = Roofs {
            window: self.window,
            roofs: self.roofs.unwrap(),
        };
        (self.level.unwrap(), roofs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{Canvas, Vertex};
    use crate::fringe::{draw_fringes, draw_trims};
    use crate::layer::{Dir, Part, Solid};
    use crate::level::MeshPiece;
    use crate::level_file::Spawn;
    use crate::mesh::Mesh;
    use crate::prop::{Prop, draw_prop};
    use crate::rng::Rng;

    /// Triangles of `vertices` moved `by` (as the renderer moves them),
    /// each vertex all it says, rounded (to a thousandth of a pixel).
    fn triangles(vertices: &[Vertex], by: [f32; 2]) -> Vec<[Vec<i64>; 3]> {
        let r = |v: f32| (v * 1000.0).round() as i64;
        let key = |v: &Vertex| {
            // Where a fringe grows from moves too; for anything else it
            // means nothing.
            let edge = if v.reach > 0.0 {
                [v.edge[0] + by[0], v.edge[1] + by[1], v.edge[2], v.edge[3]]
            } else {
                [0.0; 4]
            };
            let at = [v.pos[0] + by[0], v.pos[1] + by[1]];
            [&at[..], &v.uv, &v.cell, &v.color, &edge, &[v.reach, v.z]]
                .concat()
                .into_iter()
                .map(r)
                .collect::<Vec<_>>()
        };
        vertices
            .chunks(3)
            .map(|t| [key(&t[0]), key(&t[1]), key(&t[2])])
            .collect()
    }

    /// A level file `cols` x `rows` x `depth` of random tiles: floor,
    /// wall, void, splits, stairs.
    fn file(seed: u32, cols: usize, rows: usize, depth: usize) -> LevelFile {
        let mut rng = Rng::new(seed);
        let parts = [Part::Floor(0), Part::Wall(0), Part::Void, Part::Liquid(0)];
        let layers: Vec<Vec<Tile>> = (0..depth)
            .map(|_| {
                (0..cols * rows)
                    .map(|_| match rng.unit() {
                        r if r < 0.5 => Tile::FLOOR,
                        r if r < 0.55 => Tile::Stairs(Dir::Down, 0),
                        _ => Tile::split(
                            crate::layer::Corner::CLOCKWISE[rng.range(0, 3)],
                            parts[rng.range(0, 3)],
                            parts[rng.range(0, 3)],
                        ),
                    })
                    .collect()
            })
            .collect();
        let mut layers = layers;
        // Somewhere to stand.
        layers[depth - 1][0] = Tile::FLOOR;
        LevelFile::new(
            cols,
            rows,
            &layers,
            Spawn::at_tile(depth - 1, [0, 0]),
            crate::tileset::testing::tileset(),
            "player",
            "npc",
        )
        // And props here and there (not where the player starts).
        .with_props(
            (0..depth)
                .map(|_| {
                    let mut props = Vec::new();
                    for t in 1..cols * rows {
                        if rng.unit() < 0.05 {
                            let kind = rng.range(0, 1) as u8;
                            props.push(Prop {
                                at: [t % cols, t / cols],
                                kind,
                            });
                        }
                    }
                    props
                })
                .collect(),
        )
    }

    #[test]
    fn windows_center_on_the_player_inside_the_level() {
        let reach = WINDOW_REACH as i32;
        let wide = 2 * WINDOW_REACH + 1;
        let size = [1000, 700, 40];
        // In the middle: centered.
        let w = Window::around(size, [500, 300], 20);
        assert_eq!(w.origin, [500 / 32 - reach, 300 / 32 - reach]);
        assert_eq!(w.chunks, [wide, wide]);
        assert_eq!((w.cols, w.rows), (wide * CHUNK, wide * CHUNK));
        assert_eq!(
            (w.base, w.depth),
            (20 - WINDOW_BELOW, WINDOW_BELOW + WINDOW_ABOVE + 1)
        );
        // Near the edges: pushed inside, the last chunk cut at the edge.
        let w = Window::around(size, [995, 2], 1);
        assert_eq!(w.origin, [1000usize.div_ceil(32) as i32 - wide as i32, 0]);
        assert_eq!(w.cols, 1000 - w.origin[0] as usize * CHUNK);
        assert_eq!(w.base, 0);
        let top = Window::around(size, [0, 0], 39);
        assert_eq!(top.base + top.depth, 40);
        // A small level is one window, whole.
        let small = [100, 60, 7];
        assert!(Window::around(small, [50, 30], 6).is_whole(small));
        assert!(!w.is_whole(size));
        // Its chunks.
        assert_eq!(w.keys().count(), wide * wide * w.depth);
        assert_eq!(w.local((0, w.origin[0] + 1, 2)), Some((0, 1, 2)));
        assert_eq!(w.local((w.base + w.depth, w.origin[0], 0)), None);
        assert_eq!(w.local((0, w.origin[0] - 1, 0)), None);
    }

    #[test]
    fn a_window_is_the_level_seen_through_it() {
        let file = file(3, 260, 70, 11);
        let whole = file.level();
        let source = Rc::new(FileSource::new(&file));
        let w = Window::around(source.size(), [250, 60], 10);
        assert!(!w.is_whole(source.size()));
        let (level, _) = Build::new(source, w, 10).finish();
        let [x0, y0] = w.tile_origin();
        assert_eq!(level.depth(), w.depth);
        assert_eq!(level.layer(0).origin(), [x0, y0]);
        for k in 0..w.depth {
            for y in 0..w.rows as i32 {
                for x in 0..w.cols as i32 {
                    let [wx, wy] = [x + x0, y + y0];
                    let here = (k, x, y);
                    assert_eq!(
                        level.layer(k).tile(x, y),
                        whole.layer(w.base + k).tile(wx, wy),
                        "{here:?}"
                    );
                    assert_eq!(
                        level.movement(k).solid(x, y),
                        whole.movement(w.base + k).solid(wx, wy),
                        "{here:?}"
                    );
                }
            }
        }
        // Past its edge it's wall, like any level's.
        assert_eq!(level.layer(0).solid(-1, 0), Solid::Full);
    }

    #[test]
    fn tiles_look_the_same_through_any_window() {
        use crate::draw::Canvas;
        use crate::fringe::{draw_fringes, draw_trims};
        let mut file = file(6, 400, 40, 1);
        // Materials with variants, edges and trims, so there's a choice to
        // make the same way.
        file.tileset = crate::tileset::testing::tileset();
        file.tileset.floors[0].cells = (0..8).map(|i| [i, 1]).collect();
        file.tileset.floors[0].edge.width = 3.0;
        file.tileset.floors[0].trim = Some(crate::tileset::Trim {
            cells: vec![[9, 3], [10, 3]],
            density: 0.8,
            inset: 2.0,
        });
        let source: Rc<dyn ChunkSource> = Rc::new(FileSource::new(&file));
        let a = Window::around(source.size(), [100, 0], 0);
        let b = Window::around(source.size(), [200, 0], 0);
        assert_ne!(a.origin, b.origin);
        let (la, lb) = (
            Build::new(source.clone(), a, 0).finish().0,
            Build::new(source, b, 0).finish().0,
        );
        let tileset = &file.tileset;
        // Every tile both windows hold, drawn by each; but those along
        // either's edge, whose neighbors past it are wall to it (see the
        // module's docs).
        let (ax, bx) = (a.tile_origin()[0] as usize, b.tile_origin()[0] as usize);
        for x in bx + 1..ax + a.cols - 1 {
            for y in 0..40 {
                let draw = |level: &Level, local: usize| {
                    let mut canvas = Canvas::new(tileset);
                    let layer = level.layer(0);
                    layer.draw_tile(tileset, &mut canvas, local, y);
                    draw_fringes(layer, tileset, &mut canvas, local, y);
                    draw_trims(layer, tileset, &mut canvas, local, y);
                    // Where in the sheet, apart from where in the level.
                    canvas
                        .vertices
                        .iter()
                        .map(|v| (v.cell, v.color))
                        .collect::<Vec<_>>()
                };
                assert_eq!(draw(&la, x - ax), draw(&lb, x - bx), "({x}, {y})");
            }
        }
    }

    #[test]
    fn windows_reuse_the_pieces_of_the_one_before() {
        use crate::level::movement_layer;
        let mut file = file(7, 300, 90, 5);
        // Art with variants, fringes and trims, so there's all of it to draw.
        file.tileset.floors[0].cells = (0..8).map(|i| [i, 1]).collect();
        file.tileset.floors[0].edge.width = 3.0;
        file.tileset.floors[0].trim = Some(crate::tileset::Trim {
            cells: vec![[9, 3], [10, 3]],
            density: 0.8,
            inset: 2.0,
        });
        let whole = file.level();
        let source: Rc<dyn ChunkSource> = Rc::new(FileSource::new(&file));
        let (a, b) = (
            Window::around(source.size(), [100, 40], 4),
            Window::around(source.size(), [140, 40], 4),
        );
        assert_eq!(b.origin, [a.origin[0] + 1, a.origin[1]]);
        let first = Build::new(source.clone(), a, 4);
        let pieces = first.pieces();
        first.finish();
        let (made, drawn) = {
            let p = pieces.lock().unwrap();
            (p.made(), p.meshes_made())
        };
        let (level, _) = Build::new(source.clone(), b, 4)
            .with_pieces(pieces.clone())
            .finish();
        // Only the chunks new to it were made: a column, on each layer, of
        // tiles, movement and each view.
        let p = pieces.lock().unwrap();
        let seen = level.visible_layers(4).len();
        assert_eq!(p.made() - made, b.chunks[1] * (2 * b.depth + seen));
        // And what's drawn, of each layer seen.
        assert_eq!(p.meshes_made() - drawn, b.chunks[1] * seen);
        drop(p);

        // And all is as if made afresh, here and at the level's corner,
        // where its chunks are cut short.
        let afresh = |level: &Level, b: Window| {
            let [x0, y0] = b.tile_origin();
            let layers: Vec<Layer> = (0..b.depth)
                .map(|k| {
                    let tile = |t: usize| {
                        let [x, y] = [(t % b.cols) as i32 + x0, (t / b.cols) as i32 + y0];
                        whole.layer(b.base + k).tile(x, y)
                    };
                    // Its props, those in the window, where they are in it.
                    let props: Vec<Prop> = whole
                        .layer(b.base + k)
                        .props()
                        .filter_map(|p| {
                            let [x, y] = [p.at[0] as i32 - x0, p.at[1] as i32 - y0];
                            let inside =
                                (0..b.cols as i32).contains(&x) && (0..b.rows as i32).contains(&y);
                            inside.then_some(Prop {
                                at: [x as usize, y as usize],
                                kind: p.kind,
                            })
                        })
                        .collect();
                    Layer::from_tiles(b.cols, b.rows, (0..b.cols * b.rows).map(tile).collect())
                        .at([x0, y0])
                        .with_props(&props)
                })
                .collect();
            let movement = layers
                .iter()
                .map(|l| movement_layer(l, &file.tileset))
                .collect();
            let fresh = Level::from_parts(layers, movement, file.tileset.clone());
            let same = |a: &Layer, b: &Layer, what: &str| {
                assert_eq!(a.corners(), b.corners(), "{what}");
                assert_eq!(a.outline_edges(), b.outline_edges(), "{what}");
                assert_eq!(a.concave_corners(), b.concave_corners(), "{what}");
            };
            for k in 0..b.depth {
                same(level.layer(k), fresh.layer(k), &format!("layer {k}"));
                same(
                    level.movement(k),
                    fresh.movement(k),
                    &format!("movement {k}"),
                );
            }
            for k in level.visible_layers(4) {
                same(
                    &level.view(4, k),
                    &fresh.view(4, k),
                    &format!("view of {k}"),
                );
                // What's drawn: each piece's triangles, where it goes, and
                // those along the chunks' edges, are those of drawing the
                // whole layer at once (to within rounding where they're
                // moved).
                let (m, layer) = (level.meshes(k), fresh.layer(k));
                let tileset = &file.tileset;
                let whole = |draw: fn(&Layer, &Tileset, &mut Canvas, usize, usize)| {
                    let mut canvas = Canvas::new(tileset);
                    for y in 0..b.rows {
                        for x in 0..b.cols {
                            draw(layer, tileset, &mut canvas, x, y);
                        }
                    }
                    triangles(&canvas.vertices, [0.0; 2])
                };
                let quarters = |q: fn(Tile) -> u8| {
                    let mesh = Mesh::quarters(b.cols, b.rows, Canvas::new(tileset), |x, y| {
                        q(layer.tile(x as i32, y as i32))
                    });
                    triangles(&mesh.vertices, [0.0; 2])
                };
                let pieces = |of: fn(&MeshPiece) -> &Mesh, along: Option<&Mesh>| {
                    let mut all = triangles(along.map_or(&[], |m| &m.vertices), [0.0; 2]);
                    for (i, piece) in m.pieces.iter().enumerate() {
                        all.extend(triangles(&of(piece).vertices, m.corner(i)));
                    }
                    all.sort();
                    all
                };
                let draw_tile: fn(&Layer, &Tileset, &mut Canvas, usize, usize) =
                    |l, t, c, x, y| l.draw_tile(t, c, x, y);
                let pairs = [
                    (pieces(|p| &p.tiles, None), whole(draw_tile), "tiles"),
                    (
                        pieces(|p| &p.fringes, Some(&m.fringes)),
                        whole(draw_fringes),
                        "fringes",
                    ),
                    (
                        pieces(|p| &p.trims, Some(&m.trims)),
                        whole(draw_trims),
                        "trims",
                    ),
                    (
                        pieces(|p| &p.voids, None),
                        quarters(|t| t.void_quarters()),
                        "voids",
                    ),
                    (
                        pieces(|p| &p.liquid, None),
                        quarters(|t| t.liquid_quarters()),
                        "liquid",
                    ),
                    (pieces(|p| &p.props, None), whole(draw_prop), "props"),
                ];
                for (drawn, mut expected, what) in pairs {
                    expected.sort();
                    assert!(!expected.is_empty() || what == "liquid", "{what} of {k}");
                    assert!(drawn == expected, "{what} of {k}");
                }
            }
        };
        afresh(&level, b);
        let c = Window::around(source.size(), [295, 85], 4);
        assert!(!c.cols.is_multiple_of(CHUNK) && !c.rows.is_multiple_of(CHUNK));
        let (corner, _) = Build::new(source.clone(), c, 4)
            .with_pieces(pieces)
            .finish();
        afresh(&corner, c);
    }

    #[test]
    fn builds_go_a_piece_at_a_time_and_wait_for_chunks() {
        struct Slow(
            FileSource,
            std::sync::Mutex<std::collections::HashSet<ChunkKey>>,
        );
        impl ChunkSource for Slow {
            fn size(&self) -> [usize; 3] {
                self.0.size()
            }
            fn tileset(&self) -> &Tileset {
                self.0.tileset()
            }
            /// Ready from the second time it's asked.
            fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
                let asked = !self.1.lock().unwrap().insert(key);
                asked.then(|| self.0.chunk(key)).flatten()
            }
            fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
                self.0.roof(cx, cy)
            }
        }
        let file = file(4, 80, 64, 3);
        let source = Rc::new(Slow(FileSource::new(&file), Default::default()));
        let w = Window::around(source.size(), [10, 10], 2);
        let mut build = Build::new(source, w, 2);
        let mut steps = 1;
        while !build.step() {
            steps += 1;
            assert!(steps < 100);
        }
        // The roofs, each layer, the level, then each layer seen; each
        // layer waiting once for its chunks, all asked for together.
        let seen = build.level.as_ref().unwrap().visible_layers(2).len();
        assert_eq!(steps, 1 + 2 * w.depth + 1 + seen);
        // What's drawn of those, to have ready: what the level draws.
        let made: Vec<u64> = build.meshes().map(|m| m.id()).collect();
        let (level, _) = build.finish();
        let seen = level.visible_layers(2);
        let mut drawn: Vec<u64> = seen
            .iter()
            .flat_map(|&k| level.meshes(k).all().map(|m| m.id()))
            .collect();
        let below = seen.iter().filter(|&&k| k < 2);
        drawn.extend(below.map(|&k| level.shaft_mesh(2, k).id()));
        assert_eq!(made, drawn);
        // And it comes apart: each layer, each to move by, and each seen
        // through.
        assert!(level.take_apart().len() >= 2 * w.depth);
    }

    #[test]
    fn roofs_are_the_lowest_layer_open_from_there_up() {
        let file = file(5, 40, 10, 6);
        let source = Rc::new(FileSource::new(&file));
        let level = file.level();
        let window = Window::around(source.size(), [0, 0], 0);
        let (_, roofs) = Build::new(source.clone(), window, 0).finish();
        assert!(source.roof(2, 0).is_none(), "past the edge");
        for y in 0..10 {
            for x in 0..40 {
                let roof = roofs.at([x as i32, y as i32]);
                for (q, bit) in QUARTERS.into_iter().enumerate() {
                    for top in 0..6 {
                        let open = |k: usize| {
                            let tile = level.layer(k).tile(x as i32, y as i32);
                            tile.stairs().is_some() || tile.void_quarters() & bit != 0
                        };
                        let sky = (top + 1..6).all(open);
                        assert_eq!(
                            roof[q] as usize <= top + 1,
                            sky,
                            "({x}, {y}) {q} from {top}"
                        );
                        let column = source.roof(x as i32 / 32, 0).unwrap();
                        assert_eq!(column[y * 32 + x % 32], roof);
                    }
                }
            }
        }
    }
}
