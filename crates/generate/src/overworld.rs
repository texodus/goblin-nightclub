//! The terrain above ground: the heightmap and its cliffs, lakes, rivers,
//! the holes in the plains, and ramps up the cliffs.

use super::*;
use crate::noise::{fbm, lattice, quantiles};

/// Rivers in a region (see [`World::river_at`]): how wide, in tiles, and
/// how far apart, in terrain scales.
const RIVER_WIDTH: f32 = 2.5;
const RIVER_SCALE: f32 = 10.0;

impl World {
    /// Heights from noise: the lowest `water_share` of the surface is left
    /// at the plains' height for lakes; of the rest, `hill_share` is hills
    /// and above, and of that `mountain_share` is the mountains, spread
    /// over the heights above the hills. Steps between neighbors are
    /// limited to one, so every cliff is one layer tall. The spawn and its
    /// surroundings are plains.
    pub(super) fn terrain(&mut self) {
        let seed = match self.plan() {
            Some(plan) => plan.terrain_seed,
            None => self.rng.range(0, u16::MAX as usize) as u32,
        };
        let p = &self.params;
        let top = p.overworld_layers - 1;
        let scale = p.terrain_scale.max(2.0);
        let noise: Vec<f32> = self
            .tiles()
            .map(|t| {
                let [x, y] = self.absolute(t);
                fbm(seed, x as f32 / scale, y as f32 / scale)
            })
            .collect();
        // A region goes by the whole world's lines.
        let (hills, mountains) = match self.plan() {
            Some(plan) => (plan.hills, plan.mountains),
            None => {
                let at = quantiles(noise.clone());
                let (lines, _) = terrain_lines(p);
                (at(lines[0]), at(lines[1]))
            }
        };
        let peaks = top.saturating_sub(2).max(1) as f32;
        for (i, &n) in noise.iter().enumerate() {
            self.height[i] = if top < 2 || n < hills {
                1
            } else if top < 3 || n < mountains {
                2
            } else {
                // The mountains' heights, 3 and up, by how far above the
                // mountain line.
                let t = (n - mountains) / (1.0 - mountains).max(1e-6);
                3 + ((t * peaks) as usize).min(top - 3)
            };
        }
        self.noise = noise;
        // A region's rivers are cut down to the plains before any heights
        // are evened out (see `World::river_at`).
        if self.region.is_some() {
            for t in self.tiles() {
                if self.river_at(t).is_some() && !self.near_spawn(t, 2) {
                    let i = self.index(t);
                    self.height[i] = 1;
                }
            }
        }
        // No step of more than one between neighbors: lower the high side.
        self.relax_heights();
        for t in self.tiles() {
            if self.near_spawn(t, 2) {
                let i = self.index(t);
                self.height[i] = 1;
            }
        }
        // Relax again around the flattened spawn.
        self.relax_heights();
    }

    /// Lakes where the noise is lowest: water on the plains' layer, over
    /// the rock of the layer below. Not at the spawn, and not where the
    /// relaxed terrain is still high.
    pub(super) fn lakes(&mut self) {
        let share = self.params.water_share.clamp(0.0, 0.9);
        let level = match self.plan() {
            Some(plan) => plan.lake_level,
            None => quantiles(self.noise.clone())(share),
        };
        let plains = self.over(1);
        for t in self.tiles() {
            let i = self.index(t);
            let low = share > 0.0 && self.noise[i] <= level && self.height[i] == 1;
            if low && self.interior(t) && !self.near_spawn(t, 2) && self.rests(plains, t) {
                self.layers[plains][i] = self.water;
            }
        }
    }

