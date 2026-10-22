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

//! Props on a generated world (see [`goblin_nightclub_core::prop`]): trees
//! on the plains' grass, thick in groves and absent between; rocks here
//! and there on cave floors; and torches, spread out, on any floor below
//! ground.
//!
//! Each stands alone: every tile around it open floor, with nothing else
//! standing there. So none can close a way through, a ring of floor always
//! going round it, and no level's connectivity, stairs or spawn needs
//! checking again. And each is decided by where it is in the whole world
//! and what's within two tiles of it, so a world made whole and one made a
//! region at a time (each region from its own tiles and apron) have the
//! same props.

use goblin_nightclub_core::layer::{Part, Tile};
use goblin_nightclub_core::prop::{Prop, PropId};

use super::{CAVE_FLOOR, PLAINS, SPAWN_CLEARANCE, tileset};
use crate::noise::{fbm, lattice};

/// Trees on at most this share of the plains' grass, where groves are
/// thickest.
const TREES: f32 = 0.3;
/// How big groves are, in tiles (the noise saying how thick trees are),
/// and below what of that noise there are none.
const GROVE_SCALE: f32 = 16.0;
const GROVE_FROM: f32 = 0.5;
/// Rocks on this share of cave floor.
const ROCKS: f32 = 0.03;
/// Below ground, the world is cut into blocks this many tiles square, each
/// with a spot for a torch somewhere in it but its edges (so no two are
/// side by side), and a torch there on this share of them: where it's
/// open floor all round. A torch stands rather than a rock beside it.
const TORCH_SPACING: i32 = 12;
const TORCHES: f32 = 0.6;

/// The props on `tiles` (each layer's, row by row, `size` across and
/// down, its top-left tile at `origin` in the whole world), within
/// `within` of them (from, and up to; at least two tiles in from their
/// edges, but at the world's), for a world from `seed` whose ground level
/// is `ground` and whose spawn is tile `spawn.1` on layer `spawn.0`: each
/// layer's, where they are from `within`'s corner.
pub(crate) fn place(
    seed: u32,
    tiles: &[Vec<Tile>],
    size: [usize; 2],
    origin: [i32; 2],
    within: [[i32; 2]; 2],
    ground: usize,
    spawn: (usize, [i32; 2]),
) -> Vec<Vec<Prop>> {
    let t = tileset();
    let kinds = |name: &str| t.prop(name).expect("the tileset has trees and rocks");
    let floor = |name: &str| t.floor(name).expect("the tileset has its floors");
    let (tree, rock, torch) = (kinds("tree"), kinds("rock"), kinds("torch"));
    let (grass, cave) = (floor(PLAINS), floor(CAVE_FLOOR));
    let [w, h] = size.map(|n| n as i32);
    tiles
        .iter()
        .enumerate()
        .map(|(k, layer)| {
            // Above ground, trees on grass in groves; below, rocks on caves'
            // floor.
            let (kind, on, outdoors): (PropId, _, _) = if k >= ground {
                (tree, grass, true)
            } else {
                (rock, cave, false)
            };
            let tile = |x: i32, y: i32| {
                ((0..w).contains(&x) && (0..h).contains(&y)).then(|| layer[(y * w + x) as usize])
            };
            let open = |x: i32, y: i32| matches!(tile(x, y), Some(Tile::Whole(Part::Floor(_))));
            let world = |x: i32, y: i32| [origin[0] + x, origin[1] + y];
            // How likely one is at (x, y), and its draw: one stands where
            // the draw is under the chance.
            let chance = |x: i32, y: i32| {
                if !outdoors {
                    return ROCKS;
                }
                let [wx, wy] = world(x, y).map(|v| v as f32 / GROVE_SCALE);
                let grove = fbm(seed ^ 0x6A09_E667, wx, wy);
                TREES * ((grove - GROVE_FROM) / (1.0 - GROVE_FROM)).clamp(0.0, 1.0)
            };
            let layer_seed = (k as u32).wrapping_mul(0x3C6E_F372);
            let draw = |x: i32, y: i32| {
                let [wx, wy] = world(x, y);
                lattice(seed ^ 0xBB67_AE85 ^ layer_seed, wx, wy)
            };
            // Room for one: open all round, clear of the spawn.
            let room = |x: i32, y: i32| {
                let [wx, wy] = world(x, y);
                let near_spawn = k == spawn.0
                    && (wx - spawn.1[0]).abs().max((wy - spawn.1[1]).abs()) <= SPAWN_CLEARANCE + 1;
                !near_spawn && (-1..=1).all(|dy| (-1..=1).all(|dx| open(x + dx, y + dy)))
            };
            // A torch here: below ground, at its block's spot, if its block
            // has one.
            let torch_at = |x: i32, y: i32| {
                let [wx, wy] = world(x, y);
                let block = [wx, wy].map(|v| v.div_euclid(TORCH_SPACING));
                let pick = |salt: u32| lattice(seed ^ salt ^ layer_seed, block[0], block[1]);
                let spot = |i: u32| {
                    let jitter = pick(0x9B05_688C + i) * (TORCH_SPACING - 2) as f32;
                    block[i as usize] * TORCH_SPACING + 1 + jitter as i32
                };
                !outdoors
                    && pick(0x510E_527F) < TORCHES
                    && [wx, wy] == [spot(0), spot(1)]
                    && room(x, y)
            };
            // Wanted here: on its floor, with room, and drawn.
            let wanted = |x: i32, y: i32| {
                tile(x, y) == Some(Tile::Whole(Part::Floor(on)))
                    && room(x, y)
                    && draw(x, y) < chance(x, y)
            };
            // Of those wanted side by side, the one with the lowest draw
            // stands (the first, by place, if two are the same).
            let first = |x: i32, y: i32| {
                let mine = (draw(x, y), [y, x]);
                (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| {
                        let (nx, ny) = (x + dx, y + dy);
                        (dx, dy) == (0, 0) || !wanted(nx, ny) || (draw(nx, ny), [ny, nx]) > mine
                    })
                })
            };
            let by_torch =
                |x: i32, y: i32| (-1..=1).any(|dy| (-1..=1).any(|dx| torch_at(x + dx, y + dy)));
            let [lo, hi] = within;
            (lo[1]..hi[1])
                .flat_map(|y| (lo[0]..hi[0]).map(move |x| (x, y)))
                .filter_map(|(x, y)| {
                    let kind = if torch_at(x, y) {
                        torch
                    } else if wanted(x, y) && first(x, y) && !by_torch(x, y) {
                        kind
                    } else {
                        return None;
                    };
                    Some(Prop {
                        at: [(x - lo[0]) as usize, (y - lo[1]) as usize],
                        kind,
                    })
                })
                .collect()
        })
        .collect()
}
