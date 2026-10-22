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

//! The outline of a layer's solid space: its edges, linked into loops, the
//! corners where it turns, and indexes of them by line and by place, all
//! worked out once when the layer is built.

use super::*;

/// A piece of the outline of solid space, oriented with solid on its right
/// (so outlines run clockwise on screen, since y points down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: [f32; 2],
    pub b: [f32; 2],
    /// The tile whose solid part this edge bounds; outside the level for
    /// the level's own boundary.
    pub solid: [i32; 2],
}

impl Edge {
    /// Which way it heads, in eighths of a turn clockwise on screen from
    /// east.
    pub fn heading(&self) -> i32 {
        let ([ax, ay], [bx, by]) = (vertex(self.a), vertex(self.b));
        match [(bx - ax).signum(), (by - ay).signum()] {
            [1, 0] => 0,
            [1, 1] => 1,
            [0, 1] => 2,
            [-1, 1] => 3,
            [-1, 0] => 4,
            [-1, -1] => 5,
            [0, -1] => 6,
            _ => 7,
        }
    }
}

/// A closed loop of outline, with solid on its right: clockwise on screen
/// around solid, counter-clockwise around holes and the level's boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// Where the loop turns, in order; it closes from the last back to the
    /// first.
    pub points: Vec<[f32; 2]>,
    /// For each segment `points[i]` to `points[i + 1]`, the level's outline
    /// edges (indices into [`Layer::outline_edges`]) it's made of, in order.
    pub runs: Vec<Vec<usize>>,
}

/// A corner where the outline turns away from solid (an inside corner,
/// seen from open space), with the inward normals of the faces before and
/// after it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConcaveCorner {
    pub at: [f32; 2],
    pub before: [f32; 2],
    pub after: [f32; 2],
}

/// A grid line outline edges can lie on, in tiles: horizontal (`y`),
/// vertical (`x`), or diagonal (`x + y` rising to the right, `x - y`
/// falling).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Line {
    Horizontal(i32),
    Vertical(i32),
    Rising(i32),
    Falling(i32),
}

