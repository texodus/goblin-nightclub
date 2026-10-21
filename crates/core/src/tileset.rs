//! Tile sheets: which cells of a sheet image draw which floors, walls,
//! liquids, stairs, props and sprites. A tileset is data a level carries (see
//! [`crate::level_file`]), as JSON with string keys; the sheet image itself
//! is loaded separately at runtime and handed to the renderer as a
//! [`Sheet`]. Nothing here knows any material by name: the engine only
//! asks a part's kind (floor, wall, liquid, void), its cells and its
//! properties (a wall's bevel colors, a liquid's translucency, how its
//! edges meet other materials: see [`Edge`] and [`Trim`]).
//!
//! In memory each table is a list in the file's order, and tiles refer to
//! entries by index ([`FloorId`], [`WallId`], [`LiquidId`]); files refer to
//! them by name, so a file survives its tables being reordered.

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::layer::{FloorId, LiquidId, Part, TILE, WallId};
use crate::palette::{self, Color};
use crate::prop::PropId;

/// A cell of the sheet: column and row.
pub type Cell = [u16; 2];

/// Entries a table may have: ids are a byte.
pub const MAX_ENTRIES: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Tileset {
    /// Edge of a cell in the sheet, in pixels. At the default zoom, a tile
    /// is drawn this many screen pixels across.
    pub cell: u32,
    /// Where the sheet image is: a URL, or a `data:` URL holding it.
    pub image: String,
    /// A solid white cell, for drawing plain colored shapes through the
    /// same pipeline (the sheet's color times the shape's).
    pub white: Cell,
    /// Stairs up, stairs down.
    pub stairs: [Cell; 2],
    pub floors: Vec<Floor>,
    pub walls: Vec<Wall>,
    pub liquids: Vec<Liquid>,
    /// Kinds of things standing on tiles (see [`crate::prop`]).
    pub props: Vec<PropKind>,
    pub sprites: Vec<Sprite>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Floor {
    pub name: String,
    /// Variants, picked per tile.
    pub cells: Vec<Cell>,
    pub edge: Edge,
    pub trim: Option<Trim>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub name: String,
    /// Variants, picked per tile: the wall seen from above.
    pub cells: Vec<Cell>,
    /// Bevel colors along faces facing up or left, and down or right; for
    /// a wall with a fringe (see [`Edge`]), its rim's.
    pub light: Color,
    pub dark: Color,
    pub edge: Edge,
    pub trim: Option<Trim>,
}

/// Void to the game (impassable, open to sight, the layer below showing
/// through), drawn as a translucent tile over what's beneath.
#[derive(Clone, Debug, PartialEq)]
pub struct Liquid {
    pub name: String,
    pub cells: Vec<Cell>,
    /// Opacity, 0 to 1.
    pub alpha: f32,
    /// Only its priority counts: a liquid has no fringe.
    pub edge: Edge,
    pub trim: Option<Trim>,
}

/// How a material's edges meet other materials. Where two meet, the one
/// that ranks higher grows a ragged fringe over the other, `width` pixels
/// of the art at most: walls rank over floors, floors over liquids,
/// liquids over void (stairs over everything), and within a kind the
/// higher `priority`. A material with no width keeps straight edges (and
/// a wall, its bevels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edge {
    pub priority: u8,
    /// In pixels of the art.
    pub width: f32,
    /// Blended over the outermost pixels of a floor's fringe, by its
    /// alpha, to outline it. (A wall's are its bevel colors.)
    pub rim: Option<Color>,
}

/// Small sprites scattered along a material's edges with other
/// materials, like tufts of grass, pebbles or foam.
#[derive(Clone, Debug, PartialEq)]
pub struct Trim {
    /// Sprites, picked at random.
    pub cells: Vec<Cell>,
    /// Chance of one along each edge of a tile, 0 to 1.
    pub density: f32,
    /// How far into the material each sits, from the edge, in pixels of
    /// the art: 0 straddles it.
    pub inset: f32,
}

