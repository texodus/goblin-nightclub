//! A layer's tiles, stored in square chunks of [`CHUNK`] tiles a side. Each
//! chunk keeps only the distinct tiles it uses (its palette) and an index
//! into it per tile, so uniform stretches, like solid rock underground or
//! open sky above, cost next to nothing. Chunks are the unit levels are to
//! be loaded, generated and saved in.

use std::sync::Arc;

use crate::layer::Tile;

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
        Self { palette, cells }
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
        Some(Self { palette, cells })
    }

    /// Are the two the same tiles, written the same way (see
    /// [`identical`])?
    pub fn same(&self, other: &Chunk) -> bool {
        (0..CELLS).all(|i| {
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

    /// Chunks across and down.
    pub fn size(&self) -> [usize; 2] {
        [self.across, self.chunks.len() / self.across.max(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{Corner, Part};

    #[test]
    fn uniform_chunks_hold_one_tile() {
        let rock = Chunk::filled(Tile::WALL);
        assert_eq!(rock.uniform(), Some(Tile::WALL));
        assert_eq!(rock.get(31, 31), Tile::WALL);
        let made = Chunk::from_fn(|_, _| Tile::VOID);
        assert_eq!(made.uniform(), Some(Tile::VOID));
        assert_eq!(made.palette(), &[Tile::VOID]);
    }

    #[test]
    fn tiles_come_back_as_they_went_in() {
        let tile = |x: usize, y: usize| match (x + 2 * y) % 5 {
            0 => Tile::FLOOR,
            1 => Tile::WALL,
            2 => Tile::slope(Corner::CLOCKWISE[(x + y) % 4]),
            3 => Tile::VOID,
            _ => Tile::Whole(Part::Floor(((x * y) % 7) as u8)),
        };
        let chunk = Chunk::from_fn(tile);
        assert!(chunk.uniform().is_none());
        for y in 0..CHUNK {
            for x in 0..CHUNK {
                assert_eq!(chunk.get(x, y), tile(x, y), "({x}, {y})");
            }
        }
        // A split given from either corner is one tile to `==`, but comes
        // back as it was given.
        let given = |x: usize, _| {
            if x.is_multiple_of(2) {
                Tile::split(Corner::NW, Part::Wall(0), Part::Floor(0))
            } else {
                Tile::split(Corner::SE, Part::Floor(0), Part::Wall(0))
            }
        };
        let same = Chunk::from_fn(given);
        assert_eq!(same.palette().len(), 2);
        for x in 0..2 {
            assert!(identical(same.get(x, 0), given(x, 0)));
        }
    }

    #[test]
    fn more_than_256_tiles_take_two_bytes() {
        let tile = |x: usize, y: usize| {
            let i = y * CHUNK + x;
            if i < 256 {
                Tile::Whole(Part::Floor(i as u8))
            } else {
                Tile::Whole(Part::Wall((i % 256) as u8))
            }
        };
        let chunk = Chunk::from_fn(tile);
        assert_eq!(chunk.palette().len(), 512);
        assert!(matches!(chunk.cells, Cells::Wide(_)));
        assert_eq!(chunk.get(5, 20), tile(5, 20));
    }

    #[test]
    fn grids_of_any_size_read_back() {
        let (cols, rows) = (70, 33);
        let tiles: Vec<Tile> = (0..cols * rows)
            .map(|i| {
                if (i * 7) % 11 < 3 {
                    Tile::WALL
                } else {
                    Tile::FLOOR
                }
            })
            .collect();
        let grid = Chunks::from_tiles(cols, rows, &tiles);
        assert_eq!(grid.size(), [3, 2]);
        for y in 0..rows {
            for x in 0..cols {
                assert_eq!(grid.get(x, y), tiles[y * cols + x]);
            }
        }
        // The last chunk down is mostly past the edge; all one tile.
        let solid = Chunks::from_tiles(40, 40, &vec![Tile::WALL; 1600]);
        assert!((0..2).all(|c| solid.chunk(c, 1).uniform() == Some(Tile::WALL)));
    }
}
