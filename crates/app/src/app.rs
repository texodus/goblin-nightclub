//! The wasm-bindgen surface: JS owns the frame loop, input events and the
//! HTML interface (the start menu, the editor's toolbar) and forwards them
//! to [`App`], which runs a game or the editor.

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::renderer::Renderer;
use goblin_nightclub_core::layer::Corner;
use goblin_nightclub_core::level_file::LevelFile;
use goblin_nightclub_core::{HEIGHT, WIDTH};
use goblin_nightclub_editor::{self as editor, BrushShape, Editor, Tool};
use goblin_nightclub_game::Game;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

enum Mode {
    /// The start menu is up; nothing is drawn.
    Menu,
    Game(Box<Game>),
    Editor,
}

#[wasm_bindgen]
pub struct App {
    mode: Mode,
    /// Kept while playing or in the menu, so the editor picks up where it
    /// left off (e.g. after trying the level out).
    editor: Option<Editor>,
    renderer: Renderer,
    last_time: Option<f64>,
}

fn js_err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
impl App {
    /// Initialize WebGPU on `canvas`, at the start menu. Async because
    /// adapter/device requests are promises in the browser.
    pub async fn create(canvas: HtmlCanvasElement) -> Result<App, JsError> {
        // The canvas is sized already (see `resize`), unless it's empty.
        if canvas.width() * canvas.height() == 0 {
            canvas.set_width(WIDTH);
            canvas.set_height(HEIGHT);
        }
        let (width, height) = (canvas.width(), canvas.height());
        let renderer = Renderer::new(canvas, width, height).await?;
        Ok(App {
            mode: Mode::Menu,
            editor: None,
            renderer,
            last_time: None,
        })
    }

    fn view(&self) -> [f32; 2] {
        let [w, h] = self.renderer.size();
        [w as f32, h as f32]
    }

    /// Switch to `mode`, forgetting what the last level's views had seen.
    fn switch(&mut self, mode: Mode) {
        self.mode = mode;
        self.renderer.reset();
    }

    /// Back to the start menu.
    pub fn menu(&mut self) {
        if let Mode::Game(game) = &mut self.mode {
            game.key_pressed("Escape");
        }
        self.mode = Mode::Menu;
    }

    /// Start a game on a newly generated level, from `seed`.
    pub fn new_game(&mut self, seed: u32) {
        let [w, h] = self.view();
        self.switch(Mode::Game(Box::new(Game::new(w, h, seed))));
    }

    /// Start a game on a saved level (see [`LevelFile`]).
    pub fn load_game(&mut self, json: &str, seed: u32) -> Result<(), JsError> {
        let file = LevelFile::from_json(json).map_err(js_err)?;
        let game = Game::from_file(&file, self.view(), seed);
        self.switch(Mode::Game(Box::new(game)));
        Ok(())
    }

    /// Open the editor, on the level last edited, or a new one.
    pub fn editor(&mut self) {
        if self.editor.is_none() {
            let (cols, rows, depth) = (editor::NEW_COLS, editor::NEW_ROWS, editor::NEW_LAYERS);
            self.editor = Some(Editor::new(cols, rows, depth, self.view()));
        }
        self.switch(Mode::Editor);
    }

    /// Try out the level being edited, from its spawn.
    pub fn play_edited(&mut self, seed: u32) {
        if let Some(editor) = &self.editor {
            let game = Game::from_file(&editor.to_file(), self.view(), seed);
            self.switch(Mode::Game(Box::new(game)));
        }
    }

    fn edit(&mut self) -> Option<&mut Editor> {
        match self.mode {
            Mode::Editor => self.editor.as_mut(),
            _ => None,
        }
    }

    /// Start editing a blank level of this size.
    pub fn editor_new(&mut self, cols: usize, rows: usize, layers: usize) {
        self.editor = Some(Editor::new(cols, rows, layers, self.view()));
        self.switch(Mode::Editor);
    }

    /// Start editing a saved level.
    pub fn editor_load(&mut self, json: &str) -> Result<(), JsError> {
        let file = LevelFile::from_json(json).map_err(js_err)?;
        self.editor = Some(Editor::from_file(&file, self.view()));
        self.switch(Mode::Editor);
        Ok(())
    }

    /// The level being edited, as JSON (see [`LevelFile`]).
    pub fn editor_save(&self) -> Option<String> {
        self.editor.as_ref().map(|e| e.to_file().to_json())
    }

    /// The palette, as JSON: a list of `{name, cell}`, `cell` the swatch's
    /// `[column, row]` in the tile sheet (or null).
    pub fn editor_palette(&self) -> String {
        serde_json::to_string(&editor::swatches()).unwrap()
    }

    /// The editor's settings, as JSON (see [`editor::State`]).
    pub fn editor_state(&self) -> Option<String> {
        self.editor
            .as_ref()
            .map(|e| serde_json::to_string(&e.state()).unwrap())
    }

    /// One of `pencil`, `rect`, `fill`, `pick`, `spawn`.
    pub fn editor_tool(&mut self, tool: &str) {
        let tool = match tool {
            "rect" => Tool::Rect,
            "fill" => Tool::Fill,
            "pick" => Tool::Pick,
            "spawn" => Tool::Spawn,
            _ => Tool::Pencil,
        };
        if let Some(e) = self.edit() {
            e.set_tool(tool);
        }
    }

