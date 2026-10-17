//! A level: a stack of layers, from the bottom (0) up, and the single-layer
//! grids derived from them that movement and sight run on.
//!
//! - Movement on a layer: only floor is walkable. Walls block, and so does
//!   void, whatever is below it; half a tile can be either.
//! - Sight from layer `top` down to a lower layer `k`: walls on the layers
//!   above `k` up to `top` block it, but `k`'s own walls don't (looking
//!   down, you see over them, onto their tops); and layer `k` shows only
//!   through a shaft, where every layer above it up to `top` is void. On
//!   `top` itself, its walls block sight as usual.
//!
//! All of these are ordinary [`Layer`]s, so rays, collision, outlines and
//! visibility work on them unchanged.

use crate::generate::Style;
use crate::layer::{Layer, Solid, TILE};

/// Layers a level may have (the renderer keeps textures for each).
pub const MAX_LAYERS: usize = 8;

/// Tiles a level may have per layer: the renderer keeps a level-sized mask
/// of what's been seen on each layer, a byte per pixel.
pub const MAX_TILES: usize = 128 * 128;

pub struct Level {
    layers: Vec<Layer>,
    movement: Vec<Layer>,
    /// `sight[top][k]` for `k <= top`.
    sight: Vec<Vec<Layer>>,
    /// `shafts[top][k][tile]`: the quarters of each tile (as bits) open
    /// from layer `k` up to `top`, and whether any are.
    shafts: Vec<Vec<Vec<u8>>>,
    shafts_any: Vec<Vec<Vec<bool>>>,
}

