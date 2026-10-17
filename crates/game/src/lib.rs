//! Platform-independent game state. Nothing here touches the browser or GPU.

use goblin_nightclub_core::camera::Camera;
use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::entity::{Entity, EntityId, Kind};
use goblin_nightclub_core::frame::{Frame, LayerFrame};
pub use goblin_nightclub_core::input::Input;
use goblin_nightclub_core::layer::{Dir, Shape, Solid, TILE, quarter_shapes};
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::level_file::LevelFile;
use goblin_nightclub_core::memory::Memory;
use goblin_nightclub_core::palette;
use goblin_nightclub_core::path;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::tileset::{self, Cell, ZOOM};
use goblin_nightclub_core::vision::Vision;
use goblin_nightclub_generate::Params;

pub struct Game {
    pub input: Input,
    level: Level,
    /// The player's layer.
    top: usize,
    /// The ground level: the lowest layer under the open sky (see
    /// [`goblin_nightclub_core::level_file::LevelFile::ground`]).
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

const PLAYER_SIZE: [f32; 2] = [8.0, 8.0];
const PLAYER_SPRITE: &tileset::Sprite = &tileset::SPRITES[0];
const PLAYER_SPEED: f32 = 90.0;

/// Pause on reaching the destination.
const AUTO_PAUSE: bool = true;

/// Size of a generated level, in tiles.
const LEVEL_COLS: usize = 96;
const LEVEL_ROWS: usize = 60;

/// How far the player can see, in pixels, indoors: under a floor or wall
/// of some layer above. `f32::INFINITY` is unlimited.
const VIEW_RADIUS: f32 = 96.0;
/// How far outdoors: on the top layer, or with nothing but void (or
/// water) directly above on every layer up to it.
const VIEW_RADIUS_OUTDOORS: f32 = 5.0 * VIEW_RADIUS;
/// Looking down, each layer below the player's is seen this much less far,
/// down to `VIEW_RADIUS_MIN`.
const VIEW_RADIUS_STEP: f32 = 16.0;
const VIEW_RADIUS_MIN: f32 = 2.0 * TILE;

/// From outdoors, layers below the ground level are seen only this
/// fraction as far (after the step per layer).
const UNDERGROUND_FROM_ABOVE: f32 = 1.0 / 5.0;

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
/// NPCs spawn at least this far (pixels) from the player.
const NPC_SPAWN_DIST: f32 = 60.0;

/// The pause button, in interface pixels from the top-right corner.
const BUTTON_SIZE: [f32; 2] = [14.0, 12.0];
const BUTTON_MARGIN: f32 = 3.0;

impl Game {
    /// A new game on a generated level, seen through a view `width` x
    /// `height` pixels.
    pub fn new(width: f32, height: f32, seed: u32, params: &Params) -> Self {
        let (cols, rows) = (LEVEL_COLS, LEVEL_ROWS);
        // Spawn centered in the middle tile, on the plains.
        let player = [
            (cols / 2) as f32 * TILE + (TILE - PLAYER_SIZE[0]) / 2.0,
            (rows / 2) as f32 * TILE + (TILE - PLAYER_SIZE[1]) / 2.0,
        ];
        let level = goblin_nightclub_generate::generate_level(seed, cols, rows, player, params);
        let top = goblin_nightclub_generate::spawn_layer(params).min(level.depth() - 1);
        let mut game = Self::with_level_at(level, top, player, [width, height], seed);
        game.ground = goblin_nightclub_generate::ground_layer(params).min(game.level.depth() - 1);
        game
    }

    /// A game on a saved level, the player at its spawn.
    pub fn from_file(file: &LevelFile, view: [f32; 2], seed: u32) -> Self {
        let [x, y] = file.spawn.tile;
        let player = [
            x as f32 * TILE + (TILE - PLAYER_SIZE[0]) / 2.0,
            y as f32 * TILE + (TILE - PLAYER_SIZE[1]) / 2.0,
        ];
        let mut game = Self::with_level_at(file.level(), file.spawn.layer, player, view, seed);
        game.ground = file.ground.min(game.level.depth() - 1);
        game
    }

