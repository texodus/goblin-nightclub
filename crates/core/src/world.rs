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

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::art::LayerMeshes;
use crate::chunk::{CHUNK, Chunk, ChunkKey, Chunks};
use crate::layer::{Layer, Piece, QUARTERS, Tile};
use crate::level::{Level, MAX_VISIBLE, movement_solid};
use crate::level_file::LevelFile;
use crate::mesh::Mesh;
use crate::pieces::{PieceOf, Pieces};
use crate::tileset::Tileset;

/// Chunks a window reaches either side of the chunk it's centered on.
pub const WINDOW_REACH: usize = 3;
/// Layers a window reaches below and above the player's: enough below to
/// see all that's drawn ([`MAX_VISIBLE`]) with a layer to spare for the
/// stairs, and above for the stairs.
pub const WINDOW_BELOW: usize = MAX_VISIBLE;
pub const WINDOW_ABOVE: usize = 2;

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
    fn chunk_size(&self, x: usize, y: usize) -> [usize; 2] {
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
#[path = "tests/world_tests.rs"]
mod tests;
