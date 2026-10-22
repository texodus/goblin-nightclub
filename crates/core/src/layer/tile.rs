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

//! A single tile: its parts (floor, wall, liquid, void, stairs), split
//! along a diagonal or whole, and the shapes they take.

use crate::geom::Shape;

/// Tile edge length in pixels.
pub const TILE: f32 = 10.0;

/// A corner of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    NW,
    NE,
    SW,
    SE,
}

/// A side of a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    N,
    S,
    W,
    E,
}

impl Corner {
    /// Clockwise on screen, starting top-left.
    pub const CLOCKWISE: [Corner; 4] = [Corner::NW, Corner::NE, Corner::SE, Corner::SW];

    /// Position within the tile, in tiles: 0 or 1 on each axis.
    pub fn offset(self) -> [i32; 2] {
        match self {
            Corner::NW => [0, 0],
            Corner::NE => [1, 0],
            Corner::SW => [0, 1],
            Corner::SE => [1, 1],
        }
    }

    /// The corner at `offset` within the tile (see [`Corner::offset`]).
    pub fn at(offset: [i32; 2]) -> Corner {
        match offset {
            [0, 0] => Corner::NW,
            [1, 0] => Corner::NE,
            [0, 1] => Corner::SW,
            _ => Corner::SE,
        }
    }

    /// This corner of tile `t`, in pixels.
    pub fn of_tile(self, t: [i32; 2]) -> [f32; 2] {
        let [dx, dy] = self.offset();
        [(t[0] + dx) as f32 * TILE, (t[1] + dy) as f32 * TILE]
    }

    pub fn opposite(self) -> Corner {
        let [x, y] = self.offset();
        Corner::at([1 - x, 1 - y])
    }

    /// The two corners joined by the diagonal face of a slope solid here.
    pub(super) fn face_ends(self) -> [Corner; 2] {
        match self {
            Corner::NW | Corner::SE => [Corner::NE, Corner::SW],
            Corner::NE | Corner::SW => [Corner::NW, Corner::SE],
        }
    }

    /// The two sides meeting at this corner.
    pub fn sides(self) -> [Side; 2] {
        match self {
            Corner::NW => [Side::N, Side::W],
            Corner::NE => [Side::N, Side::E],
            Corner::SW => [Side::S, Side::W],
            Corner::SE => [Side::S, Side::E],
        }
    }
}

impl Side {
    pub const ALL: [Side; 4] = [Side::N, Side::S, Side::W, Side::E];

    pub fn opposite(self) -> Side {
        match self {
            Side::N => Side::S,
            Side::S => Side::N,
            Side::W => Side::E,
            Side::E => Side::W,
        }
    }

    /// Offset to the neighboring tile across this side.
    pub fn offset(self) -> [i32; 2] {
        match self {
            Side::N => [0, -1],
            Side::S => [0, 1],
            Side::W => [-1, 0],
            Side::E => [1, 0],
        }
    }

    /// The corners at either end of this side.
    pub(super) fn ends(self) -> [Corner; 2] {
        match self {
            Side::N => [Corner::NW, Corner::NE],
            Side::S => [Corner::SW, Corner::SE],
            Side::W => [Corner::NW, Corner::SW],
            Side::E => [Corner::NE, Corner::SE],
        }
    }
}

/// Which way a staircase leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
}

/// Index of a wall type in the level's tileset ([`crate::tileset::Tileset::walls`]).
pub type WallId = u8;
/// Index of a floor type in the level's tileset ([`crate::tileset::Tileset::floors`]).
pub type FloorId = u8;
/// Index of a liquid in the level's tileset ([`crate::tileset::Tileset::liquids`]).
pub type LiquidId = u8;

/// What a part of a tile is made of. The engine knows only these kinds;
/// which material an id is (planks, rock, water) is the tileset's business.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Wall(WallId),
    Floor(FloorId),
    /// No floor: the layer below shows through.
    Void,
    /// A liquid: void to the game (impassable, open to sight, the layer
    /// below showing through), drawn as a translucent tile over what's
    /// beneath.
    Liquid(LiquidId),
}

/// The shape of some part of a tile: what rays, collision and outlines run
/// on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solid {
    None,
    Full,
    /// The right triangle at this corner, with the diagonal face running
    /// between the two neighboring corners.
    Slope(Corner),
}

/// One grid cell: wholly one part, or split along a diagonal into two.
/// Tiles are equal when made of the same parts in the same places (a
/// split can be given from either corner).
#[derive(Clone, Copy, Debug)]
pub enum Tile {
    Whole(Part),
    /// `corner`'s half is `at`, the other half `rest`. Never the same part
    /// on both sides (that's `Whole`).
    Split {
        corner: Corner,
        at: Part,
        rest: Part,
    },
    /// Floor leading to the layer above or below.
    Stairs(Dir, FloorId),
}

