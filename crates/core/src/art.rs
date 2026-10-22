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

//! What's drawn of a layer that only changes with its tiles, kept a chunk
//! at a time so a window can reuse the chunks it shares with the one
//! before (see [`crate::world`]), and the GPU keep them as long; and how
//! a layer's tiles and their bevels are drawn.

use std::sync::Arc;

use crate::chunk::CHUNK;
use crate::draw::{Canvas, faces_light};
use crate::fringe;
use crate::geom::{Rect, Shape, inset_corner, inward_normal};
use crate::layer::{Dir, Halves, Layer, Part, Solid, TILE};
use crate::mesh::Mesh;
use crate::palette;
use crate::prop::draw_prop;
use crate::tileset::{self, Tileset};

/// Draw the art of tile `(x, y)` from `tileset`: each part's floor,
/// wall or liquid (translucent), nothing where void, stairs over the
/// whole tile.
pub fn draw_tile(layer: &Layer, tileset: &Tileset, canvas: &mut Canvas, x: usize, y: usize) {
    let tile = layer.tile(x as i32, y as i32);
    let origin = [x as f32 * TILE, y as f32 * TILE];
    let [ax, ay] = layer.absolute(x, y);
    let mut part = |shape: Option<Shape>, part: Part| {
        let (cell, color) = match part {
            Part::Floor(f) => (
                tileset::variant(&tileset.floors[f as usize].cells, ax, ay),
                palette::WHITE,
            ),
            Part::Wall(w) => (
                tileset::variant(&tileset.walls[w as usize].cells, ax, ay),
                palette::WHITE,
            ),
            // Translucent, so what's beneath shows through.
            Part::Liquid(l) => {
                let liquid = &tileset.liquids[l as usize];
                (
                    tileset::variant(&liquid.cells, ax, ay),
                    [1.0, 1.0, 1.0, liquid.alpha],
                )
            }
            Part::Void => return,
        };
        if let Some(shape) = shape {
            canvas.textured(&shape, origin, cell, color);
        }
    };
    if let Some(dir) = tile.stairs() {
        let cell = match dir {
            Dir::Up => tileset.stairs[0],
            Dir::Down => tileset.stairs[1],
        };
        canvas.textured(
            &Solid::Full.shape(x, y).unwrap(),
            origin,
            cell,
            palette::WHITE,
        );
        return;
    }
    match tile.halves() {
        Halves::Whole(p) => part(Solid::Full.shape(x, y), p),
        Halves::Split(c, at, rest) => {
            part(Solid::Slope(c).shape(x, y), at);
            part(Solid::Slope(c.opposite()).shape(x, y), rest);
        }
    }
}

/// Draw every tile's art (see [`draw_tile`]).
pub fn draw_tiles(layer: &Layer, tileset: &Tileset, canvas: &mut Canvas) {
    for y in 0..layer.rows {
        for x in 0..layer.cols {
            draw_tile(layer, tileset, canvas, x, y);
        }
    }
}

/// Draw a bevel `width` wide along the outline edges `bevel(edge
/// index)` picks that reach into `within`: the wall's light color on
/// faces facing up or left, its dark color on faces facing down or
/// right. Strips along each loop are mitered at its corners so they
/// meet exactly. Walls with a fringe (see [`crate::fringe`]) have none:
/// their fringe's rim takes its place.
pub fn draw_bevels(
    layer: &Layer,
    tileset: &Tileset,
    canvas: &mut Canvas,
    bevel: impl Fn(usize) -> bool,
    width: f32,
    within: Rect,
) {
    let [lo, hi] = within;
    let drawn = |i: usize| {
        let e = &layer.outline_edges()[i];
        let reaches = |axis: usize| {
            e.a[axis].max(e.b[axis]) + width >= lo[axis]
                && e.a[axis].min(e.b[axis]) - width <= hi[axis]
        };
        let fringed = || {
            let wall = layer.tile(e.solid[0], e.solid[1]).wall_id().unwrap_or(0);
            tileset.walls[wall as usize].edge.width > 0.0
        };
        reaches(0) && reaches(1) && layer.in_bounds(e.solid) && !fringed() && bevel(i)
    };
    let edges = layer.outline_edges();
    let normal = |i: usize| inward_normal(edges[i].a, edges[i].b);
    for i in layer.edges_over(within, width) {
        if !drawn(i) {
            continue;
        }
        let e = edges[i];
        let n = normal(i);
        // Along a straight run the strip's inner edge is just `width`
        // in; where the loop turns it's mitered with the next run's.
        let inner = |p: [f32; 2], other: usize, before: bool| {
            if edges[other].heading() == e.heading() {
                [p[0] + n[0] * width, p[1] + n[1] * width]
            } else if before {
                inset_corner(p, normal(other), n, width)
            } else {
                inset_corner(p, n, normal(other), width)
            }
        };
        let [before, after] = layer.linked(i);
        let a = inner(e.a, before, true);
        let b = inner(e.b, after, false);
        let wall = layer.tile(e.solid[0], e.solid[1]).wall_id().unwrap_or(0);
        let wall = &tileset.walls[wall as usize];
        let color = if faces_light(n) {
            wall.light
        } else {
            wall.dark
        };
        canvas.poly(&[e.a, e.b, b, a], color);
    }
}

