//! Levels saved as JSON: the size, where the player starts, the ground
//! level, which sprites the player and NPCs wear, the tileset (see
//! [`crate::tileset`]), and each layer's tiles, bottom layer first, row by
//! row. A level file is everything a level needs but the sheet image,
//! which [`Tileset::image`] points to.
//!
//! Each tile is a short string, naming floor, wall and liquid types from
//! the tileset (so files survive reordering its tables):
//!
//! - `void`, `floor:<name>`, `wall:<name>`, `liquid:<name>`: wholly that
//!   part;
//! - `<corner>/<part>/<part>`, e.g. `NW/wall:brick/floor:plain`: split
//!   along a diagonal, the first part in that corner's half;
//! - `up/<floor>`, `down/<floor>`: stairs, over that floor type.

use serde::{Deserialize, Serialize};

use crate::entity::PLAYER_SIZE;
use crate::layer::{Corner, Dir, Layer, Part, TILE, Tile};
use crate::level::{Level, MAX_LAYERS, MAX_TILES, can_stand};
use crate::tileset::Tileset;

/// The version written. Earlier formats (1: before tilesets were data; 2:
/// the spawn as a tile) aren't read.
pub const FORMAT: u32 = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct LevelFile {
    pub format: u32,
    pub cols: usize,
    pub rows: usize,
    pub spawn: Spawn,
    /// The ground level: the lowest layer under the open sky. Layers below
    /// it are underground: looked down to from outdoors, they're seen
    /// through dark mist rather than light. 0 puts the whole level above
    /// ground.
    pub ground: usize,
    /// How the player and the NPCs are drawn.
    pub player: Character,
    pub npc: Character,
    pub tileset: Tileset,
    /// Bottom first; each a list of rows, each a list of tiles.
    pub layers: Vec<Vec<Vec<Tile>>>,
}

/// Where the player starts: the center of their box, in world units
/// ([`TILE`] to a tile), on `layer`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    pub layer: usize,
    pub pos: [f32; 2],
}

impl Spawn {
    /// The middle of tile `tile` on `layer`.
    pub fn at_tile(layer: usize, tile: [i32; 2]) -> Self {
        Self {
            layer,
            pos: [(tile[0] as f32 + 0.5) * TILE, (tile[1] as f32 + 0.5) * TILE],
        }
    }
}

/// Everything about a level but its tiles: what a packed level's header
/// holds (see [`crate::packed`]), and what a game on a level of chunks
/// starts from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelMeta {
    pub cols: usize,
    pub rows: usize,
    /// Layers.
    pub depth: usize,
    pub spawn: Spawn,
    #[serde(default)]
    pub ground: usize,
    pub player: Character,
    pub npc: Character,
    pub tileset: Tileset,
}

/// Tiles across or down a level of chunks may have: a bound on what a
/// file may ask for.
pub const MAX_SIDE: usize = 1 << 20;

impl LevelMeta {
    /// Check what can be without the tiles (that the player fits at the
    /// spawn can't).
    pub fn check(&self) -> Result<(), String> {
        if self.depth == 0 || self.depth > MAX_LAYERS {
            return Err(format!("a level has 1 to {MAX_LAYERS} layers"));
        }
        if !(1..=MAX_SIDE).contains(&self.cols) || !(1..=MAX_SIDE).contains(&self.rows) {
            return Err(format!("a level is 1 to {MAX_SIDE} tiles across and down"));
        }
        let [x, y] = self.spawn.pos;
        let inside = (0.0..self.cols as f32 * TILE).contains(&x)
            && (0.0..self.rows as f32 * TILE).contains(&y);
        if self.spawn.layer >= self.depth || !inside {
            return Err("the spawn is outside the level".into());
        }
        if self.ground >= self.depth {
            return Err("the ground level is above the level's top".into());
        }
        for (who, c) in [("player", &self.player), ("npc", &self.npc)] {
            if self.tileset.sprite(&c.sprite).is_none() {
                return Err(format!(
                    "the {who}'s sprite {:?} isn't in the tileset",
                    c.sprite
                ));
            }
        }
        Ok(())
    }
}

/// How a kind of character is drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Character {
    /// A sprite's name in the tileset.
    pub sprite: String,
}

