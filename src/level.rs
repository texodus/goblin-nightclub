//! A level: a stack of layers, from the bottom (0) up, and the single-layer
//! grids derived from them that movement and sight run on.
//!
//! - Movement on a layer: walls block, and so do pits, void with no wall
//!   below to stand on (a void over a wall is that wall's top, walkable).
//! - Sight from layer `top` down to a lower layer `k`: walls on the layers
//!   above `k` up to `top` block it, but `k`'s own walls don't (looking
//!   down, you see over them, onto their tops); and layer `k` shows only
//!   through a shaft, where every layer above it up to `top` is void. On
//!   `top` itself, its walls block sight as usual.
//!
//! All of these are ordinary [`Layer`]s, so rays, collision, outlines and
//! visibility work on them unchanged.

use crate::generate::Style;
use crate::layer::{Layer, TILE, Tile};

/// Layers a level may have (the renderer keeps textures for each).
pub const MAX_LAYERS: usize = 8;

pub struct Level {
    layers: Vec<Layer>,
    movement: Vec<Layer>,
    /// `sight[top][k]` for `k <= top`.
    sight: Vec<Vec<Layer>>,
    /// `shafts[top][k][tile]`.
    shafts: Vec<Vec<Vec<bool>>>,
}

impl Level {
    /// A level from its layers, bottom first: up to [`MAX_LAYERS`], all the
    /// same size.
    pub fn new(layers: Vec<Layer>) -> Self {
        let (cols, rows) = (layers[0].cols, layers[0].rows);
        assert!(layers.iter().all(|l| l.cols == cols && l.rows == rows));
        assert!(layers.len() <= MAX_LAYERS, "at most {MAX_LAYERS} layers");
        let n = cols * rows;
        let at = |l: &Layer, i: usize| l.tile((i % cols) as i32, (i / cols) as i32);

        let movement = (0..layers.len())
            .map(|i| {
                let tiles = (0..n)
                    .map(|t| match at(&layers[i], t) {
                        // Standing on the wall below; elsewhere a pit.
                        Tile::Void if i == 0 => Tile::Full,
                        Tile::Void => Tile::Full.carve(at(&layers[i - 1], t).wall()),
                        Tile::Stairs(_) => Tile::Empty,
                        tile => tile,
                    })
                    .collect();
                Layer::from_tiles(cols, rows, tiles)
            })
            .collect();

        let sight = (0..layers.len())
            .map(|top| {
                (0..=top)
                    .map(|k| {
                        let tiles = (0..n)
                            .map(|t| {
                                layers[k..=top]
                                    .iter()
                                    .fold(Tile::Empty, |acc, l| acc.union(at(l, t).wall()))
                            })
                            .collect();
                        Layer::from_tiles(cols, rows, tiles)
                    })
                    .collect()
            })
            .collect();

        let shafts = (0..layers.len())
            .map(|top| {
                (0..=top)
                    .map(|k| {
                        (0..n)
                            .map(|t| layers[k + 1..=top].iter().all(|l| at(l, t) == Tile::Void))
                            .collect()
                    })
                    .collect()
            })
            .collect();

        Self {
            layers,
            movement,
            sight,
            shafts,
        }
    }

    /// Generate a random level in `style`, the player starting on the top
    /// layer at `spawn` (pixels).
    pub fn generate(style: Style, seed: u32, cols: usize, rows: usize, spawn: [f32; 2]) -> Self {
        crate::generate::generate_level(style, seed, cols, rows, spawn)
    }

    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    pub fn cols(&self) -> usize {
        self.layers[0].cols
    }

    pub fn rows(&self) -> usize {
        self.layers[0].rows
    }

    pub fn layer(&self, i: usize) -> &Layer {
        &self.layers[i]
    }

    /// What blocks movement on layer `i`: walls and pits.
    pub fn movement(&self, i: usize) -> &Layer {
        &self.movement[i]
    }

    /// What blocks sight from layer `top` down to layer `k`: the walls of
    /// every layer in between.
    pub fn sight(&self, top: usize, k: usize) -> &Layer {
        &self.sight[top][k]
    }

