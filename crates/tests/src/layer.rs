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

use goblin_nightclub_core::art::draw_bevels;
use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::geom::{EVERYWHERE, Rect, distance, inset_corner, inward_normal};
use goblin_nightclub_core::layer::testing::check_outlines;
use goblin_nightclub_core::layer::*;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset::{self, Tileset};

const SIZE: [f32; 2] = [8.0, 8.0];

#[track_caller]
fn assert_near(a: [f32; 2], b: [f32; 2]) {
    assert!(
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3,
        "{a:?} != {b:?}"
    );
}

#[test]
fn slides_along_tiled_wall_without_snagging() {
    // A floor of separate tiles along row 5 (y 50..60).
    let level = Layer::from_ascii(&[
        "....................",
        "....................",
        "....................",
        "....................",
        "....................",
        "####################",
    ]);
    let mut pos = [5.0, 41.0];
    for _ in 0..100 {
        pos = level.move_box(pos, SIZE, [1.0, 1.0]);
        assert!(!level.collides(pos, SIZE));
    }
    assert_near(pos, [105.0, 42.0]);
}

#[test]
fn slides_up_a_slope() {
    // Solid below the "/" face x + y = 70.
    let level = Layer::from_ascii(&[
        "......◿#", //
        ".....◿##", //
        "....◿###", //
        "...◿####", //
        "..◿#####", //
        ".◿######", //
        "◿#######", //
    ]);
    let size = [4.0, 4.0];
    let mut pos = [20.0, 30.0];
    for _ in 0..30 {
        pos = level.move_box(pos, size, [1.0, 0.0]);
        assert!(!level.collides(pos, size), "{pos:?}");
    }
    // Resting on the face: bottom-right corner on x + y = 70.
    assert!((pos[0] + pos[1] + 8.0 - 70.0).abs() < 1e-2, "{pos:?}");
    assert!(pos[1] < 30.0 && pos[0] > 24.0, "slid up and along: {pos:?}");
}

#[test]
fn slopes_slide_in_every_direction() {
    // A lone slope of each kind; push a box into it from all 8 directions.
    for tile in ["◸", "◹", "◺", "◿"] {
        let rows = [".....", ".....", &format!("..{tile}.."), ".....", "....."];
        let level = Layer::from_ascii(&rows);
        for d in [
            [1., 0.],
            [-1., 0.],
            [0., 1.],
            [0., -1.],
            [1., 1.],
            [1., -1.],
            [-1., 1.],
            [-1., -1.],
        ] {
            let start = [25.0 - d[0] * 12.0 - 2.0, 25.0 - d[1] * 12.0 - 2.0];
            let mut pos = start;
            for _ in 0..40 {
                pos = level.move_box(pos, [4.0, 4.0], [d[0] * 0.7, d[1] * 0.7]);
                assert!(!level.collides(pos, [4.0, 4.0]), "{tile} {d:?} {pos:?}");
            }
            assert!(pos != start, "{tile} {d:?}: stuck at start");
        }
    }
}

#[test]
fn settles_in_a_wedge() {
    // A 135-degree corner where the face x + y = 60 meets the wall x = 40.
    let level = Layer::from_ascii(&[
        "....#", //
        "....#", //
        "...◿#", //
        "..◿##", //
        ".◿###", //
    ]);
    let size = [6.0, 6.0];
    // Pushing right and a little down: onto the face, then along it.
    let mut pos = [10.0, 5.0];
    let mut trail = Vec::new();
    for _ in 0..80 {
        pos = level.move_box(pos, size, [1.0, 0.5]);
        assert!(!level.collides(pos, size), "{pos:?}");
        trail.push(pos);
    }
    // Wedged against both: right edge on the wall, corner on the face,
    // and holding still rather than jittering.
    assert_near(pos, [34.0, 14.0]);
    assert!(
        trail[trail.len() - 10..].iter().all(|p| *p == pos),
        "{:?}",
        &trail[trail.len() - 10..]
    );
}

