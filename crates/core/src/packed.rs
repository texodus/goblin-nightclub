//! Levels packed in chunks (see [`crate::chunk`]): a binary file a game
//! can read a piece at a time, from memory or from a server by range
//! requests. First come the header and an index of where each chunk's
//! bytes are; then each chunk, deflated on its own, is read only when
//! it's wanted. Also diffs: a level's chunks that differ from another's,
//! packed the same way, read as an [`Overlay`] over that other.
//!
//! The format, little-endian:
//!
//! - `GNPK`, then the version (u32)
//! - the header's length (u32), then the header: [`Header`] as JSON
//! - the index: for each chunk, its layer (u32), column and row (i32),
//!   and where its bytes are, an offset from the start (u64) and a length
//!   (u32)
//! - the roof index: for each chunk column (see [`Roof`]), its column and
//!   row (i32), and offset and length
//! - the bytes: each chunk and roof, deflated. A chunk is its palette's
//!   length (u16), its palette (6 bytes a tile: see [`encode_tile`]), and
//!   each tile's index into it, row by row: none if it holds one tile, a
//!   byte each while it holds up to 256, else two; then (from version 2)
//!   how many props stand on it (u16) and each one's tile, its index row
//!   by row (u16), and kind (u8), in order of tile. A roof is each tile's
//!   per quarter (u16), row by row.
//!
//! Version 1, without props, is read too: as if none stood anywhere.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec_with_limit;
use serde::{Deserialize, Serialize};

use crate::chunk::{CHUNK, Chunk, ChunkKey, Chunks};
use crate::layer::{Corner, Dir, Part, Tile};
use crate::level::MAX_TILES;
use crate::level_file::{LevelFile, LevelMeta};
use crate::prop::Prop;
use crate::tileset::Tileset;
use crate::world::{ChunkSource, FileSource, Roof, roofs};

const MAGIC: &[u8; 4] = b"GNPK";
const VERSION: u32 = 2;
/// Bytes of an index entry: a chunk's, a roof's.
const CHUNK_ENTRY: usize = 24;
const ROOF_ENTRY: usize = 20;
/// Chunks (and roofs) kept read, the most recently read.
const KEEP: usize = 4096;

/// What a packed level says about itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// Tells levels apart: a diff names the level it's over by this. A
    /// hash of everything else.
    pub id: String,
    /// For a diff, the id of the level it changes.
    #[serde(default)]
    pub base: Option<String>,
    /// Everything about the level but its tiles. For a diff, these replace
    /// its base's.
    pub level: LevelMeta,
    /// Tiles a side of a chunk.
    pub chunk: usize,
    /// Entries in the index and the roof index.
    pub chunks: usize,
    pub roofs: usize,
}

/// `meta`'s level, every chunk and roof of it from `source` (which must
/// have them all ready), packed.
pub fn pack(meta: &LevelMeta, source: &dyn ChunkSource) -> Vec<u8> {
    let [across, down] = [meta.cols.div_ceil(CHUNK), meta.rows.div_ceil(CHUNK)];
    let columns = || (0..down as i32).flat_map(move |y| (0..across as i32).map(move |x| (x, y)));
    let chunks = (0..meta.depth)
        .flat_map(|k| columns().map(move |(x, y)| (k, x, y)))
        .map(|key| (key, source.chunk(key).expect("every chunk ready")))
        .collect();
    let roofs = columns()
        .map(|(x, y)| ((x, y), source.roof(x, y).expect("every roof ready")))
        .collect();
    write(meta, None, chunks, roofs)
}

/// Just `chunks` and `roofs` of `meta`'s level, packed: a piece of it, to
/// hand over (from a worker making it, say) and read like any packed
/// level.
pub fn pack_part(
    meta: &LevelMeta,
    chunks: Vec<(ChunkKey, Arc<Chunk>)>,
    roofs: Vec<((i32, i32), Roof)>,
) -> Vec<u8> {
    write(meta, None, chunks, roofs)
}

/// A level file, packed.
pub fn pack_file(file: &LevelFile) -> Vec<u8> {
    pack(&file.meta(), &FileSource::new(file))
}

