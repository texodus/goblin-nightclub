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

//! Going from one layer to another, by the stairs or moving the view: the
//! layers fade and zoom between the two views.

use super::*;

/// Going from one layer to another, as shown: the layers zoom from one
/// layer's view to the other's as the upper of the two fades. Down, the
/// layer left fades out as it grows past the viewer, the one beneath
/// growing into its place; up, the one come to fades in as it shrinks into
/// place from past the viewer, the one left shrinking beneath it.
pub(super) struct LayerShift {
    /// The layer left, in the whole level.
    pub(super) from: usize,
    /// How far along: from 0 to 1.
    pub(super) t: f32,
}

/// What's shown of a [`LayerShift`] at a moment: the layer drawn as the
/// viewer's, how opaque it is, how many layers deeper each is drawn, and a
/// layer beneath it to draw whether it shows through or not (as if it were
/// the viewer's, which it was or is about to be).
pub(super) struct Shown {
    pub(super) top: usize,
    pub(super) fade: f32,
    pub(super) sink: f32,
    pub(super) beneath: Option<usize>,
}

/// Seconds going from one layer to another takes to show (see
/// [`LayerShift`]).
pub(super) const LAYER_SHIFT_TIME: f32 = 0.3;

impl Game {
    /// Stepping onto stairs (from another tile) takes the player to the
    /// layer they lead to, at the same spot. Arriving on the stairs at the
    /// other end isn't stepping onto them, so there's no bouncing back.
    pub(super) fn take_stairs(&mut self, before: [i32; 2]) {
        let here = tile_of(center(self.player, PLAYER_SIZE));
        if here == before {
            return;
        }
        let from = self.layer();
        match self.level.layer(self.top).tile(here[0], here[1]).stairs() {
            Some(Dir::Down) if self.top > 0 => self.top -= 1,
            Some(Dir::Up) if self.top + 1 < self.level.depth() => self.top += 1,
            _ => return,
        }
        // A view moved off the player stays where it is.
        if self.view.is_none() {
            self.shift = Some(LayerShift { from, t: 0.0 });
        }
    }

    /// The layer the view is from, as a layer of the game's level: the
    /// player's, unless it's been moved off it; for a level played a
    /// window at a time, no further than the window's layers.
    pub(super) fn view_top(&self) -> usize {
        let base = self.layer() - self.top;
        self.view.map_or(self.top, |v| {
            v.saturating_sub(base).min(self.level.depth() - 1)
        })
    }

    /// Move the view `by` layers up (down, if negative), as far as there
    /// are layers, the player staying where they are.
    pub(super) fn move_view(&mut self, by: i32) {
        let from = self.view_top();
        let to = (from as i32 + by).clamp(0, self.level.depth() as i32 - 1) as usize;
        if to != from {
            self.set_view(Some(to));
        }
    }

    /// View from layer `to` of the game's level from now on, or (`None`,
    /// or the player's layer) from the player's, following them; going
    /// there as the stairs do, if it's another layer.
    pub(super) fn set_view(&mut self, to: Option<usize>) {
        let base = self.layer() - self.top;
        let from = self.view_top();
        self.view = to.filter(|&k| k != self.top).map(|k| base + k);
        if self.view_top() != from {
            self.shift = Some(LayerShift {
                from: base + from,
                t: 0.0,
            });
        }
    }

    /// What's shown of going from one layer to another, if anything is.
    pub(super) fn shown(&self) -> Shown {
        let top = self.view_top();
        let still = Shown {
            top,
            fade: 1.0,
            sink: 0.0,
            beneath: None,
        };
        let base = self.layer() - self.top;
        // The layer left, if it's still in the window, next to this one.
        let from = self.shift.as_ref().and_then(|s| {
            let from = s.from.checked_sub(base)?;
            (from.abs_diff(top) == 1 && from < self.level.depth()).then_some((from, s.t))
        });
        let Some((from, t)) = from else {
            return still;
        };
        // Eased in and out. Either way it ends as the view from this
        // layer: the one above it gone, or this one in place.
        let t = t.clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        if from > top {
            Shown {
                top: from,
                fade: 1.0 - t,
                sink: -t,
                beneath: Some(top),
            }
        } else {
            Shown {
                fade: t,
                sink: t - 1.0,
                beneath: Some(from),
                ..still
            }
        }
    }
}
