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

//! Making sure the player can get everywhere that's kept: reachability
//! across layers and stairs, fords, causeways, bridges and ramps where
//! it's cut off, and rock where it can't be joined up.

use super::*;

impl World {
    /// In a region, every staircase on each underworld layer joined to the
    /// others there by a path of floor, dug where need be (through rock,
    /// across a hole on a bridge, through water by a ford), so a layer of
    /// the dungeon can always be crossed from the stairs that come down to
    /// it to the stairs that go on down, however deep, and whatever its
    /// caves came out like. Never under water on the layer above (which
    /// needs rock beneath it), and only where things may be placed.
    pub(super) fn join_stairs(&mut self) {
        let m = self.params.underworld_layers;
        for k in 0..m {
            let stairs: Vec<[i32; 2]> = self
                .tiles()
                .filter(|&t| self.layers[k][self.index(t)].stairs().is_some())
                .collect();
            let Some((&first, rest)) = stairs.split_first() else {
                continue;
            };
            let open = |w: &World, t: [i32; 2]| {
                w.in_bounds(t)
                    && (w.placeable(t) || w.layers[k][w.index(t)].stairs().is_some())
                    && (k + 1 >= w.layers.len() || !w.beside_water(k + 1, t))
            };
            let mut joined = vec![false; self.cols * self.rows];
            joined[self.index(first)] = true;
            for &from in rest {
                // The shortest way to anything joined already.
                let mut came: Vec<Option<[i32; 2]>> = vec![None; self.cols * self.rows];
                let mut queue = std::collections::VecDeque::from([from]);
                came[self.index(from)] = Some(from);
                let mut found = None;
                while let Some(t) = queue.pop_front() {
                    if joined[self.index(t)] {
                        found = Some(t);
                        break;
                    }
                    for side in Side::ALL {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        if open(self, n) && came[self.index(n)].is_none() {
                            came[self.index(n)] = Some(t);
                            queue.push_back(n);
                        }
                    }
                }
                let Some(mut at) = found else {
                    continue;
                };
                loop {
                    let i = self.index(at);
                    joined[i] = true;
                    let tile = self.layers[k][i];
                    if tile.stairs().is_none() && !tile.is_floor() {
                        self.layers[k][i] = Tile::Whole(Part::Floor(if tile == self.water {
                            self.floors.bed
                        } else if tile.void_quarters() != 0 {
                            self.floors.bridge
                        } else {
                            self.floors.cave
                        }));
                    }
                    if at == from {
                        break;
                    }
                    at = came[i].unwrap();
                }
            }
        }
    }

    /// Which tiles of each layer the player can walk to from the spawn,
    /// straight from the tiles (no [`Level`] needed): walls and void
    /// block, stairs join layers. Also each layer's movement grid.
    fn reachable(&self) -> (Vec<Vec<bool>>, Vec<Vec<Solid>>) {
        let (cols, rows) = (self.cols, self.rows);
        let grids: Vec<Vec<Solid>> = self
            .layers
            .iter()
            .map(|tiles| {
                tiles
                    .iter()
                    .map(|t| Solid::from_quarters(t.wall().quarters() | t.void_quarters(), true))
                    .collect()
            })
            .collect();
        let depth = self.layers.len();
        let mut seeds: Vec<Vec<[i32; 2]>> = vec![Vec::new(); depth];
        if self.in_bounds(self.spawn) {
            seeds[self.over(1)].push(self.spawn);
        }
        // A region's world goes on past its edges: what's open there can
        // be got to, as far as it knows.
        if self.region.is_some() {
            let (c, r) = (cols as i32, rows as i32);
            let edge = (0..c)
                .flat_map(|x| [[x, 0], [x, r - 1]])
                .chain((0..r).flat_map(|y| [[0, y], [c - 1, y]]));
            for t in edge {
                for (k, seeds) in seeds.iter_mut().enumerate() {
                    if self.layers[k][self.index(t)].is_floor() {
                        seeds.push(t);
                    }
                }
            }
        }
        let mut reached = vec![vec![false; cols * rows]; depth];
        loop {
            let mut grew = false;
            for k in 0..depth {
                if seeds[k].is_empty() {
                    continue;
                }
                let more = reachable_from(&grids[k], cols, rows, &std::mem::take(&mut seeds[k]));
                for i in 0..cols * rows {
                    if more[i] && !reached[k][i] {
                        reached[k][i] = true;
                        grew = true;
                        let at = [(i % cols) as i32, (i / cols) as i32];
                        match self.layers[k][i].stairs() {
                            Some(Dir::Down) if k > 0 => seeds[k - 1].push(at),
                            Some(Dir::Up) if k + 1 < depth => seeds[k + 1].push(at),
                            _ => {}
                        }
                    }
                }
            }
            if !grew {
                return (reached, grids);
            }
        }
    }

