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
//!
//! What sight needs from `top` down is worked out from one summary per
//! layer, how far up each tile stays void (its `open_above`), when
//! first asked for, and kept only while the viewer is on or next to `top`:
//! a level keeps a few layers of these whatever its depth, not one for
//! every pair of layers.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::art::{LayerMeshes, MeshPiece};
use crate::chunk::CHUNK;
use crate::draw::Canvas;
use crate::layer::{Layer, Piece, QUARTERS, Solid, TILE, Tile};
use crate::mesh::Mesh;
use crate::path;
use crate::pieces::{PieceOf, Pieces};
use crate::tileset::Tileset;
use crate::vision::{Circle, Lamp};

/// Layers a level may have: a bound on what a file may ask for.
pub const MAX_LAYERS: usize = 1024;

/// Layers drawn at once: the viewer's and up to this many less one below
/// it (the renderer keeps textures for each). Further down, under that many
/// sheets of mist and that much parallax, they'd hardly show.
pub const MAX_VISIBLE: usize = 8;

/// What blocks movement in `tile`: wall and void (liquid included); a part
/// of a tile no shape can be rounds up to the whole tile. All of it, if
/// a prop in the way stands on it (`blocked`).
pub fn movement_solid(tile: Tile, blocked: bool) -> Solid {
    if blocked {
        return Solid::Full;
    }
    Solid::from_quarters(tile.wall().quarters() | tile.void_quarters(), true)
}

/// Is a prop in the way of walking on `layer`'s tile `(x, y)`, its kinds
/// `tileset`'s?
fn prop_blocks(layer: &Layer, tileset: &Tileset, x: i32, y: i32) -> bool {
    layer
        .prop(x, y)
        .is_some_and(|k| tileset.props[k as usize].blocks)
}

/// Where `layer` blocks movement (see [`movement_solid`]), its props'
/// kinds `tileset`'s, as a layer of its own.
pub fn movement_layer(layer: &Layer, tileset: &Tileset) -> Layer {
    let (cols, rows) = (layer.cols, layer.rows);
    let solids = (0..cols * rows)
        .map(|t| {
            let (x, y) = ((t % cols) as i32, (t / cols) as i32);
            movement_solid(layer.tile(x, y), prop_blocks(layer, tileset, x, y))
        })
        .collect();
    Layer::from_solids(cols, rows, solids)
}

/// Can a box of `size` stand centered at `center` on a layer of `tiles`
/// (`cols` x `rows`, row by row), by the movement rules? Outside the
/// layer is solid.
pub fn can_stand(
    tiles: &[Tile],
    cols: usize,
    rows: usize,
    center: [f32; 2],
    size: [f32; 2],
) -> bool {
    let pos = [center[0] - size[0] / 2.0, center[1] - size[1] / 2.0];
    path::tiles_under(center, size).into_iter().all(|t| {
        let inside = t[0] >= 0 && t[1] >= 0 && (t[0] as usize) < cols && (t[1] as usize) < rows;
        let solid = if inside {
            movement_solid(tiles[t[1] as usize * cols + t[0] as usize], false)
        } else {
            Solid::Full
        };
        !solid.hits_box(t, pos, size)
    })
}

/// Tiles a level may have per layer: a bound on what a file may ask for.
/// Every layer is kept in memory whole, so in practice memory is the limit.
pub const MAX_TILES: usize = 1 << 20;