#[test]
fn fast_moves_dont_tunnel() {
    // One-tile walls at x 20..30: full tiles, and stacked slopes whose
    // left sides form a continuous face.
    for rows in [["..#..", "..#..", "..#.."], ["..◸..", "..◸..", "..◸.."]] {
        let level = Layer::from_ascii(&rows);
        let size = [3.0, 3.0];
        for y in [1.0, 8.5, 13.0, 26.0] {
            let mut pos = [2.0, y];
            for _ in 0..10 {
                pos = level.move_box(pos, size, [9.0, 0.0]);
            }
            assert!(
                pos[0] + size[0] <= 20.0 + 1e-3,
                "through {rows:?} at y={y}: {pos:?}"
            );
        }
    }
}

#[test]
fn random_walks_never_enter_solid() {
    let mut rng = Rng::new(5);
    for seed in 0..30 {
        let level = Layer::soup(seed, 32, 20);
        let size = [6.0, 6.0];
        let Some(mut pos) = (0..1000)
            .map(|_| [rng.unit() * 310.0, rng.unit() * 190.0])
            .find(|p| !level.collides(*p, size))
        else {
            continue;
        };
        for _ in 0..500 {
            let a = rng.unit() * std::f32::consts::TAU;
            let len = rng.unit() * 9.0;
            pos = level.move_box(pos, size, [a.cos() * len, a.sin() * len]);
            assert!(!level.collides(pos, size), "seed {seed} at {pos:?}");
        }
    }
}

fn level_with(x: i32, y: i32, w: i32, h: i32) -> Layer {
    let mut tiles = vec![Tile::FLOOR; 100];
    for ty in y..y + h {
        for tx in x..x + w {
            tiles[(ty * 10 + tx) as usize] = Tile::WALL;
        }
    }
    Layer::from_tiles(10, 10, tiles)
}

#[test]
fn stops_flush_against_block() {
    let level = level_with(5, 0, 1, 10);
    // One big move and many small ones both end flush at x = 50 - 8.
    let pos = level.move_box([31.0, 1.0], SIZE, [100.0, 0.0]);
    assert_near(pos, [42.0, 1.0]);
    let mut pos = [31.0, 1.0];
    for _ in 0..20 {
        pos = level.move_box(pos, SIZE, [1.5, 0.0]);
    }
    assert_near(pos, [42.0, 1.0]);
    assert!(!level.collides(pos, SIZE));
}

#[test]
fn slides_along_wall() {
    let level = level_with(5, 0, 1, 10);
    let pos = level.move_box([42.0, 10.0], SIZE, [1.0, 1.0]);
    assert_near(pos, [42.0, 11.0]);
}

#[test]
fn edge_is_solid() {
    let level = level_with(9, 9, 1, 1);
    assert_near(level.move_box([1.0, 1.0], SIZE, [-3.0, -3.0]), [0.0, 0.0]);
}

/// Shorthand: does the segment from `a` to `b` have line of sight?
fn sees(level: &Layer, a: [f32; 2], b: [f32; 2]) -> bool {
    level.los(a, b)
}

#[test]
fn rays_hit_slope_sides_and_face() {
    // Slope solid in the NW half of tile (1,1): x 10..20, y 10..20.
    let level = Layer::from_ascii(&["...", ".◸.", "..."]);
    let left = level.raycast([5.0, 12.5], [1.0, 0.0], f32::INFINITY);
    assert!(
        (left - 5.0).abs() < 1e-4,
        "hits the left side at x=10: {left}"
    );
    let right = level.raycast([25.0, 17.5], [-1.0, 0.0], f32::INFINITY);
    assert!(
        (right - 12.5).abs() < 1e-4,
        "hits the face at x=12.5: {right}"
    );
    // From below-right, the face is visible but not what's behind it.
    assert!(sees(&level, [25.0, 25.0], [15.0, 15.0]));
    assert!(!sees(&level, [25.0, 25.0], [12.0, 12.0]));
}