impl Line {
    /// The grid lines running through both pixel-space points (within
    /// `tolerance` pixels).
    pub fn through(p: [f32; 2], q: [f32; 2], tolerance: f32) -> Vec<Line> {
        let on = |f: fn([f32; 2]) -> f32, make: fn(i32) -> Line| {
            let (a, b) = (f(p), f(q));
            let k = (a / TILE).round();
            ((a - b).abs() < tolerance && (a - k * TILE).abs() < tolerance).then(|| make(k as i32))
        };
        [
            on(|p| p[1], Line::Horizontal),
            on(|p| p[0], Line::Vertical),
            on(|p| p[0] + p[1], Line::Rising),
            on(|p| p[0] - p[1], Line::Falling),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// Indices of a layer's outline edges by the grid line each lies on: a
/// slot per line through a `cols` x `rows` layer, the horizontal ones top
/// to bottom, then the vertical, rising and falling ones, by their number.
#[derive(Default)]
pub(super) struct Lines {
    cols: i32,
    rows: i32,
    /// Where each slot's indices start in `items`, then the end.
    starts: Vec<u32>,
    items: Vec<usize>,
}

impl Lines {
    fn new(cols: usize, rows: usize, edges: &[Edge]) -> Self {
        let mut lines = Self {
            cols: cols as i32,
            rows: rows as i32,
            starts: Vec::new(),
            items: vec![0; edges.len()],
        };
        let slots = 3 * (cols + rows) + 4;
        let slot = |e: &Edge| lines.slot(line_of(e)).expect("edges lie inside");
        let slots_of: Vec<usize> = edges.iter().map(slot).collect();
        let mut starts = vec![0u32; slots + 1];
        for &k in &slots_of {
            starts[k + 1] += 1;
        }
        for k in 1..starts.len() {
            starts[k] += starts[k - 1];
        }
        let mut next = starts.clone();
        for (i, &k) in slots_of.iter().enumerate() {
            lines.items[next[k] as usize] = i;
            next[k] += 1;
        }
        lines.starts = starts;
        lines
    }

    /// `line`'s slot, if it runs through the layer.
    fn slot(&self, line: Line) -> Option<usize> {
        let (c, r) = (self.cols, self.rows);
        let (at, lo, hi, before) = match line {
            Line::Horizontal(y) => (y, 0, r, 0),
            Line::Vertical(x) => (x, 0, c, r + 1),
            Line::Rising(s) => (s, 0, c + r, r + c + 2),
            Line::Falling(d) => (d, -r, c, 2 * (r + c) + 3),
        };
        (lo..=hi).contains(&at).then(|| (before + at - lo) as usize)
    }

    fn on(&self, line: Line) -> &[usize] {
        let Some(k) = self.slot(line).filter(|&k| k + 1 < self.starts.len()) else {
            return &[];
        };
        &self.items[self.starts[k] as usize..self.starts[k + 1] as usize]
    }
}

/// The grid point at pixel position `p`, in tiles.
fn vertex(p: [f32; 2]) -> [i32; 2] {
    [(p[0] / TILE).round() as i32, (p[1] / TILE).round() as i32]
}

/// How far clockwise a turn from heading `from` to `to` is, in eighths of
/// a turn: from -3, sharply left, through 0, straight on, to 4, back.
fn turn(from: i32, to: i32) -> i32 {
    (to - from + 11).rem_euclid(8) - 3
}

/// The grid line an outline edge (a tile long) lies on.
fn line_of(e: &Edge) -> Line {
    let ([ax, ay], [bx, by]) = (vertex(e.a), vertex(e.b));
    if ay == by {
        Line::Horizontal(ay)
    } else if ax == bx {
        Line::Vertical(ax)
    } else if ax + ay == bx + by {
        Line::Rising(ax + ay)
    } else {
        Line::Falling(ax - ay)
    }
}

/// Grid point `(gx, gy)` in pixels, if the outline of a grid shaped
/// `solid(x, y)` turns there (or ends, or meets itself).
pub(super) fn corner_at(solid: &impl Fn(i32, i32) -> Solid, gx: i32, gy: i32) -> Option<[f32; 2]> {
    let nw = solid(gx - 1, gy - 1);
    let ne = solid(gx, gy - 1);
    let sw = solid(gx - 1, gy);
    let se = solid(gx, gy);

    // The ways outline runs from this vertex.
    let mut dirs = [[0i32; 2]; 8];
    let mut n = 0;
    // An axis edge is outline where exactly one side covers it.
    for (d, outline) in [
        ([0, -1], nw.covers(Side::E) != ne.covers(Side::W)),
        ([0, 1], sw.covers(Side::E) != se.covers(Side::W)),
        ([-1, 0], nw.covers(Side::S) != sw.covers(Side::N)),
        ([1, 0], ne.covers(Side::S) != se.covers(Side::N)),
    ] {
        if outline {
            dirs[n] = d;
            n += 1;
        }
    }
    // A slope's face is outline, running from this vertex across its tile
    // when the vertex is one of the face's ends.
    for (tile, d, at) in [
        (nw, [-1, -1], Corner::SE),
        (ne, [1, -1], Corner::SW),
        (sw, [-1, 1], Corner::NE),
        (se, [1, 1], Corner::NW),
    ] {
        if let Solid::Slope(s) = tile
            && at != s
            && at != s.opposite()
        {
            dirs[n] = d;
            n += 1;
        }
    }

    let straight = n == 2 && dirs[0][0] == -dirs[1][0] && dirs[0][1] == -dirs[1][1];
    (n > 0 && !straight).then_some([gx as f32 * TILE, gy as f32 * TILE])
}

/// Onto `out`, the outline edges of a grid shaped `solid(x, y)` along the
/// top and left sides of tile `(x, y)` (all of them, over every tile and
/// one more row and column), and its slope's face if it's `inside` the
/// grid; oriented with the solid side on the right.
pub(super) fn edges_at(
    solid: &impl Fn(i32, i32) -> Solid,
    x: i32,
    y: i32,
    inside: bool,
    out: &mut Vec<Edge>,
) {
    let px = |x: i32, y: i32| [x as f32 * TILE, y as f32 * TILE];
    let here = solid(x, y);
    let (above, left) = (solid(x, y - 1), solid(x - 1, y));
    match (here.covers(Side::N), above.covers(Side::S)) {
        (true, false) => out.push(Edge {
            a: px(x, y),
            b: px(x + 1, y),
            solid: [x, y],
        }),
        (false, true) => out.push(Edge {
            a: px(x + 1, y),
            b: px(x, y),
            solid: [x, y - 1],
        }),
        _ => {}
    }
    match (here.covers(Side::W), left.covers(Side::E)) {
        (true, false) => out.push(Edge {
            a: px(x, y + 1),
            b: px(x, y),
            solid: [x, y],
        }),
        (false, true) => out.push(Edge {
            a: px(x, y),
            b: px(x, y + 1),
            solid: [x - 1, y],
        }),
        _ => {}
    }
    if let Solid::Slope(c) = here
        && inside
    {
        // The face joins the two corners beside the solid one; orient it so
        // the solid corner is on its right.
        let [mut a, mut b] = c.face_ends().map(|k| k.of_tile([x, y]));
        let s = c.of_tile([x, y]);
        let (e, to_solid) = ([b[0] - a[0], b[1] - a[1]], [s[0] - a[0], s[1] - a[1]]);
        if -e[1] * to_solid[0] + e[0] * to_solid[1] < 0.0 {
            std::mem::swap(&mut a, &mut b);
        }
        out.push(Edge {
            a,
            b,
            solid: [x, y],
        });
    }
}

/// A chunk's own part of a layer's geometry, the same whichever window
/// it's seen through, so made once it can be reused by each (see
/// [`Layer::from_pieces`]): its tiles, their shapes, and the corners and
/// edges of the outline at its grid points and tile sides; all but those
/// along its top and left, which depend on the chunks beside it.
pub struct Piece {
    pub(super) tiles: Arc<Chunk>,
    /// Its tiles across and down in the level: fewer than [`CHUNK`] at the
    /// level's right and bottom edges.
    pub(super) size: [usize; 2],
    /// Row by row.
    pub(super) solids: Vec<Solid>,
    /// In pixels from its top-left corner, row by row (as a layer's are),
    /// and where each row's start in them, then the end.
    pub(super) corners: Vec<[f32; 2]>,
    pub(super) corner_rows: Vec<u32>,
    pub(super) edges: Vec<Edge>,
    pub(super) edge_rows: Vec<u32>,
}

impl Piece {
    /// The piece of a layer that's chunk `tiles`, `size` of it in the
    /// level.
    pub fn new(tiles: Arc<Chunk>, size: [usize; 2]) -> Self {
        let solids = (0..size[0] * size[1])
            .map(|i| tiles.get(i % size[0], i / size[0]).wall())
            .collect();
        Self::shaped(tiles, size, solids)
    }

    /// The piece of a layer of default tiles shaped `solids`, `size` of
    /// them row by row (see [`Layer::from_solids`]).
    pub fn from_solids(size: [usize; 2], solids: Vec<Solid>) -> Self {
        let [w, h] = size;
        let tiles =
            Chunk::from_fn(|x, y| Tile::from_solid(solids[y.min(h - 1) * w + x.min(w - 1)]));
        Self::shaped(Arc::new(tiles), size, solids)
    }

    fn shaped(tiles: Arc<Chunk>, size: [usize; 2], solids: Vec<Solid>) -> Self {
        let [w, h] = size.map(|n| n as i32);
        let solid = |x: i32, y: i32| solids[(y * w + x) as usize];
        let (mut corners, mut edges) = (Vec::new(), Vec::new());
        // The first row, along its top, has none.
        let (mut corner_rows, mut edge_rows) = (vec![0, 0], vec![0, 0]);
        for y in 1..h {
            for x in 1..w {
                corners.extend(corner_at(&solid, x, y));
                edges_at(&solid, x, y, true, &mut edges);
            }
            corner_rows.push(corners.len() as u32);
            edge_rows.push(edges.len() as u32);
        }
        Self {
            tiles,
            size,
            solids,
            corners,
            corner_rows,
            edges,
            edge_rows,
        }
    }
}

/// Edge of the square blocks [`Buckets`] sorts things into, in world units.
const BUCKET: f32 = 8.0 * TILE;

/// Indices of things in a level (corners, edges), by which square blocks
/// of the level their bounding boxes overlap.
#[derive(Default)]
pub(super) struct Buckets {
    /// Things in all.
    len: usize,
    /// Blocks across and down.
    blocks: [usize; 2],
    /// Where each block's indices start in `items`, row by row, then the
    /// end.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Buckets {
    /// Sort things into the blocks of a level `cols` x `rows` tiles, by
    /// their bounding boxes.
    fn new(cols: usize, rows: usize, boxes: &[Rect]) -> Self {
        let blocks = [cols, rows].map(|n| ((n as f32 * TILE / BUCKET).ceil() as usize).max(1));
        let mut buckets = Self {
            len: boxes.len(),
            blocks,
            starts: vec![0; blocks[0] * blocks[1] + 1],
            items: Vec::new(),
        };
        // Count each block's things, then place them.
        for &b in boxes {
            for k in buckets.blocks_over(b, 0.0) {
                buckets.starts[k + 1] += 1;
            }
        }
        for k in 1..buckets.starts.len() {
            buckets.starts[k] += buckets.starts[k - 1];
        }
        let mut next = buckets.starts.clone();
        buckets.items = vec![0; *buckets.starts.last().unwrap() as usize];
        for (i, &b) in boxes.iter().enumerate() {
            for k in buckets.blocks_over(b, 0.0) {
                buckets.items[next[k] as usize] = i as u32;
                next[k] += 1;
            }
        }
        buckets
    }

    /// Indices of the blocks that `rect`, grown by `margin`, overlaps.
    fn blocks_over(&self, rect: Rect, margin: f32) -> impl Iterator<Item = usize> + use<> {
        let span = |axis: usize| {
            let block =
                |v: f32| ((v / BUCKET).floor().max(0.0) as usize).min(self.blocks[axis] - 1);
            block(rect[0][axis] - margin)..=block(rect[1][axis] + margin)
        };
        let (xs, ys) = (span(0), span(1));
        let across = self.blocks[0];
        ys.flat_map(move |y| xs.clone().map(move |x| y * across + x))
    }

    /// Indices of the things that may come within `radius` of `p` (all of
    /// them for an infinite radius), in increasing order, each once.
    fn near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.over([p, p], radius)
    }

    /// Indices of the things that may reach into `rect` grown by `margin`
    /// (all of them for an infinite one), in increasing order, each once.
    fn over(&self, rect: Rect, margin: f32) -> Vec<usize> {
        let finite = rect.iter().flatten().all(|v| v.is_finite()) && margin.is_finite();
        if !finite || self.starts.is_empty() {
            return (0..self.len).collect();
        }
        let mut found: Vec<usize> = self
            .blocks_over(rect, margin)
            .flat_map(|k| &self.items[self.starts[k] as usize..self.starts[k + 1] as usize])
            .map(|&i| i as usize)
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }
}

impl Layer {
    /// The rest of a level made from its outline's corners and edges: the
    /// edges linked into loops, the concave corners, and all three by
    /// where they are.
    pub(super) fn link(mut self) -> Self {
        let (cols, rows) = (self.cols, self.rows);
        self.edges_by_line = Lines::new(cols, rows, &self.edges);
        self.next = self.link_edges();
        self.prev = vec![0; self.next.len()];
        for (i, &n) in self.next.iter().enumerate() {
            self.prev[n as usize] = i as u32;
        }
        self.concave = self.find_concave_corners();
        let corners: Vec<Rect> = self.corners.iter().map(|&c| [c, c]).collect();
        let edges: Vec<Rect> = self.edges.iter().map(|e| bounds(&[e.a, e.b])).collect();
        let concave: Vec<Rect> = self.concave.iter().map(|c| [c.at, c.at]).collect();
        self.corner_buckets = Buckets::new(cols, rows, &corners);
        self.edge_buckets = Buckets::new(cols, rows, &edges);
        self.concave_buckets = Buckets::new(cols, rows, &concave);
        self
    }

    /// The outline edges that may reach into `rect` grown by `margin`, in
    /// increasing order.
    pub fn edges_over(&self, rect: Rect, margin: f32) -> Vec<usize> {
        self.edge_buckets.over(rect, margin)
    }

    /// The outline edges before and after edge `i` along its loop.
    pub fn linked(&self, i: usize) -> [usize; 2] {
        [self.prev[i] as usize, self.next[i] as usize]
    }

    /// Grid vertices (in pixels) where the outline of solid space turns:
    /// the only places a visibility boundary can change direction. Gathers
    /// the outline edges meeting at each vertex (4 axis directions plus 4
    /// diagonals from slope faces); it's a corner unless exactly two meet
    /// and they continue straight.
    pub fn corners(&self) -> &[[f32; 2]] {
        &self.corners
    }

    pub(super) fn find_corners(&self) -> Vec<[f32; 2]> {
        let solid = |x: i32, y: i32| self.solid(x, y);
        let mut out = Vec::new();
        for gy in 0..=self.rows as i32 {
            for gx in 0..=self.cols as i32 {
                out.extend(corner_at(&solid, gx, gy));
            }
        }
        out
    }

    /// Every piece of the outline of solid space: tile sides with solid on
    /// exactly one side, and slope faces, each one tile long.
    pub fn outline_edges(&self) -> &[Edge] {
        &self.edges
    }

    pub(super) fn find_edges(&self) -> Vec<Edge> {
        let solid = |x: i32, y: i32| self.solid(x, y);
        let (cols, rows) = (self.cols as i32, self.rows as i32);
        let mut out = Vec::new();
        for y in 0..=rows {
            for x in 0..=cols {
                edges_at(&solid, x, y, x < cols && y < rows, &mut out);
            }
        }
        out
    }

    /// The outline of solid space as closed loops, straight runs merged.
    pub fn outlines(&self) -> Vec<Outline> {
        let mut used = vec![false; self.edges.len()];
        let mut out = Vec::new();
        for first in 0..self.edges.len() {
            if used[first] {
                continue;
            }
            // Walk the loop edge by edge.
            let mut chain = vec![first];
            used[first] = true;
            let mut at = self.next[first] as usize;
            while at != first {
                used[at] = true;
                chain.push(at);
                at = self.next[at] as usize;
            }

            // Start at a turn, then merge runs of edges going the same way.
            let dir = |k: usize| self.edges[chain[k]].heading();
            let n = chain.len();
            let start = (0..n).find(|&k| dir(k) != dir((k + n - 1) % n));
            chain.rotate_left(start.unwrap_or(0));
            let mut outline = Outline {
                points: Vec::new(),
                runs: Vec::new(),
            };
            for (k, &i) in chain.iter().enumerate() {
                if k == 0 || self.edges[i].heading() != self.edges[chain[k - 1]].heading() {
                    outline.points.push(self.edges[i].a);
                    outline.runs.push(Vec::new());
                }
                outline.runs.last_mut().unwrap().push(i);
            }
            out.push(outline);
        }
        out
    }

    /// Indices into [`Layer::outline_edges`] of every edge reaching
    /// within `radius` of `p`, and maybe a few more, in order.
    pub fn edges_near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.edge_buckets.near(p, radius)
    }

    /// Indices into [`Layer::corners`] of every corner within `radius` of
    /// `p`, and maybe a few more, in order.
    pub fn corners_near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.corner_buckets.near(p, radius)
    }

