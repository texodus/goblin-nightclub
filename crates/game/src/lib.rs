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

//! Platform-independent game state. Nothing here touches the browser or GPU.

mod opening;
mod route;
mod shift;
mod sight;
mod stream;
mod ui;

use goblin_nightclub_core::art::draw_bevels;
use std::any::Any;
use std::rc::Rc;

use goblin_nightclub_core::camera::{self, Camera};
use goblin_nightclub_core::chunk::CHUNK;
use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::entity::{Entity, EntityId, Kind, PLAYER_SIZE};
use goblin_nightclub_core::frame::{Frame, LayerFrame};
use goblin_nightclub_core::geom::{distance, length};
use goblin_nightclub_core::input::Input;
use goblin_nightclub_core::layer::{Dir, QUARTERS, Solid, TILE, quarter_at};
use goblin_nightclub_core::level::{Level, MAX_VISIBLE};
use goblin_nightclub_core::level_file::{LevelFile, LevelMeta};
use goblin_nightclub_core::memory::Memory;
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::palette;
use goblin_nightclub_core::path;
use goblin_nightclub_core::prop::draw_prop;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset::{Cell, Tileset};
use goblin_nightclub_core::vision::{Around, Vision};
use goblin_nightclub_core::world::{Build, ChunkSource, FileSource, Window};
use goblin_nightclub_generate::Params;

pub use opening::Opening;
use route::Route;
use shift::{LAYER_SHIFT_TIME, LayerShift};
use sight::{UNDERGROUND_FROM_ABOVE, VIEW_RADIUS, VIEW_RADIUS_MIN, VIEW_RADIUS_STEP};
use stream::Stream;

/// The sheet cells the characters are drawn with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprites {
    pub player: Cell,
    pub npc: Cell,
}

impl Sprites {
    /// The sprites `file` names, from its tileset.
    pub fn from_file(file: &LevelFile) -> Self {
        Self::from_meta(&file.meta())
    }

    /// The sprites a level's `meta` names, from its tileset.
    pub fn from_meta(meta: &LevelMeta) -> Self {
        let cell = |name: &str| {
            meta.tileset
                .sprite(name)
                .unwrap_or_else(|| panic!("no sprite {name:?} in the tileset"))
                .cell
        };
        Self {
            player: cell(&meta.player.sprite),
            npc: cell(&meta.npc.sprite),
        }
    }
}

/// A game on a level, played whole, or a window at a time if it's too big
/// (see [`goblin_nightclub_core::world`]). Then everything here, positions
/// and layers alike, is the window's, but for `ground`.
pub struct Game {
    pub input: Input,
    level: Level,
    /// For a level played a window at a time: where the window is, the
    /// next one, and what's kept between them.
    stream: Option<Stream>,
    /// What's left of the last window's level, to drop a little at a time.
    litter: Vec<Box<dyn Any>>,
    /// Going from one layer to another, as it's shown.
    shift: Option<LayerShift>,
    sprites: Sprites,
    /// The player's layer.
    top: usize,
    /// The layer the view is from, in the whole level, once it's been
    /// moved off the player's (see [`Game::view_top`]); until then, `None`,
    /// and the view follows the player from layer to layer.
    view: Option<usize>,
    /// The ground level: the lowest layer under the open sky (see
    /// [`goblin_nightclub_core::level_file::LevelFile::ground`]), in the
    /// whole level.
    ground: usize,
    /// Per layer: what's visible of it and remembered about it.
    visions: Vec<Vision>,
    memories: Vec<Memory>,
    player: [f32; 2],
    npcs: Vec<Entity>,
    rng: Rng,
    /// Seconds of unpaused play.
    time: f32,
    /// While paused, nothing in the world moves.
    paused: bool,
    /// Where the player is walking, if anywhere.
    route: Option<Route>,
    camera: Camera,
    /// Last pointer position, in view pixels.
    pointer: Option<[f32; 2]>,
    /// What sight was last worked out from: the player's center, layer,
    /// view radius and the ground level. Unchanged, so is what's seen.
    observed: Option<([f32; 2], usize, f32, usize)>,
}