#[test]
fn rays_graze_slope_faces_and_corners() {
    let level = Layer::from_ascii(&["...", ".◸.", "..."]);
    // Exactly along the face from corner (20,10) to corner (10,20).
    assert!(sees(&level, [25.0, 5.0], [5.0, 25.0]));
    // A hair inside the solid side of it.
    assert!(!sees(&level, [24.9, 5.0], [4.9, 25.0]));
    // Through the empty corner (20,20) only.
    assert!(sees(&level, [25.0, 15.0], [15.0, 25.0]));
}

#[test]
fn slopes_pinch_at_shared_vertex() {
    // Both reach vertex (10,10): a ray through it is blocked.
    let pinch = Layer::from_ascii(&["◿.", ".◸"]);
    assert!(!sees(&pinch, [15.0, 5.0], [5.0, 15.0]));
    // Only the first reaches it: the ray grazes past.
    let graze = Layer::from_ascii(&["◿.", ".◿"]);
    assert!(sees(&graze, [15.0, 5.0], [5.0, 15.0]));
}

#[test]
fn rays_graze_along_straight_faces() {
    // Along y = 10, the top face of the block, with open floor above.
    let level = Layer::from_ascii(&["....", ".##.", "...."]);
    assert!(sees(&level, [0.0, 10.0], [40.0, 10.0]));
    assert!(sees(&level, [40.0, 10.0], [0.0, 10.0]));
    // Along x = 20, through the middle of the block: blocked.
    assert!(!sees(&level, [20.0, 0.0], [20.0, 30.0]));
    // Along y = 10 between diagonally touching blocks: pinched.
    let pinch = Layer::from_ascii(&["#...", ".#..", "...."]);
    assert!(!sees(&pinch, [0.0, 10.0], [40.0, 10.0]));
}

#[test]
fn rays_start_inside_slope_tiles() {
    let level = Layer::from_ascii(&["◸.."]);
    // Empty half of the slope tile, looking right: open to the far edge.
    let open = level.raycast([8.0, 8.0], [1.0, 0.0], f32::INFINITY);
    assert!((open - 22.0).abs() < 1e-4, "{open}");
    // Solid half: blocked immediately.
    assert_eq!(level.raycast([2.0, 2.0], [1.0, 0.0], f32::INFINITY), 0.0);
}

#[test]
fn corners_follow_diagonal_outline() {
    // Solid below the diagonal from (30,0) to (0,30).
    let level = Layer::from_ascii(&["..◿", ".◿#", "◿##"]);
    let mut corners = level.corners().to_vec();
    corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(corners, vec![[0.0, 0.0], [0.0, 30.0], [30.0, 0.0]]);

    // A lone slope: its three corners.
    let lone = Layer::from_ascii(&["...", ".◹.", "..."]);
    let c = lone.corners();
    for v in [[10.0, 10.0], [20.0, 10.0], [20.0, 20.0]] {
        assert!(c.contains(&v), "{v:?} in {c:?}");
    }
    assert!(!c.contains(&[10.0, 20.0]));
}

#[test]
fn outline_edges_cover_sides_and_faces() {
    let level = Layer::from_ascii(&["◸"]);
    let edges: Vec<_> = level
        .outline_edges()
        .iter()
        .map(|e| (e.a, e.b, e.solid))
        .collect();
    // Solid on the right of each: the face (solid toward the NW corner),
    // and the empty half's right and bottom sides against the
    // out-of-bounds wall.
    assert!(
        edges.contains(&([10.0, 0.0], [0.0, 10.0], [0, 0])),
        "{edges:?}"
    );
    assert!(
        edges.contains(&([10.0, 10.0], [10.0, 0.0], [1, 0])),
        "{edges:?}"
    );
    assert!(
        edges.contains(&([0.0, 10.0], [10.0, 10.0], [0, 1])),
        "{edges:?}"
    );
    assert_eq!(edges.len(), 3, "{edges:?}");
}

/// Twice the signed area: positive clockwise on screen (around solid).
fn area2(points: &[[f32; 2]]) -> f32 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

