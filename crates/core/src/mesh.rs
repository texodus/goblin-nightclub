//! Triangles that only change with a level's tiles: a layer's tile art,
//! where it's void or liquid, where it shows through to a layer above.
//! Grouped by square chunks of the level, so the renderer can keep them on
//! the GPU and each frame draw only the chunks in view.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::camera::Rect;
use crate::draw::{Canvas, Vertex};
use crate::layer::{TILE, quarter_shapes};

/// Tiles along each side of a chunk.
pub const CHUNK: usize = 16;

/// Tells meshes apart, so a renderer knows when it has one already.
static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub struct Mesh {
    id: u64,
    pub vertices: Vec<Vertex>,
    /// Chunks across and down.
    chunks: [usize; 2],
    /// Where each chunk's vertices start, row by row, then the end.
    starts: Vec<u32>,
    /// How far past its chunk a chunk's triangles may reach, in world
    /// units.
    reach: f32,
}

impl Mesh {
    /// A mesh over a level `cols` x `rows` tiles, built a chunk at a time:
    /// `draw(columns, rows, canvas)` adds the triangles of the tiles in
    /// those ranges, which must lie within them, or reach at most half a
    /// tile past them (fringes and trims do). `canvas` is empty to
    /// start and sets which sheet the triangles draw from.
    pub fn build(
        cols: usize,
        rows: usize,
        mut canvas: Canvas,
        mut draw: impl FnMut(Range<usize>, Range<usize>, &mut Canvas),
    ) -> Self {
        let chunks = [cols.div_ceil(CHUNK), rows.div_ceil(CHUNK)];
        let mut starts = Vec::with_capacity(chunks[0] * chunks[1] + 1);
        for cy in 0..chunks[1] {
            for cx in 0..chunks[0] {
                starts.push(canvas.vertices.len() as u32);
                let xs = cx * CHUNK..((cx + 1) * CHUNK).min(cols);
                let ys = cy * CHUNK..((cy + 1) * CHUNK).min(rows);
                draw(xs, ys, &mut canvas);
            }
        }
        starts.push(canvas.vertices.len() as u32);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            vertices: canvas.vertices,
            chunks,
            starts,
            reach: TILE / 2.0,
        }
    }

    /// The same mesh, its triangles reaching up to `reach` (world units)
    /// past their chunks instead of half a tile: props' art, which
    /// overhangs the tiles around its own.
    pub fn reaching(mut self, reach: f32) -> Self {
        self.reach = self.reach.max(reach);
        self
    }

    /// The parts of each tile that `quarters(x, y)` picks (see
    /// [`quarter_shapes`]), in a plain color: for masks. Whole tiles side
    /// by side in a row are one rectangle.
    pub fn quarters(
        cols: usize,
        rows: usize,
        canvas: Canvas,
        quarters: impl Fn(usize, usize) -> u8,
    ) -> Self {
        Self::build(cols, rows, canvas, |xs, ys, canvas| {
            for y in ys {
                let mut run: Option<usize> = None;
                let end_run = |canvas: &mut Canvas, run: &mut Option<usize>, x: usize| {
                    if let Some(start) = run.take() {
                        let [x0, x1] = [start as f32 * TILE, x as f32 * TILE];
                        let [y0, y1] = [y as f32 * TILE, (y + 1) as f32 * TILE];
                        canvas.poly(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], [1.0; 4]);
                    }
                };
                for x in xs.clone() {
                    let q = quarters(x, y);
                    if q == 0b1111 {
                        run.get_or_insert(x);
                        continue;
                    }
                    end_run(canvas, &mut run, x);
                    for shape in quarter_shapes(x, y, q) {
                        canvas.poly(&shape, [1.0; 4]);
                    }
                }
                end_run(canvas, &mut run, xs.end);
            }
        })
    }

    /// The vertices of its chunk `(cx, cy)`.
    pub fn chunk_vertices(&self, cx: usize, cy: usize) -> &[Vertex] {
        let i = cy * self.chunks[0] + cx;
        &self.vertices[self.starts[i] as usize..self.starts[i + 1] as usize]
    }

    /// Different for every mesh built, even with the same triangles.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// All of the vertices.
    pub fn all(&self) -> Range<u32> {
        0..self.vertices.len() as u32
    }

    /// Ranges of the vertices covering every chunk whose triangles may
    /// reach into `rect` (and maybe a little more): one range per row of
    /// chunks, as a row's chunks lie side by side in `vertices`. Empty
    /// ranges are left out.
    pub fn ranges(&self, rect: Rect) -> Vec<Range<u32>> {
        let size = CHUNK as f32 * TILE;
        let span = |axis: usize| {
            let lo = ((rect[0][axis] - self.reach) / size).floor().max(0.0);
            let hi = ((rect[1][axis] + self.reach) / size)
                .ceil()
                .min(self.chunks[axis] as f32);
            lo as usize..(hi as usize).max(lo as usize)
        };
        let (xs, ys) = (span(0), span(1));
        if xs.is_empty() {
            return Vec::new();
        }
        ys.map(|cy| {
            let row = cy * self.chunks[0];
            self.starts[row + xs.start]..self.starts[row + xs.end]
        })
        .filter(|r| !r.is_empty())
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tileset;

    fn area(vertices: &[Vertex]) -> f32 {
        vertices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn runs_of_whole_tiles_merge_within_chunks() {
        let tileset = tileset::testing::tileset();
        let cols = CHUNK + 4;
        // Row 0 whole, row 1 half tiles (quarters N and E), row 2 empty.
        let quarters = |_: usize, y: usize| [0b1111, 0b0011, 0][y];
        let mesh = Mesh::quarters(cols, 3, Canvas::new(&tileset), quarters);
        // Row 0 is a rectangle in each of the two chunks it crosses; each
        // half tile is a triangle.
        assert_eq!(mesh.vertices.len(), 2 * 6 + cols * 3);
        let whole = cols as f32 * TILE * TILE;
        assert!((area(&mesh.vertices) - 1.5 * whole).abs() < 1e-2);
    }

    #[test]
    fn ranges_cover_the_chunks_in_view() {
        let tileset = tileset::testing::tileset();
        let (cols, rows) = (3 * CHUNK, 2 * CHUNK);
        let mesh = Mesh::quarters(cols, rows, Canvas::new(&tileset), |x, y| {
            if (x + y) % 2 == 0 { 0b0001 } else { 0 }
        });
        let size = CHUNK as f32 * TILE;
        let count = |r: Rect| -> u32 { mesh.ranges(r).iter().map(|r| r.end - r.start).sum() };
        let per_chunk = (CHUNK * CHUNK / 2 * 3) as u32;
        // Inside the middle chunk of the top row, more than half a tile
        // from its sides.
        assert_eq!(
            count([[size + 6.0, 6.0], [2.0 * size - 6.0, 10.0]]),
            per_chunk
        );
        // Straddling the four chunks around a corner: two ranges.
        let corner = [[size - 1.0, size - 1.0], [size + 1.0, size + 1.0]];
        assert_eq!(mesh.ranges(corner).len(), 2);
        assert_eq!(count(corner), 4 * per_chunk);
        // Everything, and nothing past the level.
        assert_eq!(count(crate::camera::EVERYWHERE), mesh.all().end);
        assert_eq!(count([[-50.0, -50.0], [-6.0, -6.0]]), 0);
        assert_eq!(count([[1e4, 0.0], [2e4, 10.0]]), 0);
    }
}
