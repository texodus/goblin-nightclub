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

//! The view onto a level: where it is, how big, and how far zoomed in.
//! Shared by the game and the editor.

use crate::geom::Rect;
use crate::input::Input;
use crate::layer::TILE;

/// Each wheel notch zooms by this factor.
pub const ZOOM_STEP: f32 = 1.01;
/// The most zoomed in, relative to the default.
pub const ZOOM_MAX: f32 = 3.0;
/// The most zoomed out: where a pixel of the screen spans this many pixels
/// of the art, at any size of view.
pub const ZOOM_OUT: f32 = 16.0;

/// Panning speed, in screen pixels a second.
pub const PAN_SPEED: f32 = 600.0;

/// Scale of each layer relative to the one above it, at the default
/// zoom. See [`parallax`] for other zooms.
const PARALLAX: f32 = 0.925;

/// How much the parallax is normalized against zooming out, 0 to 1. At 1
/// its strength shrinks in step with the zoom, so a layer's shift at the
/// edge of the view stays what it is at the default zoom. At 0 the
/// strength is [`PARALLAX`] at every zoom, so the shift at the edge grows
/// as the view widens. Zooming in never changes it.
const PARALLAX_NORMALIZATION: f32 = 1.0;

/// The scale of each layer relative to the one above it at `zoom` screen
/// pixels a world unit, where `default_zoom` is the zoom the art is drawn
/// at its own size (see [`crate::tileset::Tileset::zoom`] and the
/// interface scale).
pub fn parallax(zoom: f32, default_zoom: f32) -> f32 {
    let normalized = (zoom / default_zoom).min(1.0);
    let strength = 1.0 - PARALLAX_NORMALIZATION.clamp(0.0, 1.0) * (1.0 - normalized);
    1.0 - (1.0 - PARALLAX) * strength
}

/// The part of a layer in a view `view` screen pixels in size, its
/// top-left at world point `camera`, at `zoom` screen pixels a world unit,
/// when the layer is drawn `scale` times its size about the view's center
/// (parallax, for a layer below the viewer's). Padded by a tile, so
/// rounding the camera to a pixel never uncovers anything.
pub fn layer_view(camera: [f32; 2], view: [f32; 2], zoom: f32, scale: f32) -> Rect {
    // A layer's world point `w` lands on view pixel
    // `c + ((w - camera) * zoom - c) * scale`, `c` the view's center.
    let edge = |px: f32, axis: usize| {
        let c = view[axis] / 2.0;
        camera[axis] + (c + (px - c) / scale) / zoom
    };
    [
        [edge(0.0, 0) - TILE, edge(0.0, 1) - TILE],
        [edge(view[0], 0) + TILE, edge(view[1], 1) + TILE],
    ]
}

pub struct Camera {
    /// Top-left of the view, in world units.
    pub pos: [f32; 2],
    /// Size of the view, in screen pixels.
    pub view: [f32; 2],
    /// Screen pixels per world unit at which the art is at its own size
    /// (the tileset's [`crate::tileset::Tileset::zoom`]).
    base: f32,
    /// Zoom relative to the default (see [`Camera::zoom`]).
    zoom_factor: f32,
}

impl Camera {
    /// A view `view` screen pixels in size, at the default zoom, for art at
    /// its own size at `base` screen pixels a world unit.
    pub fn new(view: [f32; 2], base: f32) -> Self {
        Self {
            pos: [0.0; 2],
            view,
            base,
            zoom_factor: 1.0,
        }
    }

    /// Screen pixels per interface pixel: the interface is laid out for
    /// a [`crate::WIDTH`] x [`crate::HEIGHT`] screen and scaled up by the
    /// largest whole number that fits the view.
    pub fn ui_scale(&self) -> f32 {
        let fit = (self.view[0] / crate::WIDTH as f32).min(self.view[1] / crate::HEIGHT as f32);
        fit.floor().max(1.0)
    }

    /// Screen pixels per world unit. By default the art is drawn at the
    /// interface's scale (a pixel of the art an interface pixel), so the
    /// view shows about as much of the level at any resolution.
    pub fn zoom(&self) -> f32 {
        self.base * self.ui_scale() * self.zoom_factor
    }

    /// Zoom in (`steps > 0`) or out by that many wheel notches, keeping the
    /// world point under `p` (view pixels) where it is.
    pub fn zoom_by(&mut self, steps: i32, p: [f32; 2]) {
        let anchor = self.to_world(p);
        self.zoom_factor =
            (self.zoom_factor * ZOOM_STEP.powi(steps)).clamp(self.least_zoom(), ZOOM_MAX);
        let zoom = self.zoom();
        self.pos = [anchor[0] - p[0] / zoom, anchor[1] - p[1] / zoom];
    }

    /// The view is now `view` screen pixels: keep what's at its center
    /// there, and the zoom within bounds.
    pub fn resize(&mut self, view: [f32; 2]) {
        let center = self.to_world([self.view[0] / 2.0, self.view[1] / 2.0]);
        self.view = view;
        self.zoom_factor = self.zoom_factor.clamp(self.least_zoom(), ZOOM_MAX);
        self.center_on(center);
    }

    /// The least `zoom_factor`: see [`ZOOM_OUT`].
    fn least_zoom(&self) -> f32 {
        1.0 / (self.ui_scale() * ZOOM_OUT)
    }

    /// The world point at view pixel `p`.
    pub fn to_world(&self, p: [f32; 2]) -> [f32; 2] {
        let zoom = self.zoom();
        [self.pos[0] + p[0] / zoom, self.pos[1] + p[1] / zoom]
    }

    /// Size of the view, in world units.
    pub fn view_world(&self) -> [f32; 2] {
        [self.view[0] / self.zoom(), self.view[1] / self.zoom()]
    }

    /// Put world point `p` at the center of the view.
    pub fn center_on(&mut self, p: [f32; 2]) {
        let view = self.view_world();
        self.pos = [p[0] - view[0] / 2.0, p[1] - view[1] / 2.0];
    }

    /// Move by `delta` screen pixels.
    pub fn pan_by(&mut self, delta: [f32; 2]) {
        let zoom = self.zoom();
        self.pos = [self.pos[0] + delta[0] / zoom, self.pos[1] + delta[1] / zoom];
    }

    /// Pan with whichever of the arrow keys or WASD are held.
    pub fn pan(&mut self, input: &Input, dt: f32) {
        let dx = input.axis(&["ArrowLeft", "KeyA"], &["ArrowRight", "KeyD"]);
        let dy = input.axis(&["ArrowUp", "KeyW"], &["ArrowDown", "KeyS"]);
        self.pan_by([dx * PAN_SPEED * dt, dy * PAN_SPEED * dt]);
    }
}