#[test]
fn outlines_of_a_block_and_the_boundary() {
    let level = Layer::from_ascii(&["....", ".##.", ".##.", "...."]);
    check_outlines(&level);
    let outlines = level.outlines();
    assert_eq!(outlines.len(), 2);
    let block = outlines.iter().find(|o| area2(&o.points) > 0.0).unwrap();
    assert_eq!(block.points.len(), 4);
    assert!(
        block.runs.iter().all(|r| r.len() == 2),
        "straight runs merged"
    );
    assert!((area2(&block.points) - 2.0 * 400.0).abs() < 1e-3);
    let boundary = outlines.iter().find(|o| area2(&o.points) < 0.0).unwrap();
    assert!((area2(&boundary.points) + 2.0 * 1600.0).abs() < 1e-3);
}

#[test]
fn hollow_room_has_a_hole_loop() {
    let level = Layer::from_ascii(&[
        ".....", //
        ".###.", //
        ".#.#.", //
        ".###.", //
        ".....", //
    ]);
    check_outlines(&level);
    let mut areas: Vec<f32> = level
        .outlines()
        .into_iter()
        .map(|o| area2(&o.points) / 2.0)
        .collect();
    areas.sort_by(f32::total_cmp);
    // Layer boundary, the hole (counter-clockwise), the room's outside.
    assert_eq!(areas, vec![-2500.0, -100.0, 900.0]);
}

#[test]
fn touching_blocks_merge_and_pinches_split() {
    // An L of two blocks sharing a side: one loop, no internal edges.
    let l = Layer::from_ascii(&["....", ".#..", ".##.", "...."]);
    check_outlines(&l);
    let solid: Vec<_> = l
        .outlines()
        .into_iter()
        .filter(|o| area2(&o.points) > 0.0)
        .collect();
    assert_eq!(solid.len(), 1);
    assert_eq!(solid[0].points.len(), 6);

    // Two blocks touching only at a corner: two loops.
    let pinch = Layer::from_ascii(&["....", ".#..", "..#.", "...."]);
    check_outlines(&pinch);
    let solid: Vec<_> = pinch
        .outlines()
        .into_iter()
        .filter(|o| area2(&o.points) > 0.0)
        .collect();
    assert_eq!(solid.len(), 2);
    assert!(solid.iter().all(|o| o.points.len() == 4));
}

#[test]
fn slanted_outlines_merge_diagonal_runs() {
    // A rising bar: two full tiles with slopes filling both steps.
    let bar = Layer::from_ascii(&[".....", ".◿#..", ".#◸..", "....."]);
    check_outlines(&bar);
    let solid: Vec<_> = bar
        .outlines()
        .into_iter()
        .filter(|o| area2(&o.points) > 0.0)
        .collect();
    assert_eq!(solid.len(), 1);
    // Flat ends and two long diagonal faces: a hexagon.
    assert_eq!(solid[0].points.len(), 6, "{:?}", solid[0].points);

    for seed in 0..30 {
        check_outlines(&Layer::soup(seed, 32, 20));
    }
}

/// How outlines were found before edges were linked: chained edge by
/// edge, each the sharpest right turn among those not yet used.
fn traced_outlines(level: &Layer) -> Vec<Outline> {
    let key = |p: [f32; 2]| [(p[0] / TILE).round() as i32, (p[1] / TILE).round() as i32];
    let edges = level.outline_edges();
    let dir = |e: &Edge| {
        let [a, b] = [key(e.a), key(e.b)];
        [b[0] - a[0], b[1] - a[1]]
    };
    let mut starting: std::collections::HashMap<[i32; 2], Vec<usize>> = Default::default();
    for (i, e) in edges.iter().enumerate() {
        starting.entry(key(e.a)).or_default().push(i);
    }
    let mut used = vec![false; edges.len()];
    let mut out = Vec::new();
    for first in 0..edges.len() {
        if used[first] {
            continue;
        }
        let mut chain = vec![first];
        used[first] = true;
        loop {
            let last = *chain.last().unwrap();
            let d = dir(&edges[last]);
            let next = starting[&key(edges[last].b)]
                .iter()
                .copied()
                .filter(|&i| !used[i] || i == first)
                .max_by(|&i, &j| {
                    let turn = |k: usize| {
                        let n = dir(&edges[k]);
                        let cross = (d[0] * n[1] - d[1] * n[0]) as f32;
                        let dot = (d[0] * n[0] + d[1] * n[1]) as f32;
                        cross.atan2(dot)
                    };
                    turn(i).total_cmp(&turn(j))
                });
            match next {
                Some(i) if i == first => break,
                Some(i) => {
                    used[i] = true;
                    chain.push(i);
                }
                None => panic!("an open outline"),
            }
        }
        let turns_at = |k: usize| {
            dir(&edges[chain[k]]) != dir(&edges[chain[(k + chain.len() - 1) % chain.len()]])
        };
        let start = (0..chain.len()).find(|&k| turns_at(k)).unwrap_or(0);
        chain.rotate_left(start);
        let mut outline = Outline {
            points: Vec::new(),
            runs: Vec::new(),
        };
        for (k, &i) in chain.iter().enumerate() {
            if k == 0 || dir(&edges[i]) != dir(&edges[chain[k - 1]]) {
                outline.points.push(edges[i].a);
                outline.runs.push(Vec::new());
            }
            outline.runs.last_mut().unwrap().push(i);
        }
        out.push(outline);
    }
    out
}