pub struct Level {
    layers: Vec<Layer>,
    /// The art its tiles refer to.
    tileset: Tileset,
    movement: Vec<Layer>,
    /// For tile `t` (an index, row by row) of layer `k`, `open_above[k][t]`:
    /// per quarter (as in [`QUARTERS`]), how many layers straight up from
    /// `k` that quarter is void, without a break, up to 255. So layer `k`
    /// shows through to layer `top` in a quarter where it's at least
    /// `top - k`.
    open_above: Vec<Vec<[u8; 4]>>,
    /// Per layer, the most of `open_above` anywhere on it: how many layers
    /// up it shows through to, at most.
    deepest: Vec<u8>,
    /// Per layer, made when first asked for; see [`Level::circles`].
    circles: Vec<OnceLock<Vec<Circle>>>,
    /// Per layer, made when first asked for; see [`Level::lamps`].
    lamps: Vec<OnceLock<Vec<Lamp>>>,
    /// Per layer, made when first asked for; see [`Level::meshes`].
    meshes: Vec<OnceLock<Arc<LayerMeshes>>>,
    /// By `(top, k)`, made when first asked for and dropped once the viewer
    /// is further than a layer from `top`: [`Level::view`],
    /// [`Level::shaft`] and [`Level::shaft_mesh`].
    views: Cache<Arc<Layer>>,
    shafts: Cache<Arc<[bool]>>,
    shaft_meshes: Cache<Arc<Mesh>>,
    /// For a level that's a window onto a bigger one: where it is, and
    /// the pieces its views are made of, kept for the next window.
    placed: Option<Placed>,
}

struct Placed {
    /// Its lowest layer, and its top-left chunk, in the whole level.
    base: usize,
    origin: [i32; 2],
    pieces: Arc<Mutex<Pieces>>,
}

type Cache<T> = Mutex<HashMap<(usize, usize), T>>;

/// `cache`'s entry for `(top, k)`, made by `make` if it hasn't one. Entries
/// for layers further than one from `top` go: the viewer has left them.
fn cached<T: Clone>(cache: &Cache<T>, top: usize, k: usize, make: impl FnOnce() -> T) -> T {
    if let Some(found) = cache.lock().unwrap().get(&(top, k)) {
        return found.clone();
    }
    let made = make();
    let mut cache = cache.lock().unwrap();
    cache.retain(|&(t, _), _| t.abs_diff(top) <= 1);
    cache.insert((top, k), made.clone());
    made
}

impl Level {
    /// A level from its layers, bottom first: up to [`MAX_LAYERS`], all the
    /// same size, their tiles drawn from `tileset`.
    pub fn new(layers: Vec<Layer>, tileset: Tileset) -> Self {
        let (cols, rows) = (layers[0].cols, layers[0].rows);
        assert!(layers.iter().all(|l| l.cols == cols && l.rows == rows));
        assert!(layers.len() <= MAX_LAYERS, "at most {MAX_LAYERS} layers");
        assert!(
            cols * rows <= MAX_TILES,
            "at most {MAX_TILES} tiles a layer"
        );
        let movement = layers.iter().map(|l| movement_layer(l, &tileset)).collect();
        Self::from_parts(layers, movement, tileset)
    }

    /// A level from its layers and each one's [`movement_layer`], made
    /// beforehand (perhaps a little at a time).
    pub fn from_parts(layers: Vec<Layer>, movement: Vec<Layer>, tileset: Tileset) -> Self {
        let (cols, rows) = (layers[0].cols, layers[0].rows);
        assert!(layers.iter().all(|l| l.cols == cols && l.rows == rows));
        assert_eq!(movement.len(), layers.len());
        let n = cols * rows;
        let pos = |i: usize| ((i % cols) as i32, (i / cols) as i32);

        // From the top down: open one more layer up than the layer above
        // is, in each quarter of it that's void.
        let depth = layers.len();
        let mut open_above = vec![vec![[0u8; 4]; n]; depth];
        for k in (0..depth.saturating_sub(1)).rev() {
            let (here, above) = open_above.split_at_mut(k + 1);
            for (t, open) in here[k].iter_mut().enumerate() {
                let void = layers[k + 1].tile(pos(t).0, pos(t).1).void_quarters();
                for (q, bit) in QUARTERS.into_iter().enumerate() {
                    if void & bit != 0 {
                        open[q] = above[0][t][q].saturating_add(1);
                    }
                }
            }
        }
        let deepest = open_above
            .iter()
            .map(|l| l.iter().flatten().copied().max().unwrap_or(0))
            .collect();
        Self {
            layers,
            tileset,
            movement,
            open_above,
            deepest,
            meshes: (0..depth).map(|_| OnceLock::new()).collect(),
            circles: (0..depth).map(|_| OnceLock::new()).collect(),
            lamps: (0..depth).map(|_| OnceLock::new()).collect(),
            views: Default::default(),
            shafts: Default::default(),
            shaft_meshes: Default::default(),
            placed: None,
        }
    }

