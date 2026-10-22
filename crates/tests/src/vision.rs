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

use goblin_nightclub_core::geom::distance;
use goblin_nightclub_core::layer::{Layer, TILE};
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::vision::testing::*;
use goblin_nightclub_core::vision::*;

/// Tile soups are dense with slopes in every arrangement: the hardest
/// case. (Generated levels are checked the same way in
/// `generated_levels`.)
const SOUPS: u32 = 16;

/// Tile soups, each with a suggested origin (which may be solid).
fn levels() -> impl Iterator<Item = (Layer, [f32; 2])> {
    let spawn = [165.0, 105.0];
    (0..SOUPS).map(move |seed| (Layer::soup(seed, 32, 20), spawn))
}

#[test]
fn los_is_symmetric() {
    let mut rng = Rng::new(7);
    for (level, _) in levels() {
        for _ in 0..200 {
            let (a, b) = (rng.open_point(&level), rng.open_point(&level));
            assert_eq!(level.los(a, b), level.los(b, a), "{a:?} <-> {b:?}");
        }
    }
}

#[test]
fn los_blocked_by_wall_and_diagonal_pinch() {
    let level = Layer::from_ascii(&[
        "......", //
        "..#...", //
        "...#..", //
        "......", //
    ]);
    // Straight through the wall tile at (2, 1).
    assert!(!level.los([5.0, 15.0], [45.0, 15.0]));
    // Through open tiles except exactly at the pinch vertex (30, 20)
    // between (2,1) and (3,2).
    assert!(!level.los([15.0, 35.0], [45.0, 5.0]));
    // Grazing a single corner, (20, 20) of tile (1,1), is allowed.
    let single = Layer::from_ascii(&["....", ".#..", "....", "...."]);
    assert!(single.los([5.0, 35.0], [35.0, 5.0]));
    // A point on an open face is visible.
    assert!(level.los([25.0, 5.0], [25.0, 10.0]));
}

#[test]
fn polygon_matches_los() {
    let mut rng = Rng::new(11);
    for (level, spawn) in levels() {
        // Tile-centered and grid-vertex origins line up with many
        // corners (and slope faces) at once; random ones cover the
        // general case.
        let mut origins = vec![spawn, [160.0, 100.0]];
        for _ in 0..3 {
            let [x, y] = [rng.range(0, level.cols - 1), rng.range(0, level.rows - 1)];
            origins.push([x as f32 * TILE + 5.0, y as f32 * TILE + 5.0]);
            origins.push([x as f32 * TILE, y as f32 * TILE]);
        }
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

/// Just `circles` around.
fn circles(circles: &[Circle]) -> Around<'_> {
    Around {
        circles,
        ..Default::default()
    }
}

/// A circle around tile `t`'s center, radius `r`.
fn circle_at(t: [usize; 2], r: f32) -> Circle {
    let [x, y] = t.map(|v| v as f32 * TILE);
    Circle {
        center: [x + TILE / 2.0, y + TILE / 2.0],
        radius: r,
    }
}

#[test]
fn circles_cast_shadows_and_light_up_seen() {
    let rows: Vec<String> = (0..20).map(|_| ".".repeat(30)).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let level = Layer::from_ascii(&rows);
    let rock = circle_at([15, 10], 4.0);
    let origin = [50.0, 105.0];
    let mut v = Vision::new(&level, f32::INFINITY);
    v.update_through(&level, &level, None, None, origin, circles(&[rock]));
    // Behind it, hidden; beside it, and its own tile, seen.
    let (behind, beside) = ([250.0, 105.0], [250.0, 140.0]);
    assert!(!v.can_see(&level, behind));
    assert!(v.can_see(&level, beside));
    assert!(v.can_see(&level, rock.center));
    assert!(!inside(v.polygon(), behind));
    assert!(inside(v.polygon(), beside));
    // Its shadow widens with distance: just past it, a little off the
    // line through it is seen; far behind, hidden.
    assert!(v.can_see(&level, [170.0, 112.0]));
    assert!(!v.can_see(&level, [290.0, 112.0]));
    // Seen (to be shown whole), and explored.
    assert_eq!(v.props_seen(), &[[15, 10]]);
    assert!(v.is_explored(15, 10));
    // Without it, nothing's hidden.
    v.update(&level, origin);
    assert!(v.can_see(&level, behind));

    // Behind a wall it isn't seen.
    let mut rows: Vec<String> = rows.iter().map(|r| r.to_string()).collect();
    rows[10].replace_range(10..11, "#");
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let walled = Layer::from_ascii(&rows);
    let mut v = Vision::new(&walled, f32::INFINITY);
    v.update_through(
        &walled,
        &walled,
        None,
        None,
        origin,
        circles(&[circle_at([12, 10], 2.0)]),
    );
    assert!(v.props_seen().is_empty());
}

/// Just `lamps` around.
fn lamps(lamps: &[Lamp]) -> Around<'_> {
    Around {
        lamps,
        ..Default::default()
    }
}