/// A kind of thing standing on a tile, like a tree or a rock (see
/// [`crate::prop`]): drawn over the tiles and whoever's on them, and
/// maybe in the way of walking, or of seeing.
#[derive(Clone, Debug, PartialEq)]
pub struct PropKind {
    pub name: String,
    /// Variants, picked per tile: the top-left cell of each one's art.
    pub cells: Vec<Cell>,
    /// Cells of art across and down: more than one overhangs the tiles
    /// around its own.
    pub size: [u16; 2],
    /// Where its tile is in its art, in cells from the art's top-left
    /// corner: a tree two cells square with its trunk at the bottom middle
    /// is at `[0.5, 1]`.
    pub anchor: [f32; 2],
    /// How far up from its layer its art is drawn, in layers (see
    /// [`crate::draw::Vertex::z`]): a tree's canopy a little nearer the
    /// viewer than the ground.
    pub height: f32,
    /// Whether its tile is solid to walking.
    pub blocks: bool,
    /// The radius of a circle around its tile's center that sight can't
    /// pass, in pixels of the art: less than half a tile. 0 hides nothing.
    pub sight: f32,
    /// The light it gives off, if any: a torch's.
    pub light: Option<Light>,
}

/// Light given off by a prop (see [`PropKind::light`]), lighting what's
/// around it in its line of sight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// How far it reaches, in tiles.
    pub radius: f32,
    pub color: Color,
}

impl PropKind {
    /// How far from its tile its art reaches, in tiles: on each side, the
    /// most of left, top, right and bottom.
    pub fn overhang(&self) -> f32 {
        let [w, h] = self.size.map(f32::from);
        let [ax, ay] = self.anchor;
        [ax, ay, w - ax - 1.0, h - ay - 1.0]
            .into_iter()
            .fold(0.0, f32::max)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub name: String,
    pub cell: Cell,
}

impl Tileset {
    /// The floor type named `name`.
    pub fn floor(&self, name: &str) -> Option<FloorId> {
        self.floors
            .iter()
            .position(|f| f.name == name)
            .map(|i| i as FloorId)
    }

    /// The wall type named `name`.
    pub fn wall(&self, name: &str) -> Option<WallId> {
        self.walls
            .iter()
            .position(|w| w.name == name)
            .map(|i| i as WallId)
    }

    /// The liquid named `name`.
    pub fn liquid(&self, name: &str) -> Option<LiquidId> {
        self.liquids
            .iter()
            .position(|l| l.name == name)
            .map(|i| i as LiquidId)
    }

    /// The prop kind named `name`.
    pub fn prop(&self, name: &str) -> Option<PropId> {
        self.props
            .iter()
            .position(|p| p.name == name)
            .map(|i| i as PropId)
    }

    /// How far any prop's art reaches from its tile, in world units.
    pub fn prop_reach(&self) -> f32 {
        self.props
            .iter()
            .map(PropKind::overhang)
            .fold(0.0, f32::max)
            * TILE
    }

    pub fn sprite(&self, name: &str) -> Option<&Sprite> {
        self.sprites.iter().find(|s| s.name == name)
    }

    /// How `part`'s edges meet others' (void's are the defaults).
    pub fn edge(&self, part: Part) -> Edge {
        match part {
            Part::Floor(f) => self.floors[f as usize].edge,
            Part::Wall(w) => self.walls[w as usize].edge,
            Part::Liquid(l) => self.liquids[l as usize].edge,
            Part::Void => Edge::default(),
        }
    }

    /// What's scattered along `part`'s edges, if anything.
    pub fn trim(&self, part: Part) -> Option<&Trim> {
        match part {
            Part::Floor(f) => self.floors[f as usize].trim.as_ref(),
            Part::Wall(w) => self.walls[w as usize].trim.as_ref(),
            Part::Liquid(l) => self.liquids[l as usize].trim.as_ref(),
            Part::Void => None,
        }
    }

