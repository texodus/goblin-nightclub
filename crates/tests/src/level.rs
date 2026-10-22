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

use goblin_nightclub_core::art::draw_tiles;
use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::layer::Corner;
use goblin_nightclub_core::layer::{Layer, Solid, Tile};
use goblin_nightclub_core::level::*;
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::prop::Prop;
use std::sync::Arc;

fn level(layers: &[&[&str]]) -> Level {
    Level::new(
        layers.iter().map(|rows| Layer::from_ascii(rows)).collect(),
        goblin_nightclub_core::tileset::testing::tileset(),
    )
}

#[test]
fn only_floor_is_walkable() {
    let l = level(&[
        &["#..◸"], //
        &["~~.~"],
    ]);
    let m = l.movement(1);
    assert_eq!(m.solid(0, 0), Solid::Full, "void over a wall");
    assert_eq!(m.solid(1, 0), Solid::Full, "void over floor");
    assert_eq!(m.solid(2, 0), Solid::None, "floor");
    assert_eq!(m.solid(3, 0), Solid::Full, "void over a slope");
    let bottom = level(&[&["~."]]);
    assert_eq!(bottom.movement(0).solid(0, 0), Solid::Full);
}

#[test]
fn liquid_blocks_movement_and_shows_the_layer_below() {
    let l = level(&[&["..."], &["≈.~"]]);
    assert_eq!(l.movement(1).solid(0, 0), Solid::Full, "a pit");
    assert_eq!(l.movement(1).solid(1, 0), Solid::None);
    assert_eq!(
        &*l.shaft(1, 0),
        &[true, false, true],
        "shows through like void"
    );
    assert_eq!(l.visible_layers(1), vec![1, 0]);
}

#[test]
fn half_void_tiles_block_their_void_half() {
    let l = level(&[
        &["#◤▛◺"], //
        &["▛◥▙~"],
    ]);
    let m = l.movement(1);
    // Floor in the NW half, void in the SE half, even over a wall.
    assert_eq!(m.solid(0, 0), Solid::Full.carve(Solid::Slope(Corner::NW)));
    // Wall in one half, void in the other: blocked throughout.
    assert_eq!(m.solid(1, 0), Solid::Full);
    // Floor in the SW half: the NE half blocks.
    assert_eq!(m.solid(2, 0), Solid::Slope(Corner::NE));
}

#[test]
fn shafts_need_void_all_the_way_up() {
    let l = level(&[
        &["........"], //
        &["...~~..."],
        &["..~~...."],
    ]);
    let t = true;
    let f = false;
    // Two down, the hole through both is tile 3.
    assert_eq!(&*l.shaft(2, 0), &[f, f, f, t, f, f, f, f]);
    // One down, the top's void.
    assert_eq!(&*l.shaft(2, 1), &[f, f, t, t, f, f, f, f]);
    assert!(l.shaft(2, 2).iter().all(|s| *s), "own layer: everywhere");
    // Half a tile of void counts.
    let half = level(&[&["..."], &[".▛."]]);
    assert_eq!(&*half.shaft(1, 0), &[f, t, f]);
}

#[test]
fn walls_in_between_block_only_inside_holes() {
    let l = level(&[
        &["....."], //
        &["#.##◸"],
        &["~~..~"],
    ]);
    // Layer 0 from layer 2: layer 1's walls under layer 2's void block
    // (whole, or the part of a slope the void is over); those under its
    // floor don't.
    let v = l.view(2, 0);
    assert_eq!(v.solid(0, 0), Solid::Full, "in the hole");
    assert_eq!(v.solid(2, 0), Solid::None, "under the floor");
    assert_eq!(v.solid(3, 0), Solid::None, "under the floor");
    assert_eq!(v.solid(4, 0), Solid::Slope(Corner::NW), "in the hole");
    // Layer 1 from layer 2: only layer 2's walls (none).
    assert!((0..5).all(|x| l.view(2, 1).solid(x, 0) == Solid::None));
    // A half-void tile above: only the part under the void blocks.
    let half = level(&[&["."], &["#"], &["▛"]]);
    assert_eq!(half.view(2, 0).solid(0, 0), Solid::Slope(Corner::SE));
}

#[test]
fn looking_down_sees_over_the_lower_layers_walls() {
    let l = level(&[
        &["#..."], //
        &["~~~#"],
    ]);
    // Layer 0 from layer 1: only layer 1's wall blocks.
    assert_eq!(l.view(1, 0).solid(0, 0), Solid::None);
    assert_eq!(l.view(1, 0).solid(3, 0), Solid::Full);
    // Layer 1 itself: its own wall.
    assert_eq!(l.view(1, 1).solid(3, 0), Solid::Full);
}

