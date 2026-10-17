//! The wasm-bindgen surface: JS owns the frame loop and input events and
//! forwards them to [`App`].

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::game::Game;
use crate::renderer::Renderer;
use crate::{HEIGHT, WIDTH};

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub struct App {
    game: Game,
    renderer: Renderer,
    last_time: Option<f64>,
}

#[wasm_bindgen]
impl App {
    /// Initialize WebGPU on `canvas`. Async because adapter/device requests
    /// are promises in the browser. `seed` drives level generation.
    pub async fn create(canvas: HtmlCanvasElement, seed: u32) -> Result<App, JsError> {
        // The canvas is sized already (see `resize`), unless it's empty.
        if canvas.width() * canvas.height() == 0 {
            canvas.set_width(WIDTH);
            canvas.set_height(HEIGHT);
        }
        let (width, height) = (canvas.width(), canvas.height());
        let renderer = Renderer::new(canvas, width, height).await?;
        Ok(App {
            game: Game::new(width as f32, height as f32, seed),
            renderer,
            last_time: None,
        })
    }

    /// Advance and draw one frame. `time_ms` is the `requestAnimationFrame`
    /// timestamp.
    pub fn frame(&mut self, time_ms: f64) {
        let dt = self
            .last_time
            .map_or(0.0, |t| ((time_ms - t) / 1000.0) as f32);
        self.last_time = Some(time_ms);
        // Clamp so a backgrounded tab doesn't produce one giant step.
        self.game.update(dt.min(0.1));
        self.renderer.draw(&self.game.frame());
    }

    /// `code` is a `KeyboardEvent.code`, e.g. `"ArrowLeft"` or `"KeyW"`.
    /// `repeat` is set for auto-repeats of a held key.
    pub fn key_down(&mut self, code: &str, repeat: bool) {
        self.game.input.set(code, true);
        if !repeat {
            self.game.key_pressed(code);
        }
    }

    /// A click at `(x, y)` in canvas pixels.
    pub fn click(&mut self, x: f32, y: f32) {
        self.game.click([x, y]);
    }

    /// Zoom in (`steps > 0`) or out by `steps` levels about `(x, y)` in
    /// canvas pixels (the mouse wheel).
    pub fn zoom(&mut self, steps: i32, x: f32, y: f32) {
        self.game.zoom_by(steps, [x, y]);
    }

    /// The canvas is now `width` x `height` pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
        self.game.resize([width as f32, height as f32]);
    }

    /// The pointer moved to `(x, y)` in canvas pixels.
    pub fn pointer(&mut self, x: f32, y: f32) {
        self.game.pointer([x, y]);
    }

    pub fn key_up(&mut self, code: &str) {
        self.game.input.set(code, false);
    }
}
