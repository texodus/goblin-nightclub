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

//! Where materials meet. The grid makes every boundary a straight line;
//! here the material that ranks higher (see [`Edge`]) grows a ragged
//! fringe over the one beside it, and trims (see [`Trim`]) are scattered
//! along material edges. Both are drawn as part of a layer, under whatever
//! stands on it, in the level's meshes; the renderer cuts the fringes.
//!
//! [`Edge`]: crate::tileset::Edge
//! [`Trim`]: crate::tileset::Trim

use crate::draw::{Canvas, faces_light};
use crate::geom::{inward_normal, length};
use crate::layer::{Layer, Part, Side, Solid, TILE, Tile};
use crate::palette::{self, Color};
use crate::tileset::{self, Cell, Tileset};

/// What a tile shows along one of its sides.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    Part(Part),
    Stairs,
}

/// How a surface ranks where it meets another: walls over floors over
/// liquids over void, stairs over everything, then by priority.
fn rank(tileset: &Tileset, s: Surface) -> (u8, u8) {
    match s {
        Surface::Stairs => (4, 0),
        Surface::Part(p) => {
            let kind = match p {
                Part::Void => 0,
                Part::Liquid(_) => 1,
                Part::Floor(_) => 2,
                Part::Wall(_) => 3,
            };
            (kind, tileset.edge(p).priority)
        }
    }
}

/// What `tile` shows along its side `side`.
fn surface(tile: Tile, side: Side) -> Surface {
    match tile {
        Tile::Stairs(..) => Surface::Stairs,
        Tile::Whole(p) => Surface::Part(p),
        Tile::Split { corner, at, rest } => Surface::Part(if corner.sides().contains(&side) {
            at
        } else {
            rest
        }),
    }
}

/// An edge of one part of a tile, clockwise around the part.
struct PieceEdge {
    a: [f32; 2],
    b: [f32; 2],
    /// Unit normal pointing out of the part.
    out: [f32; 2],
    /// The tile side it runs along, or `None` for a split tile's diagonal.
    side: Option<Side>,
    /// What's beyond it: `None` past the level's edge.
    beyond: Option<Surface>,
}

/// The parts of tile `(x, y)`, each with its edges. Stairs have none.
fn pieces(layer: &Layer, x: usize, y: usize) -> Vec<(Part, Vec<PieceEdge>)> {
    let tile = layer.tile(x as i32, y as i32);
    let halves = match tile {
        Tile::Stairs(..) => return Vec::new(),
        Tile::Whole(p) => vec![(p, Solid::Full, None)],
        Tile::Split { corner, at, rest } => vec![
            (at, Solid::Slope(corner), Some(rest)),
            (rest, Solid::Slope(corner.opposite()), Some(at)),
        ],
    };
    let [x0, y0] = [x as f32 * TILE, y as f32 * TILE];
    let [x1, y1] = [x0 + TILE, y0 + TILE];
    halves
        .into_iter()
        .map(|(part, shape, other)| {
            let points = shape.shape(x, y).unwrap();
            let n = points.len();
            let edges = (0..n)
                .map(|i| {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                    let side = if a[1] == b[1] && mid[1] == y0 {
                        Some(Side::N)
                    } else if a[1] == b[1] && mid[1] == y1 {
                        Some(Side::S)
                    } else if a[0] == b[0] && mid[0] == x0 {
                        Some(Side::W)
                    } else if a[0] == b[0] && mid[0] == x1 {
                        Some(Side::E)
                    } else {
                        None
                    };
                    let beyond = match side {
                        Some(s) => {
                            let [dx, dy] = s.offset();
                            let t = [x as i32 + dx, y as i32 + dy];
                            layer
                                .in_bounds(t)
                                .then(|| surface(layer.tile(t[0], t[1]), s.opposite()))
                        }
                        None => other.map(Surface::Part),
                    };
                    let n = inward_normal(a, b);
                    PieceEdge {
                        a,
                        b,
                        out: [-n[0], -n[1]],
                        side,
                        beyond,
                    }
                })
                .collect();
            (part, edges)
        })
        .collect()
}

/// The sheet cell tile `(x, y)` draws `part` with, as [`Layer::draw_tile`]
/// does.
fn cell_of(tileset: &Tileset, part: Part, x: usize, y: usize) -> Option<Cell> {
    let cells = match part {
        Part::Floor(f) => &tileset.floors[f as usize].cells,
        Part::Wall(w) => &tileset.walls[w as usize].cells,
        Part::Liquid(l) => &tileset.liquids[l as usize].cells,
        Part::Void => return None,
    };
    Some(tileset::variant(cells, x, y))
}

