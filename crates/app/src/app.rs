//! The wasm-bindgen surface: JS owns the frame loop, input events and the
//! HTML interface (the start menu, the editor's toolbar) and forwards them
//! to [`App`], which runs a game or the editor.

use std::rc::Rc;
use std::sync::Arc;

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::generated::{Regions, WorkerSource};
use crate::http::HttpBytes;
use crate::renderer::Renderer;
use goblin_nightclub_core::layer::Corner;
use goblin_nightclub_core::level_file::LevelFile;
use goblin_nightclub_core::packed::{self, Opened, Packed};
use goblin_nightclub_core::tileset::{Sheet, Tileset};
use goblin_nightclub_core::{HEIGHT, WIDTH};
use goblin_nightclub_editor::{BrushShape, Editor, Tool};
use goblin_nightclub_game::{Game, NEW_WORLD, Opening};
use goblin_nightclub_generate::{Params, Plan, generated_meta};
use std::cell::RefCell;

#[wasm_bindgen(start)]
fn init() {
    console_error_panic_hook::set_once();
}

enum Mode {
    /// The start menu is up; nothing is drawn.
    Menu,
    /// A game starting on a level from a server (see [`App::open_url`]);
    /// nothing is drawn yet.
    Opening(Box<Pending>),
    Game(Box<Game>),
    Editor,
}

/// A game on a level from a server, starting: its header and index
/// first, then its first window.
enum Pending {
    Header { bytes: HttpBytes, seed: u32 },
    Window(Box<Opening>),
}

#[wasm_bindgen]
pub struct App {
    mode: Mode,
    /// Kept while playing or in the menu, so the editor picks up where it
    /// left off (e.g. after trying the level out).
    editor: Option<Editor>,
    /// The packed level the editor's level came from, if it did: what a
    /// diff is saved against.
    editor_base: Option<Rc<Packed<Arc<[u8]>>>>,
    /// Files to open (see [`App::stage`]).
    staged: Vec<Arc<[u8]>>,
    /// What went wrong starting a game, for JS to show.
    error: Option<String>,
    /// For a game on a world generated a region at a time: where the
    /// regions made in workers go (see [`App::deliver_region`]).
    regions: Option<Rc<RefCell<Regions>>>,
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
            editor_base: None,
            staged: Vec::new(),
            error: None,
            regions: None,
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

    /// The tileset of the level on screen, if any.
    fn tileset(&self) -> Option<&Tileset> {
        match &self.mode {
            Mode::Menu => None,
            Mode::Opening(pending) => match pending.as_ref() {
                Pending::Header { .. } => None,
                Pending::Window(opening) => Some(opening.tileset()),
            },
            Mode::Game(game) => Some(game.tileset()),
            Mode::Editor => self.editor.as_ref().map(|e| e.tileset()),
        }
    }

    /// What the current level draws with, as JSON `{image, cell}`: the
    /// sheet image's URL (or `data:` URL) and its cell size in pixels. JS
    /// loads the image and hands its pixels to `set_sheet`. Null in the
    /// menu.
    pub fn level_info(&self) -> Option<String> {
        self.tileset()
            .map(|t| serde_json::json!({ "image": t.image, "cell": t.cell }).to_string())
    }

    /// The tile sheet's pixels: `width` x `height`, RGBA row by row.
    /// Fails if the current level's tileset names cells outside it.
    pub fn set_sheet(&mut self, width: u32, height: u32, rgba: &[u8]) -> Result<(), JsError> {
        let sheet = Sheet::new([width, height], rgba.to_vec()).map_err(js_err)?;
        self.renderer.set_sheet(&sheet);
        if let Some(t) = self.tileset() {
            t.check_sheet(sheet.size).map_err(js_err)?;
        }
        Ok(())
    }

    /// Back to the start menu.
    pub fn menu(&mut self) {
        if let Mode::Game(game) = &mut self.mode {
            game.key_pressed("Escape");
        }
        self.mode = Mode::Menu;
    }

    /// Add a file to those [`App::load_staged`] or
    /// [`App::editor_load_staged`] open next: a level file (JSON), a packed
    /// level, or a diff over one.
    pub fn stage(&mut self, bytes: &[u8]) {
        self.staged.push(bytes.into());
    }