/// The chunks (and the roofs over them) of `edited` that differ from
/// `base`'s, packed as a diff over it (`base_id`, its id). `base` must
/// have them all ready, and be the same size.
pub fn diff(base: &dyn ChunkSource, base_id: &str, edited: &LevelFile) -> Result<Vec<u8>, String> {
    let meta = edited.meta();
    if base.size() != [meta.cols, meta.rows, meta.depth] {
        return Err("the level's been resized: save it whole".into());
    }
    let tiles = edited.tiles();
    let [across, down] = [meta.cols.div_ceil(CHUNK), meta.rows.div_ceil(CHUNK)];
    let mut chunks = Vec::new();
    for (k, layer) in tiles.iter().enumerate() {
        let grid = Chunks::from_tiles(meta.cols, meta.rows, layer);
        for (x, y) in (0..down).flat_map(|y| (0..across).map(move |x| (x, y))) {
            let key = (k, x as i32, y as i32);
            let theirs = base.chunk(key).ok_or("the base level isn't all loaded")?;
            if !grid.chunk(x, y).same(&theirs) {
                chunks.push((key, grid.chunk(x, y).clone()));
            }
        }
    }
    let mut roofs_changed = Vec::new();
    for (i, roof) in roofs(meta.cols, meta.rows, &tiles).into_iter().enumerate() {
        let at = ((i % across) as i32, (i / across) as i32);
        let theirs = base
            .roof(at.0, at.1)
            .ok_or("the base level isn't all loaded")?;
        if theirs != roof {
            roofs_changed.push((at, roof));
        }
    }
    Ok(write(&meta, Some(base_id), chunks, roofs_changed))
}

/// Pack `chunks` and `roofs` of `meta`'s level, as a diff over `base` if
/// that's given.
fn write(
    meta: &LevelMeta,
    base: Option<&str>,
    chunks: Vec<(ChunkKey, Arc<Chunk>)>,
    roofs: Vec<((i32, i32), Roof)>,
) -> Vec<u8> {
    let chunk_bytes: Vec<Vec<u8>> = chunks.iter().map(|(_, c)| encode_chunk(c)).collect();
    let roof_bytes: Vec<Vec<u8>> = roofs.iter().map(|(_, r)| encode_roof(r)).collect();
    // The id: a hash of all the rest.
    let mut h = Fnv::default();
    h.write(serde_json::to_string(meta).unwrap().as_bytes());
    h.write(base.unwrap_or("").as_bytes());
    for ((k, x, y), b) in chunks.iter().map(|(key, _)| key).zip(&chunk_bytes) {
        h.write(
            &[*k as u32 as i64, *x as i64, *y as i64]
                .map(i64::to_le_bytes)
                .concat(),
        );
        h.write(b);
    }
    for (((x, y), _), b) in roofs.iter().zip(&roof_bytes) {
        h.write(&[*x, *y].map(i32::to_le_bytes).concat());
        h.write(b);
    }
    let header = Header {
        id: format!("{:016x}", h.0),
        base: base.map(str::to_string),
        level: meta.clone(),
        chunk: CHUNK,
        chunks: chunks.len(),
        roofs: roofs.len(),
    };
    let header = serde_json::to_vec(&header).unwrap();
    let mut out = Vec::new();
    out.extend(MAGIC);
    out.extend(VERSION.to_le_bytes());
    out.extend((header.len() as u32).to_le_bytes());
    out.extend(&header);
    let mut offset = (out.len() + chunks.len() * CHUNK_ENTRY + roofs.len() * ROOF_ENTRY) as u64;
    for ((k, x, y), b) in chunks.iter().map(|(key, _)| key).zip(&chunk_bytes) {
        out.extend((*k as u32).to_le_bytes());
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        out.extend(offset.to_le_bytes());
        out.extend((b.len() as u32).to_le_bytes());
        offset += b.len() as u64;
    }
    for (((x, y), _), b) in roofs.iter().zip(&roof_bytes) {
        out.extend(x.to_le_bytes());
        out.extend(y.to_le_bytes());
        out.extend(offset.to_le_bytes());
        out.extend((b.len() as u32).to_le_bytes());
        offset += b.len() as u64;
    }
    for b in chunk_bytes.iter().chain(&roof_bytes) {
        out.extend(b);
    }
    out
}

/// 64-bit FNV-1a: plenty to tell levels apart.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
        }
    }
}