    /// Indices into [`Layer::concave_corners`] of every one within
    /// `radius` of `p`, and maybe a few more, in order.
    pub fn concave_near(&self, p: [f32; 2], radius: f32) -> Vec<usize> {
        self.concave_buckets.near(p, radius)
    }

    /// Indices of the outline edges lying on `line`.
    pub fn edges_on(&self, line: Line) -> &[usize] {
        self.edges_by_line.on(line)
    }

    /// Inside corners of the outline.
    pub fn concave_corners(&self) -> &[ConcaveCorner] {
        &self.concave
    }

    /// With solid on the right, a left turn (counter-clockwise on screen)
    /// bends away from the solid.
    pub(super) fn find_concave_corners(&self) -> Vec<ConcaveCorner> {
        let normal = |e: &Edge| inward_normal(e.a, e.b);
        (0..self.edges.len())
            .filter_map(|i| {
                let (before, here) = (&self.edges[self.prev[i] as usize], &self.edges[i]);
                (turn(before.heading(), here.heading()) < 0).then(|| ConcaveCorner {
                    at: here.a,
                    before: normal(before),
                    after: normal(here),
                })
            })
            .collect()
    }

    /// For each outline edge, the next along its loop: each edge's end is
    /// the start of another; where several start at one vertex (solids
    /// touching only at a corner), the sharpest right turn, which keeps the
    /// solid on the right the same and so keeps each solid's loop to
    /// itself.
    pub(super) fn link_edges(&self) -> Vec<u32> {
        // The edges by the vertex they start at.
        let across = self.cols + 1;
        let at = |p: [f32; 2]| {
            let [x, y] = vertex(p);
            y as usize * across + x as usize
        };
        let mut starts = vec![0u32; across * (self.rows + 1) + 1];
        for e in &self.edges {
            starts[at(e.a) + 1] += 1;
        }
        for k in 1..starts.len() {
            starts[k] += starts[k - 1];
        }
        let mut next = starts.clone();
        let mut items = vec![0u32; self.edges.len()];
        for (i, e) in self.edges.iter().enumerate() {
            let v = at(e.a);
            items[next[v] as usize] = i as u32;
            next[v] += 1;
        }
        let headings: Vec<i32> = self.edges.iter().map(Edge::heading).collect();
        self.edges
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let v = at(e.b);
                let from = &items[starts[v] as usize..starts[v + 1] as usize];
                // An outline is closed: something starts where each ends.
                *from
                    .iter()
                    .max_by_key(|&&j| turn(headings[i], headings[j as usize]))
                    .unwrap()
            })
            .collect()
    }
}