    /// Rivers: from a high tile, downhill by the noise (with a little
    /// wandering) to a lake, the edge, or exhaustion, carving the terrain
    /// down to the plains along the way. Every `ford_spacing` tiles a ford
    /// is left as land.
    pub(super) fn rivers(&mut self) {
        let plains = self.over(1);
        let water = |w: &World, t: [i32; 2]| w.layers[plains][w.index(t)] == w.water;
        if self.region.is_some() {
            // Along the bands (already cut down to the plains), water but
            // for the fords.
            for t in self.tiles() {
                let Some(ford) = self.river_at(t) else {
                    continue;
                };
                if !ford && self.interior(t) && !self.near_spawn(t, 2) && self.rests(plains, t) {
                    let i = self.index(t);
                    self.layers[plains][i] = self.water;
                }
            }
        }
        let whole_rivers = if self.region.is_some() {
            0
        } else {
            self.params.rivers
        };
        for _ in 0..whole_rivers {
            let highs: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| {
                    self.interior(t) && self.height[self.index(t)] >= 2 && !self.near_spawn(t, 4)
                })
                .collect();
            if highs.is_empty() {
                break;
            }
            let mut at = highs[self.rng.range(0, highs.len() - 1)];
            let mut path = vec![at];
            let mut visited = vec![false; self.cols * self.rows];
            visited[self.index(at)] = true;
            for _ in 0..(self.cols + self.rows) * 2 {
                // Lowest neighbor by noise, jittered so rivers meander.
                let mut best: Option<([i32; 2], f32)> = None;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let n = [at[0] + dx, at[1] + dy];
                        if (dx, dy) == (0, 0) || !self.in_bounds(n) || visited[self.index(n)] {
                            continue;
                        }
                        let score = self.noise[self.index(n)] + self.rng.unit() * 0.08;
                        if best.is_none_or(|(_, s)| score < s) {
                            best = Some((n, score));
                        }
                    }
                }
                let Some((next, _)) = best else {
                    break;
                };
                at = next;
                visited[self.index(at)] = true;
                if !self.interior(at) || water(self, at) {
                    break;
                }
                path.push(at);
            }
            // Carve: the river's columns become plains with water on them,
            // and their neighbors are lowered to keep steps to one.
            for (n, &t) in path.iter().enumerate() {
                if self.near_spawn(t, 2) {
                    continue;
                }
                let i = self.index(t);
                self.height[i] = 1;
                let ford = self.params.ford_spacing > 0
                    && n % self.params.ford_spacing == self.params.ford_spacing / 2;
                if !ford && self.rests(plains, t) {
                    self.layers[plains][i] = self.water;
                }
            }
            self.relax_heights();
        }
        // A lone tile of land with water on every side becomes water too.
        for t in self.tiles() {
            let i = self.index(t);
            if self.layers[plains][i] == self.water || !self.interior(t) || self.near_spawn(t, 2) {
                continue;
            }
            let landlocked = Side::ALL.into_iter().all(|side| {
                let [dx, dy] = side.offset();
                let n = [t[0] + dx, t[1] + dy];
                self.in_bounds(n) && water(self, n)
            });
            if landlocked && self.height[i] == 1 && self.rests(plains, t) {
                self.layers[plains][i] = self.water;
            }
        }
    }

    /// Write the terrain into the overworld layers: for each column, rock
    /// below its height, floor at it, void above. Water placed already is
    /// kept. The lowest layer is all rock.
    pub(super) fn raise_terrain(&mut self) {
        let n = self.params.overworld_layers;
        for t in self.tiles() {
            let i = self.index(t);
            let h = self.height[i];
            let floor = match h {
                1 => self.floors.plains,
                2 => self.floors.hills,
                _ => self.floors.mountains,
            };
            for k in 0..n {
                let layer = self.over(k);
                if self.layers[layer][i] == self.water {
                    continue;
                }
                self.layers[layer][i] = if k < h {
                    Tile::Whole(self.rock)
                } else if k == h {
                    Tile::Whole(Part::Floor(floor))
                } else {
                    Tile::VOID
                };
            }
            if !self.interior(t) {
                // The border is rock all the way up.
                for k in 0..n {
                    let layer = self.over(k);
                    self.layers[layer][i] = Tile::Whole(self.rock);
                }
            }
        }
    }

    /// Holes and canyons in the plains, opening onto a stony canyon floor
    /// below. Never on or beside water, and not at the spawn.
    pub(super) fn plains_holes(&mut self) {
        let plains = self.over(1);
        let plains_floor = self.floors.plains;
        let allowed = move |w: &World, t: [i32; 2]| {
            w.interior(t)
                && !w.near_spawn(t, 2)
                && w.height[w.index(t)] == 1
                && w.layers[plains][w.index(t)].floor_id() == Some(plains_floor)
                && !w.beside_water(plains, t)
        };
        self.carve_voids(plains, &allowed);
    }

    /// Can a ramp go at column `t`, down from its height `h` to a neighbor
    /// at `h - 1`? Land on both ends.
    pub(super) fn ramp_fits(&self, t: [i32; 2]) -> bool {
        let i = self.index(t);
        let h = self.height[i];
        if h < 2 || !self.interior(t) || self.near_spawn(t, 0) {
            return false;
        }
        if !self.layers[self.over(h)][i].is_floor() {
            return false;
        }
        Side::ALL.into_iter().any(|side| {
            let [dx, dy] = side.offset();
            let n = [t[0] + dx, t[1] + dy];
            self.in_bounds(n)
                && self.height[self.index(n)] == h - 1
                && self.layers[self.over(h - 1)][self.index(n)].is_floor()
        })
    }

    /// A ramp at column `t`: stairs down on its own layer, stairs up in the
    /// cliff face below, where the lower ground reaches it.
    pub(super) fn place_ramp(&mut self, t: [i32; 2]) {
        let i = self.index(t);
        let h = self.height[i];
        let floor = |w: &World, k: usize| match k {
            1 => w.floors.plains,
            2 => w.floors.hills,
            _ => w.floors.mountains,
        };
        let (upper, lower) = (self.over(h), self.over(h - 1));
        let (up_floor, down_floor) = (floor(self, h), floor(self, h - 1));
        self.layers[upper][i] = Tile::Stairs(Dir::Down, up_floor);
        self.layers[lower][i] = Tile::Stairs(Dir::Up, down_floor);
    }

    /// Ramps along every cliff, `ramp_spacing` apart (none if that's 0).
    pub(super) fn ramps(&mut self) {
        let spacing = self.params.ramp_spacing as i32;
        if spacing == 0 {
            return;
        }
        if let Some(plan) = self.plan() {
            // In a region, a ramp in each square of `spacing` of the whole
            // world that has a cliff to ramp, at height: the one there that
            // comes first by a hash of where it is. So neighbors agree on
            // the ramps by their border, without a word between them.
            let seed = plan.ramp_seed;
            let mut best: std::collections::HashMap<([i32; 2], usize), ([i32; 2], f32)> =
                Default::default();
            for t in self.tiles() {
                if !self.ramp_fits(t) {
                    continue;
                }
                let a = self.absolute(t);
                let cell = [a[0].div_euclid(spacing), a[1].div_euclid(spacing)];
                let key = (cell, self.height[self.index(t)]);
                let score = lattice(seed, a[0], a[1]);
                if best.get(&key).is_none_or(|&(_, s)| score < s) {
                    best.insert(key, (t, score));
                }
            }
            let mut ramps: Vec<[i32; 2]> = best.into_values().map(|(t, _)| t).collect();
            ramps.sort();
            for t in ramps {
                self.place_ramp(t);
            }
            return;
        }
        let mut placed: Vec<[i32; 2]> = Vec::new();
        for t in self.tiles() {
            if !self.ramp_fits(t) {
                continue;
            }
            let h = self.height[self.index(t)];
            let crowded = placed.iter().any(|p| {
                (p[0] - t[0]).abs().max((p[1] - t[1]).abs()) < spacing
                    && self.height[self.index(*p)] == h
            });
            if !crowded {
                self.place_ramp(t);
                placed.push(t);
            }
        }
    }

    /// No step of more than one between neighbors: lower the high side
    /// until there's none.
    fn relax_heights(&mut self) {
        loop {
            let mut changed = false;
            for t in self.tiles() {
                let i = self.index(t);
                for side in Side::ALL {
                    let [dx, dy] = side.offset();
                    let n = [t[0] + dx, t[1] + dy];
                    if self.in_bounds(n) && self.height[i] > self.height[self.index(n)] + 1 {
                        self.height[i] = self.height[self.index(n)] + 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }

    /// For a region: whether a river runs at `t`, and if so, whether it's
    /// a ford there. A region's rivers are bands about the middle line of
    /// a slow noise (so each is where it is wherever it's seen from),
    /// [`RIVER_WIDTH`] tiles wide; fords cross them every
    /// [`Params::ford_spacing`] tiles, along whichever grid line runs
    /// across.
    fn river_at(&self, t: [i32; 2]) -> Option<bool> {
        let plan = self.plan()?;
        if self.params.rivers == 0 {
            return None;
        }
        let [x, y] = self.absolute(t);
        let scale =
            self.params.terrain_scale.max(2.0) * RIVER_SCALE / (self.params.rivers as f32).sqrt();
        let n = |x: f32, y: f32| fbm(plan.river_seed, x / scale, y / scale);
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (gx, gy) = (
            n(fx + 1.0, fy) - n(fx - 1.0, fy),
            n(fx, fy + 1.0) - n(fx, fy - 1.0),
        );
        // How far the middle line is, in tiles, by the noise's slope.
        let slope = (gx.hypot(gy) / 2.0).max(1e-6);
        if (n(fx, fy) - 0.5).abs() / slope > RIVER_WIDTH / 2.0 {
            return None;
        }
        // The band runs across the slope: crossed by columns where the
        // slope is mostly up and down, by rows where it's across.
        let s = self.params.ford_spacing as i32;
        let across = if gy.abs() >= gx.abs() { x } else { y };
        Some(s > 0 && across.rem_euclid(s) == 0)
    }
}

/// Where the terrain's lines fall, as shares of the noise below them: the
/// hills' and the mountains', and the lakes'.
pub(crate) fn terrain_lines(p: &Params) -> ([f32; 2], f32) {
    let land_from = p.water_share.clamp(0.0, 0.9);
    let hills_from = land_from + (1.0 - land_from) * (1.0 - p.hill_share.clamp(0.0, 1.0));
    let mountains_from = hills_from + (1.0 - hills_from) * (1.0 - p.mountain_share.clamp(0.0, 1.0));
    ([hills_from, mountains_from], land_from)
}
