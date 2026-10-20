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
mod tests {
    use super::*;

    const SIZE: usize = 64;

    #[test]
    fn runs_round_trip() {
        let mut bytes = vec![0u8; 600];
        bytes[300..310].fill(255);
        bytes[310] = 128;
        let runs = encode(&bytes);
        assert!(runs.len() < 20, "mostly one byte: {} runs", runs.len() / 2);
        assert_eq!(decode(&runs, bytes.len()), bytes);
        assert_eq!(decode(&encode(&[]), 0), Vec::<u8>::new());
    }

    #[test]
    fn pages_are_made_only_once_seen() {
        let mut p = Pages::new(SIZE, 2, 8);
        p.begin_frame();
        assert_eq!(p.read((0, 0, 0)), None, "never seen: nothing to read");
        assert!(p.take_ops().is_empty());
        let slot = p.write((0, 0, 0)).unwrap();
        assert_eq!(p.take_ops(), vec![Op::Clear(slot)]);
        assert_eq!(p.read((0, 0, 0)), Some(slot));
        assert_eq!(p.write((0, 0, 0)), Some(slot));
        assert!(p.take_ops().is_empty(), "already there");
    }

    #[test]
    fn the_atlas_grows_then_evicts_the_page_used_least_recently() {
        let mut p = Pages::new(SIZE, 1, 2);
        p.begin_frame();
        let a = p.write((0, 0, 0)).unwrap();
        let b = p.write((0, 1, 0)).unwrap();
        assert_eq!(p.take_ops(), vec![Op::Clear(a), Op::Grow(2), Op::Clear(b)]);
        assert_eq!(p.capacity(), 2);
        // Full, and both used this frame: no room.
        assert_eq!(p.write((0, 2, 0)), None);
        p.begin_frame();
        p.read((0, 1, 0));
        // Page (0, 0) went unused longest.
        let c = p.write((0, 2, 0)).unwrap();
        assert_eq!(c, a);
        let page = (0, 0, 0);
        assert_eq!(
            p.take_ops(),
            vec![Op::Evict { page, slot: a }, Op::Clear(a)]
        );
        // Until its pixels are back on the CPU it can't be had.
        p.begin_frame();
        assert_eq!(p.read(page), None);
        assert_eq!(p.write(page), None, "nor started afresh");
        let mut pixels = vec![0u8; SIZE];
        pixels[5] = 200;
        p.stored(page, &pixels);
        // Then it comes back, into the slot used least recently.
        p.begin_frame();
        p.read((0, 2, 0));
        let back = p.read(page).unwrap();
        assert_eq!(back, b);
        let ops = p.take_ops();
        assert_eq!(
            ops[0],
            Op::Evict {
                page: (0, 1, 0),
                slot: b
            }
        );
        assert_eq!(ops[1], Op::Upload(b, pixels));
    }

    #[test]
    fn pages_with_nothing_seen_are_dropped() {
        let mut p = Pages::new(SIZE, 1, 1);
        p.begin_frame();
        p.write((0, 0, 0));
        p.begin_frame();
        p.write((0, 1, 0));
        p.stored((0, 0, 0), &[0; SIZE]);
        p.take_ops();
        p.begin_frame();
        assert_eq!(p.read((0, 0, 0)), None, "nothing to bring back");
        assert!(p.take_ops().is_empty());
        let slot = p.write((0, 0, 0)).unwrap();
        assert!(matches!(p.take_ops()[..], [Op::Evict { .. }, Op::Clear(s)] if s == slot));
    }

    #[test]
    fn stale_read_backs_are_ignored() {
        let mut p = Pages::new(SIZE, 1, 1);
        p.stored((3, 0, 0), &[1; SIZE]);
        p.begin_frame();
        assert_eq!(p.read((3, 0, 0)), None, "never left, so never stored");
    }
}
