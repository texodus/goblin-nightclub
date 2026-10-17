//! Things that move around the level, other than the player.

use std::f32::consts::TAU;

use crate::level::{Level, TILE};
use crate::rng::Rng;

pub type EntityId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Npc,
}

pub struct Entity {
    pub id: EntityId,
    pub kind: Kind,
    pub pos: [f32; 2],
    pub size: [f32; 2],
    heading: [f32; 2],
    /// Seconds until a new heading is picked.
    turn_in: f32,
}

const NPC_SIZE: [f32; 2] = [6.0, 6.0];
const NPC_SPEED: f32 = 30.0;
const NPC_TURN_MIN: f32 = 1.0;
const NPC_TURN_MAX: f32 = 3.0;

impl Entity {
    pub fn npc(id: EntityId, pos: [f32; 2]) -> Self {
        Self {
            id,
            kind: Kind::Npc,
            pos,
            size: NPC_SIZE,
            heading: [0.0; 2],
            turn_in: 0.0,
        }
    }

    /// Place `count` NPCs at random open spots at least `min_dist` pixels
    /// (center to center) from `avoid`.
    pub fn spawn_npcs(
        level: &Level,
        rng: &mut Rng,
        count: usize,
        avoid: [f32; 2],
        min_dist: f32,
    ) -> Vec<Self> {
        let (w, h) = (level.cols as f32 * TILE, level.rows as f32 * TILE);
        let mut out = Vec::with_capacity(count);
        for _ in 0..count * 100 {
            if out.len() == count {
                break;
            }
            let pos = [
                rng.unit() * (w - NPC_SIZE[0]),
                rng.unit() * (h - NPC_SIZE[1]),
            ];
            let npc = Self::npc(out.len() as EntityId, pos);
            let c = npc.center();
            if !level.collides(pos, NPC_SIZE)
                && (c[0] - avoid[0]).hypot(c[1] - avoid[1]) >= min_dist
            {
                out.push(npc);
            }
        }
        out
    }

    pub fn center(&self) -> [f32; 2] {
        sample_points(self.pos, self.size)[0]
    }

    /// Drift in a random direction, picking a new one every few seconds or
    /// on bumping into a wall.
    pub fn wander(&mut self, level: &Level, rng: &mut Rng, dt: f32) {
        self.turn_in -= dt;
        if self.turn_in <= 0.0 {
            self.turn(rng);
        }
        let delta = [
            self.heading[0] * NPC_SPEED * dt,
            self.heading[1] * NPC_SPEED * dt,
        ];
        let next = level.move_box(self.pos, self.size, delta);
        let moved = (next[0] - self.pos[0]).hypot(next[1] - self.pos[1]);
        if moved < 0.5 * NPC_SPEED * dt {
            self.turn(rng);
        }
        self.pos = next;
    }

    fn turn(&mut self, rng: &mut Rng) {
        let a = rng.unit() * TAU;
        self.heading = [a.cos(), a.sin()];
        self.turn_in = NPC_TURN_MIN + rng.unit() * (NPC_TURN_MAX - NPC_TURN_MIN);
    }
}

/// Points used to decide whether a box is seen: its center and its corners
/// pulled slightly inward (so they aren't on a wall face it's touching).
pub fn sample_points(pos: [f32; 2], size: [f32; 2]) -> [[f32; 2]; 5] {
    const INSET: f32 = 0.5;
    let [x0, y0] = [pos[0] + INSET, pos[1] + INSET];
    let [x1, y1] = [pos[0] + size[0] - INSET, pos[1] + size[1] - INSET];
    [
        [pos[0] + size[0] / 2.0, pos[1] + size[1] / 2.0],
        [x0, y0],
        [x1, y0],
        [x0, y1],
        [x1, y1],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::Style;

    #[test]
    fn npcs_spawn_clear_and_never_enter_walls() {
        for seed in 0..20 {
            let spawn = [165.0, 105.0];
            let style = if seed % 2 == 0 {
                Style::World
            } else {
                Style::Cave
            };
            let level = Level::generate(style, seed, 32, 20, spawn);
            let mut rng = Rng::new(seed ^ 0x9E37_79B9);
            let mut npcs = Entity::spawn_npcs(&level, &mut rng, 4, spawn, 40.0);
            assert_eq!(npcs.len(), 4);
            for npc in &npcs {
                let c = npc.center();
                assert!((c[0] - spawn[0]).hypot(c[1] - spawn[1]) >= 40.0);
            }
            for _ in 0..2000 {
                for npc in &mut npcs {
                    npc.wander(&level, &mut rng, 1.0 / 60.0);
                    assert!(!level.collides(npc.pos, npc.size), "seed {seed}");
                }
            }
        }
    }
}