    fn take_staged(&mut self) -> Result<Opened, JsError> {
        packed::open_files(std::mem::take(&mut self.staged)).map_err(js_err)
    }

    /// Start a game on the files staged: a level file, or a packed level
    /// and any diffs over it (played a window at a time if it's big).
    pub fn load_staged(&mut self, seed: u32) -> Result<(), JsError> {
        let game = match self.take_staged()? {
            Opened::File(file) => Game::from_file(&file, self.view(), seed),
            Opened::Chunks { source, meta, .. } => {
                Game::from_source(source, &meta, self.view(), seed).map_err(js_err)?
            }
        };
        self.switch(Mode::Game(Box::new(game)));
        Ok(())
    }

    /// Start a game on a packed level at `url`, read a piece at a time by
    /// range requests as it's played. It starts once the first window
    /// around the spawn has come (see [`App::opening`]).
    pub fn open_url(&mut self, url: &str, seed: u32) {
        let bytes = HttpBytes::new(url);
        self.switch(Mode::Opening(Box::new(Pending::Header { bytes, seed })));
    }

    /// Start a game on a newly generated world, `cols` x `rows` tiles a
    /// layer ([`NEW_WORLD`] without them), with generation parameters
    /// `params` (JSON; any left out, or all without it, take their
    /// defaults), made a region at a time as it's played: `request(rx,
    /// ry)` asks for region `(rx, ry)`, which JS has a worker make (with
    /// [`crate::generated::generate_region`]) and hands to
    /// [`App::deliver_region`]. It starts once the regions around the
    /// spawn have come (see [`App::opening`]). The world's size, `[cols,
    /// rows]`, for asking the workers.
    pub fn new_generated_game(
        &mut self,
        seed: u32,
        params: Option<String>,
        cols: Option<usize>,
        rows: Option<usize>,
        request: js_sys::Function,
    ) -> Result<Vec<usize>, JsError> {
        let params: Params = match params {
            Some(json) => serde_json::from_str(&json).map_err(js_err)?,
            None => Params::default(),
        };
        let [cols, rows] = [cols.unwrap_or(NEW_WORLD[0]), rows.unwrap_or(NEW_WORLD[1])];
        let plan = Plan::new(seed, cols, rows, &params);
        let meta = generated_meta(&plan);
        meta.check().map_err(js_err)?;
        let (source, regions) = WorkerSource::new(plan, request);
        let opening = Opening::new(Rc::new(source), &meta, self.view(), seed);
        self.regions = Some(regions);
        self.switch(Mode::Opening(Box::new(Pending::Window(Box::new(opening)))));
        Ok(vec![cols, rows])
    }

    /// Region `(rx, ry)` of the world being played, made in a worker and
    /// packed (see [`App::new_generated_game`]).
    pub fn deliver_region(&mut self, rx: usize, ry: usize, bytes: &[u8]) -> Result<(), JsError> {
        let Some(regions) = &self.regions else {
            return Ok(());
        };
        regions
            .borrow_mut()
            .deliver([rx, ry], bytes)
            .map_err(js_err)
    }

    /// Whether a game is still starting (see [`App::open_url`]).
    pub fn opening(&self) -> bool {
        matches!(self.mode, Mode::Opening(_))
    }

    /// What went wrong starting a game, if anything has since last asked.
    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    /// Start a game on a saved level (see [`LevelFile`]).
    pub fn load_game(&mut self, json: &str, seed: u32) -> Result<(), JsError> {
        let file = LevelFile::from_json(json).map_err(js_err)?;
        let game = Game::from_file(&file, self.view(), seed);
        self.switch(Mode::Game(Box::new(game)));
        Ok(())
    }

