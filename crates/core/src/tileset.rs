//! The tile sheet and what's in it, built in: art for floors, walls,
//! stairs and sprites, each a cell (or several, as variants) of
//! `assets/tiles.png` (decoded at compile time by `build.rs`). Floors and
//! walls are numbered in table order; levels refer to them by [`FloorId`]
//! and [`WallId`], generators by name.
//!
//! To add a tile, draw its cell into the sheet and add an entry here.

use crate::layer::{FloorId, TILE, WallId};
use crate::palette::{self, Color};

/// Edge of a cell in the sheet, in pixels. At the default zoom, a tile is
/// drawn this many screen pixels across.
pub const CELL: u32 = 16;

/// Sheet pixels per world unit: geometry is in world units, [`TILE`] to a
/// tile, whatever the art's size. Also the default view zoom (screen
/// pixels per world unit), where the art is shown at its own size.
pub const ZOOM: f32 = CELL as f32 / TILE;

/// The sheet as `build.rs` decoded it: width and height (little-endian
/// u32s), then RGBA pixels row by row.
static SHEET: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tiles.rgba"));

/// A cell of the sheet: column and row.
pub type Cell = [u16; 2];

/// Solid white, for drawing plain colored shapes through the same
/// pipeline (the sheet's color times the shape's).
pub const WHITE: Cell = [0, 0];
pub const STAIRS_UP: Cell = [1, 0];
pub const STAIRS_DOWN: Cell = [2, 0];

pub struct Floor {
    pub name: &'static str,
    /// Variants, picked per tile.
    pub cells: &'static [Cell],
}

pub struct Wall {
    pub name: &'static str,
    /// Variants, picked per tile: the wall seen from above.
    pub cells: &'static [Cell],
    /// Bevel colors along faces facing up or left, and down or right.
    pub light: Color,
    pub dark: Color,
}

pub struct Sprite {
    pub name: &'static str,
    pub cell: Cell,
}

pub const FLOORS: &[Floor] = &[
    Floor {
        name: "plain",
        cells: &[[0, 1], [0, 1], [0, 1], [1, 1]],
    },
    Floor {
        name: "planks",
        cells: &[[2, 1], [3, 1]],
    },
    Floor {
        name: "dirt",
        cells: &[[4, 1], [5, 1]],
    },
    Floor {
        name: "grate",
        cells: &[[6, 1], [6, 1], [7, 1]],
    },
];

pub const WALLS: &[Wall] = &[
    Wall {
        name: "rock",
        cells: &[[2, 2], [3, 2]],
        light: palette::YELLOW,
        dark: palette::RED,
    },
    Wall {
        name: "brick",
        cells: &[[0, 2], [1, 2]],
        light: palette::LIGHT_RED,
        dark: palette::BLACK,
    },
    Wall {
        name: "metal",
        cells: &[[4, 2], [5, 2]],
        light: palette::WHITE,
        dark: palette::DARK_GRAY,
    },
];

pub const SPRITES: &[Sprite] = &[
    Sprite {
        name: "player",
        cell: [0, 3],
    },
    Sprite {
        name: "npc",
        cell: [1, 3],
    },
];

/// The floor type named `name`.
pub fn floor(name: &str) -> FloorId {
    let i = FLOORS.iter().position(|f| f.name == name);
    i.unwrap_or_else(|| panic!("no floor {name:?}")) as FloorId
}

/// The wall type named `name`.
pub fn wall(name: &str) -> WallId {
    let i = WALLS.iter().position(|w| w.name == name);
    i.unwrap_or_else(|| panic!("no wall {name:?}")) as WallId
}

pub fn sprite(name: &str) -> &'static Sprite {
    let s = SPRITES.iter().find(|s| s.name == name);
    s.unwrap_or_else(|| panic!("no sprite {name:?}"))
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

/// Size of a sprite drawn in the world, in world units.
pub const SPRITE_SIZE: [f32; 2] = [CELL as f32 / ZOOM, CELL as f32 / ZOOM];

/// The sheet: its size in pixels, and RGBA pixels row by row.
pub struct Sheet {
    pub size: [u32; 2],
    pub rgba: &'static [u8],
}

pub fn sheet() -> Sheet {
    let word = |i: usize| u32::from_le_bytes(SHEET[i..i + 4].try_into().unwrap());
    Sheet {
        size: [word(0), word(4)],
        rgba: &SHEET[8..],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cell_is_in_the_sheet() {
        let sheet = sheet();
        assert_eq!(
            sheet.rgba.len(),
            (sheet.size[0] * sheet.size[1] * 4) as usize
        );
        let cols = (sheet.size[0] / CELL) as u16;
        let rows = (sheet.size[1] / CELL) as u16;
        let cells = FLOORS
            .iter()
            .flat_map(|f| f.cells)
            .chain(WALLS.iter().flat_map(|w| w.cells))
            .chain(SPRITES.iter().map(|s| &s.cell))
            .chain([&WHITE, &STAIRS_UP, &STAIRS_DOWN]);
        for c in cells {
            assert!(c[0] < cols && c[1] < rows, "{c:?} outside the sheet");
        }
        // Plain shapes draw from the white cell's top-left pixel.
        assert_eq!(&sheet.rgba[..4], &[255, 255, 255, 255]);
    }

    #[test]
    fn names_and_variants() {
        assert_eq!(WALLS[wall("brick") as usize].name, "brick");
        assert_eq!(FLOORS[floor("dirt") as usize].name, "dirt");
        assert_eq!(sprite("npc").cell, [1, 3]);
        let cells = FLOORS[0].cells;
        assert_eq!(variant(cells, 3, 4), variant(cells, 3, 4));
        let picked: std::collections::HashSet<_> = (0..20).map(|x| variant(cells, x, 0)).collect();
        assert!(picked.len() > 1, "varies across tiles");
    }
}