/// The fringes tile `(x, y)`'s parts grow over lower-ranking neighbors:
/// a strip out from each such edge, and a square where two strips meet
/// at a corner. Liquids and void grow none.
pub fn draw_fringes(layer: &Layer, tileset: &Tileset, canvas: &mut Canvas, x: usize, y: usize) {
    let origin = [x as f32 * TILE, y as f32 * TILE];
    for (part, edges) in pieces(layer, x, y) {
        let edge = tileset.edge(part);
        let [ax, ay] = layer.absolute(x, y);
        let (Some(cell), Part::Floor(_) | Part::Wall(_)) = (cell_of(tileset, part, ax, ay), part)
        else {
            continue;
        };
        if edge.width <= 0.0 {
            continue;
        }
        let reach = edge.width / tileset.zoom();
        let mine = rank(tileset, Surface::Part(part));
        // A wall's rim is its bevel color for the way it faces; a floor's,
        // its own.
        let rim = |out: [f32; 2]| -> Color {
            match part {
                Part::Wall(w) => {
                    let wall = &tileset.walls[w as usize];
                    if faces_light([-out[0], -out[1]]) {
                        wall.light
                    } else {
                        wall.dark
                    }
                }
                _ => edge.rim.unwrap_or(palette::TRANSPARENT),
            }
        };
        let grows = |e: &PieceEdge| e.beyond.is_some_and(|s| mine > rank(tileset, s));
        let at = |p: [f32; 2], v: [f32; 2], k: f32| [p[0] + v[0] * k, p[1] + v[1] * k];
        for e in edges.iter().filter(|e| grows(e)) {
            let line = [e.a[0], e.a[1], e.out[0], e.out[1]];
            let strip = match e.side {
                Some(_) => vec![e.a, e.b, at(e.b, e.out, reach), at(e.a, e.out, reach)],
                // Across the diagonal, kept inside the tile: its ends slide
                // along the sides of the other half.
                None => {
                    let slide = reach * std::f32::consts::SQRT_2;
                    let along = |p: [f32; 2], q: [f32; 2]| {
                        let d = [q[0] - p[0], q[1] - p[1]];
                        let len = length(d);
                        at(p, [d[0] / len, d[1] / len], slide)
                    };
                    // The other half's corner, where its two sides meet.
                    let far = at(e.a, e.out, TILE * std::f32::consts::FRAC_1_SQRT_2);
                    let far = at(far, [e.b[0] - e.a[0], e.b[1] - e.a[1]], 0.5);
                    vec![e.a, e.b, along(e.b, far), along(e.a, far)]
                }
            };
            canvas.fringe(&strip, origin, cell, rim(e.out), line, reach);
        }
        // Squares at corners between two sides that both grow.
        let n = edges.len();
        for i in 0..n {
            let (e, f) = (&edges[i], &edges[(i + 1) % n]);
            if !(e.side.is_some() && f.side.is_some() && grows(e) && grows(f)) {
                continue;
            }
            let v = e.b;
            let both = [e.out[0] + f.out[0], e.out[1] + f.out[1]];
            let square = [
                v,
                at(v, e.out, reach),
                at(v, both, reach),
                at(v, f.out, reach),
            ];
            let len = length(both);
            let rim = rim([both[0] / len, both[1] / len]);
            canvas.fringe(&square, origin, cell, rim, [v[0], v[1], 0.0, 0.0], reach);
        }
    }
}

/// Deterministic hash of integers to [0, 1).
fn hash(ns: [u32; 4]) -> f32 {
    let mut h: u32 = 0x9e37_79b9;
    for n in ns {
        h = (h ^ n).wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 16;
    }
    h as f32 / 4_294_967_296.0
}

/// The trims along tile `(x, y)`'s parts' edges with other surfaces
/// (inside the level): by chance, a sprite at a random spot along each,
/// moved into the part by the trim's inset.
pub fn draw_trims(layer: &Layer, tileset: &Tileset, canvas: &mut Canvas, x: usize, y: usize) {
    let origin = [x as f32 * TILE, y as f32 * TILE];
    for (part, edges) in pieces(layer, x, y) {
        let Some(trim) = tileset.trim(part) else {
            continue;
        };
        if trim.cells.is_empty() {
            continue;
        }
        let inset = trim.inset / tileset.zoom();
        let [ax, ay] = layer.absolute(x, y);
        for (i, e) in edges.iter().enumerate() {
            if e.beyond.is_none_or(|s| s == Surface::Part(part)) {
                continue;
            }
            // Each edge by its tile and direction, so the two halves of a
            // split tile differ.
            let key = |k: u32| {
                let dir = (e.out[0] * 2.0 + 3.0) as u32 * 8 + (e.out[1] * 2.0 + 3.0) as u32;
                hash([ax as u32, ay as u32, dir * 4 + i as u32, k])
            };
            if key(0) >= trim.density {
                continue;
            }
            // From the tile's corner, so it's placed the same wherever the
            // tile is drawn (see [`Canvas::sprite_from`]).
            let t = 0.2 + 0.6 * key(1);
            let [a, b] = [e.a, e.b].map(|p| [p[0] - origin[0], p[1] - origin[1]]);
            let on = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            let center = [on[0] - e.out[0] * inset, on[1] - e.out[1] * inset];
            let cell = trim.cells[(key(2) * trim.cells.len() as f32) as usize % trim.cells.len()];
            canvas.sprite_from(origin, center, cell, palette::WHITE);
        }
    }
}
