//! Platform-independent game state. Nothing here touches the browser or GPU.

use std::collections::HashSet;

use crate::draw::Canvas;
use crate::entity::{Entity, EntityId};
use crate::frame::{Frame, LayerFrame};
use crate::generate::Style;
use crate::layer::{Dir, TILE, Tile};
use crate::level::Level;
use crate::memory::Memory;
use crate::palette;
use crate::path;
use crate::rng::Rng;
use crate::vision::Vision;

#[derive(Default)]
pub struct Input {
    held: HashSet<String>,
}

impl Input {
    pub fn set(&mut self, code: &str, down: bool) {
        if down {
            self.held.insert(code.to_owned());
        } else {
            self.held.remove(code);
        }
    }

    pub fn any(&self, codes: &[&str]) -> bool {
        codes.iter().any(|c| self.held.contains(*c))
    }

    /// -1, 0 or 1 along an axis given negative and positive key sets.
    fn axis(&self, neg: &[&str], pos: &[&str]) -> f32 {
        self.any(pos) as i32 as f32 - self.any(neg) as i32 as f32
    }
}

pub struct Game {
    pub input: Input,
    level: Level,
    /// The player's layer.
    top: usize,
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
    /// Top-left of the view, in world pixels.
    camera: [f32; 2],
    /// Size of the view, in pixels.
    view: [f32; 2],
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
const PLAYER_SPEED: f32 = 90.0;

/// Pause on reaching the destination.
const AUTO_PAUSE: bool = true;

/// Camera panning speed, in pixels a second.
const CAMERA_SPEED: f32 = 160.0;

/// Which kind of level to generate, and its size in tiles.
const LEVEL_STYLE: Style = Style::Layered;
const LEVEL_COLS: usize = 64;
const LEVEL_ROWS: usize = 40;

/// How far the player can see, in pixels. `f32::INFINITY` is unlimited.
const VIEW_RADIUS: f32 = 96.0;
/// Looking down, each layer below the player's is seen this much less far,
/// down to `VIEW_RADIUS_MIN`.
const VIEW_RADIUS_STEP: f32 = 16.0;
const VIEW_RADIUS_MIN: f32 = 2.0 * TILE;

/// View distance for a layer `depth` layers below the player's.
fn view_radius(depth: usize) -> f32 {
    (VIEW_RADIUS - VIEW_RADIUS_STEP * depth as f32).max(VIEW_RADIUS_MIN)
}

/// NPCs per layer per 640 tiles of level.
const NPC_DENSITY: usize = 4;
/// NPCs spawn at least this far (pixels) from the player.
const NPC_SPAWN_DIST: f32 = 60.0;

/// The pause button, in view pixels from the top-right corner.
const BUTTON_SIZE: [f32; 2] = [14.0, 12.0];
const BUTTON_MARGIN: f32 = 3.0;

impl Game {
    /// A new game on a generated level, seen through a view `width` x
    /// `height` pixels.
    pub fn new(width: f32, height: f32, seed: u32) -> Self {
        let (cols, rows) = (LEVEL_COLS, LEVEL_ROWS);
        // Spawn centered in the middle tile, on the top layer.
        let player = [
            (cols / 2) as f32 * TILE + (TILE - PLAYER_SIZE[0]) / 2.0,
            (rows / 2) as f32 * TILE + (TILE - PLAYER_SIZE[1]) / 2.0,
        ];
        let level = Level::generate(LEVEL_STYLE, seed, cols, rows, player);
        Self::with_level(level, player, [width, height], seed)
    }

    /// A game on `level`, the player at `player` (top-left, pixels) on the
    /// top layer, seen through a view of size `view`. It starts paused.
    pub fn with_level(level: Level, player: [f32; 2], view: [f32; 2], seed: u32) -> Self {
        let top = level.depth() - 1;
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
            player,
            npcs,
            rng,
            time: 0.0,
            paused: true,
            route: None,
            camera: [0.0; 2],
            view,
            pointer: None,
        };
        game.center_camera();
        game.observe();
        game
    }