impl Level {
    /// A level from its layers, bottom first: up to [`MAX_LAYERS`], all the
    /// same size.
    pub fn new(layers: Vec<Layer>) -> Self {
        let (cols, rows) = (layers[0].cols, layers[0].rows);
        assert!(layers.iter().all(|l| l.cols == cols && l.rows == rows));
        assert!(layers.len() <= MAX_LAYERS, "at most {MAX_LAYERS} layers");
        assert!(
            cols * rows <= MAX_TILES,
            "at most {MAX_TILES} tiles a layer"
        );
        let n = cols * rows;
        let pos = |i: usize| ((i % cols) as i32, (i / cols) as i32);
        let solid = |l: &Layer, i: usize| l.solid(pos(i).0, pos(i).1);
        let void = |l: &Layer, i: usize| l.tile(pos(i).0, pos(i).1).void_quarters();

        // Blocked where wall or void. A part of a tile no shape can be
        // rounds up to blocking the whole tile.
        let movement = layers
            .iter()
            .map(|l| {
                let solids = (0..n)
                    .map(|t| Solid::from_quarters(solid(l, t).quarters() | void(l, t), true))
                    .collect();
                Layer::from_solids(cols, rows, solids)
            })
            .collect();

        let sight = (0..layers.len())
            .map(|top| {
                (0..=top)
                    .map(|k| {
                        let solids = (0..n)
                            .map(|t| {
                                layers[k..=top]
                                    .iter()
                                    .fold(Solid::None, |acc, l| acc.union(solid(l, t)))
                            })
                            .collect();
                        Layer::from_solids(cols, rows, solids)
                    })
                    .collect()
            })
            .collect();

        let shafts: Vec<Vec<Vec<u8>>> = (0..layers.len())
            .map(|top| {
                (0..=top)
                    .map(|k| {
                        (0..n)
                            .map(|t| {
                                layers[k + 1..=top]
                                    .iter()
                                    .fold(0b1111, |q, l| q & void(l, t))
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        let shafts_any = shafts
            .iter()
            .map(|by_k| {
                by_k.iter()
                    .map(|qs| qs.iter().map(|q| *q != 0).collect())
                    .collect()
            })
            .collect();

        Self {
            layers,
            movement,
            sight,
            shafts,
            shafts_any,
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

    /// What blocks movement on layer `i`: walls and void.
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

    /// For each tile, whether layer `k` shows through to layer `top`
    /// anywhere in it.
    pub fn shaft(&self, top: usize, k: usize) -> &[bool] {
        &self.shafts_any[top][k]
    }

    /// For each tile, the quarters (as bits, see [`crate::layer::quarter_shapes`]) where
    /// layer `k` shows through to layer `top`.
    pub fn shaft_quarters(&self, top: usize, k: usize) -> &[u8] {
        &self.shafts[top][k]
    }

    /// Does layer `k` show through to layer `top` at pixel `p`?
    pub fn in_shaft(&self, top: usize, k: usize, p: [f32; 2]) -> bool {
        let [x, y] = [(p[0] / TILE).floor() as i32, (p[1] / TILE).floor() as i32];
        x >= 0
            && y >= 0
            && (x as usize) < self.cols()
            && (y as usize) < self.rows()
            && self.shafts_any[top][k][y as usize * self.cols() + x as usize]
    }

    /// The layers seen from `top`: itself, and each below it showing
    /// through any shaft, top first.
    pub fn visible_layers(&self, top: usize) -> Vec<usize> {
        (0..=top)
            .rev()
            .filter(|&k| self.shafts_any[top][k].iter().any(|s| *s))
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
    fn only_floor_is_walkable() {
        let l = level(&[
            &["#..◸"], //
            &["~~.~"],
        ]);
        let m = l.movement(1);
        assert_eq!(m.solid(0, 0), Solid::Full, "void over a wall");
        assert_eq!(m.solid(1, 0), Solid::Full, "void over floor");
        assert_eq!(m.solid(2, 0), Solid::None, "floor");
        assert_eq!(m.solid(3, 0), Solid::Full, "void over a slope");
        let bottom = level(&[&["~."]]);
        assert_eq!(bottom.movement(0).solid(0, 0), Solid::Full);
    }

    #[test]
    fn half_void_tiles_block_their_void_half() {
        let l = level(&[
            &["#◤▛◺"], //
            &["▛◥▙~"],
        ]);
        let m = l.movement(1);
        // Floor in the NW half, void in the SE half, even over a wall.
        assert_eq!(m.solid(0, 0), Solid::Full.carve(Solid::Slope(Corner::NW)));
        // Wall in one half, void in the other: blocked throughout.
        assert_eq!(m.solid(1, 0), Solid::Full);
        // Floor in the SW half: the NE half blocks.
        assert_eq!(m.solid(2, 0), Solid::Slope(Corner::NE));
    }

    #[test]
    fn shafts_follow_void_quarters() {
        let l = level(&[
            &["..."], //
            &["▛▟~"],
            &["~▛▛"],
        ]);
        // Bits N = 1, E = 2, S = 4, W = 8: the SE half is 6, the NW half 9.
        assert_eq!(l.shaft_quarters(2, 0), &[6, 0, 6]);
        assert_eq!(l.shaft(2, 0), &[true, false, true]);
        assert_eq!(l.shaft_quarters(2, 1), &[15, 6, 6]);
        assert_eq!(l.layer(1).tile(1, 0).void(), Solid::Slope(Corner::NW));
    }

    #[test]
    fn sight_combines_walls_from_viewer_down() {
        let l = level(&[
            &["#...", "...."], //
            &["~~.#", "~~.."],
        ]);
        // Looking at layer 0 from layer 1: walls of both.
        let s = l.sight(1, 0);
        assert_eq!(s.solid(0, 0), Solid::Full);
        assert_eq!(s.solid(3, 0), Solid::Full);
        assert_eq!(s.solid(1, 0), Solid::None);
        // Layer 1 on its own: just its wall.
        assert_eq!(l.sight(1, 1).solid(0, 0), Solid::None);
        assert_eq!(l.sight(1, 1).solid(3, 0), Solid::Full);
    }

    #[test]
    fn looking_down_sees_over_the_lower_layers_walls() {
        let l = level(&[
            &["#..."], //
            &["~~~#"],
        ]);
        // Layer 0 from layer 1: only layer 1's wall blocks.
        assert_eq!(l.view(1, 0).solid(0, 0), Solid::None);
        assert_eq!(l.view(1, 0).solid(3, 0), Solid::Full);
        // Layer 1 itself: its own wall.
        assert_eq!(l.view(1, 1).solid(3, 0), Solid::Full);
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
