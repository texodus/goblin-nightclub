//! Platform-independent game state. Nothing here touches the browser or GPU.

mod stream;

use std::any::Any;
use std::rc::Rc;

use goblin_nightclub_core::camera::{self, Camera};
use goblin_nightclub_core::chunk::CHUNK;
use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::entity::{Entity, EntityId, Kind, PLAYER_SIZE};
use goblin_nightclub_core::frame::{Frame, LayerFrame};
pub use goblin_nightclub_core::input::Input;
use goblin_nightclub_core::layer::{Dir, QUARTERS, Solid, TILE, quarter_at};
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::level_file::{LevelFile, LevelMeta};
use goblin_nightclub_core::memory::Memory;
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::palette;
use goblin_nightclub_core::path;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset::{Cell, Tileset};
use goblin_nightclub_core::vision::Vision;
use goblin_nightclub_core::world::{Build, ChunkSource, FileSource, Window};
use goblin_nightclub_generate::Params;

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
    sprites: Sprites,
    /// The player's layer.
    top: usize,
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

/// A planned walk: the tiles it crosses and the straightened waypoints
/// (centers) the player heads for in turn.
struct Route {
    tiles: Vec<[i32; 2]>,
    waypoints: Vec<[f32; 2]>,
    next: usize,
    /// Seconds spent barely moving.
    stalled: f32,
}

const PLAYER_SPEED: f32 = 60.0;

/// Pause on reaching the destination.
const AUTO_PAUSE: bool = true;

/// Spacing of the dots marking the way a walk has left, in pixels.
const ROUTE_DOT: f32 = 4.0;

/// Size of a generated level made whole ([`new_world`]: the editor's
/// fresh worlds, and [`Game::new`]), in tiles.
const LEVEL_COLS: usize = 96;
const LEVEL_ROWS: usize = 60;

/// Size of a new game's world, in tiles: made a region at a time as it's
/// played (see [`goblin_nightclub_generate::Plan`]), so it starts as soon
/// as the regions around the spawn are made, whatever its size.
pub const NEW_WORLD: [usize; 2] = [4096, 4096];

/// How far the player can see, in pixels, indoors: under a floor or wall
/// of some layer above. `f32::INFINITY` is unlimited.
const VIEW_RADIUS: f32 = 96.0;
const VIEW_RADIUS_OUTDOORS_FACTOR: f32 = 5.0;

/// How far outdoors: on the top layer, or with nothing but void (or
/// water, or stairs) directly above on every layer up to it.
const VIEW_RADIUS_OUTDOORS: f32 = VIEW_RADIUS_OUTDOORS_FACTOR * VIEW_RADIUS;

/// Looking down, each layer below the player's is seen this much less far,
/// down to `VIEW_RADIUS_MIN`.
const VIEW_RADIUS_STEP: f32 = 16.0;
const VIEW_RADIUS_MIN: f32 = 2.0 * TILE;

/// From outdoors, layers below the ground level are seen only this
/// fraction as far (after the step per layer).
const UNDERGROUND_FROM_ABOVE: f32 = 1.0 / VIEW_RADIUS_OUTDOORS_FACTOR;

/// View distance for a layer `depth` layers below the player's, from a
/// `base` radius (indoors or out), `underground` if the layer is below
/// the ground level while the player is above it.
fn view_radius(base: f32, depth: usize, underground: bool) -> f32 {
    let radius = (base - VIEW_RADIUS_STEP * depth as f32).max(VIEW_RADIUS_MIN);
    if underground {
        (radius * UNDERGROUND_FROM_ABOVE).max(VIEW_RADIUS_MIN)
    } else {
        radius
    }
}

/// NPCs per layer per 640 tiles of level.
const NPC_DENSITY: usize = 4;
/// The same, per chunk of a level played a window at a time.
const NPC_PER_CHUNK: usize = (NPC_DENSITY * CHUNK * CHUNK).div_ceil(640);
/// NPCs spawn at least this far (pixels) from the player.
const NPC_SPAWN_DIST: f32 = 60.0;

/// Pieces of the last window's level dropped each update (see
/// [`Level::take_apart`]).
const LITTER_PER_UPDATE: usize = 2;

/// The pause button, in interface pixels from the top-right corner.
const BUTTON_SIZE: [f32; 2] = [14.0, 12.0];
const BUTTON_MARGIN: f32 = 3.0;

/// A newly generated world, from `seed`: the level a new game is played
/// on, with the player starting in its middle, on the plains.
pub fn new_world(seed: u32, params: &Params) -> LevelFile {
    let (cols, rows) = (LEVEL_COLS, LEVEL_ROWS);
    let spawn = [(cols / 2) as f32 * TILE, (rows / 2) as f32 * TILE];
    goblin_nightclub_generate::world(seed, cols, rows, spawn, params)
}