    /// Sheet pixels per world unit: geometry is in world units, [`TILE`] to
    /// a tile, whatever the art's size. Also the default view zoom (screen
    /// pixels per world unit), where the art is shown at its own size.
    pub fn zoom(&self) -> f32 {
        self.cell as f32 / TILE
    }

    /// Size of a sprite drawn in the world, in world units: a cell.
    pub fn sprite_size(&self) -> [f32; 2] {
        [TILE, TILE]
    }

    /// Every cell the tileset uses.
    pub fn cells(&self) -> impl Iterator<Item = Cell> + '_ {
        let trims = self
            .floors
            .iter()
            .map(|f| &f.trim)
            .chain(self.walls.iter().map(|w| &w.trim))
            .chain(self.liquids.iter().map(|l| &l.trim))
            .flatten()
            .flat_map(|t| t.cells.iter().copied());
        // Every cell of each prop's art.
        let props = self.props.iter().flat_map(|p| {
            let [w, h] = p.size;
            p.cells.iter().flat_map(move |&[x, y]| {
                (0..h).flat_map(move |dy| (0..w).map(move |dx| [x + dx, y + dy]))
            })
        });
        self.floors
            .iter()
            .flat_map(|f| f.cells.iter().copied())
            .chain(self.walls.iter().flat_map(|w| w.cells.iter().copied()))
            .chain(self.liquids.iter().flat_map(|l| l.cells.iter().copied()))
            .chain(trims)
            .chain(props)
            .chain(self.sprites.iter().map(|s| s.cell))
            .chain([self.white, self.stairs[0], self.stairs[1]])
    }

    /// Check the tileset against a sheet of `size` pixels: every cell must
    /// be inside it.
    pub fn check_sheet(&self, size: [u32; 2]) -> Result<(), String> {
        let (cols, rows) = (size[0] / self.cell, size[1] / self.cell);
        for c in self.cells() {
            if c[0] as u32 >= cols || c[1] as u32 >= rows {
                return Err(format!(
                    "cell {c:?} is outside the sheet ({cols} x {rows} cells)"
                ));
            }
        }
        Ok(())
    }

    /// Check the tileset itself: a cell size, something in every table
    /// that's drawn from, no table too long for its ids.
    fn check(&self) -> Result<(), String> {
        if self.cell == 0 {
            return Err("a tileset's cell size must be at least 1".into());
        }
        if self.floors.is_empty() || self.walls.is_empty() {
            return Err("a tileset needs at least one floor and one wall".into());
        }
        for (table, len) in [
            ("floors", self.floors.len()),
            ("walls", self.walls.len()),
            ("liquids", self.liquids.len()),
            ("props", self.props.len()),
        ] {
            if len > MAX_ENTRIES {
                return Err(format!("at most {MAX_ENTRIES} {table}"));
            }
        }
        let empty = self
            .floors
            .iter()
            .map(|f| (&f.name, f.cells.is_empty()))
            .chain(self.walls.iter().map(|w| (&w.name, w.cells.is_empty())))
            .chain(self.liquids.iter().map(|l| (&l.name, l.cells.is_empty())))
            .chain(self.props.iter().map(|p| (&p.name, p.cells.is_empty())))
            .find(|(_, empty)| *empty);
        if let Some((name, _)) = empty {
            return Err(format!("{name:?} has no cells"));
        }
        for p in &self.props {
            let anchored = (0..2).all(|i| (0.0..f32::from(p.size[i])).contains(&p.anchor[i]));
            if p.size.contains(&0) || !anchored {
                return Err(format!("prop {:?}: its tile must be in its art", p.name));
            }
            if !(0.0..self.cell as f32 / 2.0).contains(&p.sight) {
                return Err(format!(
                    "prop {:?}: what it hides of sight is in its tile",
                    p.name
                ));
            }
            if p.light
                .is_some_and(|l| !(l.radius > 0.0 && l.radius.is_finite()))
            {
                return Err(format!("prop {:?}: its light must reach somewhere", p.name));
            }
        }
        Ok(())
    }
}