/// A tile in 6 bytes: what it is (0 whole, 1 split, 2 stairs), then for
/// whole its part's kind (0 wall, 1 floor, 2 void, 3 liquid) and id; for
/// split its corner's part and the rest's, then the corner (as in
/// [`Corner::CLOCKWISE`]); for stairs the floor's id and the way (0 up, 1
/// down).
pub fn encode_tile(tile: Tile) -> [u8; 6] {
    let part = |p: Part| match p {
        Part::Wall(w) => [0, w],
        Part::Floor(f) => [1, f],
        Part::Void => [2, 0],
        Part::Liquid(l) => [3, l],
    };
    match tile {
        Tile::Whole(p) => {
            let [k, id] = part(p);
            [0, k, id, 0, 0, 0]
        }
        Tile::Split { corner, at, rest } => {
            let ([ka, ia], [kr, ir]) = (part(at), part(rest));
            let c = Corner::CLOCKWISE.iter().position(|&c| c == corner).unwrap() as u8;
            [1, ka, ia, kr, ir, c]
        }
        Tile::Stairs(dir, f) => [2, f, (dir == Dir::Down) as u8, 0, 0, 0],
    }
}

/// [`encode_tile`] undone, its ids checked against `tileset`.
pub fn decode_tile(b: [u8; 6], tileset: &Tileset) -> Option<Tile> {
    let part = |k: u8, id: u8| {
        let fits = |n: usize| (id as usize) < n;
        match k {
            0 if fits(tileset.walls.len()) => Some(Part::Wall(id)),
            1 if fits(tileset.floors.len()) => Some(Part::Floor(id)),
            2 => Some(Part::Void),
            3 if fits(tileset.liquids.len()) => Some(Part::Liquid(id)),
            _ => None,
        }
    };
    match b[0] {
        0 => Some(Tile::Whole(part(b[1], b[2])?)),
        1 => {
            let (at, rest) = (part(b[1], b[2])?, part(b[3], b[4])?);
            let corner = *Corner::CLOCKWISE.get(b[5] as usize)?;
            (at != rest).then_some(Tile::Split { corner, at, rest })
        }
        2 if (b[1] as usize) < tileset.floors.len() && b[2] < 2 => {
            let dir = if b[2] == 1 { Dir::Down } else { Dir::Up };
            Some(Tile::Stairs(dir, b[1]))
        }
        _ => None,
    }
}

fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let palette = chunk.palette();
    let mut raw = Vec::with_capacity(2 + palette.len() * 6 + 2 * CHUNK * CHUNK);
    raw.extend((palette.len() as u16).to_le_bytes());
    for t in palette {
        raw.extend(encode_tile(*t));
    }
    match palette.len() {
        1 => {}
        ..=256 => raw.extend(chunk.indices().map(|i| i as u8)),
        _ => raw.extend(chunk.indices().flat_map(u16::to_le_bytes)),
    }
    let props = chunk.props();
    raw.extend((props.len() as u16).to_le_bytes());
    for &(t, kind) in props {
        raw.extend(t.to_le_bytes());
        raw.push(kind);
    }
    compress_to_vec(&raw, 6)
}

/// A chunk from its bytes, as packed in format `version`.
fn decode_chunk(bytes: &[u8], tileset: &Tileset, version: u32) -> Option<Chunk> {
    const CELLS: usize = CHUNK * CHUNK;
    let most = 2 + 6 * 1024 + 2 * CELLS + 2 + 3 * CELLS;
    let raw = decompress_to_vec_with_limit(bytes, most).ok()?;
    let n = u16::from_le_bytes(raw.get(0..2)?.try_into().ok()?) as usize;
    let (palette, rest) = raw.get(2..)?.split_at_checked(6 * n)?;
    let palette: Option<Vec<Tile>> = palette
        .chunks_exact(6)
        .map(|b| decode_tile(b.try_into().unwrap(), tileset))
        .collect();
    let (indices, rest): (Vec<u16>, &[u8]) = match n {
        1 => (vec![0; CELLS], rest),
        ..=256 => {
            let (indices, rest) = rest.split_at_checked(CELLS)?;
            (indices.iter().map(|&i| i as u16).collect(), rest)
        }
        _ => {
            let (indices, rest) = rest.split_at_checked(2 * CELLS)?;
            let indices = indices.chunks_exact(2);
            (
                indices.map(|b| u16::from_le_bytes([b[0], b[1]])).collect(),
                rest,
            )
        }
    };
    let chunk = Chunk::from_parts(palette?, &indices)?;
    if version < 2 {
        return rest.is_empty().then_some(chunk);
    }
    let count = u16::from_le_bytes(rest.get(0..2)?.try_into().ok()?) as usize;
    let entries = rest.get(2..)?;
    if entries.len() != 3 * count {
        return None;
    }
    let props: Option<Vec<_>> = entries
        .chunks_exact(3)
        .map(|b| {
            let (t, kind) = (u16::from_le_bytes([b[0], b[1]]), b[2]);
            ((t as usize) < CELLS && (kind as usize) < tileset.props.len()).then_some((t, kind))
        })
        .collect();
    Some(chunk.with_props(props?))
}