    /// What blocks the view of layer `k` from layer `top`: on `top`
    /// itself its walls; looking down at a lower layer, only the walls of
    /// the layers above it, since you see over its own.
    pub fn view(&self, top: usize, k: usize) -> &Layer {
        &self.sight[top][(k + 1).min(top)]
    }

    /// For each tile, whether layer `k` shows through to layer `top`.
    pub fn shaft(&self, top: usize, k: usize) -> &[bool] {
        &self.shafts[top][k]
    }

    /// Does layer `k` show through to layer `top` at pixel `p`?
    pub fn in_shaft(&self, top: usize, k: usize, p: [f32; 2]) -> bool {
        let [x, y] = [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32];
        x >= 0
            && y >= 0
            && (x as usize) < self.cols()
            && (y as usize) < self.rows()
            && self.shafts[top][k][y as usize * self.cols() + x as usize]
    }

    /// The layers seen from `top`: itself, and each below it showing
    /// through any shaft, top first.
    pub fn visible_layers(&self, top: usize) -> Vec<usize> {
        (0..=top)
            .rev()
            .filter(|&k| self.shafts[top][k].iter().any(|s| *s))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Corner;

    fn level(layers: &[&[&str]]) -> Level {
        Level::new(layers.iter().map(|rows| Layer::from_ascii(rows)).collect())
    }

    #[test]
    fn void_over_wall_is_walkable_and_over_floor_is_a_pit() {
        let l = level(&[
            &["#..◸"], //
            &["~~.~"],
        ]);
        let m = l.movement(1);
        assert_eq!(m.tile(0, 0), Tile::Empty, "top of the wall below");
        assert_eq!(m.tile(1, 0), Tile::Full, "pit over floor");
        assert_eq!(m.tile(2, 0), Tile::Empty, "floor");
        assert_eq!(
            m.tile(3, 0),
            Tile::Slope(Corner::SE),
            "half wall top, half pit"
        );
        // On the bottom layer, void has nothing under it.
        let bottom = level(&[&["~."]]);
        assert_eq!(bottom.movement(0).tile(0, 0), Tile::Full);
    }

    #[test]
    fn stacked_voids_make_a_deeper_pit() {
        let l = level(&[&["#"], &["~"], &["~"]]);
        assert_eq!(l.movement(1).tile(0, 0), Tile::Empty, "wall top one down");
        assert_eq!(l.movement(2).tile(0, 0), Tile::Full, "two down is a pit");
    }

    #[test]
    fn sight_combines_walls_from_viewer_down() {
        let l = level(&[
            &["#...", "...."], //
            &["~~.#", "~~.."],
        ]);
        // Looking at layer 0 from layer 1: walls of both.
        let s = l.sight(1, 0);
        assert_eq!(s.tile(0, 0), Tile::Full);
        assert_eq!(s.tile(3, 0), Tile::Full);
        assert_eq!(s.tile(1, 0), Tile::Empty);
        // Layer 1 on its own: just its wall.
        assert_eq!(l.sight(1, 1).tile(0, 0), Tile::Empty);
        assert_eq!(l.sight(1, 1).tile(3, 0), Tile::Full);
    }

    #[test]
    fn looking_down_sees_over_the_lower_layers_walls() {
        let l = level(&[
            &["#..."], //
            &["~~~#"],
        ]);
        // Layer 0 from layer 1: only layer 1's wall blocks.
        assert_eq!(l.view(1, 0).tile(0, 0), Tile::Empty);
        assert_eq!(l.view(1, 0).tile(3, 0), Tile::Full);
        // Layer 1 itself: its own wall.
        assert_eq!(l.view(1, 1).tile(3, 0), Tile::Full);
    }

    #[test]
    fn shafts_need_void_all_the_way_up() {
        let l = level(&[
            &["...."], //
            &["~~.~"],
            &["~.~~"],
        ]);
        let shaft = l.shaft(2, 0);
        assert_eq!(shaft, &[true, false, false, true]);
        assert_eq!(l.shaft(2, 1), &[true, false, true, true]);
        assert!(l.shaft(2, 2).iter().all(|s| *s), "own layer: everywhere");
        assert_eq!(l.visible_layers(2), vec![2, 1, 0]);
        assert_eq!(level(&[&["."], &["."]]).visible_layers(1), vec![1]);
    }
}
