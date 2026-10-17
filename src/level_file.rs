//! Levels saved as JSON, through serde: the size, where the player
//! starts, and each layer's tiles, bottom layer first, row by row.
//!
//! Each tile is a short string, naming floor and wall types (so files
//! survive reordering the tile sheet's tables):
//!
//! - `void`, `floor:<name>`, `wall:<name>`: wholly that part;
//! - `<corner>/<part>/<part>`, e.g. `NW/wall:brick/floor:plain`: split
//!   along a diagonal, the first part in that corner's half;
//! - `up/<floor>`, `down/<floor>`: stairs, over that floor type.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::layer::{Corner, Dir, Layer, Part, Tile};
use crate::level::{Level, MAX_LAYERS, MAX_TILES};
use crate::tileset;

/// The version written; older ones may need converting when this changes.
pub const FORMAT: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LevelFile {
    pub format: u32,
    pub cols: usize,
    pub rows: usize,
    pub spawn: Spawn,
    /// Bottom first; each a list of rows, each a list of tiles.
    pub layers: Vec<Vec<Vec<Tile>>>,
}

/// Where the player starts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    pub layer: usize,
    pub tile: [i32; 2],
}

impl LevelFile {
    /// `cols` x `rows` tiles a layer, from `layers` (bottom first, each row
    /// by row).
    pub fn new(cols: usize, rows: usize, layers: &[Vec<Tile>], spawn: Spawn) -> Self {
        Self {
            format: FORMAT,
            cols,
            rows,
            spawn,
            layers: layers
                .iter()
                .map(|tiles| tiles.chunks(cols).map(<[Tile]>::to_vec).collect())
                .collect(),
        }
    }

    /// As JSON: pretty, but with each row of tiles on a line of its own.
    pub fn to_json(&self) -> String {
        let layers: Vec<String> = self
            .layers
            .iter()
            .map(|rows| {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|r| format!("      {}", compact(r)))
                    .collect();
                format!("    [\n{}\n    ]", rows.join(",\n"))
            })
            .collect();
        format!(
            "{{\n  \"format\": {},\n  \"cols\": {},\n  \"rows\": {},\n  \"spawn\": {},\n  \"layers\": [\n{}\n  ]\n}}\n",
            self.format,
            self.cols,
            self.rows,
            compact(&self.spawn),
            layers.join(",\n")
        )
    }

    /// Parse and check a level: its format, sizes, and spawn.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let file: LevelFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if file.format != FORMAT {
            return Err(format!("unsupported level format {}", file.format));
        }
        if file.layers.is_empty() || file.layers.len() > MAX_LAYERS {
            return Err(format!("a level has 1 to {MAX_LAYERS} layers"));
        }
        if file.cols == 0 || file.rows == 0 || file.cols * file.rows > MAX_TILES {
            return Err(format!("a layer has 1 to {MAX_TILES} tiles"));
        }
        for (k, layer) in file.layers.iter().enumerate() {
            if layer.len() != file.rows || layer.iter().any(|r| r.len() != file.cols) {
                return Err(format!(
                    "layer {k} isn't {} x {} tiles",
                    file.cols, file.rows
                ));
            }
        }
        let [x, y] = file.spawn.tile;
        let in_bounds = x >= 0 && y >= 0 && (x as usize) < file.cols && (y as usize) < file.rows;
        if file.spawn.layer >= file.layers.len() || !in_bounds {
            return Err("the spawn is outside the level".into());
        }
        Ok(file)
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
        )
    }
}

/// `v` as compact JSON.
fn compact<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("levels always serialize")
}

fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::NW => "NW",
        Corner::NE => "NE",
        Corner::SW => "SW",
        Corner::SE => "SE",
    }
}

fn part_name(p: Part) -> String {
    match p {
        Part::Void => "void".into(),
        Part::Floor(f) => format!("floor:{}", tileset::FLOORS[f as usize].name),
        Part::Wall(w) => format!("wall:{}", tileset::WALLS[w as usize].name),
    }
}

