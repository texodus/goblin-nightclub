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

use goblin_nightclub_core::layer::Part;
use goblin_nightclub_core::tileset::*;

#[test]
fn names_and_variants() {
    let t = testing::tileset();
    assert_eq!(t.walls[t.wall("brick").unwrap() as usize].name, "brick");
    assert_eq!(t.floors[t.floor("dirt").unwrap() as usize].name, "dirt");
    assert_eq!(t.liquid("water"), Some(0));
    assert_eq!(t.sprite("npc").unwrap().cell, [1, 3]);
    assert_eq!(t.floor("lava"), None);
    let cells = &t.floors[0].cells;
    assert_eq!(variant(cells, 3, 4), variant(cells, 3, 4));
    let picked: std::collections::HashSet<_> = (0..20).map(|x| variant(cells, x, 0)).collect();
    assert!(picked.len() > 1, "varies across tiles");
    assert_eq!(t.zoom(), 1.6);
}

#[test]
fn round_trips_as_json_in_order() {
    let t = testing::tileset();
    let json = serde_json::to_string(&t).unwrap();
    let back: Tileset = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
    // Keys keep the file's order, so ids are stable.
    assert!(json.find("\"plain\"").unwrap() < json.find("\"planks\"").unwrap());
    assert!(json.contains("\"light\":\"#ffff55\""));
    let shuffled = json.replacen("\"plain\":{\"cells\":[[0,1],[0,1],[0,1],[1,1]]},", "", 1);
    let back: Tileset = serde_json::from_str(&shuffled).unwrap();
    assert_eq!(back.floor("planks"), Some(0), "first in the file");
}

#[test]
fn edges_and_trims_round_trip_and_default() {
    let mut t = testing::tileset();
    t.floors[2].edge = Edge {
        priority: 3,
        width: 2.5,
        rim: Some([0.0, 0.0, 0.0, 0.4]),
    };
    t.walls[0].edge.width = 2.0;
    t.liquids[0].trim = Some(Trim {
        cells: vec![[9, 3], [10, 3]],
        density: 0.4,
        inset: 3.0,
    });
    let json = serde_json::to_string(&t).unwrap();
    // Close enough: the rim's alpha goes through a byte.
    let back: Tileset = serde_json::from_str(&json).unwrap();
    let rim = back.floors[2].edge.rim.unwrap();
    assert!((rim[3] - 0.4).abs() < 1.0 / 255.0);
    assert_eq!(
        (back.floors[2].edge.priority, back.floors[2].edge.width),
        (3, 2.5)
    );
    assert_eq!(back.walls[0].edge.width, 2.0);
    assert_eq!(back.liquids[0].trim, t.liquids[0].trim);
    assert_eq!(back.edge(Part::Liquid(0)), Edge::default());
    assert!(back.trim(Part::Floor(2)).is_none());
    // Left out when there's nothing to say, so older files stay as
    // they were.
    assert!(
        !serde_json::to_string(&testing::tileset())
            .unwrap()
            .contains("edge")
    );
    // Trims' cells count as the tileset's.
    assert!(back.cells().any(|c| c == [10, 3]));
    let bad = json.replacen("\"rim\":\"#00000066\"", "\"rim\":\"dark\"", 1);
    assert_ne!(bad, json);
    assert!(serde_json::from_str::<Tileset>(&bad).is_err());
}

#[test]
fn bad_tilesets_are_refused() {
    let t = testing::tileset();
    let json = serde_json::to_string(&t).unwrap();
    for (from, to) in [
        ("\"cell\":16", "\"cell\":0"),
        ("\"#ffff55\"", "\"yellow\""),
        ("\"cells\":[[2,2],[3,2]]", "\"cells\":[]"),
        ("\"radius\":5.0", "\"radius\":0.0"),
        ("\"#ffb060\"", "\"warm\""),
    ] {
        let bad = json.replacen(from, to, 1);
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Tileset>(&bad).is_err(), "{to}");
    }
    assert!(t.check_sheet([128, 80]).is_ok());
    assert!(t.check_sheet([128, 48]).is_err(), "sprites' row cut off");
    assert!(Sheet::new([2, 1], vec![0; 8]).is_ok());
    assert!(Sheet::new([2, 1], vec![0; 7]).is_err());
}
