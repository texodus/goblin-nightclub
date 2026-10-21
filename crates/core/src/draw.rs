//! Display lists: triangles in world space, built by the game and drawn
//! by the renderer. No GPU types. Each triangle shows the tile sheet
//! (see [`crate::tileset`]) times its color; plain colored shapes show the
//! sheet's white cell.

use crate::layer::TILE;
use crate::palette::Color;
use crate::tileset::{Cell, Tileset};

/// Mirrors the vertex input of shapes.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    /// Position in the tile sheet, in pixels. The default, its top-left
    /// pixel, is in the white cell.
    pub uv: [f32; 2],
    /// Top-left of the cell `uv` is in, in pixels, so sampling stays inside
    /// it.
    pub cell: [f32; 2],
    pub color: Color,
    /// For a fringe (see [`Canvas::fringe`]): a point on the edge it grows
    /// from and the edge's outward normal, or a zero normal to grow from
    /// just the point.
    pub edge: [f32; 4],
    /// For a fringe, how far it may reach past the edge, in world units;
    /// 0 for anything else.
    pub reach: f32,
    /// How far up it is from its layer toward the one above, in layers: so
    /// drawn that much larger, as layers are drawn smaller the deeper they
    /// are (parallax). 0 for the ground; a prop's art may be a little
    /// higher (see [`crate::tileset::PropKind::height`]).
    pub z: f32,
}

/// Triangles drawn in order, later ones on top.
#[derive(Clone)]
pub struct Canvas {
    pub vertices: Vec<Vertex>,
    /// Edge of a cell of the sheet, in pixels.
    cell: u32,
    /// The sheet's white cell, for plain shapes.
    white: Cell,
}

impl Canvas {
    /// An empty canvas drawing from `tileset`'s sheet.
    pub fn new(tileset: &Tileset) -> Self {
        Self {
            vertices: Vec::new(),
            cell: tileset.cell,
            white: tileset.white,
        }
    }

    /// Sheet pixels per world unit.
    fn zoom(&self) -> f32 {
        self.cell as f32 / TILE
    }