/// The tile point `p` (pixels) is in, and the quarter of it (as a bit;
/// see [`QUARTERS`]): the triangle between its diagonals.
pub fn quarter_at(p: [f32; 2]) -> ([i32; 2], u8) {
    let [x, y] = [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32];
    let (u, v) = (p[0] / TILE - x as f32, p[1] / TILE - y as f32);
    let quarter = if v < u.min(1.0 - u) {
        N
    } else if v > u.max(1.0 - u) {
        S
    } else if u < 0.5 {
        W
    } else {
        E
    };
    ([x, y], quarter)
}

/// The quarters of a tile, the triangles between its diagonals, as bits.
pub(super) const N: u8 = 1;
pub(super) const E: u8 = 2;
pub(super) const S: u8 = 4;
pub(super) const W: u8 = 8;
/// The quarters of a tile in bit order, as bits.
pub const QUARTERS: [u8; 4] = [N, E, S, W];

impl Corner {
    /// The quarters making up the half of a tile at this corner, as bits.
    pub fn quarters(self) -> u8 {
        match self {
            Corner::NW => N | W,
            Corner::NE => N | E,
            Corner::SE => S | E,
            Corner::SW => S | W,
        }
    }
}

impl Solid {
    /// Does it reach tile corner `c`, even just at a point?
    pub fn touches(self, c: Corner) -> bool {
        match self {
            Solid::None => false,
            Solid::Full => true,
            Solid::Slope(s) => c != s.opposite(),
        }
    }

    /// Does it cover the whole of side `side`?
    pub fn covers(self, side: Side) -> bool {
        match self {
            Solid::None => false,
            Solid::Full => true,
            Solid::Slope(s) => s.sides().contains(&side),
        }
    }

    /// For a slope, solidity as a linear function of tile-local `(u, v)` in
    /// `[0, 1]`: `k + g . (u, v)`, positive inside the solid, zero on the
    /// diagonal face. Returns `(k, g)`.
    pub(super) fn slope_plane(c: Corner) -> (f32, [f32; 2]) {
        match c {
            Corner::NW => (1.0, [-1.0, -1.0]),
            Corner::NE => (0.0, [1.0, -1.0]),
            Corner::SW => (0.0, [-1.0, 1.0]),
            Corner::SE => (-1.0, [1.0, 1.0]),
        }
    }

    /// Does this shape, as tile `tile`'s, overlap the pixel-space box
    /// `pos..pos+size`? Only the part of the box over the tile counts, and
    /// touching a surface doesn't.
    pub fn hits_box(self, tile: [i32; 2], pos: [f32; 2], size: [f32; 2]) -> bool {
        match self {
            Solid::None => false,
            Solid::Full => {
                let over = |a: usize| {
                    let (lo, hi) = (tile[a] as f32 * TILE, (tile[a] + 1) as f32 * TILE);
                    pos[a] + size[a] > lo + COLLIDE_EPS && pos[a] < hi - COLLIDE_EPS
                };
                over(0) && over(1)
            }
            Solid::Slope(c) => {
                // The box clipped to the tile, in tile-local units; solidity
                // is linear, so its maximum is at one of the box's corners.
                let local = |p: f32, a: usize| (p / TILE - tile[a] as f32).clamp(0.0, 1.0);
                let (u0, u1) = (local(pos[0], 0), local(pos[0] + size[0], 0));
                let (v0, v1) = (local(pos[1], 1), local(pos[1] + size[1], 1));
                let (k, g) = Solid::slope_plane(c);
                let u = if g[0] > 0.0 { u1 } else { u0 };
                let v = if g[1] > 0.0 { v1 } else { v0 };
                k + g[0] * u + g[1] * v > COLLIDE_EPS / TILE
            }
        }
    }

    /// As a set of quarters: a slope is two neighboring quarters, a full
    /// tile all four.
    pub fn quarters(self) -> u8 {
        match self {
            Solid::None => 0,
            Solid::Full => N | E | S | W,
            Solid::Slope(c) => c.quarters(),
        }
    }

    /// The shape made of quarters `q`; any other combination becomes full
    /// (`round_up`) or none.
    pub fn from_quarters(q: u8, round_up: bool) -> Solid {
        [
            Solid::None,
            Solid::Full,
            Solid::Slope(Corner::NW),
            Solid::Slope(Corner::NE),
            Solid::Slope(Corner::SE),
            Solid::Slope(Corner::SW),
        ]
        .into_iter()
        .find(|t| t.quarters() == q)
        .unwrap_or(if round_up { Solid::Full } else { Solid::None })
    }