    /// A game on `level`, the player at `player` (top-left, pixels) on the
    /// top layer, seen through a view of size `view`. It starts paused.
    pub fn with_level(level: Level, player: [f32; 2], view: [f32; 2], seed: u32) -> Self {
        let top = level.depth() - 1;
        Self::with_level_at(level, top, player, view, seed)
    }

    /// A game on `level`, the player at `player` (top-left, pixels) on
    /// layer `top`.
    pub fn with_level_at(
        level: Level,
        top: usize,
        player: [f32; 2],
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
        let mut game = Self {
            input: Input::default(),
            visions: (0..level.depth())
                .map(|k| Vision::new(level.layer(k), VIEW_RADIUS))
                .collect(),
            memories: (0..level.depth()).map(|_| Memory::default()).collect(),
            level,
            top,
            ground: 0,
            player,
            npcs,
            rng,
            time: 0.0,
            paused: true,
            route: None,
            camera: Camera::new(view),
            pointer: None,
        };
        game.center_camera();
        game.observe();
        game
    }

    pub fn update(&mut self, dt: f32) {
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
    }

    pub fn layer(&self) -> usize {
        self.top
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
        self.walk_to(tile_of(self.to_world(p)));
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

    /// Plan a walk to tile `goal` on the player's layer and set off. Plans
    /// on what the player knows: unexplored tiles are assumed walkable.
    /// Known void is ignored (no clicking through holes); a known wall
    /// sends the player as near it as they can get.
    fn walk_to(&mut self, goal: [i32; 2]) {
        let layer = self.level.layer(self.top);
        if !layer.in_bounds(goal) {
            return;
        }
        let known = self.visions[self.top].is_explored(goal[0] as usize, goal[1] as usize);
        let void = layer.tile(goal[0], goal[1]).void() != Solid::None;
        if known && void && !self.walkable(goal, goal) {
            return;
        }
        let start = tile_of(center(self.player, PLAYER_SIZE));
        let goal = if self.walkable(goal, goal) {
            goal
        } else {
            match path::nearest_reachable(|t| self.walkable(t, goal), start, goal) {
                Some(g) => g,
                None => return,
            }
        };
        // The tile the player stands on is fine (even if it's stairs).
        let passable = |t: [i32; 2]| t == start || self.walkable(t, goal);
        let Some(tiles) = path::find(passable, start, goal) else {
            return;
        };
        let mut waypoints = path::smooth(
            passable,
            center(self.player, PLAYER_SIZE),
            &tiles,
            PLAYER_SIZE,
        );
        if waypoints.is_empty() {
            // Already on the goal tile: settle on its center.
            waypoints.push(path::center(goal));
        }
        self.route = Some(Route {
            tiles,
            waypoints,
            next: 0,
            stalled: 0.0,
        });
        self.paused = false;
    }

    /// Can a route on the player's layer cross tile `t`, as far as the
    /// player knows? Unexplored tiles are assumed walkable. Stairs only as
    /// the `goal`, so a walk never changes layer on the way.
    fn walkable(&self, t: [i32; 2], goal: [i32; 2]) -> bool {
        let layer = self.level.layer(self.top);
        if !layer.in_bounds(t) {
            return false;
        }
        if layer.tile(t[0], t[1]).stairs().is_some() {
            return t == goal;
        }
        !self.visions[self.top].is_explored(t[0] as usize, t[1] as usize)
            || self.level.movement(self.top).is_open(t[0], t[1])
    }

    /// Head for the next waypoint, sliding along anything in the way.
    fn follow_route(&mut self, dt: f32) {
        let Some(route) = &mut self.route else {
            return;
        };
        let here = center(self.player, PLAYER_SIZE);
        let target = route.waypoints[route.next];
        let to = [target[0] - here[0], target[1] - here[1]];
        let dist = to[0].hypot(to[1]);
        let step = (PLAYER_SPEED * dt).min(dist);
        if dist > 1e-3 {
            let delta = [to[0] / dist * step, to[1] / dist * step];
            self.player = self
                .level
                .movement(self.top)
                .move_box(self.player, PLAYER_SIZE, delta);
        }
        let now = center(self.player, PLAYER_SIZE);
        let moved = (now[0] - here[0]).hypot(now[1] - here[1]);
        route.stalled = if moved < 0.2 * step {
            route.stalled + dt
        } else {
            0.0
        };
        if (target[0] - now[0]).hypot(target[1] - now[1]) < 0.5 {
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
        let passable = |t: [i32; 2]| here.contains(&t) || self.walkable(t, goal);
        let stops = std::iter::once(at).chain(route.waypoints[route.next..].iter().copied());
        let legs: Vec<[f32; 2]> = stops.collect();
        let blocked = legs
            .windows(2)
            .any(|w| !path::clear(&passable, w[0], w[1], PLAYER_SIZE));
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
        let mut canvas = Canvas::default();
        let Some(route) = &self.route else {
            return canvas;
        };
        let px = 1.0 / ZOOM;
        let snap = |v: f32| (v * ZOOM).floor() / ZOOM;
        let mut at = center(self.player, PLAYER_SIZE);
        for &w in &route.waypoints[route.next..] {
            let len = (w[0] - at[0]).hypot(w[1] - at[1]);
            let dots = (len / 4.0) as usize;
            for i in 1..=dots {
                let t = i as f32 * 4.0 / len;
                let p = [at[0] + (w[0] - at[0]) * t, at[1] + (w[1] - at[1]) * t];
                canvas.rect([snap(p[0]), snap(p[1])], [px, px], palette::YELLOW);
            }
            at = w;
        }
        let goal = path::center(*route.tiles.last().unwrap());
        let corner = [snap(goal[0] - 2.5), snap(goal[1] - 2.5)];
        canvas.outline(corner, [5.0, 5.0], px, palette::YELLOW);
        canvas
    }

    /// The interface, in interface pixels: the pause button, showing what a
    /// click does (pause while running, play while paused).
    fn ui(&self) -> Canvas {
        let mut canvas = Canvas::default();
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
    /// level, and on the top layer or under void (or water) on every layer
    /// above theirs? Underground, even a shaft of daylight from above
    /// doesn't count.
    pub fn outdoors(&self) -> bool {
        let c = center(self.player, PLAYER_SIZE);
        self.top >= self.ground
            && (self.top + 1..self.level.depth()).all(|k| self.level.layer(k).is_void_at(c))
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
        let visible = self.level.visible_layers(self.top);
        for k in 0..self.level.depth() {
            if !visible.contains(&k) {
                self.memories[k].clear_visible();
                continue;
            }
            let shaft = (k < self.top).then(|| self.level.shaft(self.top, k));
            // Looking down, the layer's own walls don't block the view, so
            // its floors and wall tops show alike.
            let view = self.level.view(self.top, k);
            let above = (k < self.top).then_some(view);
            let underground = self.outdoors() && k < self.ground;
            self.visions[k].set_radius(view_radius(base, self.top - k, underground));
            self.visions[k].update_through(view, self.level.layer(k), shaft, above, origin);
            let here = self.npcs.iter().filter(|n| n.layer == k);
            self.memories[k].update(view, &self.visions[k], here, self.time);
        }
    }

    /// Everything to draw this frame: the player's layer and each one
    /// showing through below it. Each layer is drawn in full and the
    /// renderer shows it only where the matching visibility applies, so live
    /// NPCs go in `visible` and last-seen markers in `remembered`.
    pub fn frame(&self) -> Frame {
        let bevel = 1.0 / ZOOM;
        let layers = self
            .level
            .visible_layers(self.top)
            .into_iter()
            .map(|k| {
                let (layer, vision) = (self.level.layer(k), &self.visions[k]);
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
                    .map(|n| (n.center(), sprite(n.kind)))
                    .collect();
                if k == self.top {
                    live.push((center(self.player, PLAYER_SIZE), PLAYER_SPRITE.cell));
                }
                let stale: Vec<([f32; 2], Cell)> = memory
                    .stale()
                    .map(|(_, m)| (center(m.pos, m.size), sprite(m.kind)))
                    .collect();

                let mut visible = Canvas::default();
                layer.draw(&mut visible, |i| vision.edge_visible()[i], bevel);
                for &(at, cell) in &live {
                    visible.sprite(at, cell, palette::WHITE);
                }
                let mut remembered = Canvas::default();
                layer.draw(&mut remembered, |i| vision.edge_seen()[i], bevel);
                for &(at, cell) in &stale {
                    remembered.sprite(at, cell, palette::WHITE);
                }

                // The parts of each tile given by `quarters(tile index)`.
                let parts = |quarters: &dyn Fn(usize) -> u8| -> Vec<Shape> {
                    (0..layer.cols * layer.rows)
                        .flat_map(|i| quarter_shapes(i % layer.cols, i / layer.cols, quarters(i)))
                        .collect()
                };
                let at = |i: usize| layer.tile((i % layer.cols) as i32, (i / layer.cols) as i32);
                let voids = parts(&|i| at(i).void_quarters());
                let water = parts(&|i| at(i).water_quarters());
                let shaft = (k < self.top).then(|| {
                    let shaft = self.level.shaft(self.top, k);
                    parts(&|i| if shaft[i] { 0b1111 } else { 0 })
                });
                // Characters drawn on this layer cover any void their
                // sprites overhang: the player, NPCs in view and last
                // sightings.
                let mut standing = Canvas::default();
                let in_view = self
                    .npcs
                    .iter()
                    .filter(|n| n.layer == k && memory.is_visible(n.id));
                for n in in_view {
                    standing.sprite(n.center(), sprite(n.kind), palette::WHITE);
                }
                if k == self.top {
                    let at = center(self.player, PLAYER_SIZE);
                    standing.sprite(at, PLAYER_SPRITE.cell, palette::WHITE);
                }
                for &(at, cell) in &stale {
                    standing.sprite(at, cell, palette::WHITE);
                }

                LayerFrame {
                    index: k,
                    visible,
                    remembered,
                    unknown: Canvas::default(),
                    polygon: vision.polygon().to_vec(),
                    lit_shapes: vision.wall_band().to_vec(),
                    voids,
                    water,
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
            ui_scale: self.ui_scale(),
            outdoors: self.outdoors(),
            ground: self.ground,
            world: self.world(),
            overlay: self.overlay(),
            ui: self.ui(),
        }
    }
}

/// The sheet cell drawn for an entity of `kind`.
fn sprite(kind: Kind) -> Cell {
    match kind {
        Kind::Npc => tileset::sprite("npc").cell,
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

    const VIEW: [f32; 2] = [320.0, 200.0];

    fn level(layers: &[&[&str]]) -> Level {
        Level::new(layers.iter().map(|rows| Layer::from_ascii(rows)).collect())
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
        let mut game = Game::with_level_at(level, k, [c[0] - 4.0, c[1] - 4.0], VIEW, 1);
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

    #[test]
    fn camera_pans_independently_and_freely() {
        let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let l = Level::new(vec![Layer::from_ascii(&rows)]);
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
        let l = Level::new(vec![Layer::from_ascii(&rows)]);
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
        let l = Level::new(vec![Layer::from_ascii(&rows)]);
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
        // The renderer is told, so it can fog the layers below, down to the
        // ground level.
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
                tile: [1, 0],
            },
        )
        .with_ground(1);
        assert_eq!(Game::from_file(&file, VIEW, 1).frame().ground, 1);
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
        assert!(top.voids.len() == 60, "a whole void tile each");
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