/// Size of a generated level made whole ([`new_world`]: the editor's
/// fresh worlds), in tiles.
const LEVEL_COLS: usize = 96;
const LEVEL_ROWS: usize = 60;

/// Size of a new game's world, in tiles: made a region at a time as it's
/// played (see [`goblin_nightclub_generate::Plan`]), so it starts as soon
/// as the regions around the spawn are made, whatever its size.
pub const NEW_WORLD: [usize; 2] = [4096, 4096];

/// NPCs per layer per 640 tiles of level.
const NPC_DENSITY: usize = 4;
/// The same, per chunk of a level played a window at a time.
const NPC_PER_CHUNK: usize = (NPC_DENSITY * CHUNK * CHUNK).div_ceil(640);
/// NPCs spawn at least this far (pixels) from the player.
const NPC_SPAWN_DIST: f32 = 60.0;

/// Pieces of the last window's level dropped each update (see
/// [`Level::take_apart`]).
const LITTER_PER_UPDATE: usize = 2;

/// A newly generated world, from `seed`: the level a new game is played
/// on, with the player starting in its middle, on the plains.
pub fn new_world(seed: u32, params: &Params) -> LevelFile {
    let (cols, rows) = (LEVEL_COLS, LEVEL_ROWS);
    let spawn = [(cols / 2) as f32 * TILE, (rows / 2) as f32 * TILE];
    goblin_nightclub_generate::world(seed, cols, rows, spawn, params)
}

impl Game {
    /// A game on a level file (generated or saved), the player at its
    /// spawn, drawn with the sprites it names. It starts paused. A level
    /// too big for one window is played a window at a time.
    pub fn from_file(file: &LevelFile, view: [f32; 2], seed: u32) -> Self {
        let [x, y] = file.spawn.pos;
        let sprites = Sprites::from_file(file);
        let (top, size) = (file.spawn.layer, [file.cols, file.rows, file.layers.len()]);
        let window = Window::around(size, tile_of([x, y]), top);
        if window.is_whole(size) {
            let player = [x - PLAYER_SIZE[0] / 2.0, y - PLAYER_SIZE[1] / 2.0];
            let mut game = Self::with_level_at(file.level(), top, player, sprites, view, seed);
            game.ground = file.ground.min(game.level.depth() - 1);
            return game;
        }
        let source: Rc<dyn ChunkSource> = Rc::new(FileSource::new(file));
        Opening::new(source, &file.meta(), view, seed)
            .finish()
            .expect("a level file's spawn fits")
    }

    /// A game on a level of chunks from `source`, `meta` saying the rest:
    /// played a window at a time, or whole if it fits in one. Its chunks
    /// must all be ready (see [`Opening`] for a source that may not be).
    pub fn from_source(
        source: Rc<dyn ChunkSource>,
        meta: &LevelMeta,
        view: [f32; 2],
        seed: u32,
    ) -> Result<Self, String> {
        Opening::new(source, meta, view, seed).finish()
    }

    /// A game on `level`, the player at `player` (top-left, pixels) on
    /// layer `top`, seen through a view of size `view`.
    pub fn with_level_at(
        level: Level,
        top: usize,
        player: [f32; 2],
        sprites: Sprites,
        view: [f32; 2],
        seed: u32,
    ) -> Self {
        let mut rng = Rng::new(seed ^ 0x9E37_79B9);
        let per_layer = (NPC_DENSITY * level.cols() * level.rows()).div_ceil(640);
        let mut npcs = Vec::new();
        for layer in 0..level.depth() {
            let first = npcs.len() as EntityId;
            for mut npc in Entity::spawn_npcs(
                level.movement(layer),
                &mut rng,
                per_layer,
                first,
                center(player, PLAYER_SIZE),
                NPC_SPAWN_DIST,
            ) {
                npc.layer = layer;
                npcs.push(npc);
            }
        }
        Self::assemble(level, top, player, sprites, view, rng, npcs)
    }