#[test]
fn lamps_show_what_they_light_beyond_the_radius() {
    let rows: Vec<String> = (0..20).map(|_| ".".repeat(40)).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let level = Layer::from_ascii(&rows);
    let origin = [55.0, 105.0];
    let lamp = [Lamp::new([105.0, 105.0], 40.0, [1.0; 4])];
    let mut v = Vision::new(&level, 30.0);
    v.update_through(&level, &level, None, None, origin, self::lamps(&lamp));
    assert_eq!(v.lights().len(), 1, "it lights what's seen");
    assert_eq!(v.reach(), 50.0 + 40.0);
    // Past the radius, lit: seen. Past its light too: not.
    assert!(v.can_see(&level, [140.0, 105.0]));
    assert!(!v.can_see(&level, [150.0, 105.0]));
    assert!(!v.can_see(&level, [55.0, 150.0]), "unlit, past the radius");
    assert!(v.is_explored(13, 10));
    assert!(!v.is_explored(16, 10));
    // The polygon goes as far.
    assert!(inside(v.polygon(), [140.0, 105.0]));
    // Further off, its light no longer reaches into the radius.
    v.update_through(
        &level,
        &level,
        None,
        None,
        [15.0, 105.0],
        self::lamps(&lamp),
    );
    assert!(v.lights().is_empty());
    assert_eq!(v.reach(), 30.0);
    assert!(!v.can_see(&level, [100.0, 105.0]));
}

#[test]
fn lamps_count_only_if_they_light_whats_seen() {
    // A corridor, and a room off it through a door, its lamp lighting
    // a little of the corridor below the door.
    let level = Layer::from_ascii(&[
        "##############################",
        "#############.....############",
        "#############.....############",
        "###############.##############",
        "#............................#",
        "##############################",
    ]);
    let lamp = [Lamp::new([155.0, 15.0], 80.0, [1.0; 4])];
    let lit = [159.0, 45.0];
    assert!(lamp[0].lit(&level, &[]).lights(&level, lit));
    let origin = [25.0, 45.0];
    // Its light and the viewer's sight overlap, but walls part them:
    // what it lights in view is all beyond the radius.
    let mut v = Vision::new(&level, 60.0);
    v.update_through(&level, &level, None, None, origin, self::lamps(&lamp));
    assert!(v.lights().is_empty());
    assert!(!v.can_see(&level, lit));
    assert!(!v.is_explored(15, 4));
    // Seeing a little further, the near edge of what it lights is in
    // the radius, and the rest of it beyond seen too.
    let mut v = Vision::new(&level, 128.0);
    v.update_through(&level, &level, None, None, origin, self::lamps(&lamp));
    assert_eq!(v.lights().len(), 1);
    assert!(distance(origin, lit) > 128.0);
    assert!(v.can_see(&level, lit));
    // Not what's past its light, nor the room, out of line of sight.
    assert!(!v.can_see(&level, [175.0, 45.0]));
    assert!(!v.can_see(&level, [155.0, 15.0]));
    // The wall it lights below the door is seen, as far as it's lit.
    assert!(band_covers(&v, [160.0, 52.0]));
    assert!(!band_covers(&v, [175.0, 52.0]));
}

