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

//! Layers made for tests, and invariant checks, here and in other crates.

use super::*;
use crate::rng::Rng;

impl Layer {
    /// Build a layer from rows of `.` (floor), `#` (wall), `◸ ◹ ◺ ◿`
    /// (wall in that corner, floor in the rest), `◤ ◥ ◣ ◢` (wall in that
    /// corner over void), `▛ ▜ ▙ ▟` (floor in that corner, void in the
    /// rest), `~` (void), `≈` (liquid), `◜ ◝ ◟ ◞` (liquid in that corner
    /// over floor) and `<` `>` (stairs up and down), for tests. Materials
    /// are the first of each kind in the tileset.
    pub fn from_ascii(rows: &[&str]) -> Self {
        use Corner::*;
        let cols = rows[0].chars().count();
        let (wall, floor, liquid) = (Part::Wall(0), Part::Floor(0), Part::Liquid(0));
        let tiles = rows
            .iter()
            .flat_map(|r| r.chars())
            .map(|c| match c {
                '.' => Tile::FLOOR,
                '~' => Tile::VOID,
                '≈' => Tile::LIQUID,
                '<' => Tile::Stairs(Dir::Up, 0),
                '>' => Tile::Stairs(Dir::Down, 0),
                '#' => Tile::WALL,
                '◸' => Tile::slope(NW),
                '◹' => Tile::slope(NE),
                '◺' => Tile::slope(SW),
                '◿' => Tile::slope(SE),
                '◤' => Tile::split(NW, wall, Part::Void),
                '◥' => Tile::split(NE, wall, Part::Void),
                '◣' => Tile::split(SW, wall, Part::Void),
                '◢' => Tile::split(SE, wall, Part::Void),
                '▛' => Tile::split(NW, floor, Part::Void),
                '▜' => Tile::split(NE, floor, Part::Void),
                '▙' => Tile::split(SW, floor, Part::Void),
                '▟' => Tile::split(SE, floor, Part::Void),
                '◜' => Tile::split(NW, liquid, floor),
                '◝' => Tile::split(NE, liquid, floor),
                '◟' => Tile::split(SW, liquid, floor),
                '◞' => Tile::split(SE, liquid, floor),
                _ => panic!("unknown tile {c:?}"),
            })
            .collect();
        Self::from_tiles(cols, rows.len(), tiles)
    }

    /// Random tiles with plenty of slopes in every arrangement, for
    /// stress-testing queries.
    pub fn soup(seed: u32, cols: usize, rows: usize) -> Self {
        let mut rng = Rng::new(seed);
        let tiles = (0..cols * rows)
            .map(|_| match rng.unit() {
                r if r < 0.10 => Solid::Full,
                r if r < 0.30 => Solid::Slope(Corner::CLOCKWISE[rng.range(0, 3)]),
                _ => Solid::None,
            })
            .collect();
        Self::from_solids(cols, rows, tiles)
    }
}

/// Each outline edge is in exactly one run, and each loop is closed:
/// every run starts where the previous one ended.
pub fn check_outlines(level: &Layer) {
    let mut seen = vec![0; level.outline_edges().len()];
    for o in level.outlines() {
        assert_eq!(o.points.len(), o.runs.len());
        for (k, run) in o.runs.iter().enumerate() {
            let edges: Vec<_> = run.iter().map(|&i| level.outline_edges()[i]).collect();
            assert_eq!(edges[0].a, o.points[k]);
            assert_eq!(edges.last().unwrap().b, o.points[(k + 1) % o.points.len()]);
            for w in edges.windows(2) {
                assert_eq!(w[0].b, w[1].a);
            }
            for &i in run {
                seen[i] += 1;
            }
        }
    }
    assert!(seen.iter().all(|&n| n == 1), "{seen:?}");
}
