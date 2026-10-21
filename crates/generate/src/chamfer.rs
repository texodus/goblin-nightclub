//! 45-degree edges where two kinds of tile meet in a stair step: walls
//! into floor, hole rims and shores both ways, rock into pools.

use super::*;

impl World {
    /// Put 45-degree edges where two kinds of tile meet in a stair step
    /// (see [`chamfer_layer`]), on every layer, bottom up: walls into
    /// floor as ever, and now hole rims and shores both ways, and rock into
    /// pools. Never where it would break a rule: not at the spawn, not on
    /// bridges or fords, water only onto solid ground and away from holes,
    /// holes away from water, and the column under a new piece of hole
    /// gets a floor like the rest of the hole's bottom.
    pub(super) fn chamfer(&mut self) {
        let rock = Tile::Whole(self.rock);
        let canyon = Tile::Whole(Part::Floor(self.floors.canyon));
        let plains = self.over(1);
        // Top down, so the floor put under a new piece of hole is there
        // before the layer below is looked at. Candidates come from the
        // layer as it was; each is checked against the level as it is when
        // its turn comes, so two chamfers can't together break a rule
        // neither breaks alone.
        for k in (0..self.layers.len()).rev() {
            for c in chamfer_layer(&self.layers[k], self.cols, self.rows) {
                let i = c.index;
                let t = [(i % self.cols) as i32, (i / self.cols) as i32];
                if !self.chamfer_allowed(k, t, c.rule)
                    || !apply(&mut self.layers[k], self.cols, self.rows, c)
                {
                    continue;
                }
                // Under a new piece of hole in the plains, the floor the
                // rest of the hole's bottom has (in the caves, the rock
                // stays: a floor there would be cut off).
                if c.rule.from == Kind::Void && k == plains && self.layers[k - 1][i] == rock {
                    self.layers[k - 1][i] = canyon;
                }
            }
        }
    }

    /// May tile `t` on layer `k` be chamfered by `rule` (see
    /// [`World::chamfer`])?
    fn chamfer_allowed(&self, k: usize, t: [i32; 2], rule: Rule) -> bool {
        if self.near_spawn(t, 0) {
            return false;
        }
        let i = self.index(t);
        let floor = self.layers[k][i].floor_id();
        let crossing = floor == Some(self.floors.bridge) || floor == Some(self.floors.bed);
        match rule {
            Rule {
                into: Kind::Floor,
                from: Kind::Void,
            } => {
                // Not over a room or stairs, and clear of water (which a
                // hole keeps a tile from).
                let soft = k == 0 || {
                    let below = self.layers[k - 1][i];
                    below == Tile::Whole(self.rock) || below.is_floor()
                };
                !crossing && soft && !self.beside_water(k, t)
            }
            Rule {
                into: Kind::Floor,
                from: Kind::Liquid,
            } => !crossing && self.rests(k, t) && !self.beside_void(k, t),
            Rule {
                into: Kind::Liquid,
                from: Kind::Floor,
            } => k + 1 >= self.layers.len() || !self.beside_water(k + 1, t),
            _ => true,
        }
    }
}

/// The kinds of tile the chamfer tells apart (materials aside).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Floor,
    Wall,
    Liquid,
    Void,
}

impl Kind {
    pub(crate) fn of(part: Part) -> Kind {
        match part {
            Part::Floor(_) => Kind::Floor,
            Part::Wall(_) => Kind::Wall,
            Part::Liquid(_) => Kind::Liquid,
            Part::Void => Kind::Void,
        }
    }
}

/// A chamfer: a tile of kind `into` whose corner is pinched by `from` on
/// both sides gets a 45-degree edge, that corner becoming `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub into: Kind,
    pub from: Kind,
}

/// The chamfers made, in order; a tile gets at most one, the first that
/// fits. Walls into floor (the old smoothing), hole rims both ways,
/// shores both ways, rock into pools.
pub const RULES: [Rule; 6] = [
    Rule {
        into: Kind::Floor,
        from: Kind::Wall,
    },
    Rule {
        into: Kind::Floor,
        from: Kind::Void,
    },
    Rule {
        into: Kind::Void,
        from: Kind::Floor,
    },
    Rule {
        into: Kind::Floor,
        from: Kind::Liquid,
    },
    Rule {
        into: Kind::Liquid,
        from: Kind::Floor,
    },
    Rule {
        into: Kind::Liquid,
        from: Kind::Wall,
    },
];

