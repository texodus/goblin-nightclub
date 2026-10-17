//! The engine's invariants hold on generated levels: these are the
//! engine's own stress tests, run here on the generator's output (they
//! can't live in the engine without a dependency cycle).

use goblin_nightclub_core::entity::Entity;
use goblin_nightclub_core::layer::testing::check_outlines;
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::level_file::{LevelFile, Spawn};
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::vision::Vision;
use goblin_nightclub_core::vision::testing::{OpenPoint, band_covers, check_polygon, is_open_point};
use goblin_nightclub_generate::{Params, generate_level, spawn_layer};

const SPAWN: [f32; 2] = [325.0, 205.0];

fn worlds() -> impl Iterator<Item = Level> {
    (0..6).map(|seed| generate_level(seed, 64, 40, SPAWN, &Params::default()))
}

/// One generated level per layer (`Level` isn't `Clone`), with an open
/// point on that layer to look from.
fn layers_with_origins() -> Vec<(Level, usize, [f32; 2])> {
    let depth = generate_level(3, 64, 40, SPAWN, &Params::default()).depth();
    let mut rng = Rng::new(9);
    (0..depth)
        .map(|k| {
            let level = generate_level(3, 64, 40, SPAWN, &Params::default());
            let origin = rng.open_point(level.layer(k));
            (level, k, origin)
        })
        .collect()
}

#[test]
fn outlines_are_closed_loops() {
    for level in worlds() {
        for k in 0..level.depth() {
            check_outlines(level.layer(k));
        }
    }
}

#[test]
fn los_is_symmetric() {
    let mut rng = Rng::new(7);
    for level in worlds() {
        for k in 0..level.depth() {
            let layer = level.layer(k);
            for _ in 0..100 {
                let (a, b) = (rng.open_point(layer), rng.open_point(layer));
                assert_eq!(
                    layer.los(a, b),
                    layer.los(b, a),
                    "layer {k}: {a:?} <-> {b:?}"
                );
            }
        }
    }
}

#[test]
fn polygon_matches_los() {
    let mut rng = Rng::new(11);
    for (level, k, origin) in layers_with_origins() {
        let layer = level.layer(k);
        for radius in [f32::INFINITY, 80.0] {
            let mut vision = Vision::new(layer, radius);
            vision.update(layer, origin);
            check_polygon(layer, &vision, &mut rng);
        }
    }
}

#[test]
fn band_never_covers_open_floor() {
    let mut rng = Rng::new(3);
    for (level, k, origin) in layers_with_origins() {
        let layer = level.layer(k);
        let mut v = Vision::new(layer, 96.0);
        v.update(layer, origin);
        for _ in 0..200 {
            let p = rng.open_point(layer);
            assert!(
                !band_covers(&v, p) || !is_open_point(layer, p),
                "layer {k}: {p:?}"
            );
        }
    }
}

#[test]
fn npcs_spawn_clear_and_never_enter_walls() {
    for (seed, level) in worlds().enumerate() {
        let k = spawn_layer(&Params::default());
        let grid = level.movement(k);
        let mut rng = Rng::new(seed as u32 ^ 0x9E37_79B9);
        let mut npcs = Entity::spawn_npcs(grid, &mut rng, 4, 0, SPAWN, 40.0);
        assert_eq!(npcs.len(), 4);
        for _ in 0..1000 {
            for npc in &mut npcs {
                npc.wander(grid, &mut rng, 1.0 / 60.0);
                assert!(!grid.collides(npc.pos, npc.size), "seed {seed}");
            }
        }
    }
}

#[test]
fn generated_levels_round_trip_as_files() {
    let level = generate_level(4, 64, 40, SPAWN, &Params::default());
    let layers: Vec<Vec<_>> = (0..level.depth())
        .map(|k| {
            (0..40)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .map(|(x, y)| level.layer(k).tile(x, y))
                .collect()
        })
        .collect();
    let spawn = Spawn {
        layer: spawn_layer(&Params::default()),
        tile: [32, 20],
    };
    let file = LevelFile::new(64, 40, &layers, spawn);
    let back = LevelFile::from_json(&file.to_json()).unwrap();
    assert_eq!(back.tiles(), layers);
    let loaded = back.level();
    assert_eq!(loaded.depth(), level.depth());
    for k in 0..level.depth() {
        assert_eq!(loaded.layer(k).tile(5, 5), level.layer(k).tile(5, 5));
    }
}
