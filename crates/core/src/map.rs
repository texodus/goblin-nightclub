//! A map of what's been explored of a level played a window at a time
//! (see [`crate::world`]), shown beyond the window: for each tile
//! explored, a code for each quarter of it (see [`QUARTERS`]) naming what's
//! drawn there. Kept a chunk at a time, only chunks with something explored.
//! Nothing on the map is ever in view, so that's all it holds: no props,
//! no entities, no outlines, no light.
//!
//! [`QUARTERS`]: crate::layer::QUARTERS

use std::collections::HashMap;
use std::sync::Arc;

use crate::chunk::{CHUNK, ChunkKey};
use crate::layer::{Dir, Part, Tile};
use crate::tileset::{Cell, Tileset};

/// A quarter of a tile never explored.
pub const UNEXPLORED: u8 = 0;
/// Codes for void (the layer below showing through) and either way of
/// stairs; after those, the tileset's floors, walls and liquids, in order.
pub const VOID: u8 = 1;
const STAIRS_UP: u8 = 2;
const STAIRS_DOWN: u8 = 3;
const FLOORS: usize = 4;

/// A chunk's tiles, row by row: each quarter's code.
pub type MapTiles = [[u8; 4]; CHUNK * CHUNK];

#[derive(Clone)]
pub struct MapChunk {
    /// The map's version when this last changed (see [`Map::version`]).
    pub version: u64,
    pub tiles: Box<MapTiles>,
}

/// What a code is drawn as: its art's cells (a tile's picked as
/// [`crate::tileset::variant`] picks it), none for void or what's
/// unexplored; and how opaque it is.
#[derive(Clone, Debug, PartialEq)]
pub struct MapArt {
    pub cells: Vec<Cell>,
    pub alpha: f32,
}

#[derive(Clone)]
pub struct Map {
    chunks: HashMap<ChunkKey, MapChunk>,
    /// Per layer, the chunks it has: the least column and row, and one
    /// past the most.
    bounds: HashMap<usize, [[i32; 2]; 2]>,
    /// By code.
    art: Arc<[MapArt]>,
    /// The codes of the first wall and the first liquid.
    walls: usize,
    liquids: usize,
    version: u64,
}

impl Map {
    /// Nothing explored of a level drawn with `tileset`.
    pub fn new(tileset: &Tileset) -> Self {
        let walls = FLOORS + tileset.floors.len();
        let liquids = walls + tileset.walls.len();
        let art = |cells: &[Cell], alpha| MapArt {
            cells: cells.to_vec(),
            alpha,
        };
        let stairs = tileset.stairs.map(|cell| art(&[cell], 1.0));
        let art = [art(&[], 1.0), art(&[], 0.0)]
            .into_iter()
            .chain(stairs)
            .chain(tileset.floors.iter().map(|f| art(&f.cells, 1.0)))
            .chain(tileset.walls.iter().map(|w| art(&w.cells, 1.0)))
            .chain(tileset.liquids.iter().map(|l| art(&l.cells, l.alpha)))
            .take(u8::MAX as usize + 1)
            .collect();
        Self {
            chunks: HashMap::new(),
            bounds: HashMap::new(),
            art,
            walls,
            liquids,
            version: 0,
        }
    }

    /// The codes of `tile`'s quarters. A tileset with more than fits in a
    /// byte has its last shown as the last that does.
    pub fn code(&self, tile: Tile) -> [u8; 4] {
        if let Some(dir) = tile.stairs() {
            return [if dir == Dir::Up {
                STAIRS_UP
            } else {
                STAIRS_DOWN
            }; 4];
        }
        tile.parts().map(|part| {
            let code = match part {
                Part::Void => VOID as usize,
                Part::Floor(f) => FLOORS + f as usize,
                Part::Wall(w) => self.walls + w as usize,
                Part::Liquid(l) => self.liquids + l as usize,
            };
            u8::try_from(code).unwrap_or(u8::MAX)
        })
    }

    /// Tile `t` (in the level) of layer `layer` is explored, and is `tile`.
    pub fn explore(&mut self, layer: usize, t: [i32; 2], tile: Tile) {
        let c = CHUNK as i32;
        let [cx, cy] = t.map(|v| v.div_euclid(c));
        let code = self.code(tile);
        let bounds = &mut self.bounds;
        let chunk = self.chunks.entry((layer, cx, cy)).or_insert_with(|| {
            let b = bounds.entry(layer).or_insert([[cx, cy], [cx + 1, cy + 1]]);
            *b = [
                [b[0][0].min(cx), b[0][1].min(cy)],
                [b[1][0].max(cx + 1), b[1][1].max(cy + 1)],
            ];
            MapChunk {
                version: 0,
                tiles: Box::new([[UNEXPLORED; 4]; CHUNK * CHUNK]),
            }
        });
        let i = (t[1].rem_euclid(c) * c + t[0].rem_euclid(c)) as usize;
        if chunk.tiles[i] != code {
            chunk.tiles[i] = code;
            self.version += 1;
            chunk.version = self.version;
        }
    }

    /// Chunk `key`'s tiles, if anything on it's been explored.
    pub fn chunk(&self, key: ChunkKey) -> Option<&MapChunk> {
        self.chunks.get(&key)
    }

    /// Is tile `t` (in the level) of layer `layer` explored?
    pub fn is_explored(&self, layer: usize, t: [i32; 2]) -> bool {
        let c = CHUNK as i32;
        let [cx, cy] = t.map(|v| v.div_euclid(c));
        let i = (t[1].rem_euclid(c) * c + t[0].rem_euclid(c)) as usize;
        self.chunk((layer, cx, cy))
            .is_some_and(|chunk| chunk.tiles[i][0] != UNEXPLORED)
    }

    /// The chunks layer `layer` has, if any: the least column and row, and
    /// one past the most.
    pub fn bounds(&self, layer: usize) -> Option<[[i32; 2]; 2]> {
        self.bounds.get(&layer).copied()
    }

    /// What each code is drawn as.
    pub fn art(&self) -> &Arc<[MapArt]> {
        &self.art
    }

    /// Goes up whenever anything on the map changes.
    pub fn version(&self) -> u64 {
        self.version
    }
}
