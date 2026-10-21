#![allow(clippy::needless_range_loop)]

use super::*;
use crate::chamfer::{apply, chamfer_layer, part_at_side};

const SPAWN: [f32; 2] = [325.0, 205.0];
const COLS: usize = 64;
const ROWS: usize = 40;

fn world(seed: u32) -> Level {
    generate_level(seed, COLS, ROWS, SPAWN, &Params::default())
}

fn water() -> Tile {
    Tile::Whole(Part::Liquid(tileset().liquid(WATER).unwrap()))
}

#[test]
fn props_stand_alone_and_close_no_way() {
    let t = tileset();
    let [tree, rock, torch] = ["tree", "rock", "torch"].map(|n| t.prop(n).unwrap());
    let (grass, cave) = (t.floor(PLAINS).unwrap(), t.floor(CAVE_FLOOR).unwrap());
    let (mut trees, mut rocks, mut torches) = (0, 0, 0);
    for seed in 0..3 {
        let params = Params::default();
        let file = super::world(seed, 160, 100, [805.0, 505.0], &params);
        let ground = file.ground;
        let tiles = file.tiles();
        let at = |k: usize, x: i32, y: i32| {
            let inside = (0..160).contains(&x) && (0..100).contains(&y);
            inside.then(|| tiles[k][y as usize * 160 + x as usize])
        };
        for (k, props) in file.props.iter().enumerate() {
            let here: std::collections::HashSet<[usize; 2]> = props.iter().map(|p| p.at).collect();
            for p in props {
                let [x, y] = p.at.map(|v| v as i32);
                // Trees on the grass above ground; rocks on caves' floor
                // below, and torches on any floor.
                let tile = at(k, x, y);
                if k >= ground {
                    assert_eq!(p.kind, tree, "seed {seed} layer {k} at {:?}", p.at);
                    assert_eq!(tile, Some(Tile::Whole(Part::Floor(grass))));
                    trees += 1;
                } else if p.kind == torch {
                    assert!(matches!(tile, Some(Tile::Whole(Part::Floor(_)))));
                    torches += 1;
                } else {
                    assert_eq!(p.kind, rock, "seed {seed} layer {k} at {:?}", p.at);
                    assert_eq!(tile, Some(Tile::Whole(Part::Floor(cave))));
                    rocks += 1;
                }
                // Open floor all round, nothing else standing there.
                for (dx, dy) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))) {
                    if (dx, dy) == (0, 0) {
                        continue;
                    }
                    assert!(matches!(
                        at(k, x + dx, y + dy),
                        Some(Tile::Whole(Part::Floor(_)))
                    ));
                    let n = [(x + dx) as usize, (y + dy) as usize];
                    assert!(
                        !here.contains(&n),
                        "seed {seed} layer {k}: props side by side at {n:?}"
                    );
                }
                // Clear of the spawn.
                let s = spawn_tile_of(&file);
                assert!(
                    k != file.spawn.layer
                        || (x - s[0]).abs().max((y - s[1]).abs()) > SPAWN_CLEARANCE
                );
            }
        }
        // Everywhere walkable without them is walkable with them, but
        // where they stand.
        let s = spawn_tile_of(&file);
        let with = reachable_across(&file.level(), file.spawn.layer, s);
        let bare = file.clone().with_props(Vec::new());
        let without = reachable_across(&bare.level(), file.spawn.layer, s);
        for k in 0..with.len() {
            let stands: std::collections::HashSet<usize> = file.props[k]
                .iter()
                .map(|p| p.at[1] * 160 + p.at[0])
                .collect();
            for i in 0..with[k].len() {
                assert!(
                    with[k][i] || !without[k][i] || stands.contains(&i),
                    "seed {seed}: layer {k} tile {i} cut off"
                );
            }
        }
    }
    assert!(
        trees > 50 && rocks > 50 && torches > 20,
        "{trees} trees, {rocks} rocks, {torches} torches"
    );
}

fn spawn_tile_of(file: &LevelFile) -> [i32; 2] {
    file.spawn.pos.map(|v| (v / TILE).floor() as i32)
}

fn spawn_tile() -> [i32; 2] {
    [(SPAWN[0] / TILE) as i32, (SPAWN[1] / TILE) as i32]
}

