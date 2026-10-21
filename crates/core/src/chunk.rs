//! A layer's tiles, stored in square chunks of [`CHUNK`] tiles a side. Each
//! chunk keeps only the distinct tiles it uses (its palette) and an index
//! into it per tile, so uniform stretches, like solid rock underground or
//! open sky above, cost next to nothing; and the props standing on its
//! tiles (see [`crate::prop`]), a short list. Chunks are the unit levels are to
//! be loaded, generated and saved in.

use std::sync::Arc;

use crate::layer::Tile;
use crate::prop::{Prop, PropId};

/// Tiles a side of a chunk.
pub const CHUNK: usize = 32;

/// A chunk of a level: its layer, then its column and row (in chunks).
pub type ChunkKey = (usize, i32, i32);

const CELLS: usize = CHUNK * CHUNK;

#[derive(Clone, Debug)]
pub struct Chunk {
    /// The distinct tiles, in order of first use.
    palette: Vec<Tile>,
    cells: Cells,
    /// The props on its tiles: each one's tile (its index, row by row)
    /// and kind, in order of tile, one a tile at most.
    props: Vec<(u16, PropId)>,
}

/// Each tile's index into the palette, row by row: none when it has one
/// tile, a byte while it has up to 256, else two.
#[derive(Clone, Debug)]
enum Cells {
    Uniform,
    Narrow(Box<[u8; CELLS]>),
    Wide(Box<[u16; CELLS]>),
}

impl Chunk {
    /// A chunk all of `tile`.
    pub fn filled(tile: Tile) -> Self {
        Self {
            palette: vec![tile],
            cells: Cells::Uniform,
            props: Vec::new(),
        }
    }

    /// A chunk of `tile(x, y)` for each tile in it, `x` and `y` from 0 to
    /// [`CHUNK`].
    pub fn from_fn(mut tile: impl FnMut(usize, usize) -> Tile) -> Self {
        let mut palette: Vec<Tile> = Vec::new();
        let mut indices = [0u16; CELLS];
        for (i, index) in indices.iter_mut().enumerate() {
            let t = tile(i % CHUNK, i / CHUNK);
            *index = match palette.iter().position(|p| identical(*p, t)) {
                Some(at) => at as u16,
                None => {
                    palette.push(t);
                    (palette.len() - 1) as u16
                }
            };
        }
        let cells = match palette.len() {
            1 => Cells::Uniform,
            ..=256 => Cells::Narrow(Box::new(indices.map(|i| i as u8))),
            _ => Cells::Wide(Box::new(indices)),
        };
        Self {
            palette,
            cells,
            props: Vec::new(),
        }
    }

    /// The tile at `(x, y)` in the chunk.
    pub fn get(&self, x: usize, y: usize) -> Tile {
        let i = y * CHUNK + x;
        match &self.cells {
            Cells::Uniform => self.palette[0],
            Cells::Narrow(cells) => self.palette[cells[i] as usize],
            Cells::Wide(cells) => self.palette[cells[i] as usize],
        }
    }

    /// Its one tile, if it has only one.
    pub fn uniform(&self) -> Option<Tile> {
        matches!(self.cells, Cells::Uniform).then(|| self.palette[0])
    }

    /// The distinct tiles it holds.
    pub fn palette(&self) -> &[Tile] {
        &self.palette
    }

    /// Each tile's index into the palette, row by row.
    pub fn indices(&self) -> impl Iterator<Item = u16> + '_ {
        (0..CELLS).map(|i| match &self.cells {
            Cells::Uniform => 0,
            Cells::Narrow(cells) => cells[i] as u16,
            Cells::Wide(cells) => cells[i],
        })
    }

    /// A chunk from its palette and each tile's index into it, row by row
    /// ([`CHUNK`] squared of them); none if they don't fit together.
    pub fn from_parts(palette: Vec<Tile>, indices: &[u16]) -> Option<Self> {
        let fits = indices.len() == CELLS && indices.iter().all(|&i| (i as usize) < palette.len());
        if palette.is_empty() || !fits {
            return None;
        }
        let cells = match palette.len() {
            1 => Cells::Uniform,
            ..=256 => Cells::Narrow(Box::new(std::array::from_fn(|i| indices[i] as u8))),
            _ => Cells::Wide(Box::new(std::array::from_fn(|i| indices[i]))),
        };
        Some(Self {
            palette,
            cells,
            props: Vec::new(),
        })
    }

    /// The prop on tile `(x, y)`, if there's one.
    pub fn prop(&self, x: usize, y: usize) -> Option<PropId> {
        let i = (y * CHUNK + x) as u16;
        let at = self.props.binary_search_by_key(&i, |&(t, _)| t).ok()?;
        Some(self.props[at].1)
    }

    /// Its props: each one's tile (its index, row by row) and kind, in
    /// order of tile.
    pub fn props(&self) -> &[(u16, PropId)] {
        &self.props
    }

    /// The same chunk with `props` on its tiles instead (each one's tile,
    /// its index row by row, and kind): of several on one tile, the last.
    pub fn with_props(mut self, mut props: Vec<(u16, PropId)>) -> Self {
        props.retain(|&(t, _)| (t as usize) < CELLS);
        props.reverse();
        props.sort_by_key(|&(t, _)| t);
        props.dedup_by_key(|&mut (t, _)| t);
        self.props = props;
        self
    }

    /// Are the two the same tiles, written the same way (see
    /// [`identical`])?
    pub fn same(&self, other: &Chunk) -> bool {
        self.props == other.props
            && (0..CELLS).all(|i| {
                identical(
                    self.get(i % CHUNK, i / CHUNK),
                    other.get(i % CHUNK, i / CHUNK),
                )
            })
    }
}

