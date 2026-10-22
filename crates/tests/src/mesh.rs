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

use goblin_nightclub_core::draw::{Canvas, Vertex};
use goblin_nightclub_core::geom::{EVERYWHERE, Rect};
use goblin_nightclub_core::layer::TILE;
use goblin_nightclub_core::mesh::*;
use goblin_nightclub_core::tileset;

fn area(vertices: &[Vertex]) -> f32 {
    vertices
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
        })
        .sum()
}

#[test]
fn runs_of_whole_tiles_merge_within_chunks() {
    let tileset = tileset::testing::tileset();
    let cols = CHUNK + 4;
    // Row 0 whole, row 1 half tiles (quarters N and E), row 2 empty.
    let quarters = |_: usize, y: usize| [0b1111, 0b0011, 0][y];
    let mesh = Mesh::quarters(cols, 3, Canvas::new(&tileset), quarters);
    // Row 0 is a rectangle in each of the two chunks it crosses; each
    // half tile is a triangle.
    assert_eq!(mesh.vertices.len(), 2 * 6 + cols * 3);
    let whole = cols as f32 * TILE * TILE;
    assert!((area(&mesh.vertices) - 1.5 * whole).abs() < 1e-2);
}

#[test]
fn ranges_cover_the_chunks_in_view() {
    let tileset = tileset::testing::tileset();
    let (cols, rows) = (3 * CHUNK, 2 * CHUNK);
    let mesh = Mesh::quarters(cols, rows, Canvas::new(&tileset), |x, y| {
        if (x + y) % 2 == 0 { 0b0001 } else { 0 }
    });
    let size = CHUNK as f32 * TILE;
    let count = |r: Rect| -> u32 { mesh.ranges(r).map(|r| r.end - r.start).sum() };
    let per_chunk = (CHUNK * CHUNK / 2 * 3) as u32;
    // Inside the middle chunk of the top row, more than half a tile
    // from its sides.
    assert_eq!(
        count([[size + 6.0, 6.0], [2.0 * size - 6.0, 10.0]]),
        per_chunk
    );
    // Straddling the four chunks around a corner: two ranges.
    let corner = [[size - 1.0, size - 1.0], [size + 1.0, size + 1.0]];
    assert_eq!(mesh.ranges(corner).count(), 2);
    assert_eq!(count(corner), 4 * per_chunk);
    // Everything, and nothing past the level.
    assert_eq!(count(EVERYWHERE), mesh.all().end);
    assert_eq!(count([[-50.0, -50.0], [-6.0, -6.0]]), 0);
    assert_eq!(count([[1e4, 0.0], [2e4, 10.0]]), 0);
}
