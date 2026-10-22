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

use super::*;
use goblin_nightclub_core::chunk::identical;
use goblin_nightclub_core::layer::{Dir, Layer};
use goblin_nightclub_core::level::Level;

/// The parameters tests go by, whatever the defaults are.
pub(crate) fn params() -> Params {
    Params {
        underworld_layers: 3,
        overworld_layers: 4,
        ..Params::default()
    }
}

/// A world in small regions, for quick tests.
fn plan(seed: u32, cols: usize, rows: usize) -> Arc<Plan> {
    Arc::new(Plan::new(seed, cols, rows, &params()).with_region(64))
}

#[test]
fn narrow_deep_dungeons_go_all_the_way_down() {
    // One region, many layers: every layer has stairs, and the player
    // can walk from the spawn to the bottom.
    let p = Arc::new(
        Plan::new(
            5,
            64,
            64,
            &Params {
                underworld_layers: 40,
                ..params()
            },
        )
        .with_region(64),
    );
    let ([cols, rows], layers) = region_tiles(&p, 0, 0);
    let level = Level::new(
        layers
            .iter()
            .map(|t| Layer::from_tiles(cols, rows, t.clone()))
            .collect(),
        tileset().clone(),
    );
    let top = spawn_layer(&p.params);
    let reached = crate::reachable_across(&level, top, p.spawn);
    assert!(reached[0].iter().any(|r| *r), "the bottom");
    assert!((1..40).all(|k| layers[k].iter().any(|t| t.stairs().is_some())));
}

/// All of a world made a region at a time: each layer's tiles, row by
/// row.
fn stitched(plan: &Arc<Plan>) -> Vec<Vec<Tile>> {
    let [cols, rows] = plan.size;
    let mut out = vec![vec![Tile::VOID; cols * rows]; plan.depth()];
    let [across, down] = plan.regions();
    for ry in 0..down {
        for rx in 0..across {
            let ([w, h], tiles) = region_tiles(plan, rx, ry);
            let [x0, y0] = [rx * plan.region, ry * plan.region];
            for (k, layer) in tiles.iter().enumerate() {
                for i in 0..w * h {
                    out[k][(y0 + i / w) * cols + x0 + i % w] = layer[i];
                }
            }
        }
    }
    out
}

#[test]
fn regions_come_out_the_same_however_made() {
    let p = plan(1, 192, 128);
    let (a, b) = (region_tiles(&p, 1, 1), region_tiles(&p, 1, 1));
    assert_eq!(a.0, b.0);
    assert!(
        a.1.iter()
            .flatten()
            .zip(b.1.iter().flatten())
            .all(|(s, t)| identical(*s, *t))
    );
    // Through the source, whichever was made before.
    let (first, later) = (Generated::new((*p).clone()), Generated::new((*p).clone()));
    first.chunk((0, 0, 0));
    first.chunk((0, 5, 3));
    let key = (4, 3, 2);
    assert!(first.chunk(key).unwrap().same(&later.chunk(key).unwrap()));
    assert_eq!(first.roof(3, 2), later.roof(3, 2));
    assert!(later.chunk((0, 6, 0)).is_none() && later.chunk((p.depth(), 0, 0)).is_none());
}

#[test]
fn neighbors_agree_on_their_border() {
    let p = plan(2, 192, 128);
    let r = p.region as i32;
    let (mut same, mut all) = (0, 0);
    // A region's apron, by its neighbor, against the neighbor's own
    // tiles: within a few tiles of their border, on every layer.
    for (a, b) in [
        ([0, 0], [1, 0]),
        ([1, 0], [2, 0]),
        ([0, 0], [0, 1]),
        ([1, 1], [2, 1]),
    ] {
        let boxed = region_box(&p, a[0], a[1]);
        let ([bw, _], theirs) = region_tiles(&p, b[0], b[1]);
        let b0 = [b[0] as i32 * r, b[1] as i32 * r];
        for y in 0..boxed.size[1] as i32 {
            for x in 0..boxed.size[0] as i32 {
                let [ax, ay] = [boxed.origin[0] + x, boxed.origin[1] + y];
                let [lx, ly] = [ax - b0[0], ay - b0[1]];
                let near = if b[0] != a[0] { lx } else { ly };
                let inside = (0..r).contains(&lx) && (0..r).contains(&ly);
                if !inside || near >= 8 {
                    continue;
                }
                for (mine, other) in boxed.layers.iter().zip(&theirs) {
                    let mine = mine[(y * boxed.size[0] as i32 + x) as usize];
                    let other = other[(ly * bw as i32 + lx) as usize];
                    // Right at the border, where each region's own tiles
                    // meet the other's, they must agree exactly.
                    assert!(near > 0 || identical(mine, other), "at ({ax}, {ay})");
                    same += identical(mine, other) as usize;
                    all += 1;
                }
            }
        }
    }
    // Further in, all but a few: cave pockets the region filled (the
    // apron of the other leaves them, but never needs to know).
    assert!(same as f32 >= 0.995 * all as f32, "{same} of {all}");
}