/// What's drawn of a layer that only changes with its tiles: a piece a
/// chunk, each where it is, and what's along the chunks' edges.
pub struct LayerMeshes {
    /// Chunks across.
    pub across: usize,
    /// Each chunk's, row by row (see [`MeshPiece`]): each in pixels from
    /// its chunk's top-left corner, [`LayerMeshes::corner`].
    pub pieces: Vec<Arc<MeshPiece>>,
    /// The fringes and trims of the tiles along the chunks' edges, which
    /// depend on the chunks beside them (and past the layer's edge, that
    /// there are none): in the layer's own pixels.
    pub fringes: Arc<Mesh>,
    pub trims: Arc<Mesh>,
}

impl LayerMeshes {
    /// What's drawn of `layer`, from its chunks' `pieces` (row by row), and
    /// the fringes and trims along their edges.
    pub fn new(layer: &Layer, tileset: &Tileset, pieces: Vec<Arc<MeshPiece>>) -> Self {
        let (cols, rows) = (layer.cols, layer.rows);
        let edge = |x: usize, y: usize| {
            let size = [cols - x / CHUNK * CHUNK, rows - y / CHUNK * CHUNK].map(|n| n.min(CHUNK));
            on_edge(size, x % CHUNK, y % CHUNK)
        };
        let along_edges = |draw: DrawTile| {
            let mesh = Mesh::build(cols, rows, Canvas::new(tileset), |xs, ys, canvas| {
                for y in ys {
                    for x in xs.clone().filter(|&x| edge(x, y)) {
                        draw(layer, tileset, canvas, x, y);
                    }
                }
            });
            Arc::new(mesh)
        };
        Self {
            across: cols.div_ceil(CHUNK),
            pieces,
            fringes: along_edges(fringe::draw_fringes),
            trims: along_edges(fringe::draw_trims),
        }
    }

    /// Where piece `i`'s chunk's top-left corner is, in pixels.
    pub fn corner(&self, i: usize) -> [f32; 2] {
        [i % self.across, i / self.across].map(|c| (c * CHUNK) as f32 * TILE)
    }

    /// All of its meshes.
    pub fn all(&self) -> impl Iterator<Item = &Mesh> {
        let pieces = self.pieces.iter().flat_map(|p| p.all());
        pieces.chain([&*self.fringes, &*self.trims])
    }
}

/// Each tile's art of `layer`, as a mesh.
fn tile_mesh(layer: &Layer, tileset: &Tileset) -> Mesh {
    Mesh::build(
        layer.cols,
        layer.rows,
        Canvas::new(tileset),
        |xs, ys, canvas| {
            for y in ys {
                for x in xs.clone() {
                    draw_tile(layer, tileset, canvas, x, y);
                }
            }
        },
    )
}

/// Draws a tile's fringes or trims: see [`fringe`].
type DrawTile = fn(&Layer, &Tileset, &mut Canvas, usize, usize);

/// What's drawn of a chunk of a layer, the same in any window, so made
/// once it can be reused by each, and kept on the GPU as long: in pixels
/// from the chunk's top-left corner. Its fringes and trims leave out the
/// tiles along its edge, which depend on the chunks beside it (see
/// [`LayerMeshes`]).
pub struct MeshPiece {
    /// Its tiles' art.
    pub tiles: Mesh,
    /// Drawn over the tiles (all pieces' tiles): where materials meet, the
    /// fringes of the higher-ranking ones (see [`crate::fringe`]), cut by
    /// the renderer.
    pub fringes: Mesh,
    /// Drawn over the fringes: trims scattered along material edges.
    pub trims: Mesh,
    /// Where it's void, liquid included.
    pub voids: Mesh,
    /// Where it's liquid.
    pub liquid: Mesh,
    /// Its props' art (see [`crate::prop`]), drawn over everything else
    /// on the layer, characters too, and reaching over the tiles around.
    pub props: Mesh,
}

impl MeshPiece {
    /// What's drawn of `chunk`, a layer's chunk alone (see
    /// [`Layer::chunk_alone`]).
    pub fn new(chunk: &Layer, tileset: &Tileset) -> Self {
        let size = [chunk.cols, chunk.rows];
        let inside = |draw: DrawTile| {
            Mesh::build(size[0], size[1], Canvas::new(tileset), |xs, ys, canvas| {
                for y in ys {
                    for x in xs.clone().filter(|&x| !on_edge(size, x, y)) {
                        draw(chunk, tileset, canvas, x, y);
                    }
                }
            })
        };
        let canvas = || Canvas::new(tileset);
        let tile = |x: usize, y: usize| chunk.tile(x as i32, y as i32);
        Self {
            tiles: tile_mesh(chunk, tileset),
            fringes: inside(fringe::draw_fringes),
            trims: inside(fringe::draw_trims),
            voids: Mesh::quarters(size[0], size[1], canvas(), |x, y| {
                tile(x, y).void_quarters()
            }),
            liquid: Mesh::quarters(size[0], size[1], canvas(), |x, y| {
                tile(x, y).liquid_quarters()
            }),
            props: Mesh::build(size[0], size[1], canvas(), |xs, ys, canvas| {
                for y in ys {
                    for x in xs.clone() {
                        draw_prop(chunk, tileset, canvas, x, y);
                    }
                }
            })
            .reaching(tileset.prop_reach()),
        }
    }

    /// All of its meshes.
    pub fn all(&self) -> [&Mesh; 6] {
        [
            &self.tiles,
            &self.fringes,
            &self.trims,
            &self.voids,
            &self.liquid,
            &self.props,
        ]
    }
}

/// Whether tile `(x, y)` of a chunk `size` is along its edge.
fn on_edge(size: [usize; 2], x: usize, y: usize) -> bool {
    x == 0 || y == 0 || x + 1 == size[0] || y + 1 == size[1]
}
