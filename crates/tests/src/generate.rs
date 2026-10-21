#![allow(clippy::needless_range_loop)]

use goblin_nightclub_core::layer::{Dir, Solid, TILE, Tile};
use goblin_nightclub_core::level::{Level, MAX_LAYERS};
use goblin_nightclub_core::level_file::{LevelFile, Spawn};
use goblin_nightclub_generate::*;

const SPAWN: [f32; 2] = [325.0, 205.0];
const COLS: usize = 64;
const ROWS: usize = 40;

fn world(seed: u32) -> Level {
    generate_level(seed, COLS, ROWS, SPAWN, &Params::default())
}

fn spawn_tile() -> [i32; 2] {
    [(SPAWN[0] / TILE) as i32, (SPAWN[1] / TILE) as i32]
}

/// Each overworld column is rock, then one floor (or water on the
/// plains), then void; and the underworld is enclosed. (Holes and canyons,
/// which open the plains onto the caves, have tests of their own.)
#[test]
fn columns_follow_the_heightmap() {
    let p = Params {
        holes: 0,
        canyons: 0,
        ..Params::default()
    };
    for seed in 0..20 {
        let level = generate_level(seed, COLS, ROWS, SPAWN, &p);
        assert_eq!(level.depth(), p.underworld_layers + p.overworld_layers);
        let under = p.underworld_layers;
        for y in 1..ROWS as i32 - 1 {
            for x in 1..COLS as i32 - 1 {
                let column: Vec<Tile> = (under..level.depth())
                    .map(|k| level.layer(k).tile(x, y))
                    .collect();
                // Ramps (stairs in a cliff) and entrance cellars (floor
                // under the plains) are the exceptions to the rule.
                let cellar = column[0].floor_id().is_some();
                if column.iter().any(|t| t.stairs().is_some()) || cellar {
                    continue;
                }
                let surface = column
                    .iter()
                    .position(|t| t.wall() != Solid::Full)
                    .unwrap_or_else(|| panic!("seed {seed}: ({x}, {y}) is rock to the top"));
                for (k, t) in column.iter().enumerate() {
                    match k.cmp(&surface) {
                        std::cmp::Ordering::Less => {
                            assert_eq!(t.wall(), Solid::Full, "seed {seed} ({x}, {y}) layer {k}")
                        }
                        std::cmp::Ordering::Equal => {
                            let ok = t.floor_id().is_some()
                                || t.liquid_quarters() != 0
                                || t.stairs().is_some();
                            assert!(ok, "seed {seed} ({x}, {y}): {t:?} at the surface");
                        }
                        std::cmp::Ordering::Greater => {
                            // Above the surface: void.
                            let ok = *t == Tile::VOID || (t.void_quarters() != 0);
                            assert!(
                                ok,
                                "seed {seed} ({x}, {y}) layer {k}: {t:?} above the surface"
                            );
                        }
                    }
                }
            }
        }
        // The underworld's border is rock, as is the overworld's.
        for k in 0..level.depth() {
            for x in 0..COLS as i32 {
                assert_eq!(level.layer(k).solid(x, 0), Solid::Full, "seed {seed}");
                assert_eq!(level.layer(k).solid(x, ROWS as i32 - 1), Solid::Full);
            }
        }
    }
}

/// Every cave and dungeon floor is reachable (the rest is filled in),
/// and so is nearly all the ground above: a hill ringed by rivers
/// with no way onto it is left as scenery.
#[test]
fn everything_walkable_is_reachable_with_ramps() {
    let p = Params::default();
    let (mut ground, mut cut_off) = (0, 0);
    for seed in 0..20 {
        let level = world(seed);
        let top = spawn_layer(&p);
        assert!(
            !level
                .movement(top)
                .collides([SPAWN[0] - 4.0, SPAWN[1] - 4.0], [8.0, 8.0]),
            "seed {seed}"
        );
        let reached = reachable_across(&level, top, spawn_tile());
        for k in 0..level.depth() {
            let m = level.movement(k);
            for i in 0..COLS * ROWS {
                let (x, y) = ((i % COLS) as i32, (i / COLS) as i32);
                let open = m.solid(x, y) != Solid::Full;
                // A canyon floor (the layer under the plains) is floor,
                // but under a hole: nobody walks there.
                let covered = k + 1 == top && level.layer(top).tile(x, y).void_quarters() != 0;
                if !open || covered {
                    continue;
                }
                if k < p.underworld_layers {
                    assert!(
                        reached[k][i],
                        "seed {seed}: layer {k} tile {i} cut off underground"
                    );
                } else {
                    ground += 1;
                    cut_off += !reached[k][i] as usize;
                }
            }
        }
    }
    assert!(
        cut_off * 100 < ground,
        "{cut_off} of {ground} ground tiles cut off"
    );
}

#[test]
fn without_ramps_the_hills_are_out_of_reach() {
    let params = Params {
        ramp_spacing: 0,
        ..Params::default()
    };
    let mut cut_off = 0;
    for seed in 0..10 {
        let level = generate_level(seed, COLS, ROWS, SPAWN, &params);
        let top = spawn_layer(&params);
        let reached = reachable_across(&level, top, spawn_tile());
        // No ramps anywhere: no stairs on the overworld but the entrances.
        let hills = top + 1;
        for i in 0..COLS * ROWS {
            let t = level
                .layer(hills)
                .tile((i % COLS) as i32, (i / COLS) as i32);
            assert!(t.stairs().is_none(), "seed {seed}: a ramp");
            if t.is_floor() && !reached[hills][i] {
                cut_off += 1;
            }
        }
    }
    assert!(cut_off > 0, "some hilltop should be unreachable");
}

