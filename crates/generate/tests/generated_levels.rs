//! The engine's invariants hold on generated levels: these are the
//! engine's own stress tests, run here on the generators' output (they
//! can't live in the engine without a dependency cycle).

use goblin_nightclub_core::entity::Entity;
use goblin_nightclub_core::layer::Layer;
use goblin_nightclub_core::layer::testing::check_outlines;
use goblin_nightclub_core::level_file::{LevelFile, Spawn};
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::vision::Vision;
use goblin_nightclub_core::vision::testing::{OpenPoint, band_covers, check_polygon, is_open_point};
use goblin_nightclub_generate::{Style, generate, generate_level};

const SPAWN: [f32; 2] = [165.0, 105.0];
const SIZE: [f32; 2] = [8.0, 8.0];

/// Generated levels of both single-layer styles.
fn levels() -> impl Iterator<Item = Layer> {
    (0..40).flat_map(|seed| {
        [Style::World, Style::Cave].map(|style| generate(style, seed, 32, 20, SPAWN))
    })
}

#[test]
fn outlines_are_closed_loops() {
    for level in levels() {
        check_outlines(&level);
    }
}

#[test]
fn blocks_avoid_the_spawn() {
    for seed in 0..200 {
        let spawn = [161.0, 101.0];
        let level = generate(Style::World, seed, 32, 20, spawn);
        assert!(!level.collides(spawn, SIZE), "seed {seed}");
        assert!((0..32).any(|x| (0..20).any(|y| !level.is_open(x, y))));
    }
}

#[test]
fn los_is_symmetric() {
    let mut rng = Rng::new(7);
    for level in levels() {
        for _ in 0..200 {
            let (a, b) = (rng.open_point(&level), rng.open_point(&level));
            assert_eq!(level.los(a, b), level.los(b, a), "{a:?} <-> {b:?}");
        }
    }
}

#[test]
fn polygon_matches_los() {
    let mut rng = Rng::new(11);
    for level in levels() {
        let mut origins = vec![SPAWN, [160.0, 100.0]];
        origins.extend((0..4).map(|_| rng.open_point(&level)));
        for origin in origins.into_iter().filter(|o| is_open_point(&level, *o)) {
            for radius in [f32::INFINITY, 80.0] {
                let mut vision = Vision::new(&level, radius);
                vision.update(&level, origin);
                check_polygon(&level, &vision, &mut rng);
            }
        }
    }
}

#[test]
fn band_never_covers_open_floor() {
    let mut rng = Rng::new(3);
    for level in levels().take(50) {
        if !is_open_point(&level, SPAWN) {
            continue;
        }
        let mut v = Vision::new(&level, 96.0);
        v.update(&level, SPAWN);
        for _ in 0..300 {
            let p = rng.open_point(&level);
            assert!(!band_covers(&v, p) || !is_open_point(&level, p), "{p:?}");
        }
    }
}

#[test]
fn npcs_spawn_clear_and_never_enter_walls() {
    for seed in 0..20 {
        let style = if seed % 2 == 0 {
            Style::World
        } else {
            Style::Cave
        };
        let level = generate(style, seed, 32, 20, SPAWN);
        let mut rng = Rng::new(seed ^ 0x9E37_79B9);
        let mut npcs = Entity::spawn_npcs(&level, &mut rng, 4, 0, SPAWN, 40.0);
        assert_eq!(npcs.len(), 4);
        for _ in 0..2000 {
            for npc in &mut npcs {
                npc.wander(&level, &mut rng, 1.0 / 60.0);
                assert!(!level.collides(npc.pos, npc.size), "seed {seed}");
            }
        }
    }
}

#[test]
fn layered_levels_round_trip_as_files() {
    let level = generate_level(Style::Layered, 4, 32, 20, SPAWN);
    let layers: Vec<Vec<_>> = (0..level.depth())
        .map(|k| {
            (0..20)
                .flat_map(|y| (0..32).map(move |x| (x, y)))
                .map(|(x, y)| level.layer(k).tile(x, y))
                .collect()
        })
        .collect();
    let spawn = Spawn {
        layer: level.depth() - 1,
        tile: [16, 10],
    };
    let file = LevelFile::new(32, 20, &layers, spawn);
    let back = LevelFile::from_json(&file.to_json()).unwrap();
    assert_eq!(back.tiles(), layers);
    let loaded = back.level();
    assert_eq!(loaded.depth(), level.depth());
    assert_eq!(loaded.layer(1).tile(5, 5), level.layer(1).tile(5, 5));
}