    /// A game on `level`, as [`Game::with_level_at`], with `npcs`, its
    /// chances from `rng`.
    fn assemble(
        level: Level,
        top: usize,
        player: [f32; 2],
        sprites: Sprites,
        view: [f32; 2],
        rng: Rng,
        npcs: Vec<Entity>,
    ) -> Self {
        let camera = Camera::new(view, level.tileset().zoom());
        let mut game = Self {
            input: Input::default(),
            visions: (0..level.depth())
                .map(|k| Vision::new(level.layer(k), VIEW_RADIUS))
                .collect(),
            memories: (0..level.depth()).map(|_| Memory::default()).collect(),
            level,
            stream: None,
            litter: Vec::new(),
            shift: None,
            sprites,
            top,
            ground: 0,
            player,
            npcs,
            rng,
            time: 0.0,
            paused: true,
            route: None,
            camera,
            pointer: None,
            observed: None,
            view: None,
        };
        game.center_camera();
        game.observe();
        game
    }

    pub fn update(&mut self, dt: f32) {
        let left = self.litter.len().saturating_sub(LITTER_PER_UPDATE);
        self.litter.truncate(left);
        // Going from one layer to another plays out, paused or not.
        if let Some(shift) = &mut self.shift {
            shift.t += dt / LAYER_SHIFT_TIME;
            if shift.t >= 1.0 {
                self.shift = None;
            }
        }
        // Arrow keys or WASD pan the view, paused or not.
        self.camera.pan(&self.input, dt);
        if self.paused {
            return;
        }
        let before = tile_of(center(self.player, PLAYER_SIZE));
        self.follow_route(dt);
        self.take_stairs(before);
        for npc in &mut self.npcs {
            npc.wander(self.level.movement(npc.layer), &mut self.rng, dt);
        }
        self.time += dt;
        self.observe();
        self.check_route();
        self.advance_stream();
    }

    /// The player's layer, in the whole level.
    pub fn layer(&self) -> usize {
        self.stream.as_ref().map_or(0, |s| s.window.base) + self.top
    }

    /// The art the level is drawn with.
    pub fn tileset(&self) -> &Tileset {
        self.level.tileset()
    }

    /// A key just went down (`KeyboardEvent.code`): Space toggles pause,
    /// Escape pauses, `=` and `-` move the view a layer up and down (the
    /// player staying where they are, and the view following them again
    /// once it's back on theirs), C centers the camera on the player, on
    /// their layer. Held keys pan the camera.
    pub fn key_pressed(&mut self, code: &str) {
        match code {
            "Space" => self.paused = !self.paused,
            "Escape" => self.paused = true,
            "Equal" => self.move_view(1),
            "Minus" => self.move_view(-1),
            "KeyC" => {
                self.center_camera();
                self.set_view(None);
            }
            _ => {}
        }
    }

    /// The pointer moved to `p` (view pixels).
    pub fn pointer(&mut self, p: [f32; 2]) {
        self.pointer = Some(p);
    }

    /// A click at `p` (view pixels): the pause button toggles pause;
    /// anywhere else, walk there.
    pub fn click(&mut self, p: [f32; 2]) {
        if self.on_button(p) {
            self.paused = !self.paused;
            return;
        }
        self.walk_to(self.on_players_layer(p));
    }

    /// The point of the player's layer drawn at `p` (view pixels): drawn
    /// smaller about the view's center the further below the view's layer
    /// it is (parallax), or larger above it, as if it were drawn.
    fn on_players_layer(&self, p: [f32; 2]) -> [f32; 2] {
        let shown = self.shown();
        let below = shown.top as f32 - self.top as f32 + shown.sink;
        if below == 0.0 {
            return self.to_world(p);
        }
        let scale = self.parallax().powf(below);
        let c = self.camera.view.map(|v| v / 2.0);
        self.to_world([0, 1].map(|a| c[a] + (p[a] - c[a]) / scale))
    }

    /// How much smaller each layer is drawn than the one above it (see
    /// [`camera::parallax`]).
    fn parallax(&self) -> f32 {
        camera::parallax(self.zoom(), self.tileset().zoom() * self.ui_scale())
    }

    /// Screen pixels per interface pixel; see [`Camera::ui_scale`].
    pub fn ui_scale(&self) -> f32 {
        self.camera.ui_scale()
    }