/// Any two kinds of part may share a tile along a diagonal, in any
/// corner; movement, sight, the liquid tint, drawing and level files
/// all follow the quarters.
#[test]
fn every_pairing_of_parts_splits_consistently() {
    use goblin_nightclub_core::draw::Canvas;
    use goblin_nightclub_core::layer::{Part, Tile};
    let tileset = goblin_nightclub_core::tileset::testing::tileset();
    let kinds = [Part::Floor(1), Part::Wall(1), Part::Liquid(0), Part::Void];
    let open = |p: Part| !matches!(p, Part::Floor(_));
    let liquid = |p: Part| matches!(p, Part::Liquid(_));
    for at in kinds {
        for rest in kinds.into_iter().filter(|r| *r != at) {
            for corner in Corner::CLOCKWISE {
                let tile = Tile::split(corner, at, rest);
                let (atq, restq) = (corner.quarters(), !corner.quarters() & 0b1111);
                let mut middle = vec![Tile::FLOOR; 9];
                middle[4] = tile;
                let level = Level::new(
                    vec![
                        Layer::from_ascii(&["...", "...", "..."]),
                        Layer::from_tiles(3, 3, middle),
                    ],
                    tileset.clone(),
                );
                let why = format!("{at:?} at {corner:?} over {rest:?}");
                // Blocked where not floor.
                let blocked =
                    (if open(at) { atq } else { 0 }) | (if open(rest) { restq } else { 0 });
                assert_eq!(
                    level.movement(1).solid(1, 1),
                    Solid::from_quarters(blocked, true),
                    "{why}: movement"
                );
                // Sight from the layer itself: its walls.
                assert_eq!(level.view(1, 1).solid(1, 1), tile.wall(), "{why}: view");
                // Open to the layer below where void or liquid.
                let void = (if matches!(at, Part::Void | Part::Liquid(_)) {
                    atq
                } else {
                    0
                }) | (if matches!(rest, Part::Void | Part::Liquid(_)) {
                    restq
                } else {
                    0
                });
                assert_eq!(tile.void_quarters(), void, "{why}");
                assert_eq!(level.shaft(1, 0)[4], void != 0, "{why}: shaft");
                assert_eq!(
                    level.visible_layers(1),
                    if void != 0 { vec![1, 0] } else { vec![1] },
                    "{why}"
                );
                let tint =
                    (if liquid(at) { atq } else { 0 }) | (if liquid(rest) { restq } else { 0 });
                assert_eq!(tile.liquid_quarters(), tint, "{why}");
                // Points in either half, by quarter.
                let layer = level.layer(1);
                let [ox, oy] = corner.offset();
                let near = [10.0 + ox as f32 * 8.0 + 1.0, 10.0 + oy as f32 * 8.0 + 1.0];
                let far = [
                    10.0 + (1 - ox) as f32 * 8.0 + 1.0,
                    10.0 + (1 - oy) as f32 * 8.0 + 1.0,
                ];
                assert_eq!(
                    layer.is_void_at(near),
                    matches!(at, Part::Void | Part::Liquid(_)),
                    "{why}"
                );
                assert_eq!(
                    layer.is_void_at(far),
                    matches!(rest, Part::Void | Part::Liquid(_)),
                    "{why}"
                );
                // Drawn: a triangle per half that isn't void, liquid
                // translucent.
                let alone = Layer::from_tiles(1, 1, vec![tile]);
                let mut canvas = Canvas::new(&tileset);
                draw_tiles(&alone, &tileset, &mut canvas);
                let halves = [at, rest].iter().filter(|p| **p != Part::Void).count();
                assert_eq!(canvas.vertices.len(), 3 * halves, "{why}: drawn");
                let translucent = canvas.vertices.iter().filter(|v| v.color[3] < 1.0).count();
                assert_eq!(
                    translucent,
                    3 * [at, rest].iter().filter(|p| liquid(**p)).count(),
                    "{why}"
                );
                // And written down.
                let text = tile.to_text(&tileset);
                assert_eq!(Tile::from_text(&text, &tileset), Ok(tile), "{why}: {text}");
            }
        }
    }
}

#[test]
fn props_in_the_way_are_solid_to_walking() {
    let t = goblin_nightclub_core::tileset::testing::tileset();
    let mut t2 = t.clone();
    t2.props[1].blocks = false;
    let tree = t.prop("tree").unwrap();
    let rock = t.prop("rock").unwrap();
    let props = [
        Prop {
            at: [1, 1],
            kind: tree,
        },
        Prop {
            at: [2, 1],
            kind: rock,
        },
    ];
    let layer = || Layer::from_ascii(&["....", "....", "...."]).with_props(&props);
    let l = Level::new(vec![layer()], t);
    assert_eq!(l.movement(0).solid(1, 1), Solid::Full);
    assert_eq!(l.movement(0).solid(2, 1), Solid::Full);
    assert_eq!(l.movement(0).solid(3, 1), Solid::None);
    // Not to the layer's own shape: they're no walls.
    assert_eq!(l.layer(0).solid(1, 1), Solid::None);
    // A prop that's not in the way isn't.
    let l = Level::new(vec![layer()], t2);
    assert_eq!(l.movement(0).solid(1, 1), Solid::Full);
    assert_eq!(l.movement(0).solid(2, 1), Solid::None);
}