    /// Open the editor, on the level last edited, or else on a newly
    /// generated world from `seed`, the same as a new game's, with
    /// generation parameters given as JSON (see [`Params`]; any left out,
    /// or all of them without `params`, take their defaults).
    pub fn editor(&mut self, seed: u32, params: Option<String>) -> Result<(), JsError> {
        if self.editor.is_none() {
            let params: Params = match params {
                Some(json) => serde_json::from_str(&json).map_err(js_err)?,
                None => Params::default(),
            };
            let world = goblin_nightclub_game::new_world(seed, &params);
            self.editor = Some(Editor::from_file(&world, self.view()));
            self.editor_base = None;
        }
        self.switch(Mode::Editor);
        Ok(())
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

    /// Start editing a blank level of this size, with the generator's
    /// tileset.
    pub fn editor_new(&mut self, cols: usize, rows: usize, layers: usize) {
        let blank = goblin_nightclub_generate::template(cols, rows, layers);
        self.editor = Some(Editor::from_file(&blank, self.view()));
        self.editor_base = None;
        self.switch(Mode::Editor);
    }

    /// Start editing a saved level.
    pub fn editor_load(&mut self, json: &str) -> Result<(), JsError> {
        let file = LevelFile::from_json(json).map_err(js_err)?;
        self.editor = Some(Editor::from_file(&file, self.view()));
        self.editor_base = None;
        self.switch(Mode::Editor);
        Ok(())
    }

    /// Edit the level in the files staged (see [`App::stage`]), small
    /// enough to edit whole. From a packed level (and any diffs over it),
    /// a diff over that level can be saved.
    pub fn editor_load_staged(&mut self) -> Result<(), JsError> {
        let (file, base) = match self.take_staged()? {
            Opened::File(file) => (file, None),
            Opened::Chunks { source, meta, base } => {
                let file = packed::to_level_file(&meta, source.as_ref()).map_err(js_err)?;
                (file, Some(base))
            }
        };
        self.editor = Some(Editor::from_file(&file, self.view()));
        self.editor_base = base;
        self.switch(Mode::Editor);
        Ok(())
    }

    /// The level being edited, packed (see [`packed`]).
    pub fn editor_save_packed(&self) -> Option<Vec<u8>> {
        (self.editor.as_ref()).map(|e| packed::pack_file(&e.to_file()))
    }

    /// Whether the level being edited came from a packed level, so a diff
    /// over it can be saved.
    pub fn editor_has_base(&self) -> bool {
        self.editor_base.is_some()
    }

    /// What's changed in the level being edited since it came from a
    /// packed level, as a diff over that level: with it, the diff opens
    /// as the level edited.
    pub fn editor_save_diff(&self) -> Result<Vec<u8>, JsError> {
        let (Some(editor), Some(base)) = (&self.editor, &self.editor_base) else {
            return Err(js_err("the level wasn't opened from a packed level"));
        };
        packed::diff(base.as_ref(), &base.header().id, &editor.to_file()).map_err(js_err)
    }

    /// The level being edited, as JSON (see [`LevelFile`]).
    pub fn editor_save(&self) -> Option<String> {
        self.editor.as_ref().map(|e| e.to_file().to_json())
    }

    /// The palette of the level being edited, as JSON: a list of
    /// `{name, cell}`, `cell` the swatch's `[column, row]` in the tile
    /// sheet (or null). None without an editor.
    pub fn editor_palette(&self) -> Option<String> {
        self.editor
            .as_ref()
            .map(|e| serde_json::to_string(&e.swatches()).unwrap())
    }

    /// The editor's settings, as JSON (see [`goblin_nightclub_editor::State`]).
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
        let view = self.view();
        match &mut self.mode {
            Mode::Menu => {}
            Mode::Opening(pending) => match pending.as_mut() {
                Pending::Header { bytes, seed } => match Packed::open(bytes.clone()) {
                    Ok(None) => {}
                    Ok(Some(packed)) => {
                        let meta = packed.header().level.clone();
                        let opening = Opening::new(Rc::new(packed), &meta, view, *seed);
                        **pending = Pending::Window(Box::new(opening));
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.mode = Mode::Menu;
                    }
                },
                Pending::Window(opening) => {
                    if opening.step() {
                        let Mode::Opening(pending) = std::mem::replace(&mut self.mode, Mode::Menu)
                        else {
                            unreachable!()
                        };
                        let Pending::Window(opening) = *pending else {
                            unreachable!()
                        };
                        match opening.finish() {
                            Ok(game) => self.mode = Mode::Game(Box::new(game)),
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
            },
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
            Mode::Menu | Mode::Opening(_) => {}
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
            Mode::Menu | Mode::Opening(_) => {}
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
            Mode::Menu | Mode::Opening(_) => {}
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
            Mode::Menu | Mode::Opening(_) => {}
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