/// Bevel strips as they were drawn from whole loops, each a quad of
/// points.
fn traced_bevels(level: &Layer, width: f32) -> Vec<[[f32; 2]; 4]> {
    let mut out = Vec::new();
    for o in traced_outlines(level) {
        let n = o.points.len();
        let normals: Vec<[f32; 2]> = (0..n)
            .map(|k| inward_normal(o.points[k], o.points[(k + 1) % n]))
            .collect();
        let inset: Vec<[f32; 2]> = (0..n)
            .map(|k| inset_corner(o.points[k], normals[(k + n - 1) % n], normals[k], width))
            .collect();
        for (k, run) in o.runs.iter().enumerate() {
            let normal = [normals[k][0] * width, normals[k][1] * width];
            let (start, end) = (o.points[k], o.points[(k + 1) % n]);
            let inner = |p: [f32; 2], corner: [f32; 2], mitered: [f32; 2]| {
                if p == corner {
                    mitered
                } else {
                    [p[0] + normal[0], p[1] + normal[1]]
                }
            };
            for &i in run {
                let e = level.outline_edges()[i];
                if level.in_bounds(e.solid) {
                    let a = inner(e.a, start, inset[k]);
                    let b = inner(e.b, end, inset[(k + 1) % n]);
                    out.push([e.a, e.b, b, a]);
                }
            }
        }
    }
    out
}

#[test]
fn linked_edges_make_the_loops_tracing_did() {
    let close = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
    let mut levels: Vec<Layer> = (0..40).map(|seed| Layer::soup(seed, 40, 30)).collect();
    // Pinches: lone full tiles touching at corners, a checkerboard.
    levels.push(Layer::from_ascii(&["#.#.", ".#.#", "#.#.", ".#.#"]));
    levels.push(Layer::from_ascii(&["....", ".#..", "..#.", "...."]));
    for level in &levels {
        check_outlines(level);
        let traced = traced_outlines(level);
        assert_eq!(level.outlines(), traced);
        // Concave corners: where a loop turns left.
        let mut concave = Vec::new();
        for o in &traced {
            let n = o.points.len();
            for k in 0..n {
                let (prev, here, next) = (
                    o.points[(k + n - 1) % n],
                    o.points[k],
                    o.points[(k + 1) % n],
                );
                let (d0, d1) = (
                    [here[0] - prev[0], here[1] - prev[1]],
                    [next[0] - here[0], next[1] - here[1]],
                );
                if d0[0] * d1[1] - d0[1] * d1[0] < 0.0 {
                    concave.push((here, inward_normal(prev, here), inward_normal(here, next)));
                }
            }
        }
        assert_eq!(level.concave_corners().len(), concave.len());
        for c in level.concave_corners() {
            assert!(
                concave
                    .iter()
                    .any(|&(at, b, a)| close(at, c.at) && close(b, c.before) && close(a, c.after)),
                "{c:?}"
            );
        }
        // Bevels: the same strips.
        let tileset = goblin_nightclub_core::tileset::testing::tileset();
        let mut canvas = Canvas::new(&tileset);
        draw_bevels(level, &tileset, &mut canvas, |_| true, 1.5, EVERYWHERE);
        let traced = traced_bevels(level, 1.5);
        let drawn = canvas.vertices.len();
        assert_eq!(drawn, traced.len() * 6, "two triangles a strip");
        let strips: Vec<[[f32; 2]; 3]> = canvas
            .vertices
            .chunks(3)
            .map(|t| [t[0].pos, t[1].pos, t[2].pos])
            .collect();
        for quad in &traced {
            let [a, b, c, d] = *quad;
            let has = |t: [[f32; 2]; 3]| {
                strips
                    .iter()
                    .any(|s| s.iter().zip(&t).all(|(p, q)| close(*p, *q)))
            };
            assert!(has([a, b, c]) && has([a, c, d]), "{quad:?}");
        }
    }
}

