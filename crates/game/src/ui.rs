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

//! What's drawn over the level: the way a walk has left, and the
//! interface (the pause button).

use super::*;

/// Spacing of the dots marking the way a walk has left, in pixels.
const ROUTE_DOT: f32 = 4.0;

/// The pause button, in interface pixels from the top-right corner.
const BUTTON_SIZE: [f32; 2] = [14.0, 12.0];

const BUTTON_MARGIN: f32 = 3.0;

impl Game {
    /// The pause button's top-left, in interface pixels.
    pub(super) fn button_pos(&self) -> [f32; 2] {
        let width = self.camera.view[0] / self.ui_scale();
        [width - BUTTON_SIZE[0] - BUTTON_MARGIN, BUTTON_MARGIN]
    }

    /// Is view pixel `p` on the pause button?
    pub(super) fn on_button(&self, p: [f32; 2]) -> bool {
        let p = p.map(|v| v / self.ui_scale());
        let b = self.button_pos();
        p[0] >= b[0] && p[1] >= b[1] && p[0] < b[0] + BUTTON_SIZE[0] && p[1] < b[1] + BUTTON_SIZE[1]
    }

    /// World-space marks over everything: the route being walked, dotted,
    /// and its destination, in lines a pixel of the art wide.
    pub(super) fn overlay(&self) -> Canvas {
        let mut canvas = Canvas::new(self.tileset());
        let Some(route) = &self.route else {
            return canvas;
        };
        let zoom = self.tileset().zoom();
        let px = 1.0 / zoom;
        let snap = |v: f32| (v * zoom).floor() / zoom;
        // Dots every `ROUTE_DOT` along the way left, counted back from the
        // goal, so each stays put as the player walks on (rather than the
        // whole line sliding along with them); those the player is within a
        // dot of are behind them.
        let way: Vec<[f32; 2]> = std::iter::once(center(self.player, PLAYER_SIZE))
            .chain(route.waypoints[route.next..].iter().copied())
            .collect();
        let total: f32 = way.windows(2).map(|s| distance(s[0], s[1])).sum();
        // How far back from the goal the leg ends, and the next dot is.
        let (mut done, mut dot) = (0.0, ROUTE_DOT);
        for leg in way.windows(2).rev() {
            let [near, far] = [leg[0], leg[1]];
            let len = distance(near, far);
            while dot <= done + len && dot <= total - ROUTE_DOT {
                let t = (dot - done) / len;
                let p = [
                    far[0] + (near[0] - far[0]) * t,
                    far[1] + (near[1] - far[1]) * t,
                ];
                canvas.rect([snap(p[0]), snap(p[1])], [px, px], palette::YELLOW);
                dot += ROUTE_DOT;
            }
            done += len;
        }
        let goal = *route.waypoints.last().unwrap();
        let corner = [snap(goal[0] - 2.5), snap(goal[1] - 2.5)];
        canvas.outline(corner, [5.0, 5.0], px, palette::YELLOW);
        canvas
    }

    /// The interface, in interface pixels: the pause button, showing what a
    /// click does (pause while running, play while paused).
    pub(super) fn ui(&self) -> Canvas {
        let mut canvas = Canvas::new(self.tileset());
        let [x, y] = self.button_pos();
        let hover = self.pointer.is_some_and(|p| self.on_button(p));
        let fill = if hover {
            palette::LIGHT_BLUE
        } else {
            palette::BLUE
        };
        canvas.rect([x, y], BUTTON_SIZE, fill);
        canvas.outline([x, y], BUTTON_SIZE, 1.0, palette::LIGHT_GRAY);
        if self.paused {
            canvas.poly(
                &[[x + 5.0, y + 3.0], [x + 10.0, y + 6.0], [x + 5.0, y + 9.0]],
                palette::WHITE,
            );
        } else {
            canvas.rect([x + 4.0, y + 3.0], [2.0, 6.0], palette::WHITE);
            canvas.rect([x + 8.0, y + 3.0], [2.0, 6.0], palette::WHITE);
        }
        canvas
    }
}
