//! Where materials meet. The grid makes every boundary a straight line;
//! here the material that ranks higher (see [`Edge`]) grows a ragged
//! fringe over the one beside it, and trims (see [`Trim`]) are scattered
//! along material edges. Both are drawn as part of a layer, under whatever
//! stands on it, in the level's meshes; the renderer cuts the fringes.
//!
//! [`Edge`]: crate::tileset::Edge
//! [`Trim`]: crate::tileset::Trim

use crate::draw::{Canvas, faces_light};
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
                    let d = [b[0] - a[0], b[1] - a[1]];
                    let len = crate::length(d);
                    PieceEdge {
                        a,
                        b,
                        out: [d[1] / len, -d[0] / len],
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
        let (Some(cell), Part::Floor(_) | Part::Wall(_)) = (cell_of(tileset, part, x, y), part)
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
                        let len = crate::length(d);
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
            let len = crate::length(both);
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
    for (part, edges) in pieces(layer, x, y) {
        let Some(trim) = tileset.trim(part) else {
            continue;
        };
        if trim.cells.is_empty() {
            continue;
        }
        let inset = trim.inset / tileset.zoom();
        for (i, e) in edges.iter().enumerate() {
            if e.beyond.is_none_or(|s| s == Surface::Part(part)) {
                continue;
            }
            // Each edge by its tile and direction, so the two halves of a
            // split tile differ.
            let key = |k: u32| {
                let dir = (e.out[0] * 2.0 + 3.0) as u32 * 8 + (e.out[1] * 2.0 + 3.0) as u32;
                hash([x as u32, y as u32, dir * 4 + i as u32, k])
            };
            if key(0) >= trim.density {
                continue;
            }
            let t = 0.2 + 0.6 * key(1);
            let on = [
                e.a[0] + (e.b[0] - e.a[0]) * t,
                e.a[1] + (e.b[1] - e.a[1]) * t,
            ];
            let center = [on[0] - e.out[0] * inset, on[1] - e.out[1] * inset];
            let cell = trim.cells[(key(2) * trim.cells.len() as f32) as usize % trim.cells.len()];
            canvas.sprite(center, cell, palette::WHITE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tileset::{Edge, Trim};

    /// The test tileset with floor 0 (`plain`) growing a 2-pixel fringe
    /// over lower floors (the rest), and rock walls a 1-pixel one, and
    /// `dirt` trimmed every edge, 3 pixels in.
    fn tileset() -> Tileset {
        let mut t = tileset::testing::tileset();
        t.floors[0].edge = Edge {
            priority: 5,
            width: 2.0,
            rim: Some([0.0, 0.0, 0.0, 0.5]),
        };
        t.walls[0].edge = Edge {
            priority: 0,
            width: 1.0,
            rim: None,
        };
        t.floors[2].trim = Some(Trim {
            cells: vec![[8, 3]],
            density: 1.0,
            inset: 3.0,
        });
        t
    }

    fn fringes(layer: &Layer, x: usize, y: usize) -> Vec<crate::draw::Vertex> {
        let tileset = tileset();
        let mut canvas = Canvas::new(&tileset);
        draw_fringes(layer, &tileset, &mut canvas, x, y);
        canvas.vertices
    }

    fn area(vertices: &[crate::draw::Vertex]) -> f32 {
        vertices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum()
    }

    #[test]
    fn higher_ranks_grow_over_lower_only() {
        // Floor beside void on the right and below; a wall above.
        let layer = Layer::from_ascii(&["##", ".~", "~~"]);
        let reach = 2.0 / tileset().zoom();
        let floor = fringes(&layer, 0, 1);
        // A strip right and one down, and the square between them, all
        // `reach` out; nothing up into the wall or left off the level.
        assert!((area(&floor) - (2.0 * TILE * reach + reach * reach)).abs() < 1e-3);
        assert!(floor.iter().all(|v| v.reach == reach));
        assert!(
            floor.iter().all(|v| v.pos[1] >= TILE - 1e-3),
            "not over the wall"
        );
        // The wall grows a 1-pixel strip down over the floor (the wall
        // beside it is its equal).
        let wall = fringes(&layer, 0, 0);
        assert!((area(&wall) - TILE / tileset().zoom()).abs() < 1e-3);
        // Void grows nothing.
        assert!(fringes(&layer, 1, 2).is_empty());
    }

    #[test]
    fn walls_rim_by_the_light_floors_by_their_color() {
        let layer = Layer::from_ascii(&["~~~", "~#~", "~~~"]);
        let t = tileset();
        let wall = fringes(&layer, 1, 1);
        let rims: Vec<Color> = wall.iter().map(|v| v.color).collect();
        assert!(rims.contains(&t.walls[0].light) && rims.contains(&t.walls[0].dark));
        let floor = fringes(&Layer::from_ascii(&["~~~", "~.~", "~~~"]), 1, 1);
        assert!(floor.iter().all(|v| v.color == [0.0, 0.0, 0.0, 0.5]));
    }

    #[test]
    fn diagonal_fringes_stay_inside_their_tile() {
        // Floor in the north-west half, void in the rest.
        let layer = Layer::from_ascii(&["▛"]);
        let v = fringes(&layer, 0, 0);
        assert!(!v.is_empty());
        for p in v.iter().map(|v| v.pos) {
            assert!(p.iter().all(|c| (-1e-3..=TILE + 1e-3).contains(c)), "{p:?}");
            // In the void half, at most `reach` from the diagonal.
            let d = (p[0] + p[1] - TILE) / std::f32::consts::SQRT_2;
            assert!(
                (-1e-3..=2.0 / tileset().zoom() + 1e-3).contains(&d),
                "{p:?}"
            );
        }
    }

    #[test]
    fn trims_line_edges_with_other_surfaces() {
        let mut layer_tiles = Layer::from_ascii(&["....", "....", "...."]);
        // Dirt in the middle of plain floor.
        let dirt = Tile::Whole(Part::Floor(2));
        layer_tiles = {
            let mut tiles: Vec<Tile> = (0..12).map(|i| layer_tiles.tile(i % 4, i / 4)).collect();
            tiles[5] = dirt;
            tiles[6] = dirt;
            Layer::from_tiles(4, 3, tiles)
        };
        let t = tileset();
        let mut canvas = Canvas::new(&t);
        for y in 0..3 {
            for x in 0..4 {
                draw_trims(&layer_tiles, &t, &mut canvas, x, y);
            }
        }
        // Density 1: one per dirt edge with plain floor (not between the
        // two dirt tiles): 6 edges, a square sprite each.
        assert_eq!(canvas.vertices.len(), 6 * 6);
        // Each 3 pixels inside the dirt: sprite centers within the two
        // tiles, pulled in from the edges.
        let inset = 3.0 / t.zoom();
        for s in canvas.vertices.chunks(6) {
            let c = [
                s.iter().map(|v| v.pos[0]).sum::<f32>() / 6.0,
                s.iter().map(|v| v.pos[1]).sum::<f32>() / 6.0,
            ];
            assert!(
                c[0] >= TILE + inset - 0.1 && c[0] <= 3.0 * TILE - inset + 0.1,
                "{c:?}"
            );
            assert!(
                c[1] >= TILE + inset - 0.1 && c[1] <= 2.0 * TILE - inset + 0.1,
                "{c:?}"
            );
        }
        // The same every time.
        let mut again = Canvas::new(&t);
        for y in 0..3 {
            for x in 0..4 {
                draw_trims(&layer_tiles, &t, &mut again, x, y);
            }
        }
        assert_eq!(
            canvas.vertices.iter().map(|v| v.pos).collect::<Vec<_>>(),
            again.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()
        );
    }
}