fn encode_roof(roof: &Roof) -> Vec<u8> {
    let raw: Vec<u8> = roof
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    compress_to_vec(&raw, 6)
}

fn decode_roof(bytes: &[u8]) -> Option<Roof> {
    let raw = decompress_to_vec_with_limit(bytes, 8 * CHUNK * CHUNK).ok()?;
    (raw.len() == 8 * CHUNK * CHUNK).then(|| {
        raw.chunks_exact(8)
            .map(|b| std::array::from_fn(|q| u16::from_le_bytes([b[2 * q], b[2 * q + 1]])))
            .collect()
    })
}

/// `meta`'s level whole, its tiles from `source`, as a level file: only
/// for one small enough, and with every chunk ready.
pub fn to_level_file(meta: &LevelMeta, source: &dyn ChunkSource) -> Result<LevelFile, String> {
    if meta.cols * meta.rows > MAX_TILES {
        return Err(format!(
            "it's too big to have whole: more than {MAX_TILES} tiles a layer"
        ));
    }
    let mut layers = Vec::with_capacity(meta.depth);
    let mut props = Vec::with_capacity(meta.depth);
    for k in 0..meta.depth {
        let mut tiles = Vec::with_capacity(meta.cols * meta.rows);
        let mut on = Vec::new();
        for y in 0..meta.rows {
            for x in 0..meta.cols {
                let key = (k, (x / CHUNK) as i32, (y / CHUNK) as i32);
                let chunk = source.chunk(key).ok_or("it isn't all loaded")?;
                tiles.push(chunk.get(x % CHUNK, y % CHUNK));
                if let Some(kind) = chunk.prop(x % CHUNK, y % CHUNK) {
                    on.push(Prop { at: [x, y], kind });
                }
            }
        }
        layers.push(tiles);
        props.push(on);
    }
    Ok(LevelFile::from_meta(meta.clone(), layers).with_props(props))
}

/// A level opened from files (see [`open_files`]).
pub enum Opened {
    /// A level file.
    File(LevelFile),
    /// A packed level, and any diffs over it: its chunks, what the last
    /// diff says about it, and the packed level they're all over.
    Chunks {
        source: Rc<dyn ChunkSource>,
        meta: LevelMeta,
        base: Rc<Packed<Arc<[u8]>>>,
    },
}

/// The level in `files`, all in memory: a level file (JSON) on its own,
/// or a packed level and any number of diffs over it, in any order (the
/// diffs taken in the order given).
pub fn open_files(files: Vec<Arc<[u8]>>) -> Result<Opened, String> {
    let is_json = |b: &[u8]| b.iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'{');
    if files.iter().any(|f| is_json(f)) {
        let [file] = &files[..] else {
            return Err("a level file is opened on its own".into());
        };
        let json = std::str::from_utf8(file).map_err(|_| "that level file isn't text")?;
        return LevelFile::from_json(json).map(Opened::File);
    }
    let mut bases = Vec::new();
    let mut diffs = Vec::new();
    for bytes in files {
        let packed = Packed::open(bytes.clone())?.ok_or("a file is cut short")?;
        match packed.header().base {
            None => bases.push(packed),
            Some(_) => diffs.push((packed.header().base.clone().unwrap(), bytes)),
        }
    }
    let [base] =
        <[_; 1]>::try_from(bases).map_err(|_| "pick one packed level, and any diffs over it")?;
    let base = Rc::new(base);
    let id = base.header().id.clone();
    let mut meta = base.header().level.clone();
    let mut source: Rc<dyn ChunkSource> = base.clone();
    for (over, bytes) in diffs {
        if over != id {
            return Err("a diff isn't of that level".into());
        }
        let overlay = Overlay::new(source, &id, bytes)?;
        meta = overlay.meta().clone();
        source = Rc::new(overlay);
    }
    Ok(Opened::Chunks { source, meta, base })
}