#[test]
fn meshes_are_made_once_and_match_the_tiles() {
    let l = level(&[&["....", "...."], &["~~..", "~≈.."]]);
    let tileset = l.tileset().clone();
    let area = |m: &Mesh| -> f32 {
        m.vertices
            .chunks(3)
            .map(|t| {
                let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum()
    };
    let top = l.meshes(1);
    assert!(Arc::ptr_eq(top, l.meshes(1)), "kept");
    // A small level is one chunk.
    assert_eq!(top.pieces.len(), 1);
    let piece = &top.pieces[0];
    let mut canvas = Canvas::new(&tileset);
    draw_tiles(l.layer(1), &tileset, &mut canvas);
    assert_eq!(piece.tiles.vertices.len(), canvas.vertices.len());
    // Four void tiles, one of them liquid; layer 0 shows through all
    // four.
    assert!((area(&piece.voids) - 400.0).abs() < 1e-3);
    assert!((area(&piece.liquid) - 100.0).abs() < 1e-3);
    assert!((area(&l.shaft_mesh(1, 0)) - 400.0).abs() < 1e-3);
    assert!(Arc::ptr_eq(&l.shaft_mesh(1, 0), &l.shaft_mesh(1, 0)));
}

/// Views, shafts and the layers seen, worked out from `open_above`,
/// match working them out directly from every layer in between, on
/// random stacks with plenty of void, half void, liquid and walls.
#[test]
fn summaries_match_looking_through_every_layer() {
    use goblin_nightclub_core::layer::{Corner, Part};
    use goblin_nightclub_core::rng::Rng;
    let parts = [Part::Floor(0), Part::Wall(0), Part::Void, Part::Liquid(0)];
    for seed in 0..12 {
        let mut rng = Rng::new(seed);
        let (cols, rows) = (9, 7);
        let depth = MAX_VISIBLE + 4;
        let layers: Vec<Layer> = (0..depth)
            .map(|_| {
                let tiles = (0..cols * rows)
                    .map(|_| {
                        // Mostly void, so shafts run deep.
                        let part = |rng: &mut Rng| {
                            if rng.unit() < 0.6 {
                                Part::Void
                            } else {
                                parts[rng.range(0, 3)]
                            }
                        };
                        let (at, rest) = (part(&mut rng), part(&mut rng));
                        Tile::split(Corner::CLOCKWISE[rng.range(0, 3)], at, rest)
                    })
                    .collect();
                Layer::from_tiles(cols, rows, tiles)
            })
            .collect();
        let tiles: Vec<Vec<Tile>> = layers
            .iter()
            .map(|l| {
                (0..cols * rows)
                    .map(|t| l.tile((t % cols) as i32, (t / cols) as i32))
                    .collect()
            })
            .collect();
        let level = Level::new(layers, goblin_nightclub_core::tileset::testing::tileset());
        let void = |j: usize, t: usize| tiles[j][t].void_quarters();
        let open =
            |top: usize, j: usize, t: usize| (j + 1..=top).fold(0b1111, |q, l| q & void(l, t));
        for top in 0..depth {
            let mut seen = vec![top];
            for k in 0..=top {
                let (view, shaft) = (level.view(top, k), level.shaft(top, k));
                for t in 0..cols * rows {
                    let (x, y) = ((t % cols) as i32, (t / cols) as i32);
                    let q = (k + 1..top)
                        .map(|j| tiles[j][t].wall().quarters() & open(top, j, t))
                        .fold(tiles[top][t].wall().quarters(), |q, w| q | w);
                    let why = format!("seed {seed}, {k} from {top}, tile {t}");
                    assert_eq!(view.solid(x, y), Solid::from_quarters(q, true), "{why}");
                    assert_eq!(shaft[t], open(top, k, t) != 0, "{why}");
                }
                if k < top && top - k < MAX_VISIBLE && shaft.iter().any(|s| *s) {
                    seen.push(k);
                }
            }
            seen[1..].sort_unstable_by(|a, b| b.cmp(a));
            assert_eq!(level.visible_layers(top), seen, "seed {seed}, from {top}");
        }
    }
}

#[test]
fn no_more_than_max_visible_layers_are_drawn() {
    let mut rows: Vec<&[&str]> = vec![&["."]];
    rows.extend(std::iter::repeat_n(&["~"] as &[&str], MAX_VISIBLE + 3));
    let l = level(&rows);
    let top = l.depth() - 1;
    let seen = l.visible_layers(top);
    assert_eq!(seen.len(), MAX_VISIBLE);
    assert_eq!(
        seen,
        (top + 1 - MAX_VISIBLE..=top).rev().collect::<Vec<_>>()
    );
    assert!(
        l.shaft(top, 0)[0],
        "the bottom still shows through, undrawn"
    );
}

#[test]
fn layers_below_are_drawn_only_with_void_all_the_way_up() {
    let l = level(&[&["...."], &["~~.~"], &["~.~~"]]);
    assert_eq!(l.visible_layers(2), vec![2, 1, 0]);
    assert_eq!(level(&[&["."], &["."]]).visible_layers(1), vec![1]);
    assert_eq!(
        level(&[&["~"], &["."], &["~"]]).visible_layers(2),
        vec![2, 1]
    );
}
