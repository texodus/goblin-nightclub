//! Platform-independent game state. Nothing here touches the browser or GPU.

use std::collections::HashSet;

use crate::entity::Entity;
use crate::frame::Frame;
use crate::level::{Level, TILE};
use crate::memory::Memory;
use crate::palette::{self, Color};
use crate::rng::Rng;
use crate::sprite::Sprite;
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
    vision: Vision,
    memory: Memory,
    player: [f32; 2],
    npcs: Vec<Entity>,
    rng: Rng,
    /// Seconds since the game started.
    time: f32,
}

const PLAYER_SIZE: [f32; 2] = [8.0, 8.0];
const PLAYER_SPEED: f32 = 90.0;

/// How far the player can see, in pixels. `f32::INFINITY` is unlimited.
const VIEW_RADIUS: f32 = 96.0;

const NPC_COUNT: usize = 4;
/// NPCs spawn at least this far (pixels) from the player.
const NPC_SPAWN_DIST: f32 = 60.0;

impl Game {
    pub fn new(width: f32, height: f32, seed: u32) -> Self {
        let cols = (width / TILE) as usize;
        let rows = (height / TILE) as usize;
        // Spawn centered in the middle tile.
        let player = [
            (cols / 2) as f32 * TILE + (TILE - PLAYER_SIZE[0]) / 2.0,
            (rows / 2) as f32 * TILE + (TILE - PLAYER_SIZE[1]) / 2.0,
        ];
        let level = Level::generate(seed, cols, rows, player);
        let mut rng = Rng::new(seed ^ 0x9E37_79B9);
        let npcs = Entity::spawn_npcs(
            &level,
            &mut rng,
            NPC_COUNT,
            center(player, PLAYER_SIZE),
            NPC_SPAWN_DIST,
        );
        let mut game = Self {
            input: Input::default(),
            vision: Vision::new(&level, VIEW_RADIUS),
            memory: Memory::default(),
            level,
            player,
            npcs,
            rng,
            time: 0.0,
        };
        game.observe();
        game
    }

    pub fn update(&mut self, dt: f32) {
        let dx = self
            .input
            .axis(&["ArrowLeft", "KeyA"], &["ArrowRight", "KeyD"]);
        let dy = self
            .input
            .axis(&["ArrowUp", "KeyW"], &["ArrowDown", "KeyS"]);
        let delta = [dx * PLAYER_SPEED * dt, dy * PLAYER_SPEED * dt];
        self.player = self.level.move_box(self.player, PLAYER_SIZE, delta);
        for npc in &mut self.npcs {
            npc.wander(&self.level, &mut self.rng, dt);
        }
        self.time += dt;
        self.observe();
    }

    /// Recompute what the player sees and remembers.
    fn observe(&mut self) {
        self.vision
            .update(&self.level, center(self.player, PLAYER_SIZE));
        self.memory
            .update(&self.level, &self.vision, &self.npcs, self.time);
    }

    /// Everything to draw this frame. Each layer is drawn in full and the
    /// renderer shows it only where the matching visibility applies, so live
    /// NPCs go in `visible` and last-seen markers in `remembered`.
    pub fn frame(&self) -> Frame {
        let mut level = Vec::new();
        self.level.sprites(&mut level);

        let mut visible = level.clone();
        for npc in &self.npcs {
            visible.push(Sprite::rect(npc.pos, npc.size, palette::LIGHT_RED));
        }
        visible.push(Sprite::rect(self.player, PLAYER_SIZE, palette::LIGHT_CYAN));

        let mut remembered = level;
        for (_, m) in self.memory.stale() {
            outline(&mut remembered, m.pos, m.size, palette::LIGHT_RED);
        }

        Frame {
            visible,
            remembered,
            unknown: Vec::new(),
            polygon: self.vision.polygon().to_vec(),
            lit_rects: self.vision.visible_blocks().to_vec(),
            origin: self.vision.origin(),
            radius: self.vision.radius(),
            time: self.time,
        }
    }
}

fn center(pos: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0]
}

/// A 1-pixel rectangle outline.
fn outline(out: &mut Vec<Sprite>, pos: [f32; 2], size: [f32; 2], color: Color) {
    let [x, y] = pos;
    let [w, h] = size;
    out.extend([
        Sprite::rect([x, y], [w, 1.0], color),
        Sprite::rect([x, y + h - 1.0], [w, 1.0], color),
        Sprite::rect([x, y], [1.0, h], color),
        Sprite::rect([x + w - 1.0, y], [1.0, h], color),
    ]);
}