    /// Screen pixels per world unit.
    pub fn zoom(&self) -> f32 {
        self.camera.zoom()
    }

    /// Zoom in (`steps > 0`) or out by that many wheel notches about `p`
    /// (view pixels).
    pub fn zoom_by(&mut self, steps: i32, p: [f32; 2]) {
        self.camera.zoom_by(steps, p);
    }

    /// The view is now `view` screen pixels.
    pub fn resize(&mut self, view: [f32; 2]) {
        self.camera.resize(view);
    }

    fn to_world(&self, p: [f32; 2]) -> [f32; 2] {
        self.camera.to_world(p)
    }

    #[cfg(test)]
    fn view_world(&self) -> [f32; 2] {
        self.camera.view_world()
    }

    /// Size of the level, in world units.
    fn world(&self) -> [f32; 2] {
        [
            self.level.cols() as f32 * TILE,
            self.level.rows() as f32 * TILE,
        ]
    }

    fn center_camera(&mut self) {
        self.camera.center_on(center(self.player, PLAYER_SIZE));
    }

    /// Everything to draw this frame: the view's layer and each one
    /// showing through below it, and the layers the player sees that those
    /// aren't (looking down from one the view's above, or moved off it), for
    /// what they see of them to be remembered. Each layer is drawn in full
    /// and the renderer shows it only where the matching visibility
    /// applies, so live NPCs go in `visible` and last-seen markers in
    /// `remembered`.
    pub fn frame(&self) -> Frame {
        let tileset = self.tileset();
        let bevel = 1.0 / tileset.zoom();
        let parallax = self.parallax();
        let shown = self.shown();
        let top = shown.top;
        let mut seen = self.level.visible_layers(top);
        if let Some(k) = shown.beneath.filter(|k| !seen.contains(k)) {
            seen.insert(1, k);
            seen.truncate(MAX_VISIBLE);
        }
        // The layers the player sees, from theirs, and whether what's in
        // sight on layer `k` is drawn as such. Following the player, it's
        // drawn as last seen while the view goes from one layer to another.
        // Moved off them, only on those they see, and only from above:
        // looking up at them, nothing covers what's below the player's
        // layer but its own sight, so nothing's in sight there.
        let sight = self.level.visible_layers(self.top);
        let in_sight = |k: usize| self.view.is_none() || top >= self.top && sight.contains(&k);
        // Layer `k`, `drawn` as one of those shown, or else only what's in
        // sight there.
        let layer_frame = |k: usize, drawn: bool| {
            let (layer, vision) = (self.level.layer(k), &self.visions[k]);
            let live = !drawn || in_sight(k);
            // Bevels a pixel of the art wide, only on faces seen: now for
            // the visible canvas, ever for the remembered one.
            // Otherwise a lit wall would show the bevel of a face behind
            // it nobody has seen.
            // Characters drawn on this layer, as sprites centered on
            // their boxes: live NPCs where visible, the last sighting of
            // each (a gray ghost, in the remembered style) elsewhere.
            let memory = &self.memories[k];
            let mut visible = Canvas::new(tileset);
            let mut remembered = Canvas::new(tileset);
            let mut standing = Canvas::new(tileset);
            if drawn {
                // The tiles are drawn from the level's meshes; just the
                // bevels in view are drawn here.
                let scale = parallax.powf((top - k) as f32 + shown.sink);
                let within =
                    camera::layer_view(self.camera.pos, self.camera.view, self.zoom(), scale);
                let mut live_sprites: Vec<([f32; 2], Cell)> = self
                    .npcs
                    .iter()
                    .filter(|n| n.layer == k)
                    .map(|n| (n.center(), self.sprite(n.kind)))
                    .collect();
                if k == self.top {
                    live_sprites.push((center(self.player, PLAYER_SIZE), self.sprites.player));
                }
                let stale: Vec<([f32; 2], Cell)> = memory
                    .stale()
                    .map(|(_, m)| (center(m.pos, m.size), self.sprite(m.kind)))
                    .collect();

                if live {
                    let seen_now = |i| vision.edge_visible()[i];
                    draw_bevels(layer, tileset, &mut visible, seen_now, bevel, within);
                }
                for &(at, cell) in &live_sprites {
                    visible.sprite(at, cell, palette::WHITE);
                }
                let seen_ever = |i| vision.edge_seen()[i];
                draw_bevels(layer, tileset, &mut remembered, seen_ever, bevel, within);
                for &(at, cell) in &stale {
                    remembered.sprite(at, cell, palette::WHITE);
                }

                // Characters drawn on this layer cover any void their
                // sprites overhang: the player, NPCs in view and last
                // sightings.
                let in_view = self
                    .npcs
                    .iter()
                    .filter(|n| n.layer == k && memory.is_visible(n.id));
                for n in in_view {
                    standing.sprite(n.center(), self.sprite(n.kind), palette::WHITE);
                }
                if k == self.top {
                    let at = center(self.player, PLAYER_SIZE);
                    standing.sprite(at, self.sprites.player, palette::WHITE);
                }
                for &(at, cell) in &stale {
                    standing.sprite(at, cell, palette::WHITE);
                }
            }

            // The props seen, whole where their art is.
            let mut props_seen = Canvas::new(tileset);
            if live {
                for &[x, y] in vision.props_seen() {
                    draw_prop(layer, tileset, &mut props_seen, x as usize, y as usize);
                }
            }

            // The layer beneath going from one layer to another is
            // shown whole, as the viewer's is.
            let through = drawn && k < top && shown.beneath != Some(k);
            let shaft = through.then(|| self.level.shaft_mesh(top, k));
            // What's in sight is remembered only where it shows through to
            // the player, below them.
            let seen_through = if self.view.is_none() && drawn {
                shaft.clone()
            } else {
                (k < self.top).then(|| self.level.shaft_mesh(self.top, k))
            };
            fn sight_of<T: Clone>(live: bool, v: &[T]) -> Vec<T> {
                if live { v.to_vec() } else { Vec::new() }
            }

            LayerFrame {
                index: k,
                meshes: self.level.meshes(k).clone(),
                props_seen,
                visible,
                remembered,
                polygon: sight_of(live, vision.polygon()),
                lit_shapes: sight_of(live, vision.wall_band()),
                lights: sight_of(live, vision.lights()),
                standing,
                shaft,
                seen_through,
            }
        };
        let layers = seen.iter().map(|&k| layer_frame(k, true)).collect();
        let sighted = sight
            .iter()
            .filter(|&&k| !seen.contains(&k) || !in_sight(k))
            .map(|&k| layer_frame(k, false))
            .collect();

        let vision = &self.visions[self.top];
        Frame {
            top,
            layers,
            sight: self.top,
            sighted,
            origin: vision.origin(),
            radius: vision.radius(),
            radius_step: VIEW_RADIUS_STEP,
            radius_min: VIEW_RADIUS_MIN,
            radius_underground: if self.outdoors() {
                UNDERGROUND_FROM_ABOVE
            } else {
                1.0
            },
            time: self.time,
            camera: self.camera.pos,
            view: self.camera.view,
            zoom: self.zoom(),
            cell: tileset.cell,
            ui_scale: self.ui_scale(),
            outdoors: self.outdoors(),
            ground: self.local_ground(),
            world: self.world(),
            offset: self
                .stream
                .as_ref()
                .map_or([0, 0], |s| s.window.tile_origin()),
            base: self.stream.as_ref().map_or(0, |s| s.window.base),
            map: self.stream.as_ref().map(|s| s.map.clone()),
            remember: true,
            fade: shown.fade,
            sink: shown.sink,
            overlay: self.overlay(),
            ui: self.ui(),
        }
    }
}

impl Game {
    /// The sheet cell drawn for an entity of `kind`.
    fn sprite(&self, kind: Kind) -> Cell {
        match kind {
            Kind::Npc => self.sprites.npc,
        }
    }
}

fn center(pos: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0]
}

fn tile_of(p: [f32; 2]) -> [i32; 2] {
    [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32]
}

#[cfg(test)]
#[path = "game_tests.rs"]
mod tests;