#[test]
fn props_stand_alone_across_region_borders() {
    let p = plan(5, 192, 128);
    let layers = stitched(&p);
    let [cols, rows] = p.size;
    let [across, down] = p.regions();
    let mut props: Vec<std::collections::HashSet<[i32; 2]>> = vec![Default::default(); p.depth()];
    for ry in 0..down {
        for rx in 0..across {
            let (_, _, made) = region_made(&p, rx, ry);
            let [x0, y0] = [rx * p.region, ry * p.region].map(|v| v as i32);
            for (k, list) in made.iter().enumerate() {
                props[k].extend(
                    list.iter()
                        .map(|q| [x0 + q.at[0] as i32, y0 + q.at[1] as i32]),
                );
            }
        }
    }
    assert!(props.iter().map(|p| p.len()).sum::<usize>() > 50);
    let floor = |k: usize, x: i32, y: i32| {
        let inside = (0..cols as i32).contains(&x) && (0..rows as i32).contains(&y);
        inside
            && matches!(
                layers[k][y as usize * cols + x as usize],
                Tile::Whole(goblin_nightclub_core::layer::Part::Floor(_))
            )
    };
    for (k, here) in props.iter().enumerate() {
        for &[x, y] in here {
            for (dx, dy) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))) {
                if (dx, dy) != (0, 0) {
                    assert!(floor(k, x + dx, y + dy), "layer {k}: around ({x}, {y})");
                    assert!(
                        !here.contains(&[x + dx, y + dy]),
                        "layer {k}: beside ({x}, {y})"
                    );
                }
            }
        }
    }
}

#[test]
fn stitched_worlds_keep_the_rules() {
    let p = plan(3, 192, 128);
    let layers = stitched(&p);
    let [cols, rows] = p.size;
    let at = |k: usize, x: i32, y: i32| layers[k][y as usize * cols + x as usize];
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && x < cols as i32 && y < rows as i32;
    for k in 0..p.depth() {
        for y in 0..rows as i32 {
            for x in 0..cols as i32 {
                let t = at(k, x, y);
                // Water rests on solid ground.
                if t.liquid_quarters() != 0 && k > 0 {
                    for (dx, dy) in (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (dx, dy))) {
                        if inside(x + dx, y + dy) {
                            let under = at(k - 1, x + dx, y + dy);
                            let solid = under.wall().quarters() | under.liquid_quarters();
                            assert_eq!(solid, 0b1111, "under water at ({x}, {y}) layer {k}");
                        }
                    }
                }
                // Stairs come in pairs.
                match t.stairs() {
                    Some(Dir::Down) => {
                        assert_eq!(at(k - 1, x, y).stairs(), Some(Dir::Up), "({x}, {y}) {k}")
                    }
                    Some(Dir::Up) => {
                        assert_eq!(at(k + 1, x, y).stairs(), Some(Dir::Down), "({x}, {y}) {k}")
                    }
                    None => {}
                }
            }
        }
    }
    // Cliffs a layer tall, across region borders too: where a column
    // is just terrain (rock up to a floor, void above), its neighbors'
    // heights are within one of its.
    let m = p.params.underworld_layers;
    let height = |x: i32, y: i32| {
        let h = (m..p.depth()).find(|&k| {
            !matches!(
                at(k, x, y),
                Tile::Whole(goblin_nightclub_core::layer::Part::Wall(_))
            )
        })?;
        // Plains and up: not the floor of a hole or a cellar.
        let plain = h > m
            && at(h, x, y).is_floor()
            && (h + 1..p.depth()).all(|k| at(k, x, y) == Tile::VOID);
        plain.then_some(h)
    };
    for y in 1..rows as i32 - 1 {
        for x in 1..cols as i32 - 1 {
            let Some(h) = height(x, y) else { continue };
            for (nx, ny) in [(x + 1, y), (x, y + 1)] {
                if let Some(n) = height(nx, ny) {
                    assert!(h.abs_diff(n) <= 1, "a cliff {h} to {n} at ({x}, {y})");
                }
            }
        }
    }
    // The player can walk from the spawn to most of the plains.
    let level = Level::new(
        layers
            .iter()
            .map(|t| Layer::from_tiles(cols, rows, t.clone()))
            .collect(),
        tileset().clone(),
    );
    let top = spawn_layer(&p.params);
    let reached = crate::reachable_across(&level, top, p.spawn);
    let plains: Vec<usize> = (0..cols * rows)
        .filter(|&i| layers[top][i].is_floor())
        .collect();
    let got = plains.iter().filter(|&&i| reached[top][i]).count();
    assert!(
        got as f32 >= 0.98 * plains.len() as f32,
        "{got} of {}",
        plains.len()
    );
}
