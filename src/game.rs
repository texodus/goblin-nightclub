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
    /// Seconds since the game started.
    time: f32,
}

const PLAYER_SIZE: [f32; 2] = [8.0, 8.0];
const PLAYER_SPEED: f32 = 90.0;

/// Which kind of level to generate.
const LEVEL_STYLE: Style = Style::Layered;

/// How far the player can see, in pixels. `f32::INFINITY` is unlimited.
const VIEW_RADIUS: f32 = 96.0;

/// NPCs per layer.
const NPC_COUNT: usize = 4;
/// NPCs spawn at least this far (pixels) from the player.
const NPC_SPAWN_DIST: f32 = 60.0;

impl Game {
    pub fn new(width: f32, height: f32, seed: u32) -> Self {
        let cols = (width / TILE) as usize;
        let rows = (height / TILE) as usize;
        // Spawn centered in the middle tile, on the top layer.
        let player = [
            (cols / 2) as f32 * TILE + (TILE - PLAYER_SIZE[0]) / 2.0,
            (rows / 2) as f32 * TILE + (TILE - PLAYER_SIZE[1]) / 2.0,
        ];
        let level = Level::generate(LEVEL_STYLE, seed, cols, rows, player);
        Self::with_level(level, player, seed)
    }

    /// A game on `level`, the player at `player` (top-left, pixels) on the
    /// top layer.
    pub fn with_level(level: Level, player: [f32; 2], seed: u32) -> Self {
        let top = level.depth() - 1;
        let mut rng = Rng::new(seed ^ 0x9E37_79B9);
        let mut npcs = Vec::new();
        for layer in 0..level.depth() {
            let first = npcs.len() as EntityId;
            for mut npc in Entity::spawn_npcs(
                level.movement(layer),
                &mut rng,
                NPC_COUNT,
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
        let before = tile_of(center(self.player, PLAYER_SIZE));
        self.player = self
            .level
            .movement(self.top)
            .move_box(self.player, PLAYER_SIZE, delta);
        self.take_stairs(before);
        for npc in &mut self.npcs {
            npc.wander(self.level.movement(npc.layer), &mut self.rng, dt);
        }
        self.time += dt;
        self.observe();
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

    pub fn layer(&self) -> usize {
        self.top
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
            time: self.time,
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

    fn level(layers: &[&[&str]]) -> Level {
        Level::new(layers.iter().map(|rows| Layer::from_ascii(rows)).collect())
    }

    fn walk(game: &mut Game, key: &str, frames: usize) {
        game.input.set(key, true);
        for _ in 0..frames {
            game.update(1.0 / 60.0);
        }
        game.input.set(key, false);
    }

    #[test]
    fn stairs_go_down_and_up_without_bouncing() {
        let l = level(&[
            &[
                "..........",
                "..........",
                "....<.....",
                "..........",
                "..........",
            ],
            &[
                "..........",
                "..........",
                "....>.....",
                "..........",
                "..........",
            ],
        ]);
        let mut game = Game::with_level(l, [11.0, 21.0], 1);
        assert_eq!(game.layer(), 1);
        // Walk right onto the stairs at (4,2): down a layer, and stay there
        // while standing on the stairs at the bottom.
        walk(&mut game, "ArrowRight", 22);
        assert_eq!(game.layer(), 0);
        walk(&mut game, "ArrowRight", 5);
        assert_eq!(game.layer(), 0, "no bouncing back");
        // Off the stairs (center at x = 85.5) and back on (1.5px a frame
        // to x = 48): up again.
        walk(&mut game, "ArrowRight", 20);
        walk(&mut game, "ArrowLeft", 25);
        assert_eq!(game.layer(), 1);
    }

    #[test]
    fn looking_down_sees_past_the_lower_layers_walls() {
        // A wall below between the viewer and an NPC, all under an opening.
        let l = level(&[
            &["..........", "...#......", ".........."],
            &["~~~~~~~~~~", "~~~~~~~~~~", "~~~~~~~~~~"],
        ]);
        let mut game = Game::with_level(l, [1.0, 11.0], 1);
        game.npcs.clear();
        let mut behind = Entity::npc(100, [62.0, 12.0]);
        behind.layer = 0;
        game.npcs.push(behind);
        game.observe();
        assert!(game.memories[0].is_visible(100));
        assert!(game.visions[0].is_explored(8, 1), "floor behind the wall");
    }

    #[test]
    fn npcs_below_are_seen_only_through_holes() {
        let l = level(&[
            &["..........", "..........", ".........."],
            &["..........", "....~~....", "....~~...."],
        ]);
        let mut game = Game::with_level(l, [1.0, 1.0], 1);
        game.npcs.clear();
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