    /// One of `whole`, `NW`, `NE`, `SW`, `SE`.
    pub fn editor_shape(&mut self, shape: &str) {
        let shape = match shape {
            "NW" => BrushShape::Split(Corner::NW),
            "NE" => BrushShape::Split(Corner::NE),
            "SW" => BrushShape::Split(Corner::SW),
            "SE" => BrushShape::Split(Corner::SE),
            _ => BrushShape::Whole,
        };
        if let Some(e) = self.edit() {
            e.set_shape(shape);
        }
    }

    /// Choose palette entry `index` as the primary (or `secondary`) brush.
    pub fn editor_brush(&mut self, index: usize, secondary: bool) {
        if let Some(e) = self.edit() {
            e.set_brush(index, secondary);
        }
    }

    pub fn editor_layer(&mut self, layer: usize) {
        if let Some(e) = self.edit() {
            e.set_layer(layer);
        }
    }

    /// Add a layer of floor above (or below) the one being edited.
    pub fn editor_add_layer(&mut self, above: bool) {
        if let Some(e) = self.edit() {
            e.add_layer(above);
        }
    }

    /// Remove the layer being edited.
    pub fn editor_remove_layer(&mut self) {
        if let Some(e) = self.edit() {
            e.remove_layer();
        }
    }

    pub fn editor_undo(&mut self) {
        if let Some(e) = self.edit() {
            e.undo();
        }
    }

    pub fn editor_redo(&mut self) {
        if let Some(e) = self.edit() {
            e.redo();
        }
    }

    /// Advance and draw one frame. `time_ms` is the `requestAnimationFrame`
    /// timestamp.
    pub fn frame(&mut self, time_ms: f64) {
        let dt = self
            .last_time
            .map_or(0.0, |t| ((time_ms - t) / 1000.0) as f32);
        self.last_time = Some(time_ms);
        // Clamp so a backgrounded tab doesn't produce one giant step.
        let dt = dt.min(0.1);
        match &mut self.mode {
            Mode::Menu => {}
            Mode::Game(game) => {
                game.update(dt);
                self.renderer.draw(&game.frame());
            }
            Mode::Editor => {
                if let Some(editor) = &mut self.editor {
                    editor.update(dt);
                    self.renderer.draw(&editor.frame());
                }
            }
        }
    }

    /// `code` is a `KeyboardEvent.code`, e.g. `"ArrowLeft"` or `"KeyW"`.
    /// `repeat` is set for auto-repeats of a held key.
    pub fn key_down(&mut self, code: &str, repeat: bool) {
        match &mut self.mode {
            Mode::Game(game) => {
                game.input.set(code, true);
                if !repeat {
                    game.key_pressed(code);
                }
            }
            Mode::Editor => {
                if let Some(e) = &mut self.editor {
                    e.input.set(code, true);
                }
            }
            Mode::Menu => {}
        }
    }

    pub fn key_up(&mut self, code: &str) {
        match &mut self.mode {
            Mode::Game(game) => game.input.set(code, false),
            Mode::Editor => {
                if let Some(e) = &mut self.editor {
                    e.input.set(code, false);
                }
            }
            Mode::Menu => {}
        }
    }

    /// Mouse button `button` (0 left, 1 middle, 2 right) went down at
    /// `(x, y)` in canvas pixels.
    pub fn mouse_down(&mut self, x: f32, y: f32, button: u32) {
        match &mut self.mode {
            Mode::Game(game) if button == 0 => game.click([x, y]),
            Mode::Editor => {
                if let Some(e) = &mut self.editor {
                    e.mouse_down([x, y], button);
                }
            }
            _ => {}
        }
    }

    pub fn mouse_up(&mut self, x: f32, y: f32, button: u32) {
        if let Some(e) = self.edit() {
            e.mouse_up([x, y], button);
        }
    }

    /// The pointer moved to `(x, y)` in canvas pixels.
    pub fn pointer(&mut self, x: f32, y: f32) {
        match &mut self.mode {
            Mode::Game(game) => game.pointer([x, y]),
            Mode::Editor => {
                if let Some(e) = &mut self.editor {
                    e.pointer([x, y]);
                }
            }
            Mode::Menu => {}
        }
    }

    /// Zoom in (`steps > 0`) or out by `steps` notches about `(x, y)` in
    /// canvas pixels (the mouse wheel).
    pub fn zoom(&mut self, steps: i32, x: f32, y: f32) {
        match &mut self.mode {
            Mode::Game(game) => game.zoom_by(steps, [x, y]),
            Mode::Editor => {
                if let Some(e) = &mut self.editor {
                    e.zoom_by(steps, [x, y]);
                }
            }
            Mode::Menu => {}
        }
    }

    /// The canvas is now `width` x `height` pixels.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.renderer.resize(width, height);
        let view = [width as f32, height as f32];
        if let Mode::Game(game) = &mut self.mode {
            game.resize(view);
        }
        if let Some(e) = &mut self.editor {
            e.resize(view);
        }
    }
}
