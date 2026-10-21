use goblin_nightclub_core::art::MeshPiece;
use goblin_nightclub_core::art::draw_tile;
use goblin_nightclub_core::chunk::CHUNK;
use goblin_nightclub_core::layer::{Layer, QUARTERS, Tile};
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::level_file::LevelFile;
use std::rc::Rc;

use goblin_nightclub_core::draw::{Canvas, Vertex};
use goblin_nightclub_core::fringe::{draw_fringes, draw_trims};
use goblin_nightclub_core::layer::{Dir, Part, Solid};
use goblin_nightclub_core::tileset::Tileset;
use goblin_nightclub_core::world::*;

use goblin_nightclub_core::level_file::Spawn;
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::prop::{Prop, draw_prop};
use goblin_nightclub_core::rng::Rng;

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
                        goblin_nightclub_core::layer::Corner::CLOCKWISE[rng.range(0, 3)],
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
        goblin_nightclub_core::tileset::testing::tileset(),
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
    use goblin_nightclub_core::draw::Canvas;
    use goblin_nightclub_core::fringe::{draw_fringes, draw_trims};
    let mut file = file(6, 400, 40, 1);
    // Materials with variants, edges and trims, so there's a choice to
    // make the same way.
    file.tileset = goblin_nightclub_core::tileset::testing::tileset();
    file.tileset.floors[0].cells = (0..8).map(|i| [i, 1]).collect();
    file.tileset.floors[0].edge.width = 3.0;
    file.tileset.floors[0].trim = Some(goblin_nightclub_core::tileset::Trim {
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
                draw_tile(layer, tileset, &mut canvas, local, y);
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
    use goblin_nightclub_core::level::movement_layer;
    let mut file = file(7, 300, 90, 5);
    // Art with variants, fringes and trims, so there's all of it to draw.
    file.tileset.floors[0].cells = (0..8).map(|i| [i, 1]).collect();
    file.tileset.floors[0].edge.width = 3.0;
    file.tileset.floors[0].trim = Some(goblin_nightclub_core::tileset::Trim {
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
                |l, t, c, x, y| draw_tile(l, t, c, x, y);
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
