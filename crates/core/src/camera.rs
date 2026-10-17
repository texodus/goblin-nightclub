//! The view onto a level: where it is, how big, and how far zoomed in.
//! Shared by the game and the editor.

use crate::input::Input;

/// Each wheel notch zooms by this factor.
pub const ZOOM_STEP: f32 = 1.01;
/// The most zoomed in, relative to the default. Zooming out stops where a
/// pixel of the art is a pixel of the screen.
pub const ZOOM_MAX: f32 = 3.0;

/// Panning speed, in screen pixels a second.
pub const PAN_SPEED: f32 = 600.0;

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
        let min = 1.0 / self.ui_scale();
        self.zoom_factor = (self.zoom_factor * ZOOM_STEP.powi(steps)).clamp(min, ZOOM_MAX);
        let zoom = self.zoom();
        self.pos = [anchor[0] - p[0] / zoom, anchor[1] - p[1] / zoom];
    }

    /// The view is now `view` screen pixels: keep what's at its center
    /// there, and the zoom within bounds.
    pub fn resize(&mut self, view: [f32; 2]) {
        let center = self.to_world([self.view[0] / 2.0, self.view[1] / 2.0]);
        self.view = view;
        self.zoom_factor = self.zoom_factor.clamp(1.0 / self.ui_scale(), ZOOM_MAX);
        self.center_on(center);
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