/// The file as it is on disk: tiles as text.
#[derive(Serialize, Deserialize)]
struct Raw {
    format: u32,
    cols: usize,
    rows: usize,
    spawn: Spawn,
    #[serde(default)]
    ground: usize,
    player: Character,
    npc: Character,
    tileset: Tileset,
    layers: Vec<Vec<Vec<String>>>,
}

impl LevelFile {
    /// `cols` x `rows` tiles a layer, from `layers` (bottom first, each row
    /// by row), drawn from `tileset`, the player and NPCs wearing the
    /// sprites named.
    pub fn new(
        cols: usize,
        rows: usize,
        layers: &[Vec<Tile>],
        spawn: Spawn,
        tileset: Tileset,
        player: &str,
        npc: &str,
    ) -> Self {
        Self {
            format: FORMAT,
            cols,
            rows,
            spawn,
            ground: 0,
            player: Character {
                sprite: player.into(),
            },
            npc: Character { sprite: npc.into() },
            tileset,
            layers: layers
                .iter()
                .map(|tiles| tiles.chunks(cols).map(<[Tile]>::to_vec).collect())
                .collect(),
        }
    }

    /// A blank level: `depth` layers of `cols` x `rows` of the tileset's
    /// first floor, the spawn in the middle of the top one. Sizes are
    /// clamped to what a level may have.
    pub fn blank(
        cols: usize,
        rows: usize,
        depth: usize,
        tileset: Tileset,
        player: &str,
        npc: &str,
    ) -> Self {
        let depth = depth.clamp(1, MAX_LAYERS);
        let cols = cols.clamp(1, MAX_TILES);
        let rows = rows.clamp(1, MAX_TILES / cols);
        let layers = vec![vec![Tile::FLOOR; cols * rows]; depth];
        let spawn = Spawn::at_tile(depth - 1, [(cols / 2) as i32, (rows / 2) as i32]);
        Self::new(cols, rows, &layers, spawn, tileset, player, npc)
    }

    /// The same level with `ground` as its ground level.
    pub fn with_ground(mut self, ground: usize) -> Self {
        self.ground = ground;
        self
    }

