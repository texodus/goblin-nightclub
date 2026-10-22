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

//! Which chunks of the map (see [`goblin_nightclub_core::map`]) are on the
//! GPU: each in a slot of a buffer, put there when it's first wanted (or
//! has changed since), and let go only when there's no room for another.
//! The map itself stays on the CPU, so a chunk let go is just put back when
//! it's wanted again.
//!
//! [`MapPages`] decides; the renderer carries out the [`MapOp`]s it asks
//! for. Nothing here touches the GPU, so it's tested natively.

use std::collections::HashMap;

use goblin_nightclub_core::chunk::ChunkKey;

/// What the renderer must do for what [`MapPages`] decided, in order.
#[derive(Debug, PartialEq)]
pub enum MapOp {
    /// Make room for this many slots, keeping what's in them.
    Grow(u32),
    /// Put chunk `key`'s tiles in slot `slot`.
    Upload { slot: u32, key: ChunkKey },
}

pub struct MapPages {
    capacity: u32,
    max: u32,
    resident: HashMap<ChunkKey, Resident>,
    free: Vec<u32>,
    frame: u64,
    ops: Vec<MapOp>,
}

struct Resident {
    slot: u32,
    /// The chunk's version in it (see [`goblin_nightclub_core::map::MapChunk`]).
    version: u64,
    /// The frame it was last used in.
    used: u64,
}

impl MapPages {
    /// Room for `capacity` chunks, growing to `max`.
    pub fn new(capacity: u32, max: u32) -> Self {
        Self {
            capacity,
            max: max.max(capacity),
            resident: HashMap::new(),
            free: (0..capacity).rev().collect(),
            frame: 0,
            ops: Vec::new(),
        }
    }

    /// A new frame: chunks used in it won't be let go during it.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The slot to read chunk `key` from, as of `version`: put there if it
    /// isn't, or was at another version. `None` if there's no room for it.
    pub fn slot(&mut self, key: ChunkKey, version: u64) -> Option<u32> {
        if let Some(r) = self.resident.get_mut(&key) {
            r.used = self.frame;
            if r.version != version {
                r.version = version;
                self.ops.push(MapOp::Upload { slot: r.slot, key });
            }
            return Some(r.slot);
        }
        let slot = self.allocate()?;
        self.ops.push(MapOp::Upload { slot, key });
        let used = self.frame;
        self.resident.insert(
            key,
            Resident {
                slot,
                version,
                used,
            },
        );
        Some(slot)
    }

    /// What to do, since last asked.
    pub fn take_ops(&mut self) -> Vec<MapOp> {
        std::mem::take(&mut self.ops)
    }

    /// A free slot: one never used, else a new one (growing), else the
    /// slot of the chunk used least recently, if not this frame.
    fn allocate(&mut self) -> Option<u32> {
        if let Some(slot) = self.free.pop() {
            return Some(slot);
        }
        if self.capacity < self.max {
            let grown = (self.capacity * 2).clamp(1, self.max);
            self.ops.push(MapOp::Grow(grown));
            self.free.extend((self.capacity..grown).rev());
            self.capacity = grown;
            return self.free.pop();
        }
        let (&key, &Resident { slot, .. }) = self
            .resident
            .iter()
            .filter(|(_, r)| r.used < self.frame)
            .min_by_key(|(_, r)| r.used)?;
        self.resident.remove(&key);
        Some(slot)
    }
}

#[cfg(test)]
#[path = "map_pages_tests.rs"]
mod tests;