#[test]
fn polygon_matches_los_around_circles() {
    let mut rng = Rng::new(17);
    for (level, spawn) in levels() {
        // Circles of every size a prop may have, on open tiles.
        let circles: Vec<Circle> = (0..60)
            .filter_map(|_| {
                let t = [rng.range(0, level.cols - 1), rng.range(0, level.rows - 1)];
                let open = level.solid(t[0] as i32, t[1] as i32)
                    == goblin_nightclub_core::layer::Solid::None;
                open.then(|| circle_at(t, 1.0 + rng.unit() * 3.9))
            })
            .collect();
        let outside = |o: [f32; 2]| {
            circles
                .iter()
                .all(|c| distance(o, c.center) > c.radius + 0.5)
        };
        let mut origins = vec![spawn];
        origins.extend((0..5).map(|_| rng.open_point(&level)));
        for origin in origins
            .into_iter()
            .filter(|o| is_open_point(&level, *o) && outside(*o))
        {
            for radius in [f32::INFINITY, 80.0] {
                let mut vision = Vision::new(&level, radius);
                vision.update_through(&level, &level, None, None, origin, self::circles(&circles));
                check_polygon(&level, &vision, &mut rng);
            }
        }
    }
}

#[test]
fn radius_limits_sight() {
    let level = Layer::from_ascii(&["..........", "..........", ".........."]);
    let origin = [5.0, 15.0];
    let mut near = Vision::new(&level, 30.0);
    near.update(&level, origin);
    assert!(near.can_see(&level, [34.0, 15.0]));
    assert!(!near.can_see(&level, [36.0, 15.0]));
    for [x, y] in near.polygon() {
        assert!((x - origin[0]).hypot(y - origin[1]) <= 30.0 + 1e-3);
    }

    let mut far = Vision::new(&level, f32::INFINITY);
    far.update(&level, origin);
    assert!(far.can_see(&level, [95.0, 25.0]));
}

#[test]
fn band_is_one_tile_behind_visible_faces() {
    // A 3x3 block; from straight left only its left face (30px) shows.
    let level = Layer::from_ascii(&[".....", ".###.", ".###.", ".###.", "....."]);
    let mut v = Vision::new(&level, f32::INFINITY);
    v.update(&level, [5.0, 25.0]);
    assert!((band_area(&v) - 300.0).abs() < 1e-2, "{}", band_area(&v));
    assert!(band_covers(&v, [15.0, 25.0]) && !band_covers(&v, [25.0, 25.0]));

    // Out of range: nothing.
    let mut short = Vision::new(&level, 3.0);
    short.update(&level, [5.0, 25.0]);
    assert!(short.wall_band().is_empty());
}

#[test]
fn band_fills_inside_corners() {
    // Walls along the top and left meet at an inside corner at (10,10).
    let level = Layer::from_ascii(&["#####", "#....", "#...."]);
    let mut v = Vision::new(&level, f32::INFINITY);
    v.update(&level, [30.0, 20.0]);
    // Top face 40px + left face 20px, a tile deep, plus the corner tile.
    assert!((band_area(&v) - 700.0).abs() < 1e-2, "{}", band_area(&v));
    assert!(band_covers(&v, [5.0, 5.0]), "corner tile lit");
}