/// The part of `tile` along its side `s`: of a split tile, the half that
/// side belongs to.
pub(crate) fn part_at_side(tile: Tile, s: Side) -> Part {
    match tile {
        Tile::Whole(p) => p,
        Tile::Stairs(_, f) => Part::Floor(f),
        Tile::Split { corner, at, rest } => {
            if corner.sides().contains(&s) {
                at
            } else {
                rest
            }
        }
    }
}

/// The part of `tile` at its corner `c`: of a split tile, the half that
/// corner is in, or none if the split's diagonal runs through it.
fn part_at_corner(tile: Tile, c: Corner) -> Option<Part> {
    match tile {
        Tile::Whole(p) => Some(p),
        Tile::Stairs(_, f) => Some(Part::Floor(f)),
        Tile::Split { corner, at, .. } if corner == c => Some(at),
        Tile::Split { corner, rest, .. } if corner == c.opposite() => Some(rest),
        Tile::Split { .. } => None,
    }
}

/// A chamfer to make: tile `index`'s `corner`, by `rule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Chamfer {
    pub index: usize,
    pub corner: Corner,
    pub rule: Rule,
}

/// Does `rule` fit tile `(x, y)` of `tiles` at `corner` right now: a whole
/// tile of the rule's kind, the other kind across the corner's two sides,
/// its own kind across the other two and on the diagonal between them
/// (where that diagonal tile touches this one)? If so, the part the
/// corner becomes (the material of the neighbor it continues).
fn fits(
    tiles: &[Tile],
    cols: usize,
    rows: usize,
    x: i32,
    y: i32,
    corner: Corner,
    rule: Rule,
) -> Option<Part> {
    let tile = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= cols as i32 || y >= rows as i32 {
            Tile::WALL
        } else {
            tiles[y as usize * cols + x as usize]
        }
    };
    let Tile::Whole(own) = tile(x, y) else {
        return None;
    };
    if Kind::of(own) != rule.into {
        return None;
    }
    // What's across each side, as far as this tile is concerned.
    let across = |side: Side| {
        let [dx, dy] = side.offset();
        part_at_side(tile(x + dx, y + dy), side.opposite())
    };
    let [a, b] = corner.sides();
    let [c, d] = corner.opposite().sides();
    let [ox, oy] = [c, d]
        .map(|s| s.offset())
        .into_iter()
        .fold([0, 0], |acc, o| [acc[0] + o[0], acc[1] + o[1]]);
    // The diagonal tile touches this one at the corner opposite the cut.
    let diagonal = part_at_corner(tile(x + ox, y + oy), corner);
    let ok = Kind::of(across(a)) == rule.from
        && Kind::of(across(b)) == rule.from
        && Kind::of(across(c)) == rule.into
        && Kind::of(across(d)) == rule.into
        && diagonal.is_some_and(|p| Kind::of(p) == rule.into);
    ok.then(|| across(a))
}

/// Where to put 45-degree edges in stair-step corners of `tiles`: a whole
/// tile of one kind with another kind on two neighboring sides (say north
/// and west), its own kind on the other two and on the diagonal between
/// them, gets that corner cut to the other kind (see [`RULES`] and
/// [`fits`]). Each tile keeps a way through to its own region, so this
/// never thins a wall or cuts a passage. At most one per tile, the first
/// rule and corner that fit, decided from `tiles` as they are; the caller
/// applies them one by one, checking each still [`fits`] when its turn
/// comes (see [`apply`]), so no two together strand a piece of ground.
pub(crate) fn chamfer_layer(tiles: &[Tile], cols: usize, rows: usize) -> Vec<Chamfer> {
    let mut found = Vec::new();
    for y in 0..rows as i32 {
        for x in 0..cols as i32 {
            let found_here = RULES.iter().find_map(|&rule| {
                Corner::CLOCKWISE
                    .into_iter()
                    .find(|&corner| fits(tiles, cols, rows, x, y, corner, rule).is_some())
                    .map(|corner| Chamfer {
                        index: y as usize * cols + x as usize,
                        corner,
                        rule,
                    })
            });
            found.extend(found_here);
        }
    }
    found
}

/// Make chamfer `c` in `tiles` if it still fits; whether it was made.
pub(crate) fn apply(tiles: &mut [Tile], cols: usize, rows: usize, c: Chamfer) -> bool {
    let (x, y) = ((c.index % cols) as i32, (c.index / cols) as i32);
    match fits(tiles, cols, rows, x, y, c.corner, c.rule) {
        Some(part) => {
            tiles[c.index] = tiles[c.index].chamfer(c.corner, part);
            true
        }
        None => false,
    }
}
