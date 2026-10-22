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

//! Props: things standing on tiles, like trees and rocks, at most one a
//! tile. What each kind looks like and does is the tileset's (see
//! [`crate::tileset::PropKind`]); which stands where is kept with the
//! tiles, in chunks (see [`crate::chunk`]).

/// A kind of prop: an index into the tileset's props.
pub type PropId = u8;

/// A prop standing on a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prop {
    /// Its tile: column and row.
    pub at: [usize; 2],
    pub kind: PropId,
}

use crate::draw::Canvas;
use crate::layer::{Layer, TILE};
use crate::palette;
use crate::tileset::{self, Tileset};

/// Draw the prop on tile `(x, y)` of `layer`, if there's one: its art (a
/// variant picked per tile, as tiles' are, by where it is in the whole
/// level), a tile a cell, placed about its tile as its kind says, as high
/// as its kind says.
pub fn draw_prop(layer: &Layer, tileset: &Tileset, canvas: &mut Canvas, x: usize, y: usize) {
    let Some(kind) = layer.prop(x as i32, y as i32) else {
        return;
    };
    let kind = &tileset.props[kind as usize];
    let [ax, ay] = layer.absolute(x, y);
    let [cx, cy] = tileset::variant(&kind.cells, ax, ay);
    let corner = [
        (x as f32 - kind.anchor[0]) * TILE,
        (y as f32 - kind.anchor[1]) * TILE,
    ];
    let start = canvas.vertices.len();
    for j in 0..kind.size[1] {
        for i in 0..kind.size[0] {
            let at = [
                corner[0] + f32::from(i) * TILE,
                corner[1] + f32::from(j) * TILE,
            ];
            let [x1, y1] = [at[0] + TILE, at[1] + TILE];
            let quad = [at, [x1, at[1]], [x1, y1], [at[0], y1]];
            canvas.textured(&quad, at, [cx + i, cy + j], palette::WHITE);
        }
    }
    for v in &mut canvas.vertices[start..] {
        v.z = kind.height;
    }
}
