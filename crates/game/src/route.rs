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

//! Walking where the player clicks: the route there, following it, and
//! stopping when it runs into something just come into view.

use super::*;

/// A planned walk: the tiles it crosses and the straightened waypoints
/// (centers) the player heads for in turn.
pub(super) struct Route {
    pub(super) tiles: Vec<[i32; 2]>,
    pub(super) waypoints: Vec<[f32; 2]>,
    pub(super) next: usize,
    /// Seconds spent barely moving.
    pub(super) stalled: f32,
}

pub(super) const PLAYER_SPEED: f32 = 60.0;

/// Pause on reaching the destination.
const AUTO_PAUSE: bool = true;

impl Game {
    /// Plan a walk to world point `p` on the player's layer and set off.
    /// Plans on what the player knows: unexplored tiles are assumed
    /// walkable. A click on known void is ignored (no clicking through
    /// holes, or into water). Where the player's box fits at `p`, the walk
    /// ends exactly there, by the straightest way; where it doesn't (a
    /// wall, the edge of a 45-degree tile) it ends as near as the box
    /// gets.
    pub(super) fn walk_to(&mut self, p: [f32; 2]) {
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
    pub(super) fn follow_route(&mut self, dt: f32) {
        let Some(route) = &mut self.route else {
            return;
        };
        let here = center(self.player, PLAYER_SIZE);
        let target = route.waypoints[route.next];
        let to = [target[0] - here[0], target[1] - here[1]];
        let dist = length(to);
        let step = (PLAYER_SPEED * dt).min(dist);
        if dist > 1e-3 {
            let delta = [to[0] / dist * step, to[1] / dist * step];
            self.player = self
                .level
                .movement(self.top)
                .move_box(self.player, PLAYER_SIZE, delta);
        }
        let now = center(self.player, PLAYER_SIZE);
        let moved = distance(here, now);
        route.stalled = if moved < 0.2 * step {
            route.stalled + dt
        } else {
            0.0
        };
        if distance(now, target) < 0.5 {
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
    pub(super) fn check_route(&mut self) {
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
}