/// Where a packed level's bytes come from.
pub trait Bytes {
    /// `len` bytes from `at`, if they're here. A slow source (a server)
    /// starts getting bytes when first asked for them, and has them for a
    /// later ask.
    fn read(&self, at: u64, len: usize) -> Option<Arc<[u8]>>;
}

impl Bytes for Arc<[u8]> {
    fn read(&self, at: u64, len: usize) -> Option<Arc<[u8]>> {
        let at = usize::try_from(at).ok()?;
        Some(
            self.get(at..at.checked_add(len)?)
                .unwrap_or_default()
                .into(),
        )
    }
}

/// A packed level (or diff), read from `B` a piece at a time.
pub struct Packed<B> {
    bytes: B,
    /// Its format's version.
    version: u32,
    header: Header,
    chunks: HashMap<ChunkKey, (u64, u32)>,
    roofs: HashMap<(i32, i32), (u64, u32)>,
    /// What's been read, the most recent last.
    kept: Mutex<Kept>,
    /// What was wrong with the first chunk or roof that wouldn't read.
    problem: Mutex<Option<String>>,
}

#[derive(Default)]
struct Kept {
    chunks: HashMap<ChunkKey, Arc<Chunk>>,
    roofs: HashMap<(i32, i32), Roof>,
    order: VecDeque<Result<ChunkKey, (i32, i32)>>,
}

impl Kept {
    fn keep(&mut self, which: Result<ChunkKey, (i32, i32)>) {
        self.order.push_back(which);
        if self.order.len() > KEEP {
            match self.order.pop_front().unwrap() {
                Ok(key) => drop(self.chunks.remove(&key)),
                Err(at) => drop(self.roofs.remove(&at)),
            }
        }
    }
}

impl<B: Bytes> Packed<B> {
    /// Read a packed level's header and index from `bytes`: `None` while
    /// they're still coming (ask again later).
    pub fn open(bytes: B) -> Result<Option<Self>, String> {
        let Some(head) = bytes.read(0, 12) else {
            return Ok(None);
        };
        if head.len() < 12 || &head[0..4] != MAGIC {
            return Err("that isn't a packed level".into());
        }
        let version = u32::from_le_bytes(head[4..8].try_into().unwrap());
        if !(1..=VERSION).contains(&version) {
            return Err(format!(
                "packed level version {version} isn't supported; this version reads up to {VERSION}"
            ));
        }
        let len = u32::from_le_bytes(head[8..12].try_into().unwrap()) as usize;
        let Some(json) = bytes.read(12, len) else {
            return Ok(None);
        };
        let header: Header =
            serde_json::from_slice(&json).map_err(|e| format!("its header: {e}"))?;
        header.level.check()?;
        if header.chunk != CHUNK {
            return Err(format!(
                "its chunks are {} tiles, not {CHUNK}",
                header.chunk
            ));
        }
        let index_len = header.chunks * CHUNK_ENTRY + header.roofs * ROOF_ENTRY;
        let Some(index) = bytes.read(12 + len as u64, index_len) else {
            return Ok(None);
        };
        if index.len() != index_len {
            return Err("its index is cut short".into());
        }
        let (entries, roof_entries) = index.split_at(header.chunks * CHUNK_ENTRY);
        let le32 = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
        let le64 = |b: &[u8]| u64::from_le_bytes(b.try_into().unwrap());
        let chunks = entries
            .chunks_exact(CHUNK_ENTRY)
            .map(|e| {
                let key = (
                    le32(&e[0..4]) as usize,
                    le32(&e[4..8]) as i32,
                    le32(&e[8..12]) as i32,
                );
                (key, (le64(&e[12..20]), le32(&e[20..24])))
            })
            .collect();
        let roofs = roof_entries
            .chunks_exact(ROOF_ENTRY)
            .map(|e| {
                let at = (le32(&e[0..4]) as i32, le32(&e[4..8]) as i32);
                (at, (le64(&e[8..16]), le32(&e[16..20])))
            })
            .collect();
        Ok(Some(Self {
            bytes,
            version,
            header,
            chunks,
            roofs,
            kept: Default::default(),
            problem: Default::default(),
        }))
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// What was wrong with the first chunk or roof that wouldn't read, if
    /// any did: a wall stands in for each such chunk, open sky for each
    /// such roof.
    pub fn problem(&self) -> Option<String> {
        self.problem.lock().unwrap().clone()
    }

    fn trouble(&self, what: String) {
        self.problem.lock().unwrap().get_or_insert(what);
    }

    /// The level whole, as a level file: only for one small enough, and
    /// with every chunk ready.
    pub fn to_level_file(&self) -> Result<LevelFile, String> {
        to_level_file(&self.header.level, self)
    }

    /// The bytes of a chunk or roof the index puts at `at`.
    fn read(&self, (at, len): (u64, u32)) -> Option<Arc<[u8]>> {
        self.bytes.read(at, len as usize)
    }
}

impl<B: Bytes> ChunkSource for Packed<B> {
    fn size(&self) -> [usize; 3] {
        let l = &self.header.level;
        [l.cols, l.rows, l.depth]
    }

