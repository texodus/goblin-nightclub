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

    /// Sheet cell `cell` as a tile-sized square (see
    /// [`Tileset::sprite_size`]) centered on `center`, snapped to whole
    /// sheet pixels so it lines up with the tiles' pixels.
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

/// Unit normal of `a`-`b` pointing to its right, which is the inside of an
/// outline running clockwise on screen (y down).
pub fn inward_normal(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    let e = [b[0] - a[0], b[1] - a[1]];
    let len = crate::length(e);
    [-e[1] / len, e[0] / len]
}

/// Corner `p` of an outline moved `width` inward, between edges with
/// inward normals `before` and `after`, along the bisector, so strips that
/// wide along the two edges meet exactly (at concave and sharp corners
/// too).
pub fn inset_corner(p: [f32; 2], before: [f32; 2], after: [f32; 2], width: f32) -> [f32; 2] {
    let k = width / (1.0 + before[0] * after[0] + before[1] * after[1]);
    [
        p[0] + (before[0] + after[0]) * k,
        p[1] + (before[1] + after[1]) * k,
    ]
}

/// Whether a face with inward normal `n` faces up or left (and so is lit):
/// its outward normal, `-n`, points that way. Faces exactly up-right or
/// down-left count as lit if they face up.
pub fn faces_light(n: [f32; 2]) -> bool {
    let facing = -n[0] - n[1];
    facing < -1e-3 || (facing.abs() <= 1e-3 && n[1] > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = [1.0, 0.0, 0.0, 1.0];
    const GREEN: Color = [0.0, 1.0, 0.0, 1.0];
    const BLUE: Color = [0.0, 0.0, 1.0, 1.0];

    const ZOOM: f32 = 1.6;
    const CELL: u32 = 16;

    fn canvas() -> Canvas {
        Canvas::new(&crate::tileset::testing::tileset())
    }

    /// Color of the last triangle covering `p`, like the GPU would draw it.
    fn color_at(layer: &Canvas, p: [f32; 2]) -> Option<Color> {
        layer.vertices.chunks(3).rev().find_map(|t| {
            let cross = |a: [f32; 2], b: [f32; 2]| {
                (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
            };
            let s = [
                cross(t[0].pos, t[1].pos),
                cross(t[1].pos, t[2].pos),
                cross(t[2].pos, t[0].pos),
            ];
            let inside = s.iter().all(|v| *v >= 0.0) || s.iter().all(|v| *v <= 0.0);
            inside.then_some(t[0].color)
        })
    }

    #[test]
    fn bevels_light_top_left_and_shade_bottom_right() {
        let mut layer = canvas();
        // A 10x10 square with its bottom-right corner cut at 45 degrees.
        let shape = [
            [0.0, 0.0],
            [10.0, 0.0],
            [10.0, 5.0],
            [5.0, 10.0],
            [0.0, 10.0],
        ];
        layer.beveled(&shape, RED, GREEN, BLUE);
        assert_eq!(color_at(&layer, [5.0, 0.5]), Some(GREEN), "top");
        assert_eq!(color_at(&layer, [0.5, 5.0]), Some(GREEN), "left");
        assert_eq!(color_at(&layer, [9.5, 2.0]), Some(BLUE), "right");
        assert_eq!(color_at(&layer, [2.0, 9.5]), Some(BLUE), "bottom");
        assert_eq!(
            color_at(&layer, [7.3, 7.3]),
            Some(BLUE),
            "diagonal facing down-right"
        );
        assert_eq!(color_at(&layer, [5.0, 5.0]), Some(RED), "fill");
        assert_eq!(color_at(&layer, [9.0, 9.0]), None, "cut off");
    }

    #[test]
    fn sprites_center_on_whole_pixels_and_show_their_cell() {
        let mut canvas = canvas();
        canvas.sprite([20.0, 30.0], [1, 3], RED);
        let size = [TILE, TILE];
        let xs: Vec<f32> = canvas.vertices.iter().map(|v| v.pos[0]).collect();
        let ys: Vec<f32> = canvas.vertices.iter().map(|v| v.pos[1]).collect();
        let (x0, x1) = (
            xs.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            ys.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MIN, f32::max),
        );
        assert!((x1 - x0 - size[0]).abs() < 1e-4 && (y1 - y0 - size[1]).abs() < 1e-4);
        assert!(
            ((x0 + x1) / 2.0 - 20.0).abs() <= 0.5 / ZOOM,
            "centered, to a pixel"
        );
        assert!(((y0 + y1) / 2.0 - 30.0).abs() <= 0.5 / ZOOM);
        for v in [x0, y0] {
            assert!(
                ((v * ZOOM) - (v * ZOOM).round()).abs() < 1e-3,
                "{v} on a pixel"
            );
        }
        // The square spans exactly the cell in the sheet.
        for v in &canvas.vertices {
            let [u, w] = v.uv;
            let c = CELL as f32;
            assert!(
                (c..=2.0 * c).contains(&u) && (3.0 * c..=4.0 * c).contains(&w),
                "{u} {w}"
            );
            assert_eq!(v.color, RED);
        }
        // Plain shapes show the white cell.
        let mut plain = Canvas::new(&crate::tileset::testing::tileset());
        plain.rect([0.0, 0.0], [5.0, 5.0], RED);
        assert!(
            plain
                .vertices
                .iter()
                .all(|v| v.uv[0] < CELL as f32 && v.uv[1] < CELL as f32)
        );
    }

    #[test]
    fn bevel_strip_is_one_pixel_wide_on_diagonals() {
        let mut layer = canvas();
        // A "/" face facing up-left: lit.
        let shape = [[10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        layer.beveled(&shape, RED, GREEN, BLUE);
        let r = std::f32::consts::FRAC_1_SQRT_2;
        // Along the face's inward normal from its midpoint (5,5).
        assert_eq!(
            color_at(&layer, [5.0 + 0.9 * r, 5.0 + 0.9 * r]),
            Some(GREEN)
        );
        assert_eq!(color_at(&layer, [5.0 + 1.1 * r, 5.0 + 1.1 * r]), Some(RED));
    }
}