    pub fn update(&mut self, dt: f32) {
        self.pan_camera(dt);
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

    /// A key just went down (`KeyboardEvent.code`): Space toggles pause, C
    /// centers the camera on the player. Held keys pan the camera.
    pub fn key_pressed(&mut self, code: &str) {
        match code {
            "Space" => self.paused = !self.paused,
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
        let world = [self.camera[0] + p[0], self.camera[1] + p[1]];
        self.walk_to(tile_of(world));
    }

    /// Plan a walk to tile `goal` on the player's layer and set off. Plans
    /// on what the player knows: unexplored tiles are assumed walkable.
    /// A known pit is ignored (no clicking through holes); a known wall
    /// sends the player as near it as they can get.
    fn walk_to(&mut self, goal: [i32; 2]) {
        let layer = self.level.layer(self.top);
        if !layer.in_bounds(goal) {
            return;
        }
        let known = self.visions[self.top].is_explored(goal[0] as usize, goal[1] as usize);
        if known && layer.tile(goal[0], goal[1]) == Tile::Void && !self.walkable(goal, goal) {
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
        if let Tile::Stairs(_) = layer.tile(t[0], t[1]) {
            return t == goal;
        }
        !self.visions[self.top].is_explored(t[0] as usize, t[1] as usize)
            || self.level.movement(self.top).tile(t[0], t[1]) == Tile::Empty
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
        match self.level.layer(self.top).tile(here[0], here[1]) {
            Tile::Stairs(Dir::Down) if self.top > 0 => self.top -= 1,
            Tile::Stairs(Dir::Up) if self.top + 1 < self.level.depth() => self.top += 1,
            _ => {}
        }
    }

    /// Size of the level, in pixels.
    fn world(&self) -> [f32; 2] {
        [
            self.level.cols() as f32 * TILE,
            self.level.rows() as f32 * TILE,
        ]
    }

    /// Keep the view inside the level (centered, if the level is smaller).
    fn clamp_camera(&mut self) {
        let world = self.world();
        for ((camera, world), view) in self.camera.iter_mut().zip(world).zip(self.view) {
            let max = world - view;
            *camera = if max <= 0.0 {
                max / 2.0
            } else {
                camera.clamp(0.0, max)
            };
        }
    }

    fn center_camera(&mut self) {
        let c = center(self.player, PLAYER_SIZE);
        self.camera = [c[0] - self.view[0] / 2.0, c[1] - self.view[1] / 2.0];
        self.clamp_camera();
    }

    /// Arrow keys or WASD pan the view, paused or not.
    fn pan_camera(&mut self, dt: f32) {
        let dx = self
            .input
            .axis(&["ArrowLeft", "KeyA"], &["ArrowRight", "KeyD"]);
        let dy = self
            .input
            .axis(&["ArrowUp", "KeyW"], &["ArrowDown", "KeyS"]);
        self.camera = [
            self.camera[0] + dx * CAMERA_SPEED * dt,
            self.camera[1] + dy * CAMERA_SPEED * dt,
        ];
        self.clamp_camera();
    }

    /// The pause button's top-left, in view pixels.
    fn button_pos(&self) -> [f32; 2] {
        [self.view[0] - BUTTON_SIZE[0] - BUTTON_MARGIN, BUTTON_MARGIN]
    }

    fn on_button(&self, p: [f32; 2]) -> bool {
        let b = self.button_pos();
        p[0] >= b[0] && p[1] >= b[1] && p[0] < b[0] + BUTTON_SIZE[0] && p[1] < b[1] + BUTTON_SIZE[1]
    }

    /// World-space marks over everything: the route being walked, dotted,
    /// and its destination.
    fn overlay(&self) -> Canvas {
        let mut canvas = Canvas::default();
        let Some(route) = &self.route else {
            return canvas;
        };
        let mut at = center(self.player, PLAYER_SIZE);
        for &w in &route.waypoints[route.next..] {
            let len = (w[0] - at[0]).hypot(w[1] - at[1]);
            let dots = (len / 4.0) as usize;
            for i in 1..=dots {
                let t = i as f32 * 4.0 / len;
                let p = [at[0] + (w[0] - at[0]) * t, at[1] + (w[1] - at[1]) * t];
                canvas.rect([p[0].floor(), p[1].floor()], [1.0, 1.0], palette::YELLOW);
            }
            at = w;
        }
        let goal = path::center(*route.tiles.last().unwrap());
        canvas.outline([goal[0] - 2.0, goal[1] - 2.0], [5.0, 5.0], palette::YELLOW);
        canvas
    }

    /// View-space interface: the pause button, showing what a click does
    /// (pause while running, play while paused).
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
        canvas.outline([x, y], BUTTON_SIZE, palette::LIGHT_GRAY);
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

    /// Recompute what the player sees and remembers, on their layer and on
    /// every layer showing through below it.
    fn observe(&mut self) {
        let origin = center(self.player, PLAYER_SIZE);
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
            self.visions[k].set_radius(view_radius(self.top - k));
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
        let layers = self
            .level
            .visible_layers(self.top)
            .into_iter()
            .map(|k| {
                let (layer, vision) = (self.level.layer(k), &self.visions[k]);
                // Bevels only on faces seen: now for the visible canvas,
                // ever for the remembered one. Otherwise a lit wall would
                // show the bevel of a face behind it nobody has seen.
                let mut visible = Canvas::default();
                layer.draw(&mut visible, |i| vision.edge_visible()[i]);
                for npc in self.npcs.iter().filter(|n| n.layer == k) {
                    visible.rect(npc.pos, npc.size, palette::LIGHT_RED);
                }
                if k == self.top {
                    visible.rect(self.player, PLAYER_SIZE, palette::LIGHT_CYAN);
                }

                let mut remembered = Canvas::default();
                layer.draw(&mut remembered, |i| vision.edge_seen()[i]);
                for (_, m) in self.memories[k].stale() {
                    remembered.outline(m.pos, m.size, palette::LIGHT_RED);
                }

                let tiles_where = |f: &dyn Fn(usize) -> bool| -> Vec<([f32; 2], [f32; 2])> {
                    (0..layer.cols * layer.rows)
                        .filter(|&i| f(i))
                        .map(|i| {
                            let (x, y) = (i % layer.cols, i / layer.cols);
                            ([x as f32 * TILE, y as f32 * TILE], [TILE, TILE])
                        })
                        .collect()
                };
                let voids = tiles_where(&|i| {
                    layer.tile((i % layer.cols) as i32, (i / layer.cols) as i32) == Tile::Void
                });
                let shaft = (k < self.top).then(|| {
                    let shaft = self.level.shaft(self.top, k);
                    tiles_where(&|i| shaft[i])
                });
                // Whatever stands on this layer covers the void beneath it
                // (e.g. the player on the top of a wall below).
                let mut standing: Vec<([f32; 2], [f32; 2])> = self
                    .npcs
                    .iter()
                    .filter(|n| n.layer == k)
                    .map(|n| (n.pos, n.size))
                    .collect();
                if k == self.top {
                    standing.push((self.player, PLAYER_SIZE));
                }

                LayerFrame {
                    index: k,
                    visible,
                    remembered,
                    unknown: Canvas::default(),
                    polygon: vision.polygon().to_vec(),
                    lit_shapes: vision.wall_band().to_vec(),
                    voids,
                    standing,
                    shaft,
                }
            })
            .collect();

        let vision = &self.visions[self.top];
        Frame {
            top: self.top,
            layers,
            origin: vision.origin(),
            radius: vision.radius(),
            radius_step: VIEW_RADIUS_STEP,
            radius_min: VIEW_RADIUS_MIN,
            time: self.time,
            camera: self.camera,
            view: self.view,
            world: self.world(),
            overlay: self.overlay(),
            ui: self.ui(),
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
mod tests {
    use super::*;
    use crate::layer::Layer;

    const VIEW: [f32; 2] = [320.0, 200.0];

    fn level(layers: &[&[&str]]) -> Level {
        Level::new(layers.iter().map(|rows| Layer::from_ascii(rows)).collect())
    }

    /// A game on `level` with no NPCs, the player's box centered in tile
    /// `at`.
    fn game(level: Level, at: [i32; 2]) -> Game {
        let c = path::center(at);
        let mut game = Game::with_level(level, [c[0] - 4.0, c[1] - 4.0], VIEW, 1);
        game.npcs.clear();
        game
    }

    /// Click tile `t` (the camera's at the origin for small levels) and
    /// play until paused, up to `max` frames.
    fn click_and_run(game: &mut Game, t: [i32; 2], max: usize) {
        let c = path::center(t);
        game.click([c[0] - game.camera[0], c[1] - game.camera[1]]);
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
        let l = level(&[&[
            "################################",
            "...........................#....",
            "################################",
        ]]);
        let mut game = game(l, [0, 1]);
        assert!(
            !game.visions[1 - 1].is_explored(27, 1),
            "the block is unseen"
        );
        click_and_run(&mut game, [30, 1], 2000);
        assert!(game.paused, "stopped");
        assert!(game.route.is_none());
        assert!(player_tile(&game)[0] < 27, "before reaching the block");
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
    fn camera_pans_independently_and_stays_in_the_level() {
        let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let l = Level::new(vec![Layer::from_ascii(&rows)]);
        let mut game = game(l, [32, 20]);
        let start = game.camera;
        assert_eq!(
            start,
            [320.0 + 5.0 - 160.0, 200.0 + 5.0 - 100.0],
            "centered"
        );
        game.input.set("ArrowRight", true);
        for _ in 0..600 {
            game.update(1.0 / 60.0);
        }
        assert_eq!(game.camera[0], 640.0 - 320.0, "clamped at the edge");
        assert_eq!(game.player, game.player, "player didn't move");
        // Clicking maps through the camera.
        game.input.set("ArrowRight", false);
        let c = path::center([60, 20]);
        game.click([c[0] - game.camera[0], c[1] - game.camera[1]]);
        assert_eq!(
            *game.route.as_ref().unwrap().tiles.last().unwrap(),
            [60, 20]
        );
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
        // Open below, all void above: the player looks straight down.
        let l = level(&[
            &["................", "................"],
            &["~~~~~~~~~~~~~~~~", "~~~~~~~~~~~~~~~~"],
        ]);
        let mut game = game(l, [0, 0]);
        // From the player's center (5, 5): one NPC well inside the reduced
        // radius below, one inside the full radius but beyond the reduced.
        let near_r = VIEW_RADIUS - VIEW_RADIUS_STEP;
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
        assert_eq!(view_radius(10), VIEW_RADIUS_MIN);
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