    /// Lay `path` on layer `k`: water becomes a ford (a sandy floor), void
    /// a bridge (planks, over whatever's below).
    fn lay_crossing(&mut self, k: usize, path: Vec<[i32; 2]>) {
        for t in path {
            let i = self.index(t);
            if self.layers[k][i] == self.water {
                self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.bed));
            } else {
                self.layers[k][i] = Tile::Whole(Part::Floor(self.floors.bridge));
            }
        }
    }

    /// The connected piece of floor on layer `k` that `start` is on
    /// (across sides).
    fn region(&self, k: usize, start: [i32; 2]) -> Vec<[i32; 2]> {
        let mut seen = vec![false; self.cols * self.rows];
        let mut stack = vec![start];
        let mut out = Vec::new();
        seen[self.index(start)] = true;
        while let Some(t) = stack.pop() {
            out.push(t);
            for side in Side::ALL {
                let [dx, dy] = side.offset();
                let n = [t[0] + dx, t[1] + dy];
                if self.in_bounds(n)
                    && !seen[self.index(n)]
                    && self.layers[k][self.index(n)].is_floor()
                {
                    seen[self.index(n)] = true;
                    stack.push(n);
                }
            }
        }
        out
    }

    /// The water and void tiles of the shortest way on layer `k` from
    /// `start` (cut-off land) to land that's `reached`, within `reach`
    /// tiles: a ford across water, a bridge across a hole, or both. Water
    /// with water resting on it (on the layer above) can't be forded.
    fn crossing(
        &self,
        k: usize,
        start: [i32; 2],
        reached: &[bool],
        reach: usize,
    ) -> Option<Vec<[i32; 2]>> {
        let mut from: Vec<Option<[i32; 2]>> = vec![None; self.cols * self.rows];
        let mut seen = vec![false; self.cols * self.rows];
        let mut frontier = vec![start];
        seen[self.index(start)] = true;
        for _ in 0..reach {
            let mut next = Vec::new();
            for t in frontier {
                for side in Side::ALL {
                    let [dx, dy] = side.offset();
                    let n = [t[0] + dx, t[1] + dy];
                    if !self.in_bounds(n) || seen[self.index(n)] {
                        continue;
                    }
                    let tile = self.layers[k][self.index(n)];
                    seen[self.index(n)] = true;
                    from[self.index(n)] = Some(t);
                    if tile.is_floor() && reached[self.index(n)] {
                        // Walk back, collecting what was crossed.
                        let mut path = Vec::new();
                        let mut at = t;
                        while at != start {
                            path.push(at);
                            at = from[self.index(at)].unwrap();
                        }
                        return Some(path);
                    }
                    // Water can be forded unless water on the layer above
                    // rests on it (a ford is floor: open ground beneath).
                    let fordable = tile == self.water
                        && (k + 1 >= self.layers.len() || !self.beside_water(k + 1, n));
                    if fordable || tile == Tile::VOID {
                        next.push(n);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        None
    }

    /// Join what the player can't reach from the spawn: fords across water
    /// between separated land, ramps up to unreached ground (if ramps are
    /// allowed), and rock filling unreached underworld floor.
    pub(super) fn connect(&mut self) {
        /// A cut-off piece of plains this small sinks into the lake around
        /// it rather than getting a causeway.
        const ISLET: usize = 16;
        let m = self.params.underworld_layers;
        let plains = self.over(1);
        // Tiles given up on, per layer: nothing could join them.
        let mut skipped = vec![vec![false; self.cols * self.rows]; self.layers.len()];
        // A region gives up sooner: what's left is far from anything.
        let tries = if self.region.is_some() { 48 } else { 256 };
        for _ in 0..tries {
            let (reached, _) = self.reachable();
            let mut changed = false;

            // A ford: from a piece of cut-off plains, the shortest way
            // through water to plains that are reached. A short one if
            // there is one; else a small piece becomes lake, a large one
            // gets a causeway however long.
            let cut_off = self.tiles().find(|&t| {
                let i = self.index(t);
                self.layers[plains][i].is_floor()
                    && !reached[plains][i]
                    && self.height[i] == 1
                    && !skipped[plains][i]
            });
            if let Some(start) = cut_off {
                let far = self.cols + self.rows;
                let region = self.region(plains, start);
                let path = self
                    .crossing(plains, start, &reached[plains], 8)
                    .or_else(|| {
                        (region.len() > ISLET)
                            .then(|| self.crossing(plains, start, &reached[plains], far))
                            .flatten()
                    });
                // An islet sinks only into water: never next to a hole,
                // which must stay clear of water, and only where the
                // water would rest on solid ground (not over a cellar or
                // canyon floor).
                let in_water = region.iter().all(|&t| {
                    self.interior(t)
                        && !self.near_spawn(t, 2)
                        && self.rests(plains, t)
                        && Side::ALL.into_iter().all(|side| {
                            let [dx, dy] = side.offset();
                            let n = [t[0] + dx, t[1] + dy];
                            self.in_bounds(n) && self.layers[plains][self.index(n)] != Tile::VOID
                        })
                });
                if let Some(path) = path {
                    self.lay_crossing(plains, path);
                    changed = true;
                } else if region.len() <= ISLET && in_water {
                    for t in region {
                        let i = self.index(t);
                        self.layers[plains][i] = self.water;
                    }
                    changed = true;
                } else {
                    for t in region {
                        skipped[plains][self.index(t)] = true;
                    }
                    changed = true;
                }
            }

            // Ramps: an unreached column that could ramp down to reached
            // ground, or reached ground above unreached ground.
            if self.params.ramp_spacing > 0 && !changed {
                let candidate = self.tiles().find(|&t| {
                    if !self.ramp_fits(t) {
                        return false;
                    }
                    let i = self.index(t);
                    let h = self.height[i];
                    let here = reached[self.over(h)][i];
                    Side::ALL.into_iter().any(|side| {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        self.in_bounds(n)
                            && self.height[self.index(n)] == h - 1
                            && self.layers[self.over(h - 1)][self.index(n)].is_floor()
                            && reached[self.over(h - 1)][self.index(n)] != here
                    })
                });
                if let Some(t) = candidate {
                    self.place_ramp(t);
                    changed = true;
                }
            }
            // High ground rising straight out of water, with no land below
            // to ramp down onto: give it a shore, one tile of water made
            // bed, and a causeway from there to land that's reached; or
            // give that hill up.
            if self.params.ramp_spacing > 0 && !changed {
                let hills = self.over(2);
                let shore = self.tiles().find_map(|t| {
                    let i = self.index(t);
                    let h = self.height[i];
                    if h != 2
                        || !self.interior(t)
                        || reached[hills][i]
                        || skipped[hills][i]
                        || !self.layers[hills][i].is_floor()
                    {
                        return None;
                    }
                    let lower = |w: &World, side: Side| {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        (w.in_bounds(n) && w.height[w.index(n)] == 1).then_some(n)
                    };
                    let has_land = Side::ALL.into_iter().any(|side| {
                        lower(self, side)
                            .is_some_and(|n| self.layers[plains][self.index(n)].is_floor())
                    });
                    if has_land {
                        return None;
                    }
                    Side::ALL
                        .into_iter()
                        .find_map(|side| {
                            lower(self, side)
                                .filter(|n| self.layers[plains][self.index(*n)] == self.water)
                        })
                        .map(|n| (t, n))
                });
                if let Some((t, n)) = shore {
                    let far = self.cols + self.rows;
                    match self.crossing(plains, n, &reached[plains], far) {
                        Some(path) => {
                            self.lay_crossing(plains, path.into_iter().chain([n]).collect());
                        }
                        None => {
                            skipped[hills][self.index(t)] = true;
                        }
                    }
                    changed = true;
                }
            }
            // Bridges in the caves: cut-off cave floor joined across a
            // hole (or pool) to floor that's reached, if it's near.
            if !changed {
                for k in 0..m {
                    let cut_off = self.tiles().find(|&t| {
                        let i = self.index(t);
                        self.layers[k][i].is_floor() && !reached[k][i] && !skipped[k][i]
                    });
                    let Some(start) = cut_off else {
                        continue;
                    };
                    match self.crossing(k, start, &reached[k], 8) {
                        Some(path) => self.lay_crossing(k, path),
                        None => {
                            for t in self.region(k, start) {
                                skipped[k][self.index(t)] = true;
                            }
                        }
                    }
                    changed = true;
                    break;
                }
            }
            if !changed {
                break;
            }
        }
        // Underworld floor nobody can reach becomes rock: each pocket of it
        // wholly its own. (In a region, one reaching into the apron may be
        // joined up beyond it, and that's for the region next door, which
        // leaves it too, to see; a world made whole owns every pocket.)
        let (reached, grids) = self.reachable();
        let rock = self.rock;
        let [lo, hi] = self.own_box();
        let own = |[x, y]: [i32; 2]| x >= lo[0] && y >= lo[1] && x < hi[0] && y < hi[1];
        for k in 0..m {
            let pocket = |w: &World, i: usize| {
                grids[k][i] != Solid::Full && !reached[k][i] && w.layers[k][i] != w.water
            };
            let mut seen = vec![false; self.cols * self.rows];
            for start in 0..self.cols * self.rows {
                if seen[start] || !pocket(self, start) {
                    continue;
                }
                seen[start] = true;
                let (mut stack, mut tiles) = (vec![start], Vec::new());
                while let Some(i) = stack.pop() {
                    tiles.push(i);
                    let t = [(i % self.cols) as i32, (i / self.cols) as i32];
                    for side in Side::ALL {
                        let [dx, dy] = side.offset();
                        let n = [t[0] + dx, t[1] + dy];
                        if self.in_bounds(n) {
                            let j = self.index(n);
                            if !seen[j] && pocket(self, j) {
                                seen[j] = true;
                                stack.push(j);
                            }
                        }
                    }
                }
                let at = |i: usize| [(i % self.cols) as i32, (i / self.cols) as i32];
                if tiles.iter().all(|&i| own(at(i))) {
                    for i in tiles {
                        self.layers[k][i] = Tile::Whole(rock);
                    }
                }
            }
        }
    }
}

/// Which tiles of each layer the player can walk to from `start` on layer
/// `top`, across each layer's movement grid and up and down stairs.
pub fn reachable_across(level: &Level, top: usize, start: [i32; 2]) -> Vec<Vec<bool>> {
    let (cols, rows) = (level.cols(), level.rows());
    let grids: Vec<Vec<Solid>> = (0..level.depth())
        .map(|k| {
            let m = level.movement(k);
            (0..cols * rows)
                .map(|i| m.solid((i % cols) as i32, (i / cols) as i32))
                .collect()
        })
        .collect();
    let mut seeds: Vec<Vec<[i32; 2]>> = vec![Vec::new(); level.depth()];
    seeds[top].push(start);
    let mut reached = vec![vec![false; cols * rows]; level.depth()];
    loop {
        let mut grew = false;
        for k in 0..level.depth() {
            if seeds[k].is_empty() {
                continue;
            }
            let more = reachable_from(&grids[k], cols, rows, &std::mem::take(&mut seeds[k]));
            for i in 0..cols * rows {
                if more[i] && !reached[k][i] {
                    reached[k][i] = true;
                    grew = true;
                    let at = [(i % cols) as i32, (i / cols) as i32];
                    match level.layer(k).tile(at[0], at[1]).stairs() {
                        Some(Dir::Down) if k > 0 => seeds[k - 1].push(at),
                        Some(Dir::Up) if k + 1 < level.depth() => seeds[k + 1].push(at),
                        _ => {}
                    }
                }
            }
        }
        if !grew {
            return reached;
        }
    }
}

/// Which tiles can be reached from any of `starts` across open sides,
/// given where each tile is solid.
pub(super) fn reachable_from(
    tiles: &[Solid],
    cols: usize,
    rows: usize,
    starts: &[[i32; 2]],
) -> Vec<bool> {
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Solid::Full
        } else {
            tiles[y as usize * cols + x as usize]
        }
    };
    let mut seen = vec![false; cols * rows];
    let mut stack = starts.to_vec();
    while let Some([x, y]) = stack.pop() {
        if tile(x, y) == Solid::Full || seen[y as usize * cols + x as usize] {
            continue;
        }
        seen[y as usize * cols + x as usize] = true;
        for side in Side::ALL {
            let [dx, dy] = side.offset();
            let (nx, ny) = (x + dx, y + dy);
            if !tile(x, y).covers(side) && !tile(nx, ny).covers(side.opposite()) {
                stack.push([nx, ny]);
            }
        }
    }
    seen
}
