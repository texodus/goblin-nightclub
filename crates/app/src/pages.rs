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

//! Which pages of the layers' `seen` masks are on the GPU. A layer's mask
//! is cut into square pages, a chunk of the level each, kept in slots of a
//! texture array (the atlas) only once something on them has been seen,
//! and only while they're in use: when the atlas is full the page used
//! least recently goes back to the CPU, run-length encoded, and comes back
//! when it's wanted again. So the GPU holds what's around the viewer, not
//! every layer of the level whole.
//!
//! [`Pages`] decides; the renderer carries out the [`Op`]s it asks for.
//! Nothing here touches the GPU, so it's tested natively.

use std::collections::{HashMap, HashSet};

/// A page of one layer's mask: the layer, then the page's column and row.
pub type Page = (usize, i32, i32);

/// What the renderer must do to the atlas for what [`Pages`] decided, in
/// order: every `Grow`, then every `Evict`, then the rest. (A slot is only
/// reused after it's been evicted, and only evicted when the atlas can't
/// grow, so that order keeps every slot's history straight.)
#[derive(Debug, PartialEq)]
pub enum Op {
    /// Make the atlas this many slots, keeping what's in it.
    Grow(u32),
    /// Copy slot `slot` out for the CPU, and hand it back with
    /// [`Pages::stored`]: `page` is leaving it.
    Evict { page: Page, slot: u32 },
    /// Clear a slot, for a page never seen before.
    Clear(u32),
    /// Fill a slot with a page's pixels, coming back from the CPU.
    Upload(u32, Vec<u8>),
}

pub struct Pages {
    /// Bytes in a page: a byte a pixel.
    size: usize,
    capacity: u32,
    max: u32,
    resident: HashMap<Page, Resident>,
    free: Vec<u32>,
    /// Pages put back on the CPU, run-length encoded.
    stored: HashMap<Page, Vec<u8>>,
    /// Pages on their way back to the CPU.
    leaving: HashSet<Page>,
    frame: u64,
    ops: Vec<Op>,
}

struct Resident {
    slot: u32,
    /// The frame it was last used in.
    used: u64,
}

impl Pages {
    /// Pages of `size` bytes, in an atlas of `capacity` slots that may grow
    /// to `max`.
    pub fn new(size: usize, capacity: u32, max: u32) -> Self {
        Self {
            size,
            capacity,
            max: max.max(capacity),
            resident: HashMap::new(),
            free: (0..capacity).rev().collect(),
            stored: HashMap::new(),
            leaving: HashSet::new(),
            frame: 0,
            ops: Vec::new(),
        }
    }

    /// A new frame: pages used in it won't be evicted during it.
    pub fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// The slot to read `page` from, if anything on it has been seen:
    /// brought back from the CPU if need be. `None` too while it's on its
    /// way out, or if there's no room for it.
    pub fn read(&mut self, page: Page) -> Option<u32> {
        if let Some(r) = self.resident.get_mut(&page) {
            r.used = self.frame;
            return Some(r.slot);
        }
        if self.leaving.contains(&page) || !self.stored.contains_key(&page) {
            return None;
        }
        let slot = self.allocate()?;
        let bytes = decode(&self.stored.remove(&page).unwrap(), self.size);
        self.ops.push(Op::Upload(slot, bytes));
        self.keep(page, slot);
        Some(slot)
    }

    /// The slot to draw into what's seen of `page`: as [`Pages::read`],
    /// or a cleared one for a page never seen. `None` while it's on its
    /// way out, or if there's no room for it.
    pub fn write(&mut self, page: Page) -> Option<u32> {
        if let Some(slot) = self.read(page) {
            return Some(slot);
        }
        if self.leaving.contains(&page) || self.stored.contains_key(&page) {
            return None;
        }
        let slot = self.allocate()?;
        self.ops.push(Op::Clear(slot));
        self.keep(page, slot);
        Some(slot)
    }

    /// `page`'s pixels, copied out after it was evicted. A page nothing on
    /// which was seen after all (it was drawn into only for being near
    /// what was) is as if never made.
    pub fn stored(&mut self, page: Page, pixels: &[u8]) {
        if self.leaving.remove(&page) && pixels.iter().any(|p| *p != 0) {
            self.stored.insert(page, encode(pixels));
        }
    }

    /// What to do to the atlas, since last asked.
    pub fn take_ops(&mut self) -> Vec<Op> {
        std::mem::take(&mut self.ops)
    }

    #[cfg(test)]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    fn keep(&mut self, page: Page, slot: u32) {
        let used = self.frame;
        self.resident.insert(page, Resident { slot, used });
    }

    /// A free slot: one never used, else a new one (growing the atlas),
    /// else the slot of the page used least recently, if not this frame.
    fn allocate(&mut self) -> Option<u32> {
        if let Some(slot) = self.free.pop() {
            return Some(slot);
        }
        if self.capacity < self.max {
            let grown = (self.capacity * 2).clamp(1, self.max);
            self.ops.push(Op::Grow(grown));
            self.free.extend((self.capacity..grown).rev());
            self.capacity = grown;
            return self.free.pop();
        }
        let (&page, &Resident { slot, .. }) = self
            .resident
            .iter()
            .filter(|(_, r)| r.used < self.frame)
            .min_by_key(|(_, r)| r.used)?;
        self.resident.remove(&page);
        self.leaving.insert(page);
        self.ops.push(Op::Evict { page, slot });
        Some(slot)
    }
}

/// `bytes` as runs: a length (1 to 255), then the byte repeated.
fn encode(bytes: &[u8]) -> Vec<u8> {
    let mut runs = Vec::new();
    let mut rest = bytes;
    while let Some(&b) = rest.first() {
        let n = rest.iter().take(255).take_while(|&&c| c == b).count();
        runs.extend([n as u8, b]);
        rest = &rest[n..];
    }
    runs
}

/// Runs from [`encode`] back to `size` bytes.
fn decode(runs: &[u8], size: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size);
    for run in runs.chunks_exact(2) {
        bytes.extend(std::iter::repeat_n(run[1], run[0] as usize));
    }
    debug_assert_eq!(bytes.len(), size);
    bytes
}

#[cfg(test)]
#[path = "pages_tests.rs"]
mod tests;