#[test]
fn block_and_doorless_room_light_the_same() {
    let block = Layer::from_ascii(&[
        ".......", ".#####.", ".#####.", ".#####.", ".#####.", ".......",
    ]);
    let room = Layer::from_ascii(&[
        ".......", ".#####.", ".#...#.", ".#...#.", ".#####.", ".......",
    ]);
    // The room's inner corners add rays, so the band may be cut into
    // different pieces; compare what it covers.
    for origin in [[5.0, 5.0], [35.0, 55.0], [65.0, 30.0]] {
        let view = |level: &Layer| {
            let mut v = Vision::new(level, f32::INFINITY);
            v.update(level, origin);
            v
        };
        let (b, r) = (view(&block), view(&room));
        assert!(
            (band_area(&b) - band_area(&r)).abs() < 1e-2,
            "from {origin:?}"
        );
        for y in 0..60 {
            for x in 0..70 {
                let p = [x as f32 + 0.5, y as f32 + 0.5];
                assert_eq!(
                    band_covers(&b, p),
                    band_covers(&r, p),
                    "from {origin:?} at {p:?}"
                );
            }
        }
    }
}

#[test]
fn band_never_covers_open_floor() {
    let mut rng = Rng::new(3);
    for (level, spawn) in levels().take(50) {
        if !is_open_point(&level, spawn) {
            continue;
        }
        let mut v = Vision::new(&level, 96.0);
        v.update(&level, spawn);
        for _ in 0..300 {
            let p = rng.open_point(&level);
            assert!(!band_covers(&v, p) || !is_open_point(&level, p), "{p:?}");
        }
    }
}

#[test]
fn slope_faces_light_their_triangle() {
    let level = Layer::from_ascii(&["....#....", "..◸.#.◿..", "....#...."]);
    let mut vision = Vision::new(&level, f32::INFINITY);
    vision.update(&level, [35.0, 25.0]);
    // Inside the first slope, just behind its face; the second slope is
    // behind the wall.
    assert!(band_covers(&vision, [22.0, 12.0]));
    assert!(!band_covers(&vision, [68.0, 18.0]));
}

#[test]
fn slopes_explored_by_face_side_or_open_half() {
    // Slopes at (2,1) and (6,1); a wall at column 4 hides the second
    // from a viewer on the left.
    let level = Layer::from_ascii(&[
        "....#....", //
        "..◸.#.◿..", //
        "....#....", //
    ]);
    let mut vision = Vision::new(&level, f32::INFINITY);
    // From the lower right of the first slope: its face and open half.
    vision.update(&level, [35.0, 25.0]);
    assert!(vision.is_explored(2, 1));
    assert!(!vision.is_explored(6, 1), "behind the wall");

    // From straight left, only its left side is in view.
    let mut side = Vision::new(&level, f32::INFINITY);
    side.update(&level, [5.0, 15.0]);
    assert!(side.is_explored(2, 1), "left side seen");

    // From above left of the second slope (right of the wall): only
    // its diagonal face and open half face this way.
    let mut face = Vision::new(&level, f32::INFINITY);
    face.update(&level, [55.0, 5.0]);
    assert!(face.is_explored(6, 1));
}

#[test]
fn inner_faces_of_a_room_stay_unseen_from_outside() {
    // A doorless room with 1-tile walls, viewed from all around it.
    let level = Layer::from_ascii(&[
        ".......", //
        ".#####.", //
        ".#...#.", //
        ".#...#.", //
        ".#####.", //
        ".......", //
    ]);
    let inner: Vec<usize> = level
        .outline_edges()
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            (20.0..=50.0).contains(&e.a[0])
                && (20.0..=40.0).contains(&e.a[1])
                && (20.0..=50.0).contains(&e.b[0])
                && (20.0..=40.0).contains(&e.b[1])
        })
        .map(|(i, _)| i)
        .collect();
    assert_eq!(inner.len(), 10, "the hole's outline");
    let mut vision = Vision::new(&level, f32::INFINITY);
    for origin in [
        [5.0, 5.0],
        [65.0, 5.0],
        [35.0, 55.0],
        [5.0, 35.0],
        [65.0, 35.0],
    ] {
        vision.update(&level, origin);
        assert!(inner.iter().all(|&i| !vision.edge_visible()[i]));
    }
    assert!(inner.iter().all(|&i| !vision.edge_seen()[i]));
    assert!(vision.edge_seen().iter().any(|s| *s), "outer faces seen");
}

