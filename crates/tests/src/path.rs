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

use goblin_nightclub_core::layer::{Corner, Solid};
use goblin_nightclub_core::path::*;

const BOX: [f32; 2] = [8.0, 8.0];

/// The centers of a tile path, after its first tile.
fn centers(path: &[[i32; 2]]) -> Vec<[f32; 2]> {
    path.iter().skip(1).map(|t| center(*t)).collect()
}

/// Ground from a map: `.` open, `#` solid, `◸ ◹ ◺ ◿` solid in that
/// corner; outside, solid.
fn grid(rows: &'static [&'static str]) -> impl Fn([i32; 2]) -> Solid {
    move |[x, y]: [i32; 2]| {
        if y < 0 || (y as usize) >= rows.len() || x < 0 {
            return Solid::Full;
        }
        match rows[y as usize].chars().nth(x as usize) {
            Some('.') => Solid::None,
            Some('◸') => Solid::Slope(Corner::NW),
            Some('◹') => Solid::Slope(Corner::NE),
            Some('◺') => Solid::Slope(Corner::SW),
            Some('◿') => Solid::Slope(Corner::SE),
            _ => Solid::Full,
        }
    }
}

#[test]
fn finds_shortest_paths_without_cutting_corners() {
    let open = grid(&["......", "......", "......"]);
    let path = find(&open, [0, 0], [5, 2]).unwrap();
    assert_eq!(path.len(), 6, "two diagonals and three straight: {path:?}");

    // The diagonal from (1,1) to (2,0) would clip the corner of (1,0).
    let wall = grid(&[".#...", "...#.", "....."]);
    let path = find(&wall, [0, 0], [4, 0]).unwrap();
    for w in path.windows(2) {
        let [a, b] = [w[0], w[1]];
        if a[0] != b[0] && a[1] != b[1] {
            assert!(
                wall([b[0], a[1]]) == Solid::None && wall([a[0], b[1]]) == Solid::None,
                "cut a corner: {path:?}"
            );
        }
    }
    assert_eq!(*path.last().unwrap(), [4, 0]);
}

/// Stepping from (0,1) up to (1,0) clips the bottom-right corner of
/// (0,0): fine past a slope solid in its top-left, not past one whose
/// face runs through that corner, nor a wall.
#[test]
fn diagonal_steps_pass_open_slope_corners() {
    let past = grid(&["◸.", ".."]);
    assert_eq!(find(&past, [0, 1], [1, 0]).unwrap().len(), 2, "diagonal");
    for blocked in [
        grid(&["◹.", ".."]),
        grid(&["◿.", ".."]),
        grid(&["#.", ".."]),
    ] {
        assert_eq!(find(&blocked, [0, 1], [1, 0]).unwrap().len(), 3, "around");
    }
    // Half tiles are never stood on, and never the goal.
    assert_eq!(find(&past, [1, 1], [0, 0]), None);
    assert_eq!(
        nearest_reachable(&past, [1, 1], center([0, 0])),
        Some([1, 0])
    );
    // A point over the slope's open half may still fit the box.
    assert!(fits(&past, [9.0, 9.0], BOX), "its far corner");
    assert!(!fits(&past, [5.0, 5.0], BOX), "its middle");
    assert!(fits(&past, center([1, 1]), BOX));
}

#[test]
fn unreachable_goals() {
    let split = grid(&["..#..", "..#..", "..#.."]);
    assert_eq!(find(&split, [0, 0], [4, 0]), None);
    assert_eq!(
        nearest_reachable(&split, [0, 0], center([4, 0])),
        Some([1, 0])
    );
    // By the point, not its tile: two points in the wall at (2,0) and
    // (2,1), each nearer a different tile's center.
    assert_eq!(nearest_reachable(&split, [0, 0], [22.0, 8.0]), Some([1, 0]));
    assert_eq!(
        nearest_reachable(&split, [0, 0], [22.0, 12.0]),
        Some([1, 1])
    );
    assert_eq!(find(&split, [0, 0], [2, 0]), None, "goal itself blocked");
}

#[test]
fn sweeps_follow_the_shapes() {
    // Up the diagonal past (0,0)'s open corner, or not.
    let past = grid(&["◸..", "...", "..."]);
    assert!(clear(&past, center([0, 1]), center([1, 0]), BOX));
    let face = grid(&["◹..", "...", "..."]);
    assert!(!clear(&face, center([0, 1]), center([1, 0]), BOX));
    // Along a 45-degree shore (faces running with it, solid above and
    // to the right) the box rides over the open halves.
    let shore = grid(&["◹◹◹◹", ".◹◹◹", "..◹◹", "...◹", "...."]);
    assert!(clear(&shore, center([0, 1]), center([3, 4]), BOX));
    assert!(!clear(&shore, center([0, 1]), center([2, 1]), BOX));
    // Approaching a wall stops a pixel short of it, not at the goal.
    let wall = grid(&["...#"]);
    let stop = approach(&wall, center([1, 0]), center([3, 0]), BOX).unwrap();
    assert!((stop[0] - 26.0).abs() < 1e-3, "{stop:?}");
    assert_eq!(
        approach(&wall, center([2, 0]), center([3, 0]), BOX),
        Some([26.0, 5.0]),
        "a pixel, until it touches"
    );
    assert_eq!(approach(&wall, [26.0, 5.0], center([3, 0]), BOX), None);
    assert_eq!(
        approach(&wall, center([0, 0]), center([1, 0]), BOX),
        Some(center([1, 0]))
    );
}

#[test]
fn smoothing_cuts_straight_where_the_box_fits() {
    let open = grid(&["......", "......", "......"]);
    let path = find(&open, [0, 0], [5, 2]).unwrap();
    let points = smooth(&open, center([0, 0]), &centers(&path), BOX);
    assert_eq!(points, vec![center([5, 2])], "one straight line");

    // Around a wall: the straight line through it isn't taken.
    let wall = grid(&["......", "..##..", "......"]);
    let path = find(&wall, [0, 1], [5, 1]).unwrap();
    let points = smooth(&wall, center([0, 1]), &centers(&path), BOX);
    assert!(points.len() >= 2, "{points:?}");
    assert_eq!(*points.last().unwrap(), center([5, 1]));
    // A final point off any center is reached straight, not via the
    // last tile's center.
    let mut to_point = centers(&path);
    to_point.push([56.0, 12.0]);
    let points = smooth(&wall, center([0, 1]), &to_point, BOX);
    assert_eq!(*points.last().unwrap(), [56.0, 12.0]);
    assert!(!points.contains(&center([5, 1])), "{points:?}");

    // Along a shore: one leg, over the slopes' open halves.
    let shore = grid(&["◹◹◹◹", ".◹◹◹", "..◹◹", "...◹", "...."]);
    let path = find(&shore, [0, 1], [3, 4]).unwrap();
    assert_eq!(path.len(), 4, "diagonal all the way: {path:?}");
    let points = smooth(&shore, center([0, 1]), &centers(&path), BOX);
    assert_eq!(points, vec![center([3, 4])]);
}
