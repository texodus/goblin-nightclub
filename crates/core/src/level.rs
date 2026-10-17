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
//! The renderer draws each layer below the viewer's a little smaller than
//! the one above (parallax), and so shows a little of the layer beside
//! each hole, not just under it: the hole's sides. Those are never in the
//! shaft; the renderer remembers them with the hole they're seen down.
//!
//! All of these are ordinary [`Layer`]s, so rays, collision, outlines and
//! visibility work on them unchanged.

use crate::layer::{Layer, Solid};

/// Layers a level may have (the renderer keeps textures for each).
pub const MAX_LAYERS: usize = 8;

/// Tiles a level may have per layer: the renderer keeps a level-sized mask
/// of what's been seen on each layer, a byte per pixel.
pub const MAX_TILES: usize = 128 * 128;

pub struct Level {
    layers: Vec<Layer>,
    movement: Vec<Layer>,
    /// `views[top][k]` for `k <= top`; see [`Level::view`].
    views: Vec<Vec<Layer>>,
    /// `shafts[top][k][tile]`: whether any of the tile is open from layer
    /// `k` up to `top`.
    shafts: Vec<Vec<Vec<bool>>>,
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

        // What blocks the view of layer `k` from `top`: `top`'s walls, and
        // the walls of each layer in between where every layer above them
        // is void. Elsewhere a wall in between is under a floor, which hides
        // it, so it can't be in the way of anything seen through a hole.
        let views = (0..layers.len())
            .map(|top| {
                (0..=top)
                    .map(|k| {
                        let solids = (0..n)
                            .map(|t| {
                                let mut q = solid(&layers[top], t).quarters();
                                for j in k + 1..top {
                                    let open = layers[j + 1..=top]
                                        .iter()
                                        .fold(0b1111, |m, l| m & void(l, t));
                                    q |= solid(&layers[j], t).quarters() & open;
                                }
                                Solid::from_quarters(q, true)
                            })
                            .collect();
                        Layer::from_solids(cols, rows, solids)
                    })
                    .collect()
            })
            .collect();

        let shafts: Vec<Vec<Vec<bool>>> = (0..layers.len())
            .map(|top| {
                (0..=top)
                    .map(|k| {
                        (0..n)
                            .map(|t| {
                                layers[k + 1..=top]
                                    .iter()
                                    .fold(0b1111, |q, l| q & void(l, t))
                                    != 0
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect();
        Self {
            layers,
            movement,
            views,
            shafts,
        }
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

    /// What blocks the view of layer `k` from layer `top`: `top`'s walls,
    /// and the walls of the layers in between, but only inside holes
    /// (where every layer above them is void). Not `k`'s own walls, below
    /// `top`: looking down, you see over them, onto their tops.
    pub fn view(&self, top: usize, k: usize) -> &Layer {
        &self.views[top][k]
    }

    /// For each tile, whether layer `k` shows through to layer `top`
    /// anywhere in it: there's void all the way up.
    pub fn shaft(&self, top: usize, k: usize) -> &[bool] {
        &self.shafts[top][k]
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
    fn shafts_need_void_all_the_way_up() {
        let l = level(&[
            &["........"], //
            &["...~~..."],
            &["..~~...."],
        ]);
        let t = true;
        let f = false;
        // Two down, the hole through both is tile 3.
        assert_eq!(l.shaft(2, 0), &[f, f, f, t, f, f, f, f]);
        // One down, the top's void.
        assert_eq!(l.shaft(2, 1), &[f, f, t, t, f, f, f, f]);
        assert!(l.shaft(2, 2).iter().all(|s| *s), "own layer: everywhere");
        // Half a tile of void counts.
        let half = level(&[&["..."], &[".▛."]]);
        assert_eq!(half.shaft(1, 0), &[f, t, f]);
    }

    #[test]
    fn walls_in_between_block_only_inside_holes() {
        let l = level(&[
            &["....."], //
            &["#.##◸"],
            &["~~..~"],
        ]);
        // Layer 0 from layer 2: layer 1's walls under layer 2's void block
        // (whole, or the part of a slope the void is over); those under its
        // floor don't.
        let v = l.view(2, 0);
        assert_eq!(v.solid(0, 0), Solid::Full, "in the hole");
        assert_eq!(v.solid(2, 0), Solid::None, "under the floor");
        assert_eq!(v.solid(3, 0), Solid::None, "under the floor");
        assert_eq!(v.solid(4, 0), Solid::Slope(Corner::NW), "in the hole");
        // Layer 1 from layer 2: only layer 2's walls (none).
        assert!((0..5).all(|x| l.view(2, 1).solid(x, 0) == Solid::None));
        // A half-void tile above: only the part under the void blocks.
        let half = level(&[&["."], &["#"], &["▛"]]);
        assert_eq!(half.view(2, 0).solid(0, 0), Solid::Slope(Corner::SE));
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
    fn layers_below_are_drawn_only_with_void_all_the_way_up() {
        let l = level(&[&["...."], &["~~.~"], &["~.~~"]]);
        assert_eq!(l.visible_layers(2), vec![2, 1, 0]);
        assert_eq!(level(&[&["."], &["."]]).visible_layers(1), vec![1]);
        assert_eq!(
            level(&[&["~"], &["."], &["~"]]).visible_layers(2),
            vec![2, 1]
        );
    }
}