    pub fn rect(&mut self, pos: [f32; 2], size: [f32; 2], color: Color) {
        let [x0, y0] = pos;
        let [x1, y1] = [x0 + size[0], y0 + size[1]];
        self.poly(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], color);
    }

    /// A rectangle outline `width` wide.
    pub fn outline(&mut self, pos: [f32; 2], size: [f32; 2], width: f32, color: Color) {
        let [x, y] = pos;
        let [w, h] = size;
        self.rect([x, y], [w, width], color);
        self.rect([x, y + h - width], [w, width], color);
        self.rect([x, y], [width, h], color);
        self.rect([x + w - width, y], [width, h], color);
    }

    /// A convex polygon, as a triangle fan, in a plain color: it samples
    /// the middle of the sheet's white cell.
    pub fn poly(&mut self, points: &[[f32; 2]], color: Color) {
        let white = self.cell_origin(self.white);
        let mid = [white[0] + 0.5, white[1] + 0.5];
        self.fan(points, |_| mid, white, color);
    }

    /// Top-left pixel of sheet cell `cell`.
    fn cell_origin(&self, cell: Cell) -> [f32; 2] {
        [
            (cell[0] as u32 * self.cell) as f32,
            (cell[1] as u32 * self.cell) as f32,
        ]
    }

    /// A convex polygon showing sheet cell `cell` (tinted by `color`), the
    /// cell's top-left corner at world point `origin`, at the sheet's zoom:
    /// so a tile-sized square at a tile's corner shows the whole cell.
    pub fn textured(&mut self, points: &[[f32; 2]], origin: [f32; 2], cell: Cell, color: Color) {
        let base = self.cell_origin(cell);
        let zoom = self.zoom();
        let uv = |p: [f32; 2]| {
            [
                base[0] + (p[0] - origin[0]) * zoom,
                base[1] + (p[1] - origin[1]) * zoom,
            ]
        };
        self.fan(points, uv, base, color);
    }

    /// A strip of tile art past the edge of a tile, cut raggedly by the
    /// renderer: `points` (a convex polygon) show sheet cell `cell` as
    /// [`Canvas::textured`] would with the tile's corner at `origin`, the
    /// cell repeating past its sides, but only out to a varying distance
    /// from `edge` (see [`Vertex::edge`]), at most `reach`. Its outermost
    /// pixels have `rim` blended over them, by its alpha.
    pub fn fringe(
        &mut self,
        points: &[[f32; 2]],
        origin: [f32; 2],
        cell: Cell,
        rim: Color,
        edge: [f32; 4],
        reach: f32,
    ) {
        let start = self.vertices.len();
        self.textured(points, origin, cell, rim);
        for v in &mut self.vertices[start..] {
            v.edge = edge;
            v.reach = reach;
        }
    }

    /// Sheet cell `cell` as a tile-sized square centered on `center`,
    /// snapped to whole sheet pixels so it lines up with the tiles'
    /// pixels.
    pub fn sprite(&mut self, center: [f32; 2], cell: Cell, color: Color) {
        self.sprite_from([0.0; 2], center, cell, color);
    }

    /// The same, centered `center` from `origin`, a point on the sheet's
    /// pixel grid (a tile's corner), and snapped from there: so it lands on
    /// the same pixels, to the last bit, wherever `origin` is.
    pub fn sprite_from(&mut self, origin: [f32; 2], center: [f32; 2], cell: Cell, color: Color) {
        let size = [TILE, TILE];
        let zoom = self.zoom();
        let snap = |v: f32| (v * zoom).round() / zoom;
        let [x0, y0] = [
            origin[0] + snap(center[0] - size[0] / 2.0),
            origin[1] + snap(center[1] - size[1] / 2.0),
        ];
        let [x1, y1] = [x0 + size[0], y0 + size[1]];
        self.textured(
            &[[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
            [x0, y0],
            cell,
            color,
        );
    }

    fn fan(
        &mut self,
        points: &[[f32; 2]],
        uv: impl Fn([f32; 2]) -> [f32; 2],
        cell: [f32; 2],
        color: Color,
    ) {
        for i in 1..points.len().saturating_sub(1) {
            for pos in [points[0], points[i], points[i + 1]] {
                self.vertices.push(Vertex {
                    pos,
                    uv: uv(pos),
                    cell,
                    color,
                    ..Default::default()
                });
            }
        }
    }

    /// A convex polygon (clockwise on screen) with a 1-unit bevel: `light`
    /// on edges facing up or left, `dark` on edges facing down or right.
    #[cfg(test)]
    pub fn beveled(&mut self, points: &[[f32; 2]], fill: Color, light: Color, dark: Color) {
        let n = points.len();
        if n < 3 {
            return;
        }
        self.poly(points, fill);

        use crate::geom::{inset_corner, inward_normal};
        let normal = |i: usize| inward_normal(points[i], points[(i + 1) % n]);
        let inset: Vec<[f32; 2]> = (0..n)
            .map(|i| inset_corner(points[i], normal((i + n - 1) % n), normal(i), 1.0))
            .collect();
        for i in 0..n {
            let j = (i + 1) % n;
            let color = if faces_light(normal(i)) { light } else { dark };
            self.poly(&[points[i], points[j], inset[j], inset[i]], color);
        }
    }
}

/// Whether a face with inward normal `n` faces up or left (and so is lit):
/// its outward normal, `-n`, points that way. Faces exactly up-right or
/// down-left count as lit if they face up.
pub fn faces_light(n: [f32; 2]) -> bool {
    let facing = -n[0] - n[1];
    facing < -1e-3 || (facing.abs() <= 1e-3 && n[1] > 0.0)
}

#[cfg(test)]
#[path = "tests/draw_tests.rs"]
mod tests;
