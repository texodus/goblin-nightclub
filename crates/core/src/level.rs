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
//! layer, how far up each tile stays void (see [`Level::open_above`]), when
//! first asked for, and kept only while the viewer is on or next to `top`:
//! a level keeps a few layers of these whatever its depth, not one for
//! every pair of layers.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::chunk::CHUNK;
use crate::draw::Canvas;
use crate::fringe;
use crate::layer::{Layer, Piece, QUARTERS, Solid, TILE, Tile};
use crate::mesh::Mesh;
use crate::path;
use crate::tileset::Tileset;
use crate::world::{PieceOf, Pieces};

/// Layers a level may have: a bound on what a file may ask for.
pub const MAX_LAYERS: usize = 1024;

/// Layers drawn at once: the viewer's and up to this many less one below
/// it (the renderer keeps textures for each). Further down, under that many
/// sheets of mist and that much parallax, they'd hardly show.
pub const MAX_VISIBLE: usize = 8;

/// What blocks movement in `tile`: wall and void (liquid included); a part
/// of a tile no shape can be rounds up to the whole tile.
pub fn movement_solid(tile: Tile) -> Solid {
    Solid::from_quarters(tile.wall().quarters() | tile.void_quarters(), true)
}

/// Where `layer` blocks movement (see [`movement_solid`]), as a layer of
/// its own.
pub fn movement_layer(layer: &Layer) -> Layer {
    let (cols, rows) = (layer.cols, layer.rows);
    let solids = (0..cols * rows)
        .map(|t| movement_solid(layer.tile((t % cols) as i32, (t / cols) as i32)))
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
            movement_solid(tiles[t[1] as usize * cols + t[0] as usize])
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
    /// `open_above[k][tile]`, per quarter: see [`Level::open_above`].
    open_above: Vec<Vec<[u8; 4]>>,
    /// Per layer, the most of `open_above` anywhere on it: how many layers
    /// up it shows through to, at most.
    deepest: Vec<u8>,
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

/// What's drawn of a layer that only changes with its tiles: a piece a
/// chunk, each where it is, and what's along the chunks' edges.
pub struct LayerMeshes {
    /// Chunks across.
    pub across: usize,
    /// Each chunk's, row by row (see [`MeshPiece`]): each in pixels from
    /// its chunk's top-left corner, [`LayerMeshes::corner`].
    pub pieces: Vec<Arc<MeshPiece>>,
    /// The fringes and trims of the tiles along the chunks' edges, which
    /// depend on the chunks beside them (and past the layer's edge, that
    /// there are none): in the layer's own pixels.
    pub fringes: Arc<Mesh>,
    pub trims: Arc<Mesh>,
}

impl LayerMeshes {
    /// Where piece `i`'s chunk's top-left corner is, in pixels.
    pub fn corner(&self, i: usize) -> [f32; 2] {
        [i % self.across, i / self.across].map(|c| (c * CHUNK) as f32 * TILE)
    }

    /// All of its meshes.
    pub fn all(&self) -> impl Iterator<Item = &Mesh> {
        let pieces = self.pieces.iter().flat_map(|p| p.all());
        pieces.chain([&*self.fringes, &*self.trims])
    }
}

/// Each tile's art of `layer`, as a mesh.
fn tile_mesh(layer: &Layer, tileset: &Tileset) -> Mesh {
    Mesh::build(
        layer.cols,
        layer.rows,
        Canvas::new(tileset),
        |xs, ys, canvas| {
            for y in ys {
                for x in xs.clone() {
                    layer.draw_tile(tileset, canvas, x, y);
                }
            }
        },
    )
}

/// Draws a tile's fringes or trims: see [`fringe`].
type DrawTile = fn(&Layer, &Tileset, &mut Canvas, usize, usize);

/// What's drawn of a chunk of a layer, the same in any window, so made
/// once it can be reused by each, and kept on the GPU as long: in pixels
/// from the chunk's top-left corner. Its fringes and trims leave out the
/// tiles along its edge, which depend on the chunks beside it (see
/// [`LayerMeshes`]).
pub struct MeshPiece {
    /// Its tiles' art.
    pub tiles: Mesh,
    /// Drawn over the tiles (all pieces' tiles): where materials meet, the
    /// fringes of the higher-ranking ones (see [`crate::fringe`]), cut by
    /// the renderer.
    pub fringes: Mesh,
    /// Drawn over the fringes: trims scattered along material edges.
    pub trims: Mesh,
    /// Where it's void, liquid included.
    pub voids: Mesh,
    /// Where it's liquid.
    pub liquid: Mesh,
}

impl MeshPiece {
    /// What's drawn of `chunk`, a layer's chunk alone (see
    /// [`Layer::chunk_alone`]).
    pub fn new(chunk: &Layer, tileset: &Tileset) -> Self {
        let size = [chunk.cols, chunk.rows];
        let inside = |draw: DrawTile| {
            Mesh::build(size[0], size[1], Canvas::new(tileset), |xs, ys, canvas| {
                for y in ys {
                    for x in xs.clone().filter(|&x| !on_edge(size, x, y)) {
                        draw(chunk, tileset, canvas, x, y);
                    }
                }
            })
        };
        let canvas = || Canvas::new(tileset);
        let tile = |x: usize, y: usize| chunk.tile(x as i32, y as i32);
        Self {
            tiles: tile_mesh(chunk, tileset),
            fringes: inside(fringe::draw_fringes),
            trims: inside(fringe::draw_trims),
            voids: Mesh::quarters(size[0], size[1], canvas(), |x, y| {
                tile(x, y).void_quarters()
            }),
            liquid: Mesh::quarters(size[0], size[1], canvas(), |x, y| {
                tile(x, y).liquid_quarters()
            }),
        }
    }

    /// All of its meshes.
    pub fn all(&self) -> [&Mesh; 5] {
        [
            &self.tiles,
            &self.fringes,
            &self.trims,
            &self.voids,
            &self.liquid,
        ]
    }
}

/// Whether tile `(x, y)` of a chunk `size` is along its edge.
fn on_edge(size: [usize; 2], x: usize, y: usize) -> bool {
    x == 0 || y == 0 || x + 1 == size[0] || y + 1 == size[1]
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
        let movement = layers.iter().map(movement_layer).collect();
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

    /// For tile `t` (an index, row by row) of layer `k`: per quarter (as
    /// in [`QUARTERS`]), how many layers straight up from `k` that quarter
    /// is void, without a break, up to 255. So layer `k` shows through to
    /// layer `top` in a quarter where it's at least `top - k`.
    pub fn open_above(&self, k: usize, t: usize) -> [u8; 4] {
        self.open_above[k][t]
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
            let edge = |x: usize, y: usize| {
                let size =
                    [cols - x / CHUNK * CHUNK, rows - y / CHUNK * CHUNK].map(|n| n.min(CHUNK));
                on_edge(size, x % CHUNK, y % CHUNK)
            };
            let along_edges = |draw: DrawTile| {
                let mesh = Mesh::build(cols, rows, Canvas::new(tileset), |xs, ys, canvas| {
                    for y in ys {
                        for x in xs.clone().filter(|&x| edge(x, y)) {
                            draw(layer, tileset, canvas, x, y);
                        }
                    }
                });
                Arc::new(mesh)
            };
            Arc::new(LayerMeshes {
                across,
                pieces,
                fringes: along_edges(fringe::draw_fringes),
                trims: along_edges(fringe::draw_trims),
            })
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
mod tests {
    use super::*;
    use crate::layer::Corner;

    fn level(layers: &[&[&str]]) -> Level {
        Level::new(
            layers.iter().map(|rows| Layer::from_ascii(rows)).collect(),
            crate::tileset::testing::tileset(),
        )
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
    fn liquid_blocks_movement_and_shows_the_layer_below() {
        let l = level(&[&["..."], &["≈.~"]]);
        assert_eq!(l.movement(1).solid(0, 0), Solid::Full, "a pit");
        assert_eq!(l.movement(1).solid(1, 0), Solid::None);
        assert_eq!(
            &*l.shaft(1, 0),
            &[true, false, true],
            "shows through like void"
        );
        assert_eq!(l.visible_layers(1), vec![1, 0]);
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
        assert_eq!(&*l.shaft(2, 0), &[f, f, f, t, f, f, f, f]);
        // One down, the top's void.
        assert_eq!(&*l.shaft(2, 1), &[f, f, t, t, f, f, f, f]);
        assert!(l.shaft(2, 2).iter().all(|s| *s), "own layer: everywhere");
        // Half a tile of void counts.
        let half = level(&[&["..."], &[".▛."]]);
        assert_eq!(&*half.shaft(1, 0), &[f, t, f]);
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

    /// Any two kinds of part may share a tile along a diagonal, in any
    /// corner; movement, sight, the liquid tint, drawing and level files
    /// all follow the quarters.
    #[test]
    fn every_pairing_of_parts_splits_consistently() {
        use crate::draw::Canvas;
        use crate::layer::{Part, Tile};
        let tileset = crate::tileset::testing::tileset();
        let kinds = [Part::Floor(1), Part::Wall(1), Part::Liquid(0), Part::Void];
        let open = |p: Part| !matches!(p, Part::Floor(_));
        let liquid = |p: Part| matches!(p, Part::Liquid(_));
        for at in kinds {
            for rest in kinds.into_iter().filter(|r| *r != at) {
                for corner in Corner::CLOCKWISE {
                    let tile = Tile::split(corner, at, rest);
                    let (atq, restq) = (corner.quarters(), !corner.quarters() & 0b1111);
                    let mut middle = vec![Tile::FLOOR; 9];
                    middle[4] = tile;
                    let level = Level::new(
                        vec![
                            Layer::from_ascii(&["...", "...", "..."]),
                            Layer::from_tiles(3, 3, middle),
                        ],
                        tileset.clone(),
                    );
                    let why = format!("{at:?} at {corner:?} over {rest:?}");
                    // Blocked where not floor.
                    let blocked =
                        (if open(at) { atq } else { 0 }) | (if open(rest) { restq } else { 0 });
                    assert_eq!(
                        level.movement(1).solid(1, 1),
                        Solid::from_quarters(blocked, true),
                        "{why}: movement"
                    );
                    // Sight from the layer itself: its walls.
                    assert_eq!(level.view(1, 1).solid(1, 1), tile.wall(), "{why}: view");
                    // Open to the layer below where void or liquid.
                    let void = (if matches!(at, Part::Void | Part::Liquid(_)) {
                        atq
                    } else {
                        0
                    }) | (if matches!(rest, Part::Void | Part::Liquid(_)) {
                        restq
                    } else {
                        0
                    });
                    assert_eq!(tile.void_quarters(), void, "{why}");
                    assert_eq!(level.shaft(1, 0)[4], void != 0, "{why}: shaft");
                    assert_eq!(
                        level.visible_layers(1),
                        if void != 0 { vec![1, 0] } else { vec![1] },
                        "{why}"
                    );
                    let tint =
                        (if liquid(at) { atq } else { 0 }) | (if liquid(rest) { restq } else { 0 });
                    assert_eq!(tile.liquid_quarters(), tint, "{why}");
                    // Points in either half, by quarter.
                    let layer = level.layer(1);
                    let [ox, oy] = corner.offset();
                    let near = [10.0 + ox as f32 * 8.0 + 1.0, 10.0 + oy as f32 * 8.0 + 1.0];
                    let far = [
                        10.0 + (1 - ox) as f32 * 8.0 + 1.0,
                        10.0 + (1 - oy) as f32 * 8.0 + 1.0,
                    ];
                    assert_eq!(
                        layer.is_void_at(near),
                        matches!(at, Part::Void | Part::Liquid(_)),
                        "{why}"
                    );
                    assert_eq!(
                        layer.is_void_at(far),
                        matches!(rest, Part::Void | Part::Liquid(_)),
                        "{why}"
                    );
                    // Drawn: a triangle per half that isn't void, liquid
                    // translucent.
                    let alone = Layer::from_tiles(1, 1, vec![tile]);
                    let mut canvas = Canvas::new(&tileset);
                    alone.draw_tiles(&tileset, &mut canvas);
                    let halves = [at, rest].iter().filter(|p| **p != Part::Void).count();
                    assert_eq!(canvas.vertices.len(), 3 * halves, "{why}: drawn");
                    let translucent = canvas.vertices.iter().filter(|v| v.color[3] < 1.0).count();
                    assert_eq!(
                        translucent,
                        3 * [at, rest].iter().filter(|p| liquid(**p)).count(),
                        "{why}"
                    );
                    // And written down.
                    let text = tile.to_text(&tileset);
                    assert_eq!(Tile::from_text(&text, &tileset), Ok(tile), "{why}: {text}");
                }
            }
        }
    }

    #[test]
    fn meshes_are_made_once_and_match_the_tiles() {
        let l = level(&[&["....", "...."], &["~~..", "~≈.."]]);
        let tileset = l.tileset().clone();
        let area = |m: &Mesh| -> f32 {
            m.vertices
                .chunks(3)
                .map(|t| {
                    let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
                    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
                })
                .sum()
        };
        let top = l.meshes(1);
        assert!(Arc::ptr_eq(top, l.meshes(1)), "kept");
        // A small level is one chunk.
        assert_eq!(top.pieces.len(), 1);
        let piece = &top.pieces[0];
        let mut canvas = Canvas::new(&tileset);
        l.layer(1).draw_tiles(&tileset, &mut canvas);
        assert_eq!(piece.tiles.vertices.len(), canvas.vertices.len());
        // Four void tiles, one of them liquid; layer 0 shows through all
        // four.
        assert!((area(&piece.voids) - 400.0).abs() < 1e-3);
        assert!((area(&piece.liquid) - 100.0).abs() < 1e-3);
        assert!((area(&l.shaft_mesh(1, 0)) - 400.0).abs() < 1e-3);
        assert!(Arc::ptr_eq(&l.shaft_mesh(1, 0), &l.shaft_mesh(1, 0)));
    }

    /// Views, shafts and the layers seen, worked out from `open_above`,
    /// match working them out directly from every layer in between, on
    /// random stacks with plenty of void, half void, liquid and walls.
    #[test]
    fn summaries_match_looking_through_every_layer() {
        use crate::layer::{Corner, Part};
        use crate::rng::Rng;
        let parts = [Part::Floor(0), Part::Wall(0), Part::Void, Part::Liquid(0)];
        for seed in 0..12 {
            let mut rng = Rng::new(seed);
            let (cols, rows) = (9, 7);
            let depth = MAX_VISIBLE + 4;
            let layers: Vec<Layer> = (0..depth)
                .map(|_| {
                    let tiles = (0..cols * rows)
                        .map(|_| {
                            // Mostly void, so shafts run deep.
                            let part = |rng: &mut Rng| {
                                if rng.unit() < 0.6 {
                                    Part::Void
                                } else {
                                    parts[rng.range(0, 3)]
                                }
                            };
                            let (at, rest) = (part(&mut rng), part(&mut rng));
                            Tile::split(Corner::CLOCKWISE[rng.range(0, 3)], at, rest)
                        })
                        .collect();
                    Layer::from_tiles(cols, rows, tiles)
                })
                .collect();
            let tiles: Vec<Vec<Tile>> = layers
                .iter()
                .map(|l| {
                    (0..cols * rows)
                        .map(|t| l.tile((t % cols) as i32, (t / cols) as i32))
                        .collect()
                })
                .collect();
            let level = Level::new(layers, crate::tileset::testing::tileset());
            let void = |j: usize, t: usize| tiles[j][t].void_quarters();
            let open =
                |top: usize, j: usize, t: usize| (j + 1..=top).fold(0b1111, |q, l| q & void(l, t));
            for top in 0..depth {
                let mut seen = vec![top];
                for k in 0..=top {
                    let (view, shaft) = (level.view(top, k), level.shaft(top, k));
                    for t in 0..cols * rows {
                        let (x, y) = ((t % cols) as i32, (t / cols) as i32);
                        let q = (k + 1..top)
                            .map(|j| tiles[j][t].wall().quarters() & open(top, j, t))
                            .fold(tiles[top][t].wall().quarters(), |q, w| q | w);
                        let why = format!("seed {seed}, {k} from {top}, tile {t}");
                        assert_eq!(view.solid(x, y), Solid::from_quarters(q, true), "{why}");
                        assert_eq!(shaft[t], open(top, k, t) != 0, "{why}");
                    }
                    if k < top && top - k < MAX_VISIBLE && shaft.iter().any(|s| *s) {
                        seen.push(k);
                    }
                }
                seen[1..].sort_unstable_by(|a, b| b.cmp(a));
                assert_eq!(level.visible_layers(top), seen, "seed {seed}, from {top}");
            }
        }
    }

    #[test]
    fn only_views_near_the_viewer_are_kept() {
        let l = level(&[&["."], &["~"], &["~"], &["~"], &["~"]]);
        let near = l.view(4, 0);
        assert!(Arc::ptr_eq(&near, &l.view(4, 0)), "kept");
        l.view(3, 0);
        assert!(
            Arc::ptr_eq(&near, &l.view(4, 0)),
            "one layer away: still kept"
        );
        l.view(1, 0);
        assert!(!Arc::ptr_eq(&near, &l.view(4, 0)), "made again");
        assert!(l.views.lock().unwrap().keys().all(|&(top, _)| top >= 3));
    }

    #[test]
    fn no_more_than_max_visible_layers_are_drawn() {
        let mut rows: Vec<&[&str]> = vec![&["."]];
        rows.extend(std::iter::repeat_n(&["~"] as &[&str], MAX_VISIBLE + 3));
        let l = level(&rows);
        let top = l.depth() - 1;
        let seen = l.visible_layers(top);
        assert_eq!(seen.len(), MAX_VISIBLE);
        assert_eq!(
            seen,
            (top + 1 - MAX_VISIBLE..=top).rev().collect::<Vec<_>>()
        );
        assert!(
            l.shaft(top, 0)[0],
            "the bottom still shows through, undrawn"
        );
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