/// Are `a` and `b` written the same way? Stricter than `==`, for which a
/// split given from either corner is one tile: a chunk gives back each
/// tile exactly as it was put in.
pub fn identical(a: Tile, b: Tile) -> bool {
    match (a, b) {
        (Tile::Whole(p), Tile::Whole(q)) => p == q,
        (
            Tile::Split { corner, at, rest },
            Tile::Split {
                corner: c,
                at: a,
                rest: r,
            },
        ) => corner == c && at == a && rest == r,
        (Tile::Stairs(d, f), Tile::Stairs(e, g)) => d == e && f == g,
        _ => false,
    }
}

/// A grid of tiles `cols` x `rows`, in chunks row by row; the last row and
/// column of chunks may run past the grid's edge, and what's there is
/// never read.
#[derive(Clone, Debug)]
pub struct Chunks {
    cols: usize,
    rows: usize,
    /// Chunks across.
    across: usize,
    chunks: Vec<Arc<Chunk>>,
}

impl Chunks {
    /// From `tiles`, row by row.
    pub fn from_tiles(cols: usize, rows: usize, tiles: &[Tile]) -> Self {
        assert_eq!(tiles.len(), cols * rows);
        let (across, down) = (cols.div_ceil(CHUNK), rows.div_ceil(CHUNK));
        let chunks = (0..across * down)
            .map(|c| {
                let [x0, y0] = [c % across * CHUNK, c / across * CHUNK];
                Arc::new(Chunk::from_fn(|x, y| {
                    let [x, y] = [(x0 + x).min(cols - 1), (y0 + y).min(rows - 1)];
                    tiles[y * cols + x]
                }))
            })
            .collect();
        Self {
            cols,
            rows,
            across,
            chunks,
        }
    }

    /// From `chunks`, row by row, enough to cover the grid.
    pub fn from_chunks(cols: usize, rows: usize, chunks: Vec<Arc<Chunk>>) -> Self {
        let across = cols.div_ceil(CHUNK);
        assert_eq!(chunks.len(), across * rows.div_ceil(CHUNK));
        Self {
            cols,
            rows,
            across,
            chunks,
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// The tile at `(x, y)`, which must be inside the grid.
    pub fn get(&self, x: usize, y: usize) -> Tile {
        debug_assert!(x < self.cols && y < self.rows);
        self.chunks[y / CHUNK * self.across + x / CHUNK].get(x % CHUNK, y % CHUNK)
    }

    /// The chunk `(cx, cy)` chunks from the top left.
    pub fn chunk(&self, cx: usize, cy: usize) -> &Arc<Chunk> {
        &self.chunks[cy * self.across + cx]
    }

    /// The prop on tile `(x, y)`, which must be inside the grid, if there's
    /// one.
    pub fn prop(&self, x: usize, y: usize) -> Option<PropId> {
        self.chunks[y / CHUNK * self.across + x / CHUNK].prop(x % CHUNK, y % CHUNK)
    }

    /// Every prop, by chunk, then in order of tile.
    pub fn props(&self) -> impl Iterator<Item = Prop> + '_ {
        self.chunks.iter().enumerate().flat_map(move |(c, chunk)| {
            let [x0, y0] = [c % self.across * CHUNK, c / self.across * CHUNK];
            chunk.props().iter().map(move |&(t, kind)| {
                let at = [x0 + t as usize % CHUNK, y0 + t as usize / CHUNK];
                Prop { at, kind }
            })
        })
    }

    /// The same grid with `props` on its tiles instead (those outside it
    /// left out; of several on a tile, the last).
    pub fn with_props(mut self, props: &[Prop]) -> Self {
        let mut by_chunk: Vec<Vec<(u16, PropId)>> = vec![Vec::new(); self.chunks.len()];
        for p in props
            .iter()
            .filter(|p| p.at[0] < self.cols && p.at[1] < self.rows)
        {
            let [x, y] = p.at;
            let t = (y % CHUNK * CHUNK + x % CHUNK) as u16;
            by_chunk[y / CHUNK * self.across + x / CHUNK].push((t, p.kind));
        }
        for (chunk, props) in self.chunks.iter_mut().zip(by_chunk) {
            if !props.is_empty() || !chunk.props().is_empty() {
                *chunk = Arc::new(Chunk::clone(chunk).with_props(props));
            }
        }
        self
    }

    /// Chunks across and down.
    pub fn size(&self) -> [usize; 2] {
        [self.across, self.chunks.len() / self.across.max(1)]
    }
}

#[cfg(test)]
#[path = "tests/chunk_tests.rs"]
mod tests;