    /// Wherever either is. A result no shape can represent (two slopes at
    /// neighboring corners) becomes full, which keeps unions
    /// order-independent.
    pub fn union(self, other: Solid) -> Solid {
        Solid::from_quarters(self.quarters() | other.quarters(), true)
    }

    /// Where `self` is and `other` isn't: cutting a slope out of a full
    /// tile leaves the opposite slope. A result no shape can represent
    /// becomes none.
    pub fn carve(self, other: Solid) -> Solid {
        Solid::from_quarters(self.quarters() & !other.quarters(), false)
    }

    /// This shape in tile `(x, y)` as a convex polygon in pixels.
    pub fn shape(self, x: usize, y: usize) -> Option<Shape> {
        let point = |c: Corner| c.of_tile([x as i32, y as i32]);
        match self {
            Solid::None => None,
            Solid::Full => Some(Corner::CLOCKWISE.map(point).to_vec()),
            Solid::Slope(s) => Some(
                Corner::CLOCKWISE
                    .into_iter()
                    .filter(|c| *c != s.opposite())
                    .map(point)
                    .collect(),
            ),
        }
    }
}

impl PartialEq for Tile {
    fn eq(&self, other: &Tile) -> bool {
        self.stairs() == other.stairs() && self.parts() == other.parts()
    }
}

impl Eq for Tile {}

impl Tile {
    /// Plain floor, wall, void and liquid, of the first type of each.
    pub const FLOOR: Tile = Tile::Whole(Part::Floor(0));
    pub const WALL: Tile = Tile::Whole(Part::Wall(0));
    pub const VOID: Tile = Tile::Whole(Part::Void);
    pub const LIQUID: Tile = Tile::Whole(Part::Liquid(0));

    /// Wall in `corner`'s half, floor in the rest.
    pub fn slope(corner: Corner) -> Tile {
        Tile::split(corner, Part::Wall(0), Part::Floor(0))
    }

    /// `at` in `corner`'s half and `rest` in the other, or wholly one part
    /// if they're the same.
    pub fn split(corner: Corner, at: Part, rest: Part) -> Tile {
        if at == rest {
            Tile::Whole(at)
        } else {
            Tile::Split { corner, at, rest }
        }
    }

    /// The default tile with shape `solid`: wall there, floor elsewhere.
    pub fn from_solid(solid: Solid) -> Tile {
        match solid {
            Solid::None => Tile::FLOOR,
            Solid::Full => Tile::WALL,
            Solid::Slope(c) => Tile::slope(c),
        }
    }

    /// The part in each quarter, in `N E S W` order. Stairs are floor.
    pub fn parts(self) -> [Part; 4] {
        match self {
            Tile::Whole(p) => [p; 4],
            Tile::Stairs(_, f) => [Part::Floor(f); 4],
            Tile::Split { corner, at, rest } => {
                QUARTERS.map(|q| if corner.quarters() & q != 0 { at } else { rest })
            }
        }
    }

    /// The tile with these quarters, if one can have them.
    fn from_parts(parts: [Part; 4]) -> Option<Tile> {
        if parts.iter().all(|p| *p == parts[0]) {
            return Some(Tile::Whole(parts[0]));
        }
        Corner::CLOCKWISE.into_iter().find_map(|c| {
            let at = |q: usize| c.quarters() & QUARTERS[q] != 0;
            let first = |inside: bool| (0..4).find(|&q| at(q) == inside).map(|q| parts[q]);
            let (a, b) = (first(true)?, first(false)?);
            (0..4)
                .all(|q| parts[q] == if at(q) { a } else { b })
                .then(|| Tile::split(c, a, b))
        })
    }

    /// The quarters where `f` holds of the part.
    fn quarters_where(self, f: impl Fn(Part) -> bool) -> u8 {
        let parts = self.parts();
        (0..4)
            .filter(|&q| f(parts[q]))
            .fold(0, |m, q| m | QUARTERS[q])
    }

    /// Where it's wall.
    pub fn wall(self) -> Solid {
        Solid::from_quarters(self.quarters_where(|p| matches!(p, Part::Wall(_))), true)
    }

    /// Where it's void (or liquid), open to the layer below.
    pub fn void(self) -> Solid {
        Solid::from_quarters(self.void_quarters(), true)
    }