/// Area of the bevel strips drawn.
fn bevel_area(level: &Layer, bevel: impl Fn(usize) -> bool) -> f32 {
    let tileset = tileset::testing::tileset();
    let mut canvas = Canvas::new(&tileset);
    draw_bevels(level, &tileset, &mut canvas, bevel, 1.0, EVERYWHERE);
    canvas
        .vertices
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
        })
        .sum()
}

#[test]
fn bevels_out_of_view_are_left_out() {
    // Blocks at the left and right ends.
    let level = Layer::from_ascii(&["........", ".#....#.", "........"]);
    let tileset = tileset::testing::tileset();
    let area = |within: Rect| {
        let mut canvas = Canvas::new(&tileset);
        draw_bevels(&level, &tileset, &mut canvas, |_| true, 1.0, within);
        canvas.vertices.len()
    };
    let all = area(EVERYWHERE);
    let left = area([[0.0, 0.0], [30.0, 30.0]]);
    assert!(left > 0 && 2 * left == all, "{left} of {all}");
    assert_eq!(area([[25.0, 0.0], [55.0, 30.0]]), 0, "between them");
}

#[test]
fn fringed_walls_have_no_bevels() {
    let level = Layer::from_ascii(&["....", ".#..", "...."]);
    let mut tileset = tileset::testing::tileset();
    let count = |tileset: &Tileset| {
        let mut canvas = Canvas::new(tileset);
        draw_bevels(&level, tileset, &mut canvas, |_| true, 1.0, EVERYWHERE);
        canvas.vertices.len()
    };
    assert!(count(&tileset) > 0);
    tileset.walls[0].edge.width = 2.0;
    assert_eq!(count(&tileset), 0, "the fringe's rim instead");
}

#[test]
fn near_finds_everything_within_reach() {
    let mut rng = Rng::new(5);
    for seed in 0..10 {
        let level = Layer::soup(seed, 60, 40);
        for _ in 0..10 {
            let p = [rng.unit() * 600.0, rng.unit() * 400.0];
            let radius = rng.unit() * 200.0;
            let dist = |q: [f32; 2]| distance(p, q);
            let near = level.corners_near(p, radius);
            assert!(near.windows(2).all(|w| w[0] < w[1]), "in order, once each");
            for (i, c) in level.corners().iter().enumerate() {
                if dist(*c) <= radius {
                    assert!(near.contains(&i), "corner {c:?} near {p:?}");
                }
            }
            let near = level.edges_near(p, radius);
            for (i, e) in level.outline_edges().iter().enumerate() {
                // Its nearest point to `p`.
                let d = [e.b[0] - e.a[0], e.b[1] - e.a[1]];
                let t = (((p[0] - e.a[0]) * d[0] + (p[1] - e.a[1]) * d[1])
                    / (d[0] * d[0] + d[1] * d[1]))
                    .clamp(0.0, 1.0);
                if dist([e.a[0] + t * d[0], e.a[1] + t * d[1]]) <= radius {
                    assert!(near.contains(&i), "edge {i} near {p:?}");
                }
            }
            let near = level.concave_near(p, radius);
            for (i, c) in level.concave_corners().iter().enumerate() {
                if dist(c.at) <= radius {
                    assert!(near.contains(&i));
                }
            }
        }
        let all = level.corners_near([0.0; 2], f32::INFINITY);
        assert_eq!(all.len(), level.corners().len());
    }
}