    /// As JSON: pretty, but with each row of tiles, and each entry of the
    /// tileset's tables, on a line of its own.
    pub fn to_json(&self) -> String {
        let layers: Vec<String> = self
            .layers
            .iter()
            .map(|rows| {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|r| {
                        let tiles: Vec<String> = r
                            .iter()
                            .map(|t| compact(&t.to_text(&self.tileset)))
                            .collect();
                        format!("      [{}]", tiles.join(","))
                    })
                    .collect();
                format!("    [\n{}\n    ]", rows.join(",\n"))
            })
            .collect();
        format!(
            "{{\n  \"format\": {},\n  \"cols\": {},\n  \"rows\": {},\n  \"spawn\": {},\n  \"ground\": {},\n  \"player\": {},\n  \"npc\": {},\n  \"tileset\": {},\n  \"layers\": [\n{}\n  ]\n}}\n",
            self.format,
            self.cols,
            self.rows,
            compact(&self.spawn),
            self.ground,
            compact(&self.player),
            compact(&self.npc),
            pretty_tileset(&self.tileset),
            layers.join(",\n")
        )
    }

    /// Parse and check a level: its format, sizes, spawn, tileset and the
    /// names its tiles and characters use.
    pub fn from_json(json: &str) -> Result<Self, String> {
        // The format first, so an old file is told what's wrong with it
        // rather than which field it lacks.
        #[derive(Deserialize)]
        struct Format {
            format: u32,
        }
        let Format { format } = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if format != FORMAT {
            return Err(format!(
                "level format {format} isn't supported; this version reads format {FORMAT}"
            ));
        }
        let raw: Raw = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if raw.layers.is_empty() || raw.layers.len() > MAX_LAYERS {
            return Err(format!("a level has 1 to {MAX_LAYERS} layers"));
        }
        if raw.cols == 0 || raw.rows == 0 || raw.cols * raw.rows > MAX_TILES {
            return Err(format!("a layer has 1 to {MAX_TILES} tiles"));
        }
        let mut layers = Vec::with_capacity(raw.layers.len());
        for (k, layer) in raw.layers.iter().enumerate() {
            if layer.len() != raw.rows || layer.iter().any(|r| r.len() != raw.cols) {
                return Err(format!("layer {k} isn't {} x {} tiles", raw.cols, raw.rows));
            }
            let rows: Result<Vec<Vec<Tile>>, String> = layer
                .iter()
                .map(|r| r.iter().map(|s| Tile::from_text(s, &raw.tileset)).collect())
                .collect();
            layers.push(rows.map_err(|e| format!("layer {k}: {e}"))?);
        }
        let [x, y] = raw.spawn.pos;
        let in_bounds = x >= 0.0
            && y >= 0.0
            && x < raw.cols as f32 * TILE
            && y < raw.rows as f32 * TILE
            && x.is_finite()
            && y.is_finite();
        if raw.spawn.layer >= raw.layers.len() || !in_bounds {
            return Err("the spawn is outside the level".into());
        }
        let tiles: Vec<Tile> = layers[raw.spawn.layer].concat();
        if !can_stand(&tiles, raw.cols, raw.rows, raw.spawn.pos, PLAYER_SIZE) {
            return Err("the player doesn't fit at the spawn".into());
        }
        if raw.ground >= raw.layers.len() {
            return Err("the ground level is above the level's top".into());
        }
        for (who, c) in [("player", &raw.player), ("npc", &raw.npc)] {
            if raw.tileset.sprite(&c.sprite).is_none() {
                return Err(format!(
                    "the {who}'s sprite {:?} isn't in the tileset",
                    c.sprite
                ));
            }
        }
        Ok(Self {
            format: raw.format,
            cols: raw.cols,
            rows: raw.rows,
            spawn: raw.spawn,
            ground: raw.ground,
            player: raw.player,
            npc: raw.npc,
            tileset: raw.tileset,
            layers,
        })
    }

    /// Everything about it but its tiles.
    pub fn meta(&self) -> LevelMeta {
        LevelMeta {
            cols: self.cols,
            rows: self.rows,
            depth: self.layers.len(),
            spawn: self.spawn,
            ground: self.ground,
            player: self.player.clone(),
            npc: self.npc.clone(),
            tileset: self.tileset.clone(),
        }
    }

    /// A level from `meta` and its tiles: each layer's, row by row.
    pub fn from_meta(meta: LevelMeta, tiles: Vec<Vec<Tile>>) -> Self {
        assert_eq!(tiles.len(), meta.depth);
        Self {
            format: FORMAT,
            cols: meta.cols,
            rows: meta.rows,
            spawn: meta.spawn,
            ground: meta.ground,
            player: meta.player,
            npc: meta.npc,
            tileset: meta.tileset,
            layers: tiles
                .into_iter()
                .map(|t| t.chunks(meta.cols).map(<[Tile]>::to_vec).collect())
                .collect(),
        }
    }

    /// Each layer's tiles, row by row.
    pub fn tiles(&self) -> Vec<Vec<Tile>> {
        self.layers.iter().map(|rows| rows.concat()).collect()
    }

    pub fn level(&self) -> Level {
        Level::new(
            self.tiles()
                .into_iter()
                .map(|tiles| Layer::from_tiles(self.cols, self.rows, tiles))
                .collect(),
            self.tileset.clone(),
        )
    }
}

/// `v` as compact JSON.
fn compact<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("levels always serialize")
}

/// The tileset as JSON, one entry of each table per line, indented to sit
/// inside the level's object.
fn pretty_tileset(t: &Tileset) -> String {
    let raw = t.raw();
    let table = |key: &str, entries: Vec<(&String, String)>| {
        if entries.is_empty() {
            return format!("    {}: {{}}", compact(&key));
        }
        let entries: Vec<String> = entries
            .iter()
            .map(|(name, e)| format!("      {}: {e}", compact(name)))
            .collect();
        format!("    {}: {{\n{}\n    }}", compact(&key), entries.join(",\n"))
    };
    let lines = [
        format!("    \"cell\": {}", raw.cell),
        format!("    \"image\": {}", compact(&raw.image)),
        format!("    \"white\": {}", compact(&raw.white)),
        format!("    \"stairs\": {}", compact(&raw.stairs)),
        table(
            "floors",
            raw.floors.iter().map(|(n, e)| (n, compact(e))).collect(),
        ),
        table(
            "walls",
            raw.walls.iter().map(|(n, e)| (n, compact(e))).collect(),
        ),
        table(
            "liquids",
            raw.liquids.iter().map(|(n, e)| (n, compact(e))).collect(),
        ),
        table(
            "sprites",
            raw.sprites.iter().map(|(n, e)| (n, compact(e))).collect(),
        ),
    ];
    format!("{{\n{}\n  }}", lines.join(",\n"))
}

fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::NW => "NW",
        Corner::NE => "NE",
        Corner::SW => "SW",
        Corner::SE => "SE",
    }
}

fn part_text(p: Part, tileset: &Tileset) -> String {
    match p {
        Part::Void => "void".into(),
        Part::Floor(f) => format!("floor:{}", tileset.floors[f as usize].name),
        Part::Wall(w) => format!("wall:{}", tileset.walls[w as usize].name),
        Part::Liquid(l) => format!("liquid:{}", tileset.liquids[l as usize].name),
    }
}

fn parse_part(s: &str, tileset: &Tileset) -> Result<Part, String> {
    let missing = |kind: &str, name: &str| format!("no {kind} type {name:?} in the tileset");
    match s.split_once(':') {
        None if s == "void" => Ok(Part::Void),
        Some(("floor", name)) => tileset
            .floor(name)
            .map(Part::Floor)
            .ok_or_else(|| missing("floor", name)),
        Some(("wall", name)) => tileset
            .wall(name)
            .map(Part::Wall)
            .ok_or_else(|| missing("wall", name)),
        Some(("liquid", name)) => tileset
            .liquid(name)
            .map(Part::Liquid)
            .ok_or_else(|| missing("liquid", name)),
        _ => Err(format!("not a part: {s:?}")),
    }
}

impl Tile {
    /// The tile as a level file writes it (see the module docs).
    pub fn to_text(&self, tileset: &Tileset) -> String {
        match *self {
            Tile::Whole(p) => part_text(p, tileset),
            Tile::Split { corner, at, rest } => format!(
                "{}/{}/{}",
                corner_name(corner),
                part_text(at, tileset),
                part_text(rest, tileset)
            ),
            Tile::Stairs(dir, floor) => {
                let dir = match dir {
                    Dir::Up => "up",
                    Dir::Down => "down",
                };
                format!("{dir}/{}", tileset.floors[floor as usize].name)
            }
        }
    }

