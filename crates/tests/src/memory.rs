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

use goblin_nightclub_core::entity::Entity;
use goblin_nightclub_core::layer::Layer;
use goblin_nightclub_core::memory::*;
use goblin_nightclub_core::vision::Vision;

/// A wall in column 4 (x 40..50), open around its ends.
fn level() -> Layer {
    Layer::from_ascii(&[
        "..........", //
        "....#.....", //
        "....#.....", //
        "....#.....", //
        "..........", //
    ])
}

fn look(level: &Layer, from: [f32; 2]) -> Vision {
    let mut v = Vision::new(level, f32::INFINITY);
    v.update(level, from);
    v
}

const LEFT: [f32; 2] = [15.0, 25.0];
const RIGHT: [f32; 2] = [85.0, 25.0];

#[test]
fn marker_kept_while_spot_out_of_view() {
    let level = level();
    let mut memory = Memory::default();
    let npc = [Entity::npc(1, [22.0, 22.0])];

    // Seen: marker recorded, not stale.
    memory.update(&level, &look(&level, LEFT), &npc);
    assert!(memory.is_visible(1));
    assert_eq!(memory.stale().count(), 0);

    // Viewer goes behind the wall: stale marker at the last seen spot.
    memory.update(&level, &look(&level, RIGHT), &npc);
    assert!(!memory.is_visible(1));
    let (_, m) = memory.stale().next().expect("stale marker");
    assert_eq!(m.pos, [22.0, 22.0]);
}

#[test]
fn marker_cleared_when_spot_seen_empty() {
    let level = level();
    let mut memory = Memory::default();
    let mut npc = [Entity::npc(1, [22.0, 22.0])];
    memory.update(&level, &look(&level, LEFT), &npc);
    memory.update(&level, &look(&level, RIGHT), &npc);
    assert_eq!(memory.stale().count(), 1);

    // Meanwhile it moved behind the wall; back on the left, its old
    // spot is in plain view and empty.
    npc[0].pos = [72.0, 22.0];
    memory.update(&level, &look(&level, LEFT), &npc);
    assert!(!memory.is_visible(1));
    assert_eq!(memory.stale().count(), 0);
}
