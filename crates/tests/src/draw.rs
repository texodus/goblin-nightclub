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

use goblin_nightclub_core::draw::*;
use goblin_nightclub_core::layer::TILE;
use goblin_nightclub_core::palette::Color;

const RED: Color = [1.0, 0.0, 0.0, 1.0];

const ZOOM: f32 = 1.6;
const CELL: u32 = 16;

fn canvas() -> Canvas {
    Canvas::new(&goblin_nightclub_core::tileset::testing::tileset())
}

#[test]
fn sprites_center_on_whole_pixels_and_show_their_cell() {
    let mut canvas = canvas();
    canvas.sprite([20.0, 30.0], [1, 3], RED);
    let size = [TILE, TILE];
    let xs: Vec<f32> = canvas.vertices.iter().map(|v| v.pos[0]).collect();
    let ys: Vec<f32> = canvas.vertices.iter().map(|v| v.pos[1]).collect();
    let (x0, x1) = (
        xs.iter().copied().fold(f32::MAX, f32::min),
        xs.iter().copied().fold(f32::MIN, f32::max),
    );
    let (y0, y1) = (
        ys.iter().copied().fold(f32::MAX, f32::min),
        ys.iter().copied().fold(f32::MIN, f32::max),
    );
    assert!((x1 - x0 - size[0]).abs() < 1e-4 && (y1 - y0 - size[1]).abs() < 1e-4);
    assert!(
        ((x0 + x1) / 2.0 - 20.0).abs() <= 0.5 / ZOOM,
        "centered, to a pixel"
    );
    assert!(((y0 + y1) / 2.0 - 30.0).abs() <= 0.5 / ZOOM);
    for v in [x0, y0] {
        assert!(
            ((v * ZOOM) - (v * ZOOM).round()).abs() < 1e-3,
            "{v} on a pixel"
        );
    }
    // The square spans exactly the cell in the sheet.
    for v in &canvas.vertices {
        let [u, w] = v.uv;
        let c = CELL as f32;
        assert!(
            (c..=2.0 * c).contains(&u) && (3.0 * c..=4.0 * c).contains(&w),
            "{u} {w}"
        );
        assert_eq!(v.color, RED);
    }
    // Plain shapes show the white cell.
    let mut plain = Canvas::new(&goblin_nightclub_core::tileset::testing::tileset());
    plain.rect([0.0, 0.0], [5.0, 5.0], RED);
    assert!(
        plain
            .vertices
            .iter()
            .all(|v| v.uv[0] < CELL as f32 && v.uv[1] < CELL as f32)
    );
}