/// Of `cells`, the variant for tile `(x, y)`: varied, but the same every
/// frame.
pub fn variant(cells: &[Cell], x: usize, y: usize) -> Cell {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    cells[h as usize % cells.len()]
}

/// A sheet image, decoded: its size in pixels, and RGBA pixels row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}

impl Sheet {
    /// A sheet of `size` pixels from `rgba`, if that's the right amount.
    pub fn new(size: [u32; 2], rgba: Vec<u8>) -> Result<Self, String> {
        if rgba.len() != (size[0] * size[1] * 4) as usize {
            return Err(format!(
                "a {} x {} sheet has {} bytes of RGBA, not {}",
                size[0],
                size[1],
                size[0] * size[1] * 4,
                rgba.len()
            ));
        }
        Ok(Self { size, rgba })
    }

    /// A single white pixel: what's drawn before any sheet is loaded, so
    /// plain shapes still show.
    pub fn white() -> Self {
        Self {
            size: [1, 1],
            rgba: vec![255; 4],
        }
    }
}

// --- JSON -----------------------------------------------------------------
// Tables are JSON objects keyed by name, in order.

#[derive(Serialize, Deserialize)]
pub(crate) struct Raw {
    pub(crate) cell: u32,
    pub(crate) image: String,
    pub(crate) white: Cell,
    pub(crate) stairs: RawStairs,
    pub(crate) floors: IndexMap<String, RawFloor>,
    pub(crate) walls: IndexMap<String, RawWall>,
    #[serde(default)]
    pub(crate) liquids: IndexMap<String, RawLiquid>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub(crate) props: IndexMap<String, RawProp>,
    #[serde(default)]
    pub(crate) sprites: IndexMap<String, RawSprite>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawProp {
    cells: Vec<Cell>,
    #[serde(default = "one_cell")]
    size: [u16; 2],
    #[serde(default)]
    anchor: [f32; 2],
    #[serde(default)]
    height: f32,
    #[serde(default)]
    blocks: bool,
    #[serde(default)]
    sight: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    light: Option<RawLight>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawLight {
    radius: f32,
    color: String,
}

fn one_cell() -> [u16; 2] {
    [1, 1]
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawStairs {
    up: Cell,
    down: Cell,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawFloor {
    cells: Vec<Cell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge: Option<RawEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trim: Option<RawTrim>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawWall {
    cells: Vec<Cell>,
    light: String,
    dark: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge: Option<RawEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trim: Option<RawTrim>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawLiquid {
    cells: Vec<Cell>,
    alpha: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    edge: Option<RawEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trim: Option<RawTrim>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawEdge {
    #[serde(default)]
    priority: u8,
    #[serde(default)]
    width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rim: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawTrim {
    cells: Vec<Cell>,
    density: f32,
    #[serde(default)]
    inset: f32,
}

impl Edge {
    fn raw(&self) -> Option<RawEdge> {
        (*self != Edge::default()).then(|| RawEdge {
            priority: self.priority,
            width: self.width,
            rim: self.rim.map(palette::hex_alpha),
        })
    }

    fn from_raw(raw: Option<RawEdge>, name: &str) -> Result<Edge, String> {
        let Some(raw) = raw else {
            return Ok(Edge::default());
        };
        let rim = raw
            .rim
            .map(|s| palette::parse_hex(&s).ok_or(format!("{name}: bad rim color {s:?}")))
            .transpose()?;
        Ok(Edge {
            priority: raw.priority,
            width: raw.width.max(0.0),
            rim,
        })
    }
}

impl Trim {
    fn raw(&self) -> RawTrim {
        RawTrim {
            cells: self.cells.clone(),
            density: self.density,
            inset: self.inset,
        }
    }

    fn from_raw(raw: RawTrim) -> Trim {
        Trim {
            cells: raw.cells,
            density: raw.density.clamp(0.0, 1.0),
            inset: raw.inset,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct RawSprite {
    cell: Cell,
}

impl Tileset {
    /// The tileset as the file holds it.
    pub(crate) fn raw(&self) -> Raw {
        Raw {
            cell: self.cell,
            image: self.image.clone(),
            white: self.white,
            stairs: RawStairs {
                up: self.stairs[0],
                down: self.stairs[1],
            },
            floors: self
                .floors
                .iter()
                .map(|f| {
                    (
                        f.name.clone(),
                        RawFloor {
                            cells: f.cells.clone(),
                            edge: f.edge.raw(),
                            trim: f.trim.as_ref().map(Trim::raw),
                        },
                    )
                })
                .collect(),
            walls: self
                .walls
                .iter()
                .map(|w| {
                    (
                        w.name.clone(),
                        RawWall {
                            cells: w.cells.clone(),
                            light: palette::hex(w.light),
                            dark: palette::hex(w.dark),
                            edge: w.edge.raw(),
                            trim: w.trim.as_ref().map(Trim::raw),
                        },
                    )
                })
                .collect(),
            liquids: self
                .liquids
                .iter()
                .map(|l| {
                    (
                        l.name.clone(),
                        RawLiquid {
                            cells: l.cells.clone(),
                            alpha: l.alpha,
                            edge: l.edge.raw(),
                            trim: l.trim.as_ref().map(Trim::raw),
                        },
                    )
                })
                .collect(),
            props: self
                .props
                .iter()
                .map(|p| {
                    (
                        p.name.clone(),
                        RawProp {
                            cells: p.cells.clone(),
                            size: p.size,
                            anchor: p.anchor,
                            height: p.height,
                            blocks: p.blocks,
                            sight: p.sight,
                            light: p.light.map(|l| RawLight {
                                radius: l.radius,
                                color: palette::hex(l.color),
                            }),
                        },
                    )
                })
                .collect(),
            sprites: self
                .sprites
                .iter()
                .map(|s| (s.name.clone(), RawSprite { cell: s.cell }))
                .collect(),
        }
    }
}

impl Serialize for Tileset {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.raw().serialize(s)
    }
}

impl<'de> Deserialize<'de> for Tileset {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Tileset, D::Error> {
        use serde::de::Error;
        let raw = Raw::deserialize(d)?;
        let color = |name: &str, s: &str| {
            palette::parse_hex(s)
                .ok_or_else(|| D::Error::custom(format!("{name}: bad color {s:?}")))
        };
        let edge = |raw, name: &str| Edge::from_raw(raw, name).map_err(D::Error::custom);
        let tileset = Tileset {
            cell: raw.cell,
            image: raw.image,
            white: raw.white,
            stairs: [raw.stairs.up, raw.stairs.down],
            floors: raw
                .floors
                .into_iter()
                .map(|(name, f)| {
                    Ok(Floor {
                        edge: edge(f.edge, &name)?,
                        trim: f.trim.map(Trim::from_raw),
                        name,
                        cells: f.cells,
                    })
                })
                .collect::<Result<_, D::Error>>()?,
            walls: raw
                .walls
                .into_iter()
                .map(|(name, w)| {
                    Ok(Wall {
                        light: color(&name, &w.light)?,
                        dark: color(&name, &w.dark)?,
                        edge: edge(w.edge, &name)?,
                        trim: w.trim.map(Trim::from_raw),
                        name,
                        cells: w.cells,
                    })
                })
                .collect::<Result<_, D::Error>>()?,
            liquids: raw
                .liquids
                .into_iter()
                .map(|(name, l)| {
                    Ok(Liquid {
                        edge: edge(l.edge, &name)?,
                        trim: l.trim.map(Trim::from_raw),
                        name,
                        cells: l.cells,
                        alpha: l.alpha.clamp(0.0, 1.0),
                    })
                })
                .collect::<Result<_, D::Error>>()?,
            props: raw
                .props
                .into_iter()
                .map(|(name, p)| {
                    let light = p
                        .light
                        .map(|l| {
                            Ok(Light {
                                radius: l.radius,
                                color: color(&name, &l.color)?,
                            })
                        })
                        .transpose()?;
                    Ok(PropKind {
                        name,
                        cells: p.cells,
                        size: p.size,
                        anchor: p.anchor,
                        height: p.height,
                        blocks: p.blocks,
                        sight: p.sight,
                        light,
                    })
                })
                .collect::<Result<_, D::Error>>()?,
            sprites: raw
                .sprites
                .into_iter()
                .map(|(name, s)| Sprite { name, cell: s.cell })
                .collect(),
        };
        tileset.check().map_err(D::Error::custom)?;
        Ok(tileset)
    }
}

/// A small tileset for tests: floors `plain`, `planks`, `dirt`, `grate`;
/// walls `rock`, `brick`, `metal`; liquid `water`; props `tree` (art two
/// cells square, overhanging) and `rock`, in cells free in the sheet's
/// first rows; sprites `player`, `npc`. Cells are laid out like the placeholder sheet's.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::*;

    pub fn tileset() -> Tileset {
        let floor = |name: &str, cells: &[Cell]| Floor {
            name: name.into(),
            cells: cells.to_vec(),
            edge: Edge::default(),
            trim: None,
        };
        let wall = |name: &str, cells: &[Cell], light, dark| Wall {
            name: name.into(),
            cells: cells.to_vec(),
            light,
            dark,
            edge: Edge::default(),
            trim: None,
        };
        Tileset {
            cell: 16,
            image: "tiles.png".into(),
            white: [0, 0],
            stairs: [[1, 0], [2, 0]],
            floors: vec![
                floor("plain", &[[0, 1], [0, 1], [0, 1], [1, 1]]),
                floor("planks", &[[2, 1], [3, 1]]),
                floor("dirt", &[[4, 1], [5, 1]]),
                floor("grate", &[[6, 1], [6, 1], [7, 1]]),
            ],
            walls: vec![
                wall("rock", &[[2, 2], [3, 2]], palette::YELLOW, palette::RED),
                wall(
                    "brick",
                    &[[0, 2], [1, 2]],
                    palette::LIGHT_RED,
                    palette::BLACK,
                ),
                wall(
                    "metal",
                    &[[4, 2], [5, 2]],
                    palette::WHITE,
                    palette::DARK_GRAY,
                ),
            ],
            liquids: vec![Liquid {
                name: "water".into(),
                cells: vec![[3, 0], [4, 0]],
                alpha: 0.55,
                edge: Edge::default(),
                trim: None,
            }],
            props: vec![
                PropKind {
                    name: "tree".into(),
                    cells: vec![[6, 3]],
                    size: [2, 2],
                    anchor: [0.5, 1.0],
                    height: 0.05,
                    blocks: true,
                    sight: 3.0,
                    light: None,
                },
                PropKind {
                    name: "rock".into(),
                    cells: vec![[4, 4], [5, 4]],
                    size: [1, 1],
                    anchor: [0.0, 0.0],
                    height: 0.0,
                    blocks: true,
                    sight: 6.0,
                    light: None,
                },
                PropKind {
                    name: "torch".into(),
                    cells: vec![[2, 3]],
                    size: [1, 1],
                    anchor: [0.0, 0.0],
                    height: 0.05,
                    blocks: true,
                    sight: 0.0,
                    light: Some(Light {
                        radius: 5.0,
                        color: palette::parse_hex("#ffb060").unwrap(),
                    }),
                },
            ],
            sprites: vec![
                Sprite {
                    name: "player".into(),
                    cell: [0, 3],
                },
                Sprite {
                    name: "npc".into(),
                    cell: [1, 3],
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_variants() {
        let t = testing::tileset();
        assert_eq!(t.walls[t.wall("brick").unwrap() as usize].name, "brick");
        assert_eq!(t.floors[t.floor("dirt").unwrap() as usize].name, "dirt");
        assert_eq!(t.liquid("water"), Some(0));
        assert_eq!(t.sprite("npc").unwrap().cell, [1, 3]);
        assert_eq!(t.floor("lava"), None);
        let cells = &t.floors[0].cells;
        assert_eq!(variant(cells, 3, 4), variant(cells, 3, 4));
        let picked: std::collections::HashSet<_> = (0..20).map(|x| variant(cells, x, 0)).collect();
        assert!(picked.len() > 1, "varies across tiles");
        assert_eq!(t.zoom(), 1.6);
    }

    #[test]
    fn round_trips_as_json_in_order() {
        let t = testing::tileset();
        let json = serde_json::to_string(&t).unwrap();
        let back: Tileset = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // Keys keep the file's order, so ids are stable.
        assert!(json.find("\"plain\"").unwrap() < json.find("\"planks\"").unwrap());
        assert!(json.contains("\"light\":\"#ffff55\""));
        let shuffled = json.replacen("\"plain\":{\"cells\":[[0,1],[0,1],[0,1],[1,1]]},", "", 1);
        let back: Tileset = serde_json::from_str(&shuffled).unwrap();
        assert_eq!(back.floor("planks"), Some(0), "first in the file");
    }

    #[test]
    fn edges_and_trims_round_trip_and_default() {
        let mut t = testing::tileset();
        t.floors[2].edge = Edge {
            priority: 3,
            width: 2.5,
            rim: Some([0.0, 0.0, 0.0, 0.4]),
        };
        t.walls[0].edge.width = 2.0;
        t.liquids[0].trim = Some(Trim {
            cells: vec![[9, 3], [10, 3]],
            density: 0.4,
            inset: 3.0,
        });
        let json = serde_json::to_string(&t).unwrap();
        // Close enough: the rim's alpha goes through a byte.
        let back: Tileset = serde_json::from_str(&json).unwrap();
        let rim = back.floors[2].edge.rim.unwrap();
        assert!((rim[3] - 0.4).abs() < 1.0 / 255.0);
        assert_eq!(
            (back.floors[2].edge.priority, back.floors[2].edge.width),
            (3, 2.5)
        );
        assert_eq!(back.walls[0].edge.width, 2.0);
        assert_eq!(back.liquids[0].trim, t.liquids[0].trim);
        assert_eq!(back.edge(Part::Liquid(0)), Edge::default());
        assert!(back.trim(Part::Floor(2)).is_none());
        // Left out when there's nothing to say, so older files stay as
        // they were.
        assert!(
            !serde_json::to_string(&testing::tileset())
                .unwrap()
                .contains("edge")
        );
        // Trims' cells count as the tileset's.
        assert!(back.cells().any(|c| c == [10, 3]));
        let bad = json.replacen("\"rim\":\"#00000066\"", "\"rim\":\"dark\"", 1);
        assert_ne!(bad, json);
        assert!(serde_json::from_str::<Tileset>(&bad).is_err());
    }

    #[test]
    fn bad_tilesets_are_refused() {
        let t = testing::tileset();
        let json = serde_json::to_string(&t).unwrap();
        for (from, to) in [
            ("\"cell\":16", "\"cell\":0"),
            ("\"#ffff55\"", "\"yellow\""),
            ("\"cells\":[[2,2],[3,2]]", "\"cells\":[]"),
            ("\"radius\":5.0", "\"radius\":0.0"),
            ("\"#ffb060\"", "\"warm\""),
        ] {
            let bad = json.replacen(from, to, 1);
            assert_ne!(bad, json);
            assert!(serde_json::from_str::<Tileset>(&bad).is_err(), "{to}");
        }
        assert!(t.check_sheet([128, 80]).is_ok());
        assert!(t.check_sheet([128, 48]).is_err(), "sprites' row cut off");
        assert!(Sheet::new([2, 1], vec![0; 8]).is_ok());
        assert!(Sheet::new([2, 1], vec![0; 7]).is_err());
    }
}
