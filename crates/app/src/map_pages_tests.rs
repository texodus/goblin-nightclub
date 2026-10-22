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

#[test]
fn chunks_are_put_there_once_until_they_change() {
    let mut p = MapPages::new(2, 2);
    p.begin_frame();
    let slot = p.slot((0, 0, 0), 1).unwrap();
    assert_eq!(
        p.take_ops(),
        [MapOp::Upload {
            slot,
            key: (0, 0, 0)
        }]
    );
    p.begin_frame();
    assert_eq!(p.slot((0, 0, 0), 1), Some(slot));
    assert_eq!(p.take_ops(), [], "already there");
    assert_eq!(p.slot((0, 0, 0), 2), Some(slot), "changed: same slot");
    assert_eq!(
        p.take_ops(),
        [MapOp::Upload {
            slot,
            key: (0, 0, 0)
        }]
    );
}

#[test]
fn room_grows_to_the_most_then_the_least_recently_used_goes() {
    let mut p = MapPages::new(1, 2);
    p.begin_frame();
    let a = p.slot((0, 0, 0), 1).unwrap();
    let b = p.slot((0, 1, 0), 1).unwrap();
    assert_ne!(a, b);
    assert_eq!(
        p.take_ops(),
        [
            MapOp::Upload {
                slot: a,
                key: (0, 0, 0)
            },
            MapOp::Grow(2),
            MapOp::Upload {
                slot: b,
                key: (0, 1, 0)
            },
        ]
    );
    assert_eq!(p.slot((0, 2, 0), 1), None, "both in use this frame");
    p.begin_frame();
    assert_eq!(p.slot((0, 1, 0), 1), Some(b));
    assert_eq!(p.slot((0, 2, 0), 1), Some(a), "the one not used goes");
    assert_eq!(
        p.take_ops(),
        [MapOp::Upload {
            slot: a,
            key: (0, 2, 0)
        }]
    );
    p.begin_frame();
    assert_eq!(p.slot((0, 2, 0), 1), Some(a));
    assert_eq!(p.slot((0, 0, 0), 1), Some(b), "and comes back when wanted");
    assert_eq!(
        p.take_ops(),
        [MapOp::Upload {
            slot: b,
            key: (0, 0, 0)
        }]
    );
}