    /// The quarters that are void or liquid, as bits: open to the layer
    /// below, impassable, and no block to sight.
    pub fn void_quarters(self) -> u8 {
        self.quarters_where(|p| matches!(p, Part::Void | Part::Liquid(_)))
    }

    /// The quarters that are liquid, as bits.
    pub fn liquid_quarters(self) -> u8 {
        self.quarters_where(|p| matches!(p, Part::Liquid(_)))
    }

    /// Its liquid, if any of it is liquid.
    pub fn liquid_id(self) -> Option<LiquidId> {
        self.parts().into_iter().find_map(|p| match p {
            Part::Liquid(l) => Some(l),
            _ => None,
        })
    }

    /// Its wall type, if any of it is wall.
    pub fn wall_id(self) -> Option<WallId> {
        self.parts().into_iter().find_map(|p| match p {
            Part::Wall(w) => Some(w),
            _ => None,
        })
    }

    /// Its floor type, if any of it is floor.
    pub fn floor_id(self) -> Option<FloorId> {
        self.parts().into_iter().find_map(|p| match p {
            Part::Floor(f) => Some(f),
            _ => None,
        })
    }

    /// Wholly floor (not stairs), of any type.
    pub fn is_floor(self) -> bool {
        matches!(self, Tile::Whole(Part::Floor(_)))
    }

    /// A whole tile with `corner`'s half made `part` (any kind): a 45-degree
    /// edge between the two. Stairs and tiles already split are left as
    /// they are.
    pub fn chamfer(self, corner: Corner, part: Part) -> Tile {
        match self {
            Tile::Whole(own) => Tile::split(corner, part, own),
            _ => self,
        }
    }

    pub fn stairs(self) -> Option<Dir> {
        match self {
            Tile::Stairs(d, _) => Some(d),
            _ => None,
        }
    }

    /// The tile with `piece`'s walls stamped on: wherever `piece` is wall,
    /// that wall; elsewhere unchanged (so a slope stamped over void keeps
    /// the void beside it). A result no tile can have becomes wholly
    /// `piece`'s wall, as solid shapes round up.
    pub fn stamp(self, piece: Tile) -> Tile {
        let (mine, theirs) = (self.parts(), piece.parts());
        let Some(wall) = theirs.iter().copied().find(|p| matches!(p, Part::Wall(_))) else {
            return self;
        };
        let parts = [0, 1, 2, 3].map(|q| match theirs[q] {
            Part::Wall(_) => theirs[q],
            _ => mine[q],
        });
        Tile::from_parts(parts).unwrap_or(Tile::Whole(wall))
    }

    /// The tile with `cut` replaced by `with` (floor or void). A result no
    /// tile can have becomes wholly `with`.
    pub fn carve(self, cut: Solid, with: Part) -> Tile {
        if cut == Solid::None {
            return self;
        }
        let mine = self.parts();
        let parts = [0, 1, 2, 3].map(|q| {
            if cut.quarters() & QUARTERS[q] != 0 {
                with
            } else {
                mine[q]
            }
        });
        Tile::from_parts(parts).unwrap_or(Tile::Whole(with))
    }

    /// The part each half of the tile is, for drawing: the half at
    /// `corner` and the rest, or the whole tile.
    pub fn halves(self) -> Halves {
        match self {
            Tile::Whole(p) => Halves::Whole(p),
            Tile::Stairs(_, f) => Halves::Whole(Part::Floor(f)),
            Tile::Split { corner, at, rest } => Halves::Split(corner, at, rest),
        }
    }
}

/// Quarters `q` (bits) of tile `(x, y)` as convex polygons in pixels: one
/// if they make a whole or half tile, else a triangle each.
pub fn quarter_shapes(x: usize, y: usize, q: u8) -> Vec<Shape> {
    let solid = Solid::from_quarters(q, false);
    if solid.quarters() == q {
        return solid.shape(x, y).into_iter().collect();
    }
    let point = |c: Corner| c.of_tile([x as i32, y as i32]);
    let center = [(x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE];
    [
        (N, Corner::NW, Corner::NE),
        (E, Corner::NE, Corner::SE),
        (S, Corner::SE, Corner::SW),
        (W, Corner::SW, Corner::NW),
    ]
    .into_iter()
    .filter(|(bit, _, _)| q & bit != 0)
    .map(|(_, a, b)| vec![point(a), point(b), center])
    .collect()
}

/// How a tile divides into parts, for drawing.
pub enum Halves {
    Whole(Part),
    Split(Corner, Part, Part),
}

/// Overlap (pixels) below which a box merely touches solid space.
pub(super) const COLLIDE_EPS: f32 = 1e-3;