#[test]
fn bevels_tile_the_border_exactly() {
    // A 20x20 block: strips cover its border, 400 - 18 * 18.
    let block = Layer::from_ascii(&["....", ".##.", ".##.", "...."]);
    assert!((bevel_area(&block, |_| true) - 76.0).abs() < 1e-3);

    // An L (concave corner): 300 minus its 1px inset, 18 * 18 - 10 * 10.
    let l = Layer::from_ascii(&["....", ".#..", ".##.", "...."]);
    assert!((bevel_area(&l, |_| true) - 76.0).abs() < 1e-3);

    // A lone slope with two 45-degree corners: half of 100 minus its
    // inset triangle, whose legs are 10 - 2 - sqrt(2).
    let slope = Layer::from_ascii(&["...", ".◸.", "..."]);
    let leg = 10.0 - 2.0 - std::f32::consts::SQRT_2;
    assert!((bevel_area(&slope, |_| true) - (50.0 - leg * leg / 2.0)).abs() < 1e-3);
}

#[test]
fn bevels_only_where_picked() {
    let block = Layer::from_ascii(&["....", ".##.", ".##.", "...."]);
    assert_eq!(bevel_area(&block, |_| false), 0.0);
    // Just the top face's two edges: each 10x1 strip, mitered at the
    // outer ends (each loses a half-pixel triangle).
    let top: Vec<usize> = (0..block.outline_edges().len())
        .filter(|&i| {
            let e = block.outline_edges()[i];
            e.a[1] == 10.0 && e.b[1] == 10.0 && block.in_bounds(e.solid)
        })
        .collect();
    assert_eq!(top.len(), 2);
    let area = bevel_area(&block, |i| top.contains(&i));
    assert!((area - 19.0).abs() < 1e-3, "{area}");
}

#[test]
fn union_and_carve() {
    use Corner::*;
    use Solid::*;
    assert_eq!(Slope(NW).union(Slope(SE)), Full, "opposite slopes fill");
    assert_eq!(Slope(NW).union(Slope(NW)), Slope(NW));
    assert_eq!(
        Slope(NW).union(Slope(NE)),
        Full,
        "unrepresentable rounds up"
    );
    assert_eq!(None.union(Slope(SW)), Slope(SW));
    assert_eq!(
        Full.carve(Slope(NW)),
        Slope(SE),
        "leaves the opposite slope"
    );
    assert_eq!(
        Slope(NW).carve(Slope(NE)),
        None,
        "unrepresentable rounds down"
    );
    assert_eq!(Slope(NW).carve(Slope(SE)), Slope(NW));
    assert_eq!(Full.carve(Full), None);
}

#[test]
fn stamping_and_carving_keep_materials() {
    use Corner::*;
    let (brick, stone) = (Part::Wall(1), Part::Wall(2));
    let (planks, dirt) = (Part::Floor(1), Part::Floor(2));
    let slope = Tile::split(NW, brick, planks);
    // The stamped wall where the piece is wall; what was there before
    // elsewhere, void included. The piece's floor doesn't matter.
    assert_eq!(Tile::VOID.stamp(slope), Tile::split(NW, brick, Part::Void));
    assert_eq!(Tile::Whole(dirt).stamp(slope), Tile::split(NW, brick, dirt));
    let filled = Tile::split(SE, stone, dirt).stamp(slope);
    assert_eq!(filled, Tile::split(NW, brick, stone));
    assert_eq!(filled.wall(), Solid::Full);
    // Halves no tile can have round up to the stamped wall.
    assert_eq!(
        Tile::split(NE, stone, dirt).stamp(slope),
        Tile::Whole(brick)
    );
    assert_eq!(Tile::Whole(dirt).stamp(Tile::VOID), Tile::Whole(dirt));
    assert_eq!(
        Tile::Stairs(Dir::Up, 1).stamp(Tile::VOID),
        Tile::Stairs(Dir::Up, 1)
    );

    assert_eq!(
        Tile::Whole(brick).carve(Solid::Slope(NW), planks),
        Tile::split(SE, brick, planks)
    );
    assert_eq!(
        Tile::split(NW, brick, Part::Void).carve(Solid::Slope(NW), planks),
        Tile::split(NW, planks, Part::Void)
    );
    assert_eq!(
        Tile::Whole(brick).carve(Solid::Full, Part::Void),
        Tile::VOID
    );
    assert_eq!(Tile::split(NE, dirt, dirt), Tile::Whole(dirt), "normalized");
}