    fn tileset(&self) -> &Tileset {
        &self.header.level.tileset
    }

    fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
        if let Some(chunk) = self.kept.lock().unwrap().chunks.get(&key) {
            return Some(chunk.clone());
        }
        let bytes = self.read(*self.chunks.get(&key)?)?;
        let chunk = Arc::new(
            decode_chunk(&bytes, self.tileset(), self.version).unwrap_or_else(|| {
                self.trouble(format!("chunk {key:?} won't read"));
                Chunk::filled(Tile::WALL)
            }),
        );
        let mut kept = self.kept.lock().unwrap();
        kept.chunks.insert(key, chunk.clone());
        kept.keep(Ok(key));
        Some(chunk)
    }

    fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
        if let Some(roof) = self.kept.lock().unwrap().roofs.get(&(cx, cy)) {
            return Some(roof.clone());
        }
        let bytes = self.read(*self.roofs.get(&(cx, cy))?)?;
        let roof = decode_roof(&bytes).unwrap_or_else(|| {
            self.trouble(format!("the roof over ({cx}, {cy}) won't read"));
            vec![[0; 4]; CHUNK * CHUNK].into()
        });
        let mut kept = self.kept.lock().unwrap();
        kept.roofs.insert((cx, cy), roof.clone());
        kept.keep(Err((cx, cy)));
        Some(roof)
    }
}

/// A level with some of its chunks changed: a diff over another level.
pub struct Overlay {
    base: Rc<dyn ChunkSource>,
    meta: LevelMeta,
    chunks: HashMap<ChunkKey, Arc<Chunk>>,
    roofs: HashMap<(i32, i32), Roof>,
}

impl Overlay {
    /// `diff`, a packed diff (all of it), over `base`, whose id is
    /// `base_id`.
    pub fn new(base: Rc<dyn ChunkSource>, base_id: &str, diff: Arc<[u8]>) -> Result<Self, String> {
        let diff = Packed::open(diff)?.ok_or("the diff is cut short")?;
        let header = diff.header().clone();
        if header.base.as_deref() != Some(base_id) {
            return Err("that diff isn't of this level".into());
        }
        let meta = header.level;
        if base.size() != [meta.cols, meta.rows, meta.depth] {
            return Err("that diff is of a level another size".into());
        }
        let chunks = (diff.chunks.keys())
            .map(|&key| (key, diff.chunk(key).unwrap()))
            .collect();
        let roofs = (diff.roofs.keys())
            .map(|&(x, y)| ((x, y), diff.roof(x, y).unwrap()))
            .collect();
        if let Some(problem) = diff.problem() {
            return Err(format!("the diff: {problem}"));
        }
        Ok(Self {
            base,
            meta,
            chunks,
            roofs,
        })
    }

    /// The level as changed: these replace the base's.
    pub fn meta(&self) -> &LevelMeta {
        &self.meta
    }
}

impl ChunkSource for Overlay {
    fn size(&self) -> [usize; 3] {
        self.base.size()
    }

    fn tileset(&self) -> &Tileset {
        &self.meta.tileset
    }

    fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
        self.chunks
            .get(&key)
            .cloned()
            .or_else(|| self.base.chunk(key))
    }

    fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
        self.roofs
            .get(&(cx, cy))
            .cloned()
            .or_else(|| self.base.roof(cx, cy))
    }
}

#[cfg(test)]
#[path = "tests/packed_tests.rs"]
mod tests;