impl Game {
    /// A new game on a newly generated world (see [`new_world`]), seen
    /// through a view `width` x `height` pixels.
    pub fn new(width: f32, height: f32, seed: u32, params: &Params) -> Self {
        Self::from_file(&new_world(seed, params), [width, height], seed)
    }

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
        };
        game.center_camera();
        game.observe();
        game
    }

    pub fn update(&mut self, dt: f32) {
        let left = self.litter.len().saturating_sub(LITTER_PER_UPDATE);
        self.litter.truncate(left);
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

    /// What's drawn of the next window, as it's made: to have ready for
    /// when it's swapped in.
    pub fn upcoming(&self) -> impl Iterator<Item = &Mesh> {
        let next = self.stream.iter().flat_map(|s| &s.next);
        next.flat_map(|b| b.meshes())
    }

    /// For a level played a window at a time: once the player has moved
    /// on from the middle of the window, make the next one around them, a
    /// piece each update, and swap it in when it's made.
    fn advance_stream(&mut self) {
        let Some(stream) = &mut self.stream else {
            return;
        };
        let tile = stream.absolute(tile_of(center(self.player, PLAYER_SIZE)));
        let top = stream.window.base + self.top;
        let build = match &mut stream.next {
            Some(build) => build,
            None => {
                let want = Window::around(stream.size(), tile, top);
                if want == stream.window {
                    return;
                }
                stream.next.insert(
                    Build::new(stream.source.clone(), want, top).with_pieces(stream.pieces.clone()),
                )
            }
        };
        if !build.step() {
            return;
        }
        let build = stream.next.take().unwrap();
        // Unless the player's left it already (and the next is due).
        let chunk = tile.map(|t| t.div_euclid(CHUNK as i32));
        if build.window().local((top, chunk[0], chunk[1])).is_some() {
            self.swap(build);
        }
    }

    /// Move the game onto the window `build` makes: everything moves with
    /// the level under it, and what's kept between windows is kept.
    fn swap(&mut self, build: Build) {
        let stream = self.stream.as_mut().unwrap();
        let (old, new) = (stream.window, build.window());
        stream.keep(&self.level, &self.visions);
        stream.sleep(std::mem::take(&mut self.npcs));
        let (level, roofs) = build.finish();
        stream.roofs = roofs;
        let [ox, oy] = old.tile_origin();
        let [nx, ny] = new.tile_origin();
        let tiles = [ox - nx, oy - ny];
        let by = tiles.map(|t| t as f32 * TILE);
        let shift = |p: [f32; 2]| [p[0] + by[0], p[1] + by[1]];
        self.player = shift(self.player);
        self.camera.pos = shift(self.camera.pos);
        self.top = old.base + self.top - new.base;
        if let Some(route) = &mut self.route {
            for t in &mut route.tiles {
                *t = [t[0] + tiles[0], t[1] + tiles[1]];
            }
            for w in &mut route.waypoints {
                *w = shift(*w);
            }
        }
        // Each layer's memories go with it, if it's still in the window.
        let size = [new.cols as f32 * TILE, new.rows as f32 * TILE];
        let mut memories: Vec<Memory> = (0..new.depth).map(|_| Memory::default()).collect();
        for (k, mut memory) in std::mem::take(&mut self.memories).into_iter().enumerate() {
            let at = (old.base + k)
                .checked_sub(new.base)
                .filter(|&k| k < new.depth);
            if let Some(k) = at {
                memory.shift(by, [[0.0; 2], size]);
                memories[k] = memory;
            }
        }
        self.memories = memories;
        stream.window = new;
        self.visions = stream.visions(&level, VIEW_RADIUS);
        let avoid = center(self.player, PLAYER_SIZE);
        self.npcs = stream.wake(&level, NPC_PER_CHUNK, avoid, NPC_SPAWN_DIST);
        let old = std::mem::replace(&mut self.level, level);
        self.litter.extend(old.take_apart());
        self.observed = None;
        self.observe();
    }

    /// The ground level, as a layer of the game's level: 0 if it's below
    /// the window, its depth if it's above.
    fn local_ground(&self) -> usize {
        let base = self.stream.as_ref().map_or(0, |s| s.window.base);
        self.ground.saturating_sub(base).min(self.level.depth())
    }

    /// The player's layer, in the whole level.
    pub fn layer(&self) -> usize {
        self.stream.as_ref().map_or(0, |s| s.window.base) + self.top
    }

    /// The art the level is drawn with.
    pub fn tileset(&self) -> &Tileset {
        self.level.tileset()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// A key just went down (`KeyboardEvent.code`): Space toggles pause,
    /// Escape pauses, C centers the camera on the player. Held keys pan the
    /// camera.
    pub fn key_pressed(&mut self, code: &str) {
        match code {
            "Space" => self.paused = !self.paused,
            "Escape" => self.paused = true,
            "KeyC" => self.center_camera(),
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
        self.walk_to(self.to_world(p));
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

    /// Plan a walk to world point `p` on the player's layer and set off.
    /// Plans on what the player knows: unexplored tiles are assumed
    /// walkable. A click on known void is ignored (no clicking through
    /// holes, or into water). Where the player's box fits at `p`, the walk
    /// ends exactly there, by the straightest way; where it doesn't (a
    /// wall, the edge of a 45-degree tile) it ends as near as the box
    /// gets.
    fn walk_to(&mut self, p: [f32; 2]) {
        let goal = tile_of(p);
        let layer = self.level.layer(self.top);
        if !layer.in_bounds(goal) {
            return;
        }
        let known = self.visions[self.top].is_explored(goal[0] as usize, goal[1] as usize);
        if known && layer.is_void_at(p) {
            return;
        }
        let from = center(self.player, PLAYER_SIZE);
        let start = tile_of(from);
        // The tile the player stands on is fine (even if it's stairs).
        let ground = |t: [i32; 2]| {
            if t == start {
                Solid::None
            } else {
                self.known_solid(t, goal)
            }
        };
        // A tile to route to: the nearest reachable one from which `p` can
        // be reached straight (if the box fits at `p` at all), else just
        // the nearest.
        let fits = path::fits(&ground, p, PLAYER_SIZE);
        let candidates = path::reachable(ground, start, p);
        let straight = |t: &[i32; 2]| path::clear(&ground, path::center(*t), p, PLAYER_SIZE);
        let target = fits
            .then(|| candidates.iter().take(12).copied().find(straight))
            .flatten()
            .or(candidates.first().copied());
        let Some(target) = target else {
            return;
        };
        let Some(tiles) = path::find(ground, start, target) else {
            return;
        };
        // The way: tile centers, then the point itself, or as far toward it
        // as the box gets; straightened so the point is approached directly
        // rather than via the last center.
        let mut points: Vec<[f32; 2]> = tiles.iter().skip(1).map(|t| path::center(*t)).collect();
        let last = points.last().copied().unwrap_or(from);
        if fits && path::clear(&ground, last, p, PLAYER_SIZE) {
            points.push(p);
        } else if let Some(stop) = path::approach(&ground, last, p, PLAYER_SIZE) {
            points.push(stop);
        }
        let waypoints = path::smooth(ground, from, &points, PLAYER_SIZE);
        if waypoints.is_empty() {
            return;
        }
        self.route = Some(Route {
            tiles,
            waypoints,
            next: 0,
            stalled: 0.0,
        });
        self.paused = false;
    }

    /// What a route on the player's layer knows is solid in tile `t`:
    /// nothing where unexplored (assumed walkable), everything outside the
    /// level and on stairs (except as the `goal`, so a walk never changes
    /// layer on the way), else the movement grid's shape, so a 45-degree
    /// tile is crossed only where it's open.
    fn known_solid(&self, t: [i32; 2], goal: [i32; 2]) -> Solid {
        let layer = self.level.layer(self.top);
        if !layer.in_bounds(t) {
            return Solid::Full;
        }
        if layer.tile(t[0], t[1]).stairs().is_some() {
            return if t == goal { Solid::None } else { Solid::Full };
        }
        if !self.visions[self.top].is_explored(t[0] as usize, t[1] as usize) {
            return Solid::None;
        }
        self.level.movement(self.top).solid(t[0], t[1])
    }

    /// Head for the next waypoint, sliding along anything in the way.
    fn follow_route(&mut self, dt: f32) {
        let Some(route) = &mut self.route else {
            return;
        };
        let here = center(self.player, PLAYER_SIZE);
        let target = route.waypoints[route.next];
        let to = [target[0] - here[0], target[1] - here[1]];
        let dist = goblin_nightclub_core::length(to);
        let step = (PLAYER_SPEED * dt).min(dist);
        if dist > 1e-3 {
            let delta = [to[0] / dist * step, to[1] / dist * step];
            self.player = self
                .level
                .movement(self.top)
                .move_box(self.player, PLAYER_SIZE, delta);
        }
        let now = center(self.player, PLAYER_SIZE);
        let moved = goblin_nightclub_core::length([now[0] - here[0], now[1] - here[1]]);
        route.stalled = if moved < 0.2 * step {
            route.stalled + dt
        } else {
            0.0
        };
        if goblin_nightclub_core::length([target[0] - now[0], target[1] - now[1]]) < 0.5 {
            // Arrived: on the point exactly, since the leg was swept clear.
            let exact = [
                target[0] - PLAYER_SIZE[0] / 2.0,
                target[1] - PLAYER_SIZE[1] / 2.0,
            ];
            if !self.level.movement(self.top).collides(exact, PLAYER_SIZE) {
                self.player = exact;
            }
            route.next += 1;
        }
    }

    /// After moving and looking around: stop and pause on arriving (if
    /// `AUTO_PAUSE`), and always when the route runs into something just
    /// revealed or the player is stuck. NPCs coming into view don't stop
    /// the walk.
    fn check_route(&mut self) {
        let Some(route) = &self.route else {
            return;
        };
        if route.next >= route.waypoints.len() {
            self.route = None;
            self.paused = AUTO_PAUSE;
            return;
        }
        // Sweep the rest of the walk as the player will actually travel
        // it, straight between waypoints, against what's now known. The
        // tiles the player is on right now are fine.
        let goal = *route.tiles.last().unwrap();
        let at = center(self.player, PLAYER_SIZE);
        let here = path::tiles_under(at, PLAYER_SIZE);
        let ground = |t: [i32; 2]| {
            if here.contains(&t) {
                Solid::None
            } else {
                self.known_solid(t, goal)
            }
        };
        let stops = std::iter::once(at).chain(route.waypoints[route.next..].iter().copied());
        let legs: Vec<[f32; 2]> = stops.collect();
        let blocked = legs
            .windows(2)
            .any(|w| !path::clear(&ground, w[0], w[1], PLAYER_SIZE));
        if blocked || route.stalled > 0.3 {
            self.route = None;
            self.paused = true;
        }
    }

    /// Stepping onto stairs (from another tile) takes the player to the
    /// layer they lead to, at the same spot. Arriving on the stairs at the
    /// other end isn't stepping onto them, so there's no bouncing back.
    fn take_stairs(&mut self, before: [i32; 2]) {
        let here = tile_of(center(self.player, PLAYER_SIZE));
        if here == before {
            return;
        }
        match self.level.layer(self.top).tile(here[0], here[1]).stairs() {
            Some(Dir::Down) if self.top > 0 => self.top -= 1,
            Some(Dir::Up) if self.top + 1 < self.level.depth() => self.top += 1,
            _ => {}
        }
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

    /// The pause button's top-left, in interface pixels.
    fn button_pos(&self) -> [f32; 2] {
        let width = self.camera.view[0] / self.ui_scale();
        [width - BUTTON_SIZE[0] - BUTTON_MARGIN, BUTTON_MARGIN]
    }

    /// Is view pixel `p` on the pause button?
    fn on_button(&self, p: [f32; 2]) -> bool {
        let p = p.map(|v| v / self.ui_scale());
        let b = self.button_pos();
        p[0] >= b[0] && p[1] >= b[1] && p[0] < b[0] + BUTTON_SIZE[0] && p[1] < b[1] + BUTTON_SIZE[1]
    }

    /// World-space marks over everything: the route being walked, dotted,
    /// and its destination, in lines a pixel of the art wide.
    fn overlay(&self) -> Canvas {
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
        let length =
            |a: [f32; 2], b: [f32; 2]| goblin_nightclub_core::length([b[0] - a[0], b[1] - a[1]]);
        let total: f32 = way.windows(2).map(|s| length(s[0], s[1])).sum();
        // How far back from the goal the leg ends, and the next dot is.
        let (mut done, mut dot) = (0.0, ROUTE_DOT);
        for leg in way.windows(2).rev() {
            let [near, far] = [leg[0], leg[1]];
            let len = length(near, far);
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
    fn ui(&self) -> Canvas {
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

    /// Is the player under the open sky: on a layer at or above the ground
    /// level, and on the top layer or under void (or water) or stairs on
    /// every layer above theirs? Stairs don't count as a roof (standing on
    /// a ramp's foot, its top is right overhead), but floors and walls do.
    /// Underground, even a shaft of daylight from above doesn't count.
    pub fn outdoors(&self) -> bool {
        let c = center(self.player, PLAYER_SIZE);
        if self.top < self.local_ground() {
            return false;
        }
        // Played a window at a time, the layers above it count too.
        if let Some(stream) = &self.stream {
            let (tile, quarter) = quarter_at(c);
            let q = QUARTERS.iter().position(|&b| b == quarter).unwrap();
            let roof = stream.roofs.at(stream.absolute(tile))[q];
            return roof as usize <= stream.window.base + self.top + 1;
        }
        let [x, y] = tile_of(c);
        let open = |k: usize| {
            let layer = self.level.layer(k);
            layer.is_void_at(c) || layer.tile(x, y).stairs().is_some()
        };
        (self.top + 1..self.level.depth()).all(open)
    }

    /// Recompute what the player sees and remembers, on their layer and on
    /// every layer showing through below it. The view radius depends on
    /// being outdoors or in, and shrinks for each layer looked down to.
    fn observe(&mut self) {
        let origin = center(self.player, PLAYER_SIZE);
        let base = if self.outdoors() {
            VIEW_RADIUS_OUTDOORS
        } else {
            VIEW_RADIUS
        };
        // Others moving doesn't change what's seen, only the player does.
        let key = Some((origin, self.top, base, self.local_ground()));
        let moved = self.observed != key;
        self.observed = key;
        let visible = self.level.visible_layers(self.top);
        for k in 0..self.level.depth() {
            if !visible.contains(&k) {
                self.memories[k].clear_visible();
                continue;
            }
            // Looking down, the layer's own walls don't block the view, so
            // its floors and wall tops show alike.
            let view = self.level.view(self.top, k);
            if moved {
                let shaft = (k < self.top).then(|| self.level.shaft(self.top, k));
                let above = (k < self.top).then_some(&*view);
                let underground = self.outdoors() && k < self.local_ground();
                self.visions[k].set_radius(view_radius(base, self.top - k, underground));
                let own = self.level.layer(k);
                self.visions[k].update_through(&view, own, shaft.as_deref(), above, origin);
            }
            let here = self.npcs.iter().filter(|n| n.layer == k);
            self.memories[k].update(&view, &self.visions[k], here, self.time);
        }
    }

    /// Everything to draw this frame: the player's layer and each one
    /// showing through below it. Each layer is drawn in full and the
    /// renderer shows it only where the matching visibility applies, so live
    /// NPCs go in `visible` and last-seen markers in `remembered`.
    pub fn frame(&self) -> Frame {
        let tileset = self.tileset();
        let bevel = 1.0 / tileset.zoom();
        let parallax = camera::parallax(self.zoom(), tileset.zoom() * self.ui_scale());
        let layers = self
            .level
            .visible_layers(self.top)
            .iter()
            .map(|&k| {
                let (layer, vision) = (self.level.layer(k), &self.visions[k]);
                // The tiles are drawn from the level's meshes; just the
                // bevels in view are drawn here.
                let scale = parallax.powi((self.top - k) as i32);
                let within =
                    camera::layer_view(self.camera.pos, self.camera.view, self.zoom(), scale);
                // Bevels a pixel of the art wide, only on faces seen: now for
                // the visible canvas, ever for the remembered one.
                // Otherwise a lit wall would show the bevel of a face behind
                // it nobody has seen.
                // Characters drawn on this layer, as sprites centered on
                // their boxes: live NPCs where visible, the last sighting of
                // each (a gray ghost, in the remembered style) elsewhere.
                let memory = &self.memories[k];
                let mut live: Vec<([f32; 2], Cell)> = self
                    .npcs
                    .iter()
                    .filter(|n| n.layer == k)
                    .map(|n| (n.center(), self.sprite(n.kind)))
                    .collect();
                if k == self.top {
                    live.push((center(self.player, PLAYER_SIZE), self.sprites.player));
                }
                let stale: Vec<([f32; 2], Cell)> = memory
                    .stale()
                    .map(|(_, m)| (center(m.pos, m.size), self.sprite(m.kind)))
                    .collect();

                let mut visible = Canvas::new(tileset);
                let seen_now = |i| vision.edge_visible()[i];
                layer.draw_bevels(tileset, &mut visible, seen_now, bevel, within);
                for &(at, cell) in &live {
                    visible.sprite(at, cell, palette::WHITE);
                }
                let mut remembered = Canvas::new(tileset);
                let seen_ever = |i| vision.edge_seen()[i];
                layer.draw_bevels(tileset, &mut remembered, seen_ever, bevel, within);
                for &(at, cell) in &stale {
                    remembered.sprite(at, cell, palette::WHITE);
                }

                let meshes = self.level.meshes(k);
                let shaft = (k < self.top).then(|| self.level.shaft_mesh(self.top, k));
                // Characters drawn on this layer cover any void their
                // sprites overhang: the player, NPCs in view and last
                // sightings.
                let mut standing = Canvas::new(tileset);
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

                LayerFrame {
                    index: k,
                    meshes: meshes.clone(),
                    visible,
                    remembered,
                    unknown: Canvas::new(tileset),
                    polygon: vision.polygon().to_vec(),
                    lit_shapes: vision.wall_band().to_vec(),
                    standing,
                    shaft,
                }
            })
            .collect();

        let vision = &self.visions[self.top];
        Frame {
            depth: self.level.depth(),
            top: self.top,
            layers,
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
            remember: true,
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

/// A game on a level of chunks, starting: its first window being made, a
/// piece at a time, so a slow source (a server) needn't hold anything up.
pub struct Opening {
    build: Build,
    source: Rc<dyn ChunkSource>,
    meta: LevelMeta,
    view: [f32; 2],
    seed: u32,
}

impl Opening {
    /// Start a game on a level of chunks from `source`, `meta` saying the
    /// rest, seen through a view of size `view`.
    pub fn new(source: Rc<dyn ChunkSource>, meta: &LevelMeta, view: [f32; 2], seed: u32) -> Self {
        let size = [meta.cols, meta.rows, meta.depth];
        let window = Window::around(size, tile_of(meta.spawn.pos), meta.spawn.layer);
        Self {
            build: Build::new(source.clone(), window, meta.spawn.layer),
            source,
            meta: meta.clone(),
            view,
            seed,
        }
    }

    /// Make a piece more of the first window: whether it's all made.
    pub fn step(&mut self) -> bool {
        self.build.step()
    }

    /// The art the level is drawn with.
    pub fn tileset(&self) -> &Tileset {
        &self.meta.tileset
    }

    /// The game: once [`Opening::step`] says all's made, or right away for
    /// a source with every chunk ready. A level played whole is as if from
    /// a level file. Fails if the player doesn't fit at the spawn.
    pub fn finish(self) -> Result<Game, String> {
        let meta = &self.meta;
        let size = [meta.cols, meta.rows, meta.depth];
        let (window, pieces) = (self.build.window(), self.build.pieces());
        let (level, roofs) = self.build.finish();
        let o = window.tile_origin().map(|t| t as f32 * TILE);
        let [x, y] = meta.spawn.pos;
        let player = [
            x - o[0] - PLAYER_SIZE[0] / 2.0,
            y - o[1] - PLAYER_SIZE[1] / 2.0,
        ];
        let top = meta.spawn.layer - window.base;
        if level.movement(top).collides(player, PLAYER_SIZE) {
            return Err("the player doesn't fit at the spawn".into());
        }
        let sprites = Sprites::from_meta(meta);
        if window.is_whole(size) {
            let mut game = Game::with_level_at(level, top, player, sprites, self.view, self.seed);
            game.ground = meta.ground.min(size[2] - 1);
            return Ok(game);
        }
        let mut stream = Stream::new(self.source, window, roofs, pieces, self.seed);
        let avoid = center(player, PLAYER_SIZE);
        let npcs = stream.wake(&level, NPC_PER_CHUNK, avoid, NPC_SPAWN_DIST);
        let rng = Rng::new(self.seed ^ 0x9E37_79B9);
        let mut game = Game::assemble(level, top, player, sprites, self.view, rng, npcs);
        game.stream = Some(stream);
        game.ground = meta.ground.min(size[2] - 1);
        game.observed = None;
        game.observe();
        Ok(game)
    }
}

fn center(pos: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0]
}

fn tile_of(p: [f32; 2]) -> [i32; 2] {
    [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32]
}

#[cfg(test)]
mod tests {
    use super::*;
    use goblin_nightclub_core::layer::Layer;
    use goblin_nightclub_core::layer::Tile;
    use goblin_nightclub_core::level_file::Spawn;
    use std::sync::Arc;

    const VIEW: [f32; 2] = [320.0, 200.0];
    /// The test tileset's default zoom: 16-pixel cells over 10-unit tiles.
    const ZOOM: f32 = 1.6;

    fn tileset() -> Tileset {
        goblin_nightclub_core::tileset::testing::tileset()
    }

    fn sprites() -> Sprites {
        let t = tileset();
        Sprites {
            player: t.sprite("player").unwrap().cell,
            npc: t.sprite("npc").unwrap().cell,
        }
    }

    fn level(layers: &[&[&str]]) -> Level {
        Level::new(
            layers.iter().map(|rows| Layer::from_ascii(rows)).collect(),
            tileset(),
        )
    }

    /// A game on `level` with no NPCs, the player's box centered in tile
    /// `at`.
    fn game(level: Level, at: [i32; 2]) -> Game {
        let top = level.depth() - 1;
        game_on(level, top, at)
    }

    /// The same, on layer `k`.
    fn game_on(level: Level, k: usize, at: [i32; 2]) -> Game {
        let c = path::center(at);
        let mut game = Game::with_level_at(level, k, [c[0] - 4.0, c[1] - 4.0], sprites(), VIEW, 1);
        game.npcs.clear();
        game
    }

    /// Click world point `p`, through the camera.
    fn click_world(game: &mut Game, p: [f32; 2]) {
        game.click([
            (p[0] - game.camera.pos[0]) * game.zoom(),
            (p[1] - game.camera.pos[1]) * game.zoom(),
        ]);
    }

    /// Click tile `t` and play until paused, up to `max` frames.
    fn click_and_run(game: &mut Game, t: [i32; 2], max: usize) {
        click_world(game, path::center(t));
        for _ in 0..max {
            if game.paused {
                break;
            }
            game.update(1.0 / 60.0);
        }
    }

    fn player_tile(game: &Game) -> [i32; 2] {
        tile_of(center(game.player, PLAYER_SIZE))
    }

    /// A level too big for one window, `depth` layers of floor: here and
    /// there walls (away from row `ROW`, kept clear to walk along), holes
    /// in the layers above the spawn's (three from the top, so mostly
    /// indoors, but not everywhere), and stairs down two tiles east of the
    /// spawn on every layer but the bottom.
    fn big_file(cols: usize, rows: usize, depth: usize) -> LevelFile {
        const ROW: usize = 60;
        let spawn = [20, ROW as i32];
        let top = depth - 3;
        let layers: Vec<Vec<Tile>> = (0..depth)
            .map(|k| {
                (0..cols * rows)
                    .map(|i| {
                        let (x, y) = (i % cols, i / cols);
                        let clear = y.abs_diff(ROW) <= 2;
                        if k > 0 && [x as i32, y as i32] == [spawn[0] + 2, spawn[1]] {
                            Tile::Stairs(Dir::Down, 0)
                        } else if !clear && x % 37 == 0 && y % 5 == 0 {
                            Tile::WALL
                        } else if k > top && (x / 5 + y / 3) % 4 == 0 {
                            Tile::VOID
                        } else {
                            Tile::FLOOR
                        }
                    })
                    .collect()
            })
            .collect();
        LevelFile::new(
            cols,
            rows,
            &layers,
            Spawn::at_tile(top, spawn),
            tileset(),
            "player",
            "npc",
        )
    }

    /// Where the player's center is in the whole level, and their layer.
    fn whereabouts(game: &Game) -> ([f32; 2], usize) {
        let s = game.stream.as_ref().unwrap();
        let o = s.window.tile_origin();
        let c = center(game.player, PLAYER_SIZE);
        (
            [c[0] + o[0] as f32 * TILE, c[1] + o[1] as f32 * TILE],
            game.layer(),
        )
    }

    /// Walk to `p` (a point in the whole level) on the player's layer, and
    /// on until paused, checking the player moves smoothly, windows and
    /// all, and `each` frame whatever else: how many times the window
    /// moved.
    fn walk_far(game: &mut Game, p: [f32; 2], each: &mut dyn FnMut(&Game)) -> usize {
        let o = game.stream.as_ref().unwrap().window.tile_origin();
        game.walk_to([p[0] - o[0] as f32 * TILE, p[1] - o[1] as f32 * TILE]);
        let (mut was, mut moves) = (whereabouts(game).0, 0);
        let mut window = game.stream.as_ref().unwrap().window;
        for _ in 0..4000 {
            if game.paused {
                break;
            }
            game.update(1.0 / 60.0);
            let (now, _) = whereabouts(game);
            let step = goblin_nightclub_core::length([now[0] - was[0], now[1] - was[1]]);
            assert!(
                step <= PLAYER_SPEED / 60.0 + 0.6,
                "jumped {step} to {now:?}"
            );
            was = now;
            let w = game.stream.as_ref().unwrap().window;
            if w != window {
                moves += 1;
                window = w;
            }
            each(game);
        }
        assert!(game.paused, "arrived");
        moves
    }

    #[test]
    fn packed_levels_play_as_their_files_do() {
        use goblin_nightclub_core::packed::{Packed, pack_file};
        for file in [big_file(300, 120, 12), new_world(3, &Params::default())] {
            let packed = Packed::open(Arc::<[u8]>::from(pack_file(&file)))
                .unwrap()
                .unwrap();
            let meta = packed.header().level.clone();
            let from_pack = Game::from_source(Rc::new(packed), &meta, VIEW, 1).unwrap();
            let from_file = Game::from_file(&file, VIEW, 1);
            assert_eq!(from_pack.stream.is_some(), from_file.stream.is_some());
            assert_eq!(
                (from_pack.player, from_pack.top),
                (from_file.player, from_file.top)
            );
            assert_eq!(from_pack.npcs.len(), from_file.npcs.len());
            let at = |g: &Game| g.npcs.iter().map(|n| n.pos).collect::<Vec<_>>();
            assert_eq!(at(&from_pack), at(&from_file), "the same NPCs");
            assert_eq!(from_pack.outdoors(), from_file.outdoors());
        }
    }

    #[test]
    fn openings_wait_for_a_slow_source() {
        use goblin_nightclub_core::chunk::{Chunk, ChunkKey};
        use goblin_nightclub_core::world::Roof;
        /// Each chunk and roof ready from the second time it's asked for.
        struct Slow(
            FileSource,
            std::sync::Mutex<std::collections::HashSet<(usize, i32, i32)>>,
        );
        impl Slow {
            fn asked(&self, key: (usize, i32, i32)) -> bool {
                !self.1.lock().unwrap().insert(key)
            }
        }
        impl ChunkSource for Slow {
            fn size(&self) -> [usize; 3] {
                self.0.size()
            }
            fn tileset(&self) -> &Tileset {
                self.0.tileset()
            }
            fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
                self.asked(key).then(|| self.0.chunk(key)).flatten()
            }
            fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
                self.asked((usize::MAX, cx, cy))
                    .then(|| self.0.roof(cx, cy))
                    .flatten()
            }
        }
        let file = big_file(300, 120, 12);
        let slow = Rc::new(Slow(FileSource::new(&file), Default::default()));
        let mut opening = Opening::new(slow, &file.meta(), VIEW, 1);
        let mut steps = 1;
        while !opening.step() {
            steps += 1;
        }
        assert!(steps > 12, "waited, a layer at a time: {steps}");
        let game = opening.finish().unwrap();
        assert_eq!(game.player, Game::from_file(&file, VIEW, 1).player);
    }

    #[test]
    fn generated_worlds_start_from_their_regions() {
        use goblin_nightclub_generate::{Generated, Plan, generated_meta};
        // Small enough to play whole, but made a region at a time.
        let params = Params {
            underworld_layers: 3,
            ..Params::default()
        };
        let plan = Plan::new(4, 192, 128, &params).with_region(64);
        let meta = generated_meta(&plan);
        let game = Game::from_source(Rc::new(Generated::new(plan)), &meta, VIEW, 4).unwrap();
        assert!(game.stream.is_none());
        assert_eq!(game.layer(), meta.spawn.layer);
        assert!(game.outdoors(), "the spawn's on the plains");
        assert!(!game.frame().layers.is_empty());
    }

    #[test]
    fn small_levels_are_played_whole() {
        let params = Params {
            underworld_layers: 3,
            ..Params::default()
        };
        let file = new_world(3, &params);
        assert!(Game::from_file(&file, VIEW, 3).stream.is_none());
    }

    #[test]
    fn big_levels_are_played_a_window_at_a_time() {
        let file = big_file(300, 120, 12);
        let whole = file.level();
        let mut game = Game::from_file(&file, VIEW, 1);
        let start = whereabouts(&game).0;
        let w = game.stream.as_ref().expect("too big to play whole").window;
        assert!(w.cols < 300 && w.depth < 12);
        // The ground level is the bottom: outdoors only under holes all the
        // way up, as the whole level has it.
        let outdoors = |game: &Game| {
            let (p, top) = whereabouts(game);
            let [x, y] = tile_of(p);
            let open = |k: usize| {
                let layer = whole.layer(k);
                layer.is_void_at(p) || layer.tile(x, y).stairs().is_some()
            };
            (top + 1..12).all(open)
        };
        assert_eq!(game.outdoors(), outdoors(&game));
        // East along the clear row, out of sight of the start.
        let mut moves = 0;
        let mut was_outdoors = std::collections::HashSet::new();
        let mut check = |game: &Game| {
            assert_eq!(
                game.outdoors(),
                outdoors(game),
                "at {:?}",
                whereabouts(game)
            );
            was_outdoors.insert(game.outdoors());
        };
        for leg in 1..=4 {
            moves += walk_far(
                &mut game,
                [start[0] + leg as f32 * 450.0, start[1]],
                &mut check,
            );
            let ids: std::collections::HashSet<_> = game.npcs.iter().map(|n| n.id).collect();
            assert_eq!(ids.len(), game.npcs.len(), "NPCs are each awake once");
            assert!(game.npcs.iter().all(|n| n.layer < game.level.depth()));
        }
        assert!(moves >= 2, "the window moved on: {moves}");
        assert_eq!(was_outdoors.len(), 2, "in and out");
        assert!(whereabouts(&game).0[0] > start[0] + 1700.0);
        // Back to the start: what was explored there still is.
        let mut back = 0;
        for leg in (0..4).rev() {
            back += walk_far(
                &mut game,
                [start[0] + leg as f32 * 450.0, start[1]],
                &mut |_| {},
            );
        }
        assert!(back >= 2);
        let s = game.stream.as_ref().unwrap();
        let o = s.window.tile_origin();
        let k = game.top;
        let explored = |dx: i32| {
            let t = [tile_of(start)[0] + dx - o[0], tile_of(start)[1] - o[1]];
            game.visions[k].is_explored(t[0] as usize, t[1] as usize)
        };
        assert!((-3..=3).all(explored), "around the start");
    }

    #[test]
    fn going_down_moves_the_window_down() {
        let file = big_file(300, 120, 14);
        let mut game = Game::from_file(&file, VIEW, 1);
        let (start, top) = whereabouts(&game);
        assert_eq!(top, 11);
        let base = game.stream.as_ref().unwrap().window.base;
        // Onto the stairs, off them and back on, layer after layer.
        for down in 1..=6 {
            walk_far(&mut game, [start[0] + 2.0 * TILE, start[1]], &mut |_| {});
            assert_eq!(whereabouts(&game).1, 11 - down);
            walk_far(&mut game, [start[0] + TILE, start[1]], &mut |_| {});
        }
        let s = game.stream.as_ref().unwrap();
        assert!(s.window.base < base, "{} from {base}", s.window.base);
        assert!(game.top < game.level.depth());
        assert_eq!(game.layer(), s.window.base + game.top);
    }

    #[test]
    fn clicking_walks_there_and_pauses() {
        let l = level(&[&["..........", "....#.....", "....#.....", ".........."]]);
        let mut game = game(l, [1, 2]);
        assert!(game.paused, "starts paused");
        click_and_run(&mut game, [8, 2], 600);
        assert!(game.paused);
        assert_eq!(player_tile(&game), [8, 2]);
        assert!(game.route.is_none());
    }

    #[test]
    fn pause_button_and_freezing() {
        let l = level(&[&["..........", ".........."]]);
        let mut game = game(l, [1, 1]);
        let b = game.button_pos();
        game.click([b[0] + 2.0, b[1] + 2.0]);
        assert!(!game.paused, "button unpauses");
        game.click([b[0] + 2.0, b[1] + 2.0]);
        assert!(game.paused, "and pauses again");
        assert!(game.route.is_none(), "the button never starts a walk");
        let (time, pos) = (game.time, game.player);
        for _ in 0..30 {
            game.update(1.0 / 60.0);
        }
        assert_eq!(
            (game.time, game.player),
            (time, pos),
            "paused: nothing moves"
        );
    }

    #[test]
    fn stops_when_the_route_runs_into_something_revealed() {
        // A long corridor, blocked far ahead, beyond the view radius.
        let wall = "#".repeat(64);
        let mut open = ".".repeat(64);
        open.replace_range(55..56, "#");
        let l = level(&[&[wall.as_str(), open.as_str(), wall.as_str()]]);
        let mut game = game(l, [0, 1]);
        assert!(!game.visions[0].is_explored(55, 1), "the block is unseen");
        click_and_run(&mut game, [62, 1], 4000);
        assert!(game.paused, "stopped");
        assert!(game.route.is_none());
        assert!(player_tile(&game)[0] < 55, "before reaching the block");
    }

    #[test]
    fn keeps_walking_when_an_npc_comes_into_view() {
        let l = level(&[&[
            "################################",
            "................................",
            "################################",
        ]]);
        let mut game = game(l, [0, 1]);
        game.npcs.push(Entity::npc(7, [292.0, 12.0]));
        click_and_run(&mut game, [31, 1], 2000);
        assert!(game.memories[0].is_visible(7), "seen on the way");
        assert_eq!(player_tile(&game), [31, 1], "walked all the way");
    }

    #[test]
    fn routes_avoid_stairs_but_can_end_on_them() {
        let l = level(&[
            &["..........", "....<.....", ".........."],
            &["..........", "....>.....", ".........."],
        ]);
        let mut game = game(l, [1, 1]);
        click_and_run(&mut game, [8, 1], 600);
        assert_eq!(game.layer(), 1, "walked around the stairs");
        assert_eq!(player_tile(&game), [8, 1]);

        // Onto the stairs: down a layer, and paused there.
        click_and_run(&mut game, [4, 1], 600);
        assert_eq!(game.layer(), 0);
        assert!(game.paused);
        // Off them and back on: up again (no bouncing on arrival below).
        click_and_run(&mut game, [7, 1], 600);
        assert_eq!(game.layer(), 0);
        click_and_run(&mut game, [4, 1], 600);
        assert_eq!(game.layer(), 1);
    }

    /// Along a chamfered shore the route steps diagonally over the
    /// shore tiles' open halves instead of going around them.
    #[test]
    fn routes_cut_past_chamfered_corners() {
        // (0,0) is water in its top-left half: the step from (0,1) to (1,0)
        // clips its open corner.
        let shore = level(&[&["◜..", "...", "..."]]);
        let mut g = game(shore, [0, 1]);
        click_world(&mut g, path::center([1, 0]));
        assert_eq!(
            g.route.as_ref().unwrap().tiles.len(),
            2,
            "one diagonal step"
        );
        // Whole water there: around.
        let lake = level(&[&["≈..", "...", "..."]]);
        let mut g = game(lake, [0, 1]);
        click_world(&mut g, path::center([1, 0]));
        assert_eq!(g.route.as_ref().unwrap().tiles.len(), 3);
    }

    /// A walk ends exactly where the click was, if the box fits there:
    /// anywhere in a tile, or over the open half of a 45-degree tile.
    #[test]
    fn clicks_land_exactly_where_they_point() {
        let l = level(&[&["......", "......", "......", "......"]]);
        let mut g = game(l, [1, 1]);
        let p = [43.3, 27.7];
        click_world(&mut g, p);
        let waypoints = g.route.as_ref().unwrap().waypoints.clone();
        assert_eq!(
            waypoints.len(),
            1,
            "straight there, no tile center on the way: {waypoints:?}"
        );
        let last = waypoints[0];
        assert!(
            (last[0] - p[0]).abs() < 1e-3 && (last[1] - p[1]).abs() < 1e-3,
            "{last:?}"
        );
        // The destination marker (the outline drawn last: four strips of
        // two triangles) sits on the point, not its tile's center.
        let overlay = g.overlay();
        let marker = &overlay.vertices[overlay.vertices.len() - 24..];
        let xs: Vec<f32> = marker.iter().map(|v| v.pos[0]).collect();
        let (x0, x1) = (
            xs.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
        );
        assert!(
            ((x0 + x1) / 2.0 - p[0]).abs() < 1.0,
            "marker at {}..{}",
            x0,
            x1
        );
        for _ in 0..600 {
            if g.paused {
                break;
            }
            g.update(1.0 / 60.0);
        }
        assert!(g.paused && g.route.is_none());
        let c = center(g.player, PLAYER_SIZE);
        assert!(
            (c[0] - p[0]).abs() < 1e-3 && (c[1] - p[1]).abs() < 1e-3,
            "{c:?}"
        );
        // Over a shore tile's open half, where the box just fits (its far
        // corner touching the water's edge).
        let shore = level(&[&["......", "...◜..", "......"]]);
        let mut g = game(shore, [1, 1]);
        let p = [39.0, 19.0];
        click_world(&mut g, p);
        let last = *g.route.as_ref().unwrap().waypoints.last().unwrap();
        assert!(
            (last[0] - p[0]).abs() < 1e-3 && (last[1] - p[1]).abs() < 1e-3,
            "{last:?}"
        );
        for _ in 0..600 {
            if g.paused {
                break;
            }
            g.update(1.0 / 60.0);
        }
        let c = center(g.player, PLAYER_SIZE);
        assert!(
            (c[0] - p[0]).abs() < 1e-3 && (c[1] - p[1]).abs() < 1e-3,
            "{c:?}"
        );
        assert!(!g.level.movement(0).collides(g.player, PLAYER_SIZE));
    }

    /// Clicking where the box doesn't fit (the edge of a 45-degree tile)
    /// walks as near as it gets; clicking void (or water) does nothing.
    #[test]
    fn clicking_a_half_tile_walks_to_its_edge() {
        let l = level(&[&["#######", "...◜...", "#######"]]);
        let mut g = game(l, [1, 1]);
        // (3,1) is water in its top-left half: no standing on it, but the
        // box gets a pixel closer than (2,1)'s center before it touches.
        click_world(&mut g, [38.0, 18.0]);
        let route = g.route.as_ref().unwrap();
        assert_eq!(
            *route.tiles.last().unwrap(),
            [2, 1],
            "the nearest whole tile"
        );
        let last = *route.waypoints.last().unwrap();
        assert!(last[0] > 25.0 && last[0] < 27.0, "{last:?}");
        for _ in 0..600 {
            if g.paused {
                break;
            }
            g.update(1.0 / 60.0);
        }
        assert!(g.paused);
        let c = center(g.player, PLAYER_SIZE);
        assert!(c[0] > 25.5 && c[0] <= 26.0 + 1e-3, "{c:?}");
        assert!(!g.level.movement(0).collides(g.player, PLAYER_SIZE));
        // The water half itself: ignored.
        click_world(&mut g, [31.0, 11.0]);
        assert!(g.route.is_none() && g.paused);
        // A rim: the hole's half is ignored, the floor's half walks there.
        let rim = level(&[&["....", "....", "...."], &["....", ".▛..", "...."]]);
        let mut g = game(rim, [3, 1]);
        click_world(&mut g, [18.0, 18.0]);
        assert!(g.route.is_none(), "clicked the hole");
        click_world(&mut g, [12.0, 12.0]);
        assert!(g.route.is_some(), "clicked the floor");
    }

    #[test]
    fn camera_pans_independently_and_freely() {
        let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let l = Level::new(vec![Layer::from_ascii(&rows)], tileset());
        let mut game = game(l, [32, 20]);
        // The view is 320 x 200 screen pixels, so 200 x 125 world units.
        let view = [VIEW[0] / ZOOM, VIEW[1] / ZOOM];
        assert_eq!(view, [200.0, 125.0]);
        assert_eq!(
            game.camera.pos,
            [325.0 - view[0] / 2.0, 205.0 - view[1] / 2.0],
            "centered"
        );
        game.input.set("ArrowRight", true);
        for _ in 0..600 {
            game.update(1.0 / 60.0);
        }
        // Ten seconds of panning: well past the level's edge.
        let panned = game.camera.pos[0] - (325.0 - view[0] / 2.0);
        let expected = 10.0 * goblin_nightclub_core::camera::PAN_SPEED / ZOOM;
        assert!((panned - expected).abs() < 1.0, "{panned}");
        assert!(game.camera.pos[0] > 640.0, "no boundary");
        assert_eq!(game.player, game.player, "player didn't move");
        // Clicking maps through the camera.
        game.input.set("ArrowRight", false);
        click_world(&mut game, path::center([60, 20]));
        assert_eq!(
            *game.route.as_ref().unwrap().tiles.last().unwrap(),
            [60, 20]
        );
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let l = Level::new(vec![Layer::from_ascii(&rows)], tileset());
        let mut game = game(l, [32, 20]);
        let pointer = [100.0, 60.0];
        let anchor = game.to_world(pointer);
        game.zoom_by(1, pointer);
        assert_eq!(game.zoom(), goblin_nightclub_core::camera::ZOOM_STEP * ZOOM);
        let after = game.to_world(pointer);
        assert!((after[0] - anchor[0]).abs() < 1e-3 && (after[1] - anchor[1]).abs() < 1e-3);
        // Clicks map through the zoom too.
        click_world(&mut game, path::center([33, 21]));
        assert_eq!(
            *game.route.as_ref().unwrap().tiles.last().unwrap(),
            [33, 21]
        );
        // Zoom stops at either end (enough notches for any step size): at
        // 320 x 200, the default already
        // shows a pixel of the art per pixel of the screen.
        game.zoom_by(10_000, pointer);
        assert_eq!(game.zoom(), goblin_nightclub_core::camera::ZOOM_MAX * ZOOM);
        game.zoom_by(-10_000, pointer);
        assert_eq!(game.zoom(), ZOOM);
        assert_eq!(game.frame().zoom, game.zoom());
    }

    #[test]
    fn bigger_views_scale_up_and_zoom_out_further() {
        let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let l = Level::new(vec![Layer::from_ascii(&rows)], tileset());
        let mut game = game(l, [32, 20]);
        let center = game.to_world([160.0, 100.0]);
        // Four times the pixels each way (and a bit): the same view of the
        // level, everything four times the size.
        game.resize([1300.0, 820.0]);
        assert_eq!(game.ui_scale(), 4.0);
        assert_eq!(game.zoom(), 4.0 * ZOOM);
        let now = game.to_world([650.0, 410.0]);
        assert!((now[0] - center[0]).abs() < 1e-3 && (now[1] - center[1]).abs() < 1e-3);
        // The pause button scales with the interface.
        let b = game.button_pos();
        game.click([(b[0] + 2.0) * 4.0, (b[1] + 2.0) * 4.0]);
        assert!(!game.paused);
        // Zoomed all the way out, a pixel of the art is a screen pixel, and
        // the whole level fits, with room around it.
        let corner = game.to_world([0.0, 0.0]);
        game.zoom_by(-10_000, [0.0, 0.0]);
        assert_eq!(game.zoom(), ZOOM);
        let view = game.view_world();
        assert!(view[0] > 640.0 && view[1] > 400.0);
        assert_eq!(game.to_world([0.0, 0.0]), corner, "zoomed about the corner");
    }

    #[test]
    fn looking_down_sees_past_the_lower_layers_walls() {
        // A wall below between the viewer and an NPC, all under an opening.
        let l = level(&[
            &["..........", "...#......", ".........."],
            &["~~~~~~~~~~", "~~~~~~~~~~", "~~~~~~~~~~"],
        ]);
        let mut game = game(l, [0, 1]);
        let mut behind = Entity::npc(100, [62.0, 12.0]);
        behind.layer = 0;
        game.npcs.push(behind);
        game.observe();
        assert!(game.memories[0].is_visible(100));
        assert!(game.visions[0].is_explored(8, 1), "floor behind the wall");
    }

    #[test]
    fn lower_layers_are_seen_less_far() {
        // Open below, all void above: the player looks straight down, from
        // under the open sky.
        let (floor, void) = (".".repeat(50), "~".repeat(50));
        let l = level(&[
            &[floor.as_str(), floor.as_str()],
            &[void.as_str(), void.as_str()],
        ]);
        let mut game = game(l, [0, 0]);
        // From the player's center (5, 5): one NPC well inside the reduced
        // radius below, one inside the full radius but beyond the reduced.
        let near_r = VIEW_RADIUS_OUTDOORS - VIEW_RADIUS_STEP;
        let mut near = Entity::npc(1, [5.0 + near_r - 20.0, 2.0]);
        let mut far = Entity::npc(2, [5.0 + near_r + 6.0, 2.0]);
        near.layer = 0;
        far.layer = 0;
        game.npcs.extend([near, far]);
        game.observe();
        assert!(game.memories[0].is_visible(1));
        assert!(
            !game.memories[0].is_visible(2),
            "beyond the lower layer's radius"
        );
        assert_eq!(game.visions[0].radius(), near_r);
        assert_eq!(
            view_radius(VIEW_RADIUS, 1000, false),
            VIEW_RADIUS_MIN,
            "clamped"
        );
    }

    #[test]
    fn indoors_sees_less_far_than_outdoors() {
        // Three layers: the player on the middle one, under a roof with a
        // hole in it. The top layer is all void over the hole only.
        let l = level(&[
            &["................"],
            &["................"],
            &["....~~~~........"],
        ]);
        let mut inside = game_on(l, 1, [1, 0]);
        assert!(!inside.outdoors(), "under the roof");
        assert_eq!(inside.visions[1].radius(), VIEW_RADIUS);
        // Step under the hole: outdoors, and the radius grows.
        click_and_run(&mut inside, [5, 0], 600);
        assert_eq!(player_tile(&inside), [5, 0]);
        assert!(inside.outdoors());
        assert_eq!(inside.visions[1].radius(), VIEW_RADIUS_OUTDOORS);
        // Stairs overhead (the top of a ramp whose foot the player is on)
        // are no roof; a floor beside them is.
        let ramp = level(&[&["....<..."], &["....>~~~"]]);
        let mut foot = game_on(ramp, 0, [4, 0]);
        assert!(foot.outdoors(), "under the ramp's top");
        assert_eq!(foot.visions[0].radius(), VIEW_RADIUS_OUTDOORS);
        click_and_run(&mut foot, [2, 0], 600);
        assert_eq!(player_tile(&foot), [2, 0]);
        assert!(!foot.outdoors(), "under the floor beside it");
        // On the top layer, always outdoors.
        let top = game(level(&[&["...."], &["...."]]), [1, 0]);
        assert!(top.outdoors());
        assert_eq!(top.visions[1].radius(), VIEW_RADIUS_OUTDOORS);
        // From outdoors, what's below ground is seen a fifth as far: the
        // player on the plains (ground level) looking down a hole.
        let pit = level(&[&["...."], &["~~~~"]]);
        let mut above = game_on(pit, 1, [1, 0]);
        above.ground = 1;
        above.observe();
        assert!(above.outdoors());
        let expected = ((VIEW_RADIUS_OUTDOORS - VIEW_RADIUS_STEP) * UNDERGROUND_FROM_ABOVE)
            .max(VIEW_RADIUS_MIN);
        assert_eq!(above.visions[0].radius(), expected);
        assert_eq!(above.frame().radius_underground, UNDERGROUND_FROM_ABOVE);
        // Underground, a shaft open to the sky is still underground.
        let shaft = level(&[&["...."], &["~~~~"], &["~~~~"]]);
        let mut below_ground = game_on(shaft, 0, [1, 0]);
        assert!(
            below_ground.outdoors(),
            "ground level 0: the bottom is outdoors"
        );
        below_ground.ground = 1;
        below_ground.observe();
        assert!(!below_ground.outdoors());
        assert_eq!(below_ground.visions[0].radius(), VIEW_RADIUS);
        // The renderer is told, so it can put light mist over the layers
        // below, down to the ground level.
        assert!(top.frame().outdoors);
        assert_eq!(
            top.frame().ground,
            0,
            "a level with no file: all above ground"
        );
        let file = LevelFile::new(
            4,
            1,
            &vec![vec![Tile::FLOOR; 4]; 3],
            Spawn {
                layer: 2,
                pos: [12.5, 5.25],
            },
            tileset(),
            "player",
            "npc",
        )
        .with_ground(1);
        let from_file = Game::from_file(&file, VIEW, 1);
        assert_eq!(from_file.frame().ground, 1);
        assert_eq!(
            center(from_file.player, PLAYER_SIZE),
            [12.5, 5.25],
            "spawn to the pixel"
        );
        assert_eq!(from_file.sprites, sprites(), "sprites by name");
        assert_eq!(from_file.frame().cell, 16);
        assert!(
            !game_on(level(&[&["..."], &["..."]]), 0, [1, 0])
                .frame()
                .outdoors
        );
    }

    #[test]
    fn only_characters_drawn_cover_the_void() {
        let (floor, void) = (".".repeat(60), "~".repeat(60));
        let l = level(&[&[&floor, &floor], &[&floor, &void]]);
        let mut game = game(l, [0, 0]);
        // Along the edge of the void: one NPC in view, one beyond the
        // (outdoor) view radius.
        game.npcs.push(Entity::npc(1, [32.0, 2.0]));
        game.npcs
            .push(Entity::npc(2, [VIEW_RADIUS_OUTDOORS + 60.0, 2.0]));
        for n in &mut game.npcs {
            n.layer = 1;
        }
        game.observe();
        let frame = game.frame();
        let top = &frame.layers[0];
        // Player and the visible NPC: a square each.
        assert_eq!(top.standing.vertices.len(), 2 * 6);
        // The void row, a rectangle in each chunk it crosses.
        let voids = top.meshes.pieces.iter().flat_map(|p| &p.voids.vertices);
        let voids: Vec<_> = voids.collect();
        let area: f32 = (voids.chunks(3))
            .map(|t| {
                let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
                ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
            })
            .sum();
        assert!((area - 60.0 * TILE * TILE).abs() < 1e-2, "{area}");
    }

    #[test]
    fn standing_over_stacked_walls_still_sees_to_the_bottom() {
        // The player stands on floor over a wall over a wall, beside a hole
        // down through both upper layers.
        let l = level(&[
            &["#.....", "#....."], //
            &["#~~~~~", "#~~~~~"],
            &[".~~~~~", ".~~~~~"],
        ]);
        let game = game(l, [0, 0]);
        // The wall under the wall under the player shows at the hole's
        // edge, and so does the floor at the bottom of the hole.
        assert!(game.visions[0].is_explored(0, 0));
        assert!(game.visions[0].is_explored(4, 1));
        assert!(game.visions[1].is_explored(0, 0));
    }

    #[test]
    fn floors_above_hide_what_is_under_them() {
        // Two down, a hole at x 4..5; beside it, under the middle layer's
        // floor, a wall at x 3 and floor at x 1..2 (the roof of a room).
        let l = level(&[
            &["...#.#..", "...#.#..", "...#.#.."],
            &["....~~..", "....~~..", "....~~.."],
            &["~~~~~~~~", "~~~~~~~~", "~~~~~~~~"],
        ]);
        let game = game(l, [6, 1]);
        assert!(game.visions[0].is_explored(4, 1), "the bottom of the hole");
        assert!(game.visions[0].is_explored(5, 1), "a wall in the hole");
        assert!(
            game.visions[0].is_explored(3, 1),
            "a wall at the hole's edge"
        );
        assert!(!game.visions[0].is_explored(2, 1), "under the floor");
        assert!(!game.visions[0].is_explored(1, 1), "under the floor");
        // An NPC under that floor isn't seen.
        let mut game = game;
        let mut hidden = Entity::npc(9, [12.0, 12.0]);
        hidden.layer = 0;
        game.npcs.push(hidden);
        game.observe();
        assert!(!game.memories[0].is_visible(9));
    }

    #[test]
    fn npcs_below_are_seen_only_through_holes() {
        let l = level(&[
            &["..........", "..........", ".........."],
            &["..........", "....~~....", "....~~...."],
        ]);
        let mut game = game(l, [0, 0]);
        // One below the hole, one below the deck's floor.
        let mut under_hole = Entity::npc(100, [42.0, 12.0]);
        let mut under_floor = Entity::npc(101, [82.0, 12.0]);
        under_hole.layer = 0;
        under_floor.layer = 0;
        game.npcs.extend([under_hole, under_floor]);
        game.observe();
        assert!(game.memories[0].is_visible(100));
        assert!(!game.memories[0].is_visible(101));
        let frame = game.frame();
        assert_eq!(
            frame.layers.iter().map(|l| l.index).collect::<Vec<_>>(),
            vec![1, 0]
        );
    }
}
