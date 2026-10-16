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
        canvas.set_width(WIDTH);
        canvas.set_height(HEIGHT);
        let renderer = Renderer::new(canvas, WIDTH, HEIGHT).await?;
        Ok(App {
            game: Game::new(WIDTH as f32, HEIGHT as f32, seed),
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
    pub fn key_down(&mut self, code: &str) {
        self.game.input.set(code, true);
    }

    pub fn key_up(&mut self, code: &str) {
        self.game.input.set(code, false);
    }
}