#[test]
fn wall_tops_seen_from_above_light_whole() {
    // Below: a block, then a longer one behind it.
    let below = Layer::from_ascii(&["......", ".#.##.", "......"]);
    let look = |deck: &Layer| {
        let mut v = Vision::new(&below, f32::INFINITY);
        let sight = deck_sight(&below, deck);
        let shaft = vec![true; 18];
        v.update_through(
            &sight,
            &below,
            Some(&shaft),
            Some(deck),
            [5.0, 15.0],
            Around::default(),
        );
        band_area(&v)
    };
    // Nothing above: all three wall tops, though the far block's faces
    // are hidden behind the near one.
    let open = Layer::from_ascii(&["~~~~~~", "~~~~~~", "~~~~~~"]);
    assert!((look(&open) - 300.0).abs() < 1e-2, "{}", look(&open));
    // A wall on the layer above, between the viewer and the far block,
    // hides its top.
    let walled = Layer::from_ascii(&["~~~~~~", "~~#~~~", "~~~~~~"]);
    assert!((look(&walled) - 100.0).abs() < 1e-2, "{}", look(&walled));
}

#[test]
fn walls_under_walls_light_beside_void_in_view() {
    // Below, a wall at each end; above, walls over both, void beside
    // the left one only, and floor where the viewer stands.
    let below = Layer::from_ascii(&["#....#"]);
    let deck = Layer::from_ascii(&["#~~~.#"]);
    let mut v = Vision::new(&below, f32::INFINITY);
    let sight = deck_sight(&below, &deck);
    let shaft = [false, true, true, true, false, false];
    v.update_through(
        &sight,
        &below,
        Some(&shaft),
        Some(&deck),
        [45.0, 5.0],
        Around::default(),
    );
    // The left wall, next to the hole, lights up; the right one, next
    // to floor, doesn't.
    assert!((band_area(&v) - 100.0).abs() < 1e-2, "{}", band_area(&v));
    assert!(v.is_explored(0, 0));
    assert!(!v.is_explored(5, 0));
    // Out of range of the hole's edge, it stays dark.
    let mut near = Vision::new(&below, 20.0);
    near.update_through(
        &sight,
        &below,
        Some(&shaft),
        Some(&deck),
        [45.0, 5.0],
        Around::default(),
    );
    assert_eq!(band_area(&near), 0.0);
}

/// The walls of both layers together, as a level would combine them.
fn deck_sight(below: &Layer, deck: &Layer) -> Layer {
    let tiles = (0..below.rows as i32)
        .flat_map(|y| (0..below.cols as i32).map(move |x| (x, y)))
        .map(|(x, y)| below.solid(x, y).union(deck.solid(x, y)))
        .collect();
    Layer::from_solids(below.cols, below.rows, tiles)
}

#[test]
fn exploration_accumulates() {
    let level = Layer::from_ascii(&[
        "..........", //
        "....#.....", //
        "....#.....", //
        "....#.....", //
        "..........", //
    ]);
    let mut vision = Vision::new(&level, f32::INFINITY);
    vision.update(&level, [15.0, 25.0]);
    assert!(vision.is_explored(1, 2));
    assert!(vision.is_explored(4, 2), "wall face seen");
    assert!(!vision.is_explored(7, 2), "behind the wall");

    // Walk around the wall; earlier tiles stay explored.
    vision.update(&level, [75.0, 25.0]);
    assert!(vision.is_explored(7, 2));
    assert!(vision.is_explored(1, 2));
}