/// Under every water tile (lake, river or pool), the tile below and
/// its eight neighbors are wall or water.
#[test]
fn water_rests_on_solid_ground_and_never_at_the_spawn() {
    let p = Params::default();
    let plains = p.underworld_layers + 1;
    let (mut lakes, mut pools) = (0, 0);
    for seed in 0..20 {
        let level = world(seed);
        for k in 0..level.depth() {
            for y in 0..ROWS as i32 {
                for x in 0..COLS as i32 {
                    if level.layer(k).tile(x, y).liquid_quarters() == 0 {
                        continue;
                    }
                    if k == plains {
                        lakes += 1;
                        let s = spawn_tile();
                        assert!((x - s[0]).abs().max((y - s[1]).abs()) > SPAWN_CLEARANCE);
                    } else {
                        assert!(k < p.underworld_layers, "seed {seed}: water on layer {k}");
                        pools += 1;
                    }
                    let Some(below) = k.checked_sub(1) else {
                        continue;
                    };
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let under = level.layer(below).tile(x + dx, y + dy);
                            assert!(
                                under.wall().quarters() | under.liquid_quarters() == 0b1111,
                                "seed {seed}: water at ({x}, {y}) on layer {k} over {under:?} at ({}, {})",
                                x + dx,
                                y + dy
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(lakes > 20 * 100, "{lakes} lake tiles in 20 levels");
    assert!(pools > 20 * 10, "{pools} pool tiles in 20 levels");
}

#[test]
fn holes_keep_clear_of_water() {
    let p = Params::default();
    let plains = p.underworld_layers + 1;
    let (mut plains_holes, mut cave_holes, mut bridges) = (0, 0, 0);
    for seed in 0..20 {
        let level = world(seed);
        for k in 0..level.depth() {
            for y in 1..ROWS as i32 - 1 {
                for x in 1..COLS as i32 - 1 {
                    let t = level.layer(k).tile(x, y);
                    if k == plains && t.floor_id() == tileset().floor(BRIDGE) {
                        bridges += 1;
                    }
                    if t != Tile::VOID || k > plains {
                        continue;
                    }
                    // A hole: on the plains, or in a cave. However deep
                    // it goes, it may end on anything (a canyon floor, a
                    // cave's rock or pool, a room or stairs it stopped at,
                    // or through holes in every cave below); but it's
                    // never beside water.
                    if k == plains {
                        plains_holes += 1;
                    } else if k < p.underworld_layers {
                        cave_holes += 1;
                    }
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let n = level.layer(k).tile(x + dx, y + dy);
                            assert_eq!(
                                n.liquid_quarters(),
                                0,
                                "seed {seed}: hole at ({x}, {y}) beside water"
                            );
                        }
                    }
                }
            }
        }
    }
    assert!(
        plains_holes > 20 * 10,
        "{plains_holes} plains holes in 20 levels"
    );
    assert!(cave_holes > 20 * 10, "{cave_holes} cave holes in 20 levels");
    assert!(bridges > 0, "no bridges in 20 levels");
}

/// Tiles from rows of `#` wall, `.` floor, `~` void, `≈` water.
fn tiles(rows: &[&str]) -> Vec<Tile> {
    rows.iter()
        .flat_map(|r| r.chars())
        .map(|c| match c {
            '#' => Tile::WALL,
            '~' => Tile::VOID,
            '≈' => water(),
            _ => Tile::FLOOR,
        })
        .collect()
}

#[test]
fn chamfer_fills_stair_steps_only() {
    use Corner::*;
    let (floor, void, liquid) = (Part::Floor(0), Part::Void, water().liquid_id().unwrap());
    let mut t = tiles(&["#####", "#....", "#....", "#...."]);
    chamfer(&mut t, 5, 4);
    assert_eq!(t[5 + 1], Tile::slope(NW), "wall into floor");
    assert_eq!(t[2 * 5 + 2], Tile::FLOOR);

    let bend = tiles(&["#####", "#..##", "##.##", "#####"]);
    let mut t = bend.clone();
    chamfer(&mut t, 5, 4);
    assert_eq!(t, bend, "a bend in a passage is left alone");

    // A hole's rim: the void at its convex corners gets floor (a 2x2
    // hole becomes a diamond), the floor at a concave corner opens.
    let mut t = tiles(&[".....", ".~~..", ".~~..", "....."]);
    chamfer(&mut t, 5, 4);
    assert_eq!(t[5 + 1], Tile::split(NW, floor, void), "convex corner");
    assert_eq!(t[2 * 5 + 2], Tile::split(SE, floor, void));
    assert_eq!(t[5 + 2], Tile::split(NE, floor, void));
    assert_eq!(t[5 + 3], Tile::FLOOR, "a straight edge stays");
    let mut t = tiles(&["~~~..", "~~~..", "~~...", "~~..."]);
    chamfer(&mut t, 5, 4);
    assert_eq!(
        t[5 + 2],
        Tile::split(SE, floor, void),
        "void's convex corner"
    );
    // The floor's concave corner just below would zigzag against it:
    // the first chamfer made wins, the other no longer fits.
    assert_eq!(t[2 * 5 + 2], Tile::FLOOR, "vetoed by its neighbor");
    assert_eq!(t[2 * 5 + 3], Tile::FLOOR, "a straight edge stays");
    let mut split = t.clone();
    chamfer(&mut split, 5, 4);
    assert_eq!(split, t, "nothing cascades: split tiles are left alone");
    // On its own, the concave corner opens.
    let mut t = tiles(&["~~~~~", "~~~~~", "~~...", "~~..."]);
    chamfer(&mut t, 5, 4);
    assert_eq!(
        t[2 * 5 + 2],
        Tile::split(NW, void, floor),
        "floor's concave corner"
    );
    assert_eq!(t[5 + 2], Tile::VOID, "a straight void edge stays");

    // A shore, both ways; and rock into a pool.
    let mut t = tiles(&["..≈≈≈", "..≈≈≈", "....≈", "....."]);
    chamfer(&mut t, 5, 4);
    assert_eq!(t[5 + 1], Tile::FLOOR, "straight shore");
    assert_eq!(
        t[2 * 5 + 3],
        Tile::split(NE, Part::Liquid(liquid), floor),
        "water into the floor's corner"
    );
    assert_eq!(
        t[5 + 2],
        Tile::split(SW, floor, Part::Liquid(liquid)),
        "floor into the water's corner"
    );
    let mut t = tiles(&["#####", "#≈≈≈≈", "#≈≈≈≈", "#≈≈≈≈"]);
    chamfer(&mut t, 5, 4);
    assert_eq!(
        t[5 + 1],
        Tile::split(NW, Part::Wall(0), Part::Liquid(liquid))
    );
}

/// Generated levels have 45-degree hole rims and shores, each obeying
/// the rules: the right kinds either side, nothing under water but
/// solid ground, floor under every piece of hole on the plains, and
/// never on stairs, bridges, fords or at the spawn.
#[test]
fn shores_and_hole_rims_are_chamfered() {
    let p = Params::default();
    let plains = p.underworld_layers + 1;
    let t = tileset();
    let (bridge, ford) = (t.floor(BRIDGE), t.floor(BED));
    let mut counts = std::collections::HashMap::new();
    for seed in 0..20 {
        let level = world(seed);
        for k in 0..level.depth() {
            for y in 0..ROWS as i32 {
                for x in 0..COLS as i32 {
                    let Tile::Split { corner, at, rest } = level.layer(k).tile(x, y) else {
                        continue;
                    };
                    let (from, into) = (Kind::of(at), Kind::of(rest));
                    *counts.entry((into, from)).or_insert(0) += 1;
                    assert!(
                        RULES.contains(&Rule { into, from }),
                        "seed {seed} ({x}, {y}) layer {k}: {at:?} over {rest:?}"
                    );
                    let s = spawn_tile();
                    assert!((x - s[0]).abs().max((y - s[1]).abs()) > SPAWN_CLEARANCE);
                    // Holes and water keep off bridges and fords (a
                    // wall's corner in a plank room is fine).
                    if let (Part::Floor(f), Kind::Void | Kind::Liquid) = (rest, from) {
                        assert!(
                            Some(f) != bridge && Some(f) != ford,
                            "seed {seed}: on a crossing"
                        );
                    }
                    // The neighbors across the corner's sides are what
                    // the corner became (unless chamfered too, later).
                    for side in corner.sides() {
                        let [dx, dy] = side.offset();
                        let n = level.layer(k).tile(x + dx, y + dy);
                        assert!(
                            Kind::of(part_at_side(n, side.opposite())) == from
                                || matches!(n, Tile::Split { .. }),
                            "seed {seed} ({x}, {y}) layer {k}: {n:?} across {side:?}"
                        );
                    }
                    if from == Kind::Void && k == plains {
                        let under = level.layer(k - 1).tile(x, y);
                        assert!(
                            under.floor_id().is_some(),
                            "seed {seed} ({x}, {y}): {under:?}"
                        );
                    }
                }
            }
        }
    }
    for (into, from) in [
        (Kind::Floor, Kind::Void),
        (Kind::Void, Kind::Floor),
        (Kind::Floor, Kind::Liquid),
        (Kind::Liquid, Kind::Floor),
    ] {
        assert!(
            counts.get(&(into, from)).is_some_and(|n| *n > 20),
            "{from:?} into {into:?}: {counts:?}"
        );
    }
}

/// [`chamfer_layer`], applied.
fn chamfer(tiles: &mut [Tile], cols: usize, rows: usize) {
    for c in chamfer_layer(tiles, cols, rows) {
        apply(tiles, cols, rows, c);
    }
}