    /// A tile from a level file's text, its names looked up in `tileset`.
    pub fn from_text(s: &str, tileset: &Tileset) -> Result<Tile, String> {
        let pieces: Vec<&str> = s.split('/').collect();
        match pieces[..] {
            [part] => parse_part(part, tileset).map(Tile::Whole),
            [dir @ ("up" | "down"), floor] => {
                let Part::Floor(f) = parse_part(&format!("floor:{floor}"), tileset)? else {
                    unreachable!()
                };
                let dir = if dir == "up" { Dir::Up } else { Dir::Down };
                Ok(Tile::Stairs(dir, f))
            }
            [corner, at, rest] => {
                let corner = Corner::CLOCKWISE
                    .into_iter()
                    .find(|c| corner_name(*c) == corner)
                    .ok_or_else(|| format!("not a corner: {corner:?}"))?;
                Ok(Tile::split(
                    corner,
                    parse_part(at, tileset)?,
                    parse_part(rest, tileset)?,
                ))
            }
            _ => Err(format!("not a tile: {s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tileset::testing::tileset;

    fn file(cols: usize, rows: usize, layers: &[Vec<Tile>], spawn: Spawn) -> LevelFile {
        LevelFile::new(cols, rows, layers, spawn, tileset(), "player", "npc")
    }

    #[test]
    fn tiles_round_trip_as_text() {
        let t = tileset();
        let brick = Part::Wall(t.wall("brick").unwrap());
        let planks = Part::Floor(t.floor("planks").unwrap());
        let water = Part::Liquid(t.liquid("water").unwrap());
        for tile in [
            Tile::VOID,
            Tile::Whole(brick),
            Tile::Whole(planks),
            Tile::split(Corner::NW, brick, planks),
            Tile::split(Corner::SE, planks, Part::Void),
            Tile::Whole(water),
            Tile::split(Corner::NE, planks, water),
            Tile::Stairs(Dir::Down, t.floor("grate").unwrap()),
        ] {
            let text = tile.to_text(&t);
            assert_eq!(Tile::from_text(&text, &t), Ok(tile), "{text}");
        }
        assert_eq!(
            Tile::split(Corner::NW, brick, planks).to_text(&t),
            "NW/wall:brick/floor:planks"
        );
        assert_eq!(Tile::Whole(water).to_text(&t), "liquid:water");
        assert_eq!(
            Tile::Stairs(Dir::Up, t.floor("plain").unwrap()).to_text(&t),
            "up/plain"
        );
        assert!(Tile::from_text("wall:nonsense", &t).is_err());
        assert!(Tile::from_text("water", &t).is_err(), "format 1 spelling");
        assert!(Tile::from_text("XX/void/void", &t).is_err());
    }

    #[test]
    fn levels_round_trip_as_json() {
        // Every kind of tile: wall, floor, void, liquid, splits over floor
        // and void, and stairs.
        let level = Level::new(
            vec![
                Layer::from_ascii(&["#..◸", "..>.", "###≈"]),
                Layer::from_ascii(&["~~◤▟", "~.<.", "...."]),
            ],
            tileset(),
        );
        let (cols, rows) = (4, 3);
        let layers: Vec<Vec<Tile>> = (0..level.depth())
            .map(|k| {
                (0..rows)
                    .flat_map(|y| (0..cols).map(move |x| (x, y)))
                    .map(|(x, y)| level.layer(k).tile(x, y))
                    .collect()
            })
            .collect();
        // A spawn between tiles, to the pixel.
        let spawn = Spawn {
            layer: level.depth() - 1,
            pos: [27.5, 24.25],
        };
        let file = file(cols as usize, rows as usize, &layers, spawn).with_ground(1);
        let json = file.to_json();
        let back = LevelFile::from_json(&json).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.spawn.pos, [27.5, 24.25]);
        assert_eq!(back.ground, 1);
        assert_eq!(back.tiles(), layers);
        assert_eq!(back.player.sprite, "player");
        let loaded = back.level();
        assert_eq!(loaded.depth(), level.depth());
        assert_eq!(loaded.layer(1).tile(3, 0), level.layer(1).tile(3, 0));
        assert_eq!(loaded.tileset(), &tileset());
        // Readable: a row of tiles and a tileset entry per line.
        assert!(json.contains("\n      [\"wall:rock\",\"floor:plain\",\"floor:plain\",\"NW/wall:rock/floor:plain\"],\n"));
        assert!(json.contains("\n      \"brick\": {\"cells\":[[0,2],[1,2]],\"light\":\"#ff5555\",\"dark\":\"#000000\"}"));
        assert!(json.contains("\n    \"cell\": 16,\n"));
    }

    #[test]
    fn bad_levels_are_refused() {
        let file = file(
            2,
            1,
            &[vec![Tile::FLOOR, Tile::FLOOR]],
            Spawn::at_tile(0, [0, 0]),
        );
        let mut wrong = file.clone();
        wrong.spawn.pos = [55.0, 5.0];
        assert!(LevelFile::from_json(&wrong.to_json()).is_err(), "outside");
        let mut tight = file.clone();
        tight.spawn.pos = [2.0, 5.0];
        assert!(
            LevelFile::from_json(&tight.to_json())
                .unwrap_err()
                .contains("fit"),
            "against the level's edge"
        );
        let mut walled = file.clone();
        walled.layers[0][0][0] = Tile::WALL;
        assert!(
            LevelFile::from_json(&walled.to_json()).is_err(),
            "in a wall"
        );
        walled.spawn.pos = [15.0, 5.0];
        assert!(LevelFile::from_json(&walled.to_json()).is_ok(), "beside it");
        let mut ragged = file.clone();
        ragged.layers[0][0].pop();
        assert!(LevelFile::from_json(&ragged.to_json()).is_err());
        let mut naked = file.clone();
        naked.player.sprite = "ghost".into();
        assert!(LevelFile::from_json(&naked.to_json()).is_err());
        assert!(LevelFile::from_json("{}").is_err());
        let good = file.to_json();
        assert!(LevelFile::from_json(&good).is_ok());
        // Names the tileset doesn't have, and the old format.
        let unknown = good.replacen("floor:plain", "floor:lava", 1);
        assert!(LevelFile::from_json(&unknown).unwrap_err().contains("lava"));
        let old = good.replacen("\"format\": 3", "\"format\": 2", 1);
        assert!(LevelFile::from_json(&old).unwrap_err().contains("format 2"));
    }
}