    /// The same level, a window onto a bigger one: its lowest layer at
    /// `base` and its top-left chunk at `origin` there. What its layers
    /// are seen through ([`Level::view`]) is made of pieces kept in
    /// `pieces`, reused if they're there.
    pub fn placed(mut self, base: usize, origin: [i32; 2], pieces: Arc<Mutex<Pieces>>) -> Self {
        self.placed = Some(Placed {
            base,
            origin,
            pieces,
        });
        self
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

    pub fn tileset(&self) -> &Tileset {
        &self.tileset
    }

    /// What blocks movement on layer `i`: walls and void.
    pub fn movement(&self, i: usize) -> &Layer {
        &self.movement[i]
    }

    /// The quarters of tile `t` of layer `k` open all the way up to `top`,
    /// as bits; all of them on `top` itself.
    fn open_to(&self, top: usize, k: usize, t: usize) -> u8 {
        if k == top {
            return 0b1111;
        }
        let open = self.open_above[k][t];
        QUARTERS
            .into_iter()
            .enumerate()
            .filter(|&(q, _)| open[q] as usize >= top - k)
            .fold(0, |bits, (_, bit)| bits | bit)
    }

    /// What blocks the view of layer `k` from layer `top`: `top`'s walls,
    /// and the walls of the layers in between, but only inside holes
    /// (where every layer above them is void). Elsewhere a wall in between
    /// is under a floor, which hides it, so it can't be in the way of
    /// anything seen through a hole. Not `k`'s own walls, below `top`:
    /// looking down, you see over them, onto their tops.
    pub fn view(&self, top: usize, k: usize) -> Arc<Layer> {
        cached(&self.views, top, k, || {
            let (cols, rows) = (self.cols(), self.rows());
            let solid = |x: usize, y: usize| {
                let mut q = self.layers[top].solid(x as i32, y as i32).quarters();
                for j in k + 1..top {
                    let open = self.open_to(top, j, y * cols + x);
                    q |= self.layers[j].solid(x as i32, y as i32).quarters() & open;
                }
                Solid::from_quarters(q, true)
            };
            let Some(placed) = &self.placed else {
                let solids = (0..cols * rows)
                    .map(|t| solid(t % cols, t / cols))
                    .collect();
                return Arc::new(Layer::from_solids(cols, rows, solids));
            };
            // A chunk's piece depends only on the chunks over it from `k`
            // up to `top`, wherever the window is.
            let across = cols.div_ceil(CHUNK);
            let mut kept = placed.pieces.lock().unwrap();
            let pieces: Vec<_> = (0..across * rows.div_ceil(CHUNK))
                .map(|c| {
                    let [cx, cy] = [c % across, c / across];
                    let [w, h] = [CHUNK.min(cols - cx * CHUNK), CHUNK.min(rows - cy * CHUNK)];
                    let of = PieceOf::View {
                        top: placed.base + top,
                    };
                    let [x, y] = [cx as i32, cy as i32];
                    let key = (placed.base + k, placed.origin[0] + x, placed.origin[1] + y);
                    kept.get(of, key, || {
                        let at = |t: usize| solid(cx * CHUNK + t % w, cy * CHUNK + t / w);
                        Piece::from_solids([w, h], (0..w * h).map(at).collect())
                    })
                })
                .collect();
            drop(kept);
            Arc::new(Layer::from_pieces(cols, rows, &pieces))
        })
    }

    /// For each tile, whether layer `k` shows through to layer `top`
    /// anywhere in it: there's void all the way up.
    pub fn shaft(&self, top: usize, k: usize) -> Arc<[bool]> {
        cached(&self.shafts, top, k, || {
            (0..self.cols() * self.rows())
                .map(|t| self.open_to(top, k, t) != 0)
                .collect()
        })
    }

    /// The layers seen from `top`: itself, and each below it showing
    /// through any shaft, top first; no more than [`MAX_VISIBLE`].
    pub fn visible_layers(&self, top: usize) -> Vec<usize> {
        (top.saturating_sub(MAX_VISIBLE - 1)..=top)
            .rev()
            .filter(|&k| k == top || self.deepest[k] as usize >= top - k)
            .collect()
    }

    /// What's drawn of layer `k`: each chunk's piece, reused from the
    /// window before if it's a window onto a bigger level, and the
    /// fringes and trims along the chunks' edges.
    pub fn meshes(&self, k: usize) -> &Arc<LayerMeshes> {
        self.meshes[k].get_or_init(|| {
            let (layer, tileset) = (&self.layers[k], &self.tileset);
            let (cols, rows) = (self.cols(), self.rows());
            let across = cols.div_ceil(CHUNK);
            let make = |cx: usize, cy: usize| MeshPiece::new(&layer.chunk_alone(cx, cy), tileset);
            let chunks = 0..across * rows.div_ceil(CHUNK);
            let pieces: Vec<_> = match &self.placed {
                None => chunks
                    .map(|c| Arc::new(make(c % across, c / across)))
                    .collect(),
                Some(placed) => {
                    let mut kept = placed.pieces.lock().unwrap();
                    chunks
                        .map(|c| {
                            let [cx, cy] = [c % across, c / across];
                            let [x, y] = [cx as i32, cy as i32];
                            let key = (placed.base + k, placed.origin[0] + x, placed.origin[1] + y);
                            kept.meshes(key, || make(cx, cy))
                        })
                        .collect()
                }
            };
            Arc::new(LayerMeshes::new(layer, tileset, pieces))
        })
    }

    /// The circles sight can't pass on layer `k`: its props' (those that
    /// hide anything; see [`crate::tileset::PropKind::sight`]).
    pub fn circles(&self, k: usize) -> &[Circle] {
        self.circles[k].get_or_init(|| {
            let zoom = self.tileset.zoom();
            self.layers[k]
                .props()
                .filter_map(|p| {
                    let kind = &self.tileset.props[p.kind as usize];
                    let [x, y] = p.at.map(|t| t as f32);
                    (kind.sight > 0.0).then_some(Circle {
                        center: [(x + 0.5) * TILE, (y + 0.5) * TILE],
                        radius: kind.sight / zoom,
                    })
                })
                .collect()
        })
    }

    /// The lights on layer `k`: its props' (those that give any; see
    /// [`crate::tileset::PropKind::light`]). What each lights is worked out
    /// when first asked for (see [`Lamp::lit`]).
    pub fn lamps(&self, k: usize) -> &[Lamp] {
        self.lamps[k].get_or_init(|| {
            self.layers[k]
                .props()
                .filter_map(|p| {
                    let light = self.tileset.props[p.kind as usize].light?;
                    let [x, y] = p.at.map(|t| t as f32);
                    let center = [(x + 0.5) * TILE, (y + 0.5) * TILE];
                    Some(Lamp::new(center, light.radius * TILE, light.color))
                })
                .collect()
        })
    }

    /// Where layer `k` shows through to layer `top` (see
    /// [`Level::shaft`]), as a mesh of whole tiles.
    pub fn shaft_mesh(&self, top: usize, k: usize) -> Arc<Mesh> {
        cached(&self.shaft_meshes, top, k, || {
            let (cols, shaft) = (self.cols(), self.shaft(top, k));
            let canvas = Canvas::new(&self.tileset);
            Arc::new(Mesh::quarters(cols, self.rows(), canvas, |x, y| {
                if shaft[y * cols + x] { 0b1111 } else { 0 }
            }))
        })
    }

    /// Its layers (and those they're seen through), the bulk of it, apart:
    /// a big level takes a while to drop, so it can go a piece at a time.
    pub fn take_apart(self) -> Vec<Box<dyn Any>> {
        let Self {
            layers,
            movement,
            views,
            ..
        } = self;
        let views = views.into_inner().unwrap().into_values();
        let pieces = layers.into_iter().chain(movement);
        let pieces = pieces.map(|l| Box::new(l) as Box<dyn Any>);
        pieces
            .chain(views.map(|v| Box::new(v) as Box<dyn Any>))
            .collect()
    }
}

#[cfg(test)]
#[path = "tests/level_tests.rs"]
mod tests;
