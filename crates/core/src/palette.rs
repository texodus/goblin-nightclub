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

//! The 16-color EGA palette as linear-ish RGBA floats (sRGB values / 255).

pub type Color = [f32; 4];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

pub const BLACK: Color = rgb(0x00, 0x00, 0x00);
pub const BLUE: Color = rgb(0x00, 0x00, 0xAA);
pub const GREEN: Color = rgb(0x00, 0xAA, 0x00);
pub const CYAN: Color = rgb(0x00, 0xAA, 0xAA);
pub const RED: Color = rgb(0xAA, 0x00, 0x00);
pub const MAGENTA: Color = rgb(0xAA, 0x00, 0xAA);
pub const BROWN: Color = rgb(0xAA, 0x55, 0x00);
pub const LIGHT_GRAY: Color = rgb(0xAA, 0xAA, 0xAA);
pub const DARK_GRAY: Color = rgb(0x55, 0x55, 0x55);
pub const LIGHT_BLUE: Color = rgb(0x55, 0x55, 0xFF);
pub const LIGHT_GREEN: Color = rgb(0x55, 0xFF, 0x55);
pub const LIGHT_CYAN: Color = rgb(0x55, 0xFF, 0xFF);
pub const LIGHT_RED: Color = rgb(0xFF, 0x55, 0x55);
pub const LIGHT_MAGENTA: Color = rgb(0xFF, 0x55, 0xFF);
pub const YELLOW: Color = rgb(0xFF, 0xFF, 0x55);
pub const WHITE: Color = rgb(0xFF, 0xFF, 0xFF);

/// Not one of the sixteen: nothing, blended over anything, changes it.
pub const TRANSPARENT: Color = [0.0; 4];

/// A channel, 0 to 1, as a byte.
fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// `c` as `#rrggbb` (its alpha dropped), as tile sheets name colors.
pub fn hex(c: Color) -> String {
    format!("#{:02x}{:02x}{:02x}", byte(c[0]), byte(c[1]), byte(c[2]))
}

/// `#rrggbbaa`, or `#rrggbb` when opaque.
pub fn hex_alpha(c: Color) -> String {
    if byte(c[3]) == 255 {
        hex(c)
    } else {
        format!("{}{:02x}", hex(c), byte(c[3]))
    }
}

/// A color from `#rrggbb` (or `#rgb`), opaque, or `#rrggbbaa`.
pub fn parse_hex(s: &str) -> Option<Color> {
    let digits = s.strip_prefix('#')?;
    let channel = |from: usize, len: usize| {
        let v = u8::from_str_radix(digits.get(from..from + len)?, 16).ok()?;
        Some(if len == 1 { v * 17 } else { v })
    };
    let [r, g, b, a] = match digits.len() {
        8 => [
            channel(0, 2)?,
            channel(2, 2)?,
            channel(4, 2)?,
            channel(6, 2)?,
        ],
        6 => [channel(0, 2)?, channel(2, 2)?, channel(4, 2)?, 255],
        3 => [channel(0, 1)?, channel(1, 1)?, channel(2, 1)?, 255],
        _ => return None,
    };
    let mut c = rgb(r, g, b);
    c[3] = a as f32 / 255.0;
    Some(c)
}