#[test]
fn hole_sizes_and_depth_follow_the_parameters() {
    // On flat, dry plains, so the holes aren't trimmed by hills or water.
    let flat = Params {
        hill_share: 0.0,
        water_share: 0.0,
        rivers: 0,
        ..Params::default()
    };
    let p = Params {
        holes: 2,
        canyons: 1,
        hole_radius: [3, 3],
        canyon_width: [3, 3],
        canyon_length: [30, 30],
        hole_depth: [2, 2],
        ..flat.clone()
    };
    // The same holes, but as small as they come.
    let small = Params {
        hole_radius: [1, 1],
        canyon_width: [1, 1],
        ..p.clone()
    };
    let plains = p.underworld_layers + 1;
    // Hole tiles on the plains over `seeds`, with `params`.
    let count = |params: &Params| {
        (0..10u32)
            .map(|seed| {
                let level = generate_level(seed, COLS, ROWS, SPAWN, params);
                (1..ROWS as i32 - 1)
                    .flat_map(|y| (1..COLS as i32 - 1).map(move |x| (x, y)))
                    .filter(|&(x, y)| level.layer(plains).tile(x, y) == Tile::VOID)
                    .count()
            })
            .sum::<usize>()
    };
    let (big, little) = (count(&p), count(&small));
    assert!(
        big > 2 * little,
        "{big} hole tiles with big holes, {little} with small ones"
    );
    // Two deep: through the layer below too, onto a floor in the top
    // cave where that was rock.
    let (mut deep, mut floors_below) = (0, 0);
    for seed in 0..10 {
        let level = generate_level(seed, COLS, ROWS, SPAWN, &p);
        for y in 1..ROWS as i32 - 1 {
            for x in 1..COLS as i32 - 1 {
                if level.layer(plains).tile(x, y) == Tile::VOID
                    && level.layer(plains - 1).tile(x, y) == Tile::VOID
                {
                    deep += 1;
                    floors_below +=
                        level.layer(plains - 2).tile(x, y).floor_id().is_some() as usize;
                }
            }
        }
    }
    assert!(deep * 2 > big, "{deep} of {big} go two deep");
    assert!(floors_below > 0);
    // Depth 1: nothing opened on the layer below.
    let shallow = Params {
        hole_depth: [1, 1],
        ..p
    };
    let level = generate_level(1, COLS, ROWS, SPAWN, &shallow);
    for y in 1..ROWS as i32 - 1 {
        for x in 1..COLS as i32 - 1 {
            assert_ne!(level.layer(plains - 1).tile(x, y), Tile::VOID);
        }
    }
}

#[test]
fn stairs_come_in_pairs() {
    for seed in 0..20 {
        let level = world(seed);
        let mut pairs = 0;
        for k in 1..level.depth() {
            for y in 0..ROWS as i32 {
                for x in 0..COLS as i32 {
                    if level.layer(k).tile(x, y).stairs() == Some(Dir::Down) {
                        pairs += 1;
                        assert_eq!(
                            level.layer(k - 1).tile(x, y).stairs(),
                            Some(Dir::Up),
                            "seed {seed} ({x}, {y}) layer {k}"
                        );
                    }
                }
            }
        }
        assert!(pairs >= 2, "seed {seed}: {pairs} staircases");
    }
}

/// A generated level file has everything the game needs: the tileset,
/// the characters' sprites, the spawn on the plains and the ground
/// level there.
#[test]
fn level_files_carry_the_tileset_and_sprites() {
    let p = Params::default();
    let file = goblin_nightclub_generate::world(7, COLS, ROWS, SPAWN, &p);
    assert_eq!(file.tileset, *tileset());
    assert_eq!(file.spawn.layer, spawn_layer(&p));
    assert_eq!(file.spawn, Spawn::at_tile(spawn_layer(&p), spawn_tile()));
    assert_eq!(file.ground, ground_layer(&p));
    assert!(file.tileset.sprite(&file.player.sprite).is_some());
    assert!(file.tileset.sprite(&file.npc.sprite).is_some());
    let back = LevelFile::from_json(&file.to_json()).unwrap();
    assert_eq!(back, file);
    assert_eq!(back.level().depth(), world(7).depth());
}

/// Every cell the tileset names is in the placeholder sheet, and its
/// white cell is white (so plain shapes draw right).
#[test]
fn the_tileset_fits_the_placeholder_sheet() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../www/assets/tiles.png");
    let file = std::fs::File::open(path).expect("www/assets/tiles.png (run `npm run tiles`)");
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut rgba = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut rgba).unwrap();
    let t = tileset();
    t.check_sheet([info.width, info.height]).unwrap();
    let [cx, cy] = t.white;
    let i = ((cy as u32 * t.cell) * info.width + cx as u32 * t.cell) as usize * 4;
    assert_eq!(&rgba[i..i + 4], &[255, 255, 255, 255], "white cell");
}

#[test]
fn parameters_round_trip_and_default_when_missing() {
    let p: Params = serde_json::from_str(r#"{"rivers": 5, "water_share": 0.3}"#).unwrap();
    assert_eq!(p.rivers, 5);
    assert_eq!(p.overworld_layers, Params::default().overworld_layers);
    let back: Params = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back, p);
    // Out-of-range layer counts are clamped, not fatal.
    let many = Params {
        overworld_layers: 20,
        underworld_layers: 20,
        ..Params::default()
    };
    let level = generate_level(1, 32, 20, [165.0, 105.0], &many);
    assert!(level.depth() <= MAX_LAYERS);
}