fn parse_part(s: &str) -> Result<Part, String> {
    let named = |kind: &str, names: Vec<&str>, name: &str| {
        names
            .iter()
            .position(|n| *n == name)
            .map(|i| i as u8)
            .ok_or_else(|| format!("no {kind} type {name:?}"))
    };
    match s.split_once(':') {
        None if s == "void" => Ok(Part::Void),
        Some(("floor", name)) => named(
            "floor",
            tileset::FLOORS.iter().map(|f| f.name).collect(),
            name,
        )
        .map(Part::Floor),
        Some(("wall", name)) => named(
            "wall",
            tileset::WALLS.iter().map(|w| w.name).collect(),
            name,
        )
        .map(Part::Wall),
        _ => Err(format!("not a part: {s:?}")),
    }
}

impl std::fmt::Display for Tile {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Tile::Whole(p) => write!(f, "{}", part_name(p)),
            Tile::Split { corner, at, rest } => write!(
                f,
                "{}/{}/{}",
                corner_name(corner),
                part_name(at),
                part_name(rest)
            ),
            Tile::Stairs(dir, floor) => {
                let dir = match dir {
                    Dir::Up => "up",
                    Dir::Down => "down",
                };
                write!(f, "{dir}/{}", tileset::FLOORS[floor as usize].name)
            }
        }
    }
}

impl std::str::FromStr for Tile {
    type Err = String;

    fn from_str(s: &str) -> Result<Tile, String> {
        let pieces: Vec<&str> = s.split('/').collect();
        match pieces[..] {
            [part] => parse_part(part).map(Tile::Whole),
            [dir @ ("up" | "down"), floor] => {
                let Part::Floor(f) = parse_part(&format!("floor:{floor}"))? else {
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
                Ok(Tile::split(corner, parse_part(at)?, parse_part(rest)?))
            }
            _ => Err(format!("not a tile: {s:?}")),
        }
    }
}

impl Serialize for Tile {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Tile {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Tile, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_round_trip_as_text() {
        let brick = Part::Wall(tileset::wall("brick"));
        let planks = Part::Floor(tileset::floor("planks"));
        for tile in [
            Tile::VOID,
            Tile::Whole(brick),
            Tile::Whole(planks),
            Tile::split(Corner::NW, brick, planks),
            Tile::split(Corner::SE, planks, Part::Void),
            Tile::Stairs(Dir::Down, tileset::floor("grate")),
        ] {
            let text = tile.to_string();
            assert_eq!(text.parse::<Tile>(), Ok(tile), "{text}");
        }
        assert_eq!(
            Tile::split(Corner::NW, brick, planks).to_string(),
            "NW/wall:brick/floor:planks"
        );
        assert_eq!(
            Tile::Stairs(Dir::Up, tileset::floor("plain")).to_string(),
            "up/plain"
        );
        assert!("wall:nonsense".parse::<Tile>().is_err());
        assert!("XX/void/void".parse::<Tile>().is_err());
    }

    #[test]
    fn levels_round_trip_as_json() {
        let level = crate::generate::generate_level(
            crate::generate::Style::Layered,
            4,
            32,
            20,
            [165.0, 105.0],
        );
        let layers: Vec<Vec<Tile>> = (0..level.depth())
            .map(|k| {
                (0..20)
                    .flat_map(|y| (0..32).map(move |x| (x, y)))
                    .map(|(x, y)| level.layer(k).tile(x, y))
                    .collect()
            })
            .collect();
        let spawn = Spawn {
            layer: level.depth() - 1,
            tile: [16, 10],
        };
        let file = LevelFile::new(32, 20, &layers, spawn);
        let back = LevelFile::from_json(&file.to_json()).unwrap();
        assert_eq!(back, file);
        assert_eq!(back.tiles(), layers);
        let loaded = back.level();
        assert_eq!(loaded.depth(), level.depth());
        assert_eq!(loaded.layer(1).tile(5, 5), level.layer(1).tile(5, 5));
    }

    #[test]
    fn bad_levels_are_refused() {
        let file = LevelFile::new(
            2,
            1,
            &[vec![Tile::FLOOR, Tile::FLOOR]],
            Spawn {
                layer: 0,
                tile: [0, 0],
            },
        );
        let mut wrong = file.clone();
        wrong.spawn.tile = [5, 0];
        assert!(LevelFile::from_json(&wrong.to_json()).is_err());
        let mut ragged = file.clone();
        ragged.layers[0][0].pop();
        assert!(LevelFile::from_json(&ragged.to_json()).is_err());
        assert!(LevelFile::from_json("{}").is_err());
        assert!(LevelFile::from_json(&file.to_json()).is_ok());
    }
}
