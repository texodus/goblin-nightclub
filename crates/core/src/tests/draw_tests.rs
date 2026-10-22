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

const RED: Color = [1.0, 0.0, 0.0, 1.0];
const GREEN: Color = [0.0, 1.0, 0.0, 1.0];
const BLUE: Color = [0.0, 0.0, 1.0, 1.0];

fn canvas() -> Canvas {
    Canvas::new(&crate::tileset::testing::tileset())
}

/// Color of the last triangle covering `p`, like the GPU would draw it.
fn color_at(layer: &Canvas, p: [f32; 2]) -> Option<Color> {
    layer.vertices.chunks(3).rev().find_map(|t| {
        let cross = |a: [f32; 2], b: [f32; 2]| {
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        };
        let s = [
            cross(t[0].pos, t[1].pos),
            cross(t[1].pos, t[2].pos),
            cross(t[2].pos, t[0].pos),
        ];
        let inside = s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0);
        inside.then_some(t[0].color)
    })
}

#[test]
fn bevels_light_top_left_and_shade_bottom_right() {
    let mut layer = canvas();
    // A 10x10 square with its bottom-right corner cut at 45 degrees.
    let shape = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 5.0],
        [5.0, 10.0],
        [0.0, 10.0],
    ];
    layer.beveled(&shape, RED, GREEN, BLUE);
    assert_eq!(color_at(&layer, [5.0, 0.5]), Some(GREEN), "top");
    assert_eq!(color_at(&layer, [0.5, 5.0]), Some(GREEN), "left");
    assert_eq!(color_at(&layer, [9.5, 2.0]), Some(BLUE), "right");
    assert_eq!(color_at(&layer, [2.0, 9.5]), Some(BLUE), "bottom");
    assert_eq!(
        color_at(&layer, [7.3, 7.3]),
        Some(BLUE),
        "diagonal facing down-right"
    );
    assert_eq!(color_at(&layer, [5.0, 5.0]), Some(RED), "fill");
    assert_eq!(color_at(&layer, [9.0, 9.0]), None, "cut off");
}

#[test]
fn bevel_strip_is_one_pixel_wide_on_diagonals() {
    let mut layer = canvas();
    // A "/" face facing up-left: lit.
    let shape = [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    layer.beveled(&shape, RED, GREEN, BLUE);
    let r = std::f32::consts::FRAC_1_SQRT_2;
    // Along the face's inward normal from its midpoint (5,5).
    assert_eq!(
        color_at(&layer, [5.0 + 0.9 * r, 5.0 + 0.9 * r]),
        Some(GREEN)
    );
    assert_eq!(color_at(&layer, [5.0 + 1.1 * r, 5.0 + 1.1 * r]), Some(RED));
}