#[test]
fn liquid_is_void_to_the_game() {
    let t = Layer::from_ascii(&["≈"]).tile(0, 0);
    assert_eq!(t, Tile::LIQUID);
    assert_eq!(t.void(), Solid::Full);
    assert_eq!(t.liquid_quarters(), 0b1111);
    assert_eq!(t.liquid_id(), Some(0));
    assert_eq!(Tile::VOID.liquid_quarters(), 0);
    assert_eq!(t.wall(), Solid::None, "no block to sight");
    let half = Tile::split(Corner::NW, Part::Floor(0), Part::Liquid(1));
    assert_eq!(half.void(), Solid::Slope(Corner::SE));
    assert_eq!(half.liquid_quarters(), half.void_quarters());
    assert_eq!(half.liquid_id(), Some(1));
}

#[test]
fn points_over_void_by_quarter() {
    let l = Layer::from_ascii(&["▛~.", "≈#."]);
    // Floor in the NW half, void in the SE: the diagonal decides.
    assert!(!l.is_void_at([2.0, 3.0]));
    assert!(l.is_void_at([8.0, 7.0]));
    assert!(l.is_void_at([15.0, 5.0]), "void");
    assert!(l.is_void_at([5.0, 15.0]), "liquid counts");
    assert!(!l.is_void_at([25.0, 5.0]), "floor");
    assert!(!l.is_void_at([15.0, 15.0]), "wall");
    assert!(!l.is_void_at([-1.0, 5.0]), "outside");
}

#[test]
fn tiles_have_wall_and_void_shapes() {
    use Corner::*;
    let t = Tile::split(NW, Part::Wall(0), Part::Void);
    assert_eq!((t.wall(), t.void()), (Solid::Slope(NW), Solid::Slope(SE)));
    let t = Tile::split(SW, Part::Floor(0), Part::Void);
    assert_eq!((t.wall(), t.void()), (Solid::None, Solid::Slope(NE)));
    assert_eq!(Tile::Stairs(Dir::Up, 0).void(), Solid::None);
    let l = Layer::from_ascii(&["◤▟◿"]);
    assert_eq!(l.solid(0, 0), Solid::Slope(NW));
    assert!(l.is_open(1, 0), "floor and void, no wall");
    assert_eq!(l.solid(2, 0), Solid::Slope(SE));
}

#[test]
fn stamping_is_order_independent() {
    let all = [
        Solid::None,
        Solid::Full,
        Solid::Slope(Corner::NW),
        Solid::Slope(Corner::NE),
        Solid::Slope(Corner::SE),
        Solid::Slope(Corner::SW),
    ];
    for a in all {
        for b in all {
            for c in all {
                assert_eq!(
                    a.union(b).union(c),
                    c.union(a).union(b),
                    "{a:?} {b:?} {c:?}"
                );
                assert_eq!(a.union(b.union(c)), b.union(c).union(a));
            }
        }
    }
}

#[test]
fn boxes_collide_with_slope_solid_half_only() {
    let level = Layer::from_ascii(&["...", ".◸.", "..."]);
    // In the empty half, clear of the face.
    assert!(!level.collides([16.0, 16.0], [3.0, 3.0]));
    // Corner (16,14) pokes past the face u + v = 1, i.e. x + y = 30.
    assert!(level.collides([16.0, 13.0], [3.0, 3.0]));
    // Touching the face at a single point doesn't count.
    assert!(!level.collides([15.0, 15.0], [3.0, 3.0]));
}
