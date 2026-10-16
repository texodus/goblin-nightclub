//! Display lists: colored triangles in pixel space, built by the game and
//! drawn by the renderer. No GPU types.

use crate::palette::Color;

/// Mirrors the vertex input of shapes.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub color: Color,
}

/// Triangles drawn in order, later ones on top.
#[derive(Clone, Default)]
pub struct Layer {
    pub vertices: Vec<Vertex>,
}

impl Layer {
    pub fn rect(&mut self, pos: [f32; 2], size: [f32; 2], color: Color) {
        let [x0, y0] = pos;
        let [x1, y1] = [x0 + size[0], y0 + size[1]];
        self.poly(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]], color);
    }

    /// A 1-pixel rectangle outline.
    pub fn outline(&mut self, pos: [f32; 2], size: [f32; 2], color: Color) {
        let [x, y] = pos;
        let [w, h] = size;
        self.rect([x, y], [w, 1.0], color);
        self.rect([x, y + h - 1.0], [w, 1.0], color);
        self.rect([x, y], [1.0, h], color);
        self.rect([x + w - 1.0, y], [1.0, h], color);
    }

    /// A convex polygon, as a triangle fan.
    pub fn poly(&mut self, points: &[[f32; 2]], color: Color) {
        for i in 1..points.len().saturating_sub(1) {
            for pos in [points[0], points[i], points[i + 1]] {
                self.vertices.push(Vertex { pos, color });
            }
        }
    }

    /// A convex polygon (clockwise on screen) with a 1-pixel bevel: `light`
    /// on edges facing up or left, `dark` on edges facing down or right.
    pub fn beveled(&mut self, points: &[[f32; 2]], fill: Color, light: Color, dark: Color) {
        let n = points.len();
        if n < 3 {
            return;
        }
        self.poly(points, fill);

        // Inward normal of edge i (points[i] -> points[i + 1]); clockwise on
        // screen with y down puts the inside on the right.
        let normal = |i: usize| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            let e = [b[0] - a[0], b[1] - a[1]];
            let len = e[0].hypot(e[1]);
            [-e[1] / len, e[0] / len]
        };
        // The outline inset by 1px, each corner moved along its bisector, so
        // the strips between the two tile the border exactly.
        let inset: Vec<[f32; 2]> = (0..n)
            .map(|i| {
                let (p, q) = (normal((i + n - 1) % n), normal(i));
                let k = 1.0 / (1.0 + p[0] * q[0] + p[1] * q[1]);
                [
                    points[i][0] + (p[0] + q[0]) * k,
                    points[i][1] + (p[1] + q[1]) * k,
                ]
            })
            .collect();
        for i in 0..n {
            let j = (i + 1) % n;
            // Outward normal is the negated inward one.
            let [nx, ny] = normal(i);
            let facing = -nx - ny;
            let lit = facing < -1e-3 || (facing.abs() <= 1e-3 && ny > 0.0);
            let color = if lit { light } else { dark };
            self.poly(&[points[i], points[j], inset[j], inset[i]], color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color = [1.0, 0.0, 0.0, 1.0];
    const GREEN: Color = [0.0, 1.0, 0.0, 1.0];
    const BLUE: Color = [0.0, 0.0, 1.0, 1.0];

    /// Color of the last triangle covering `p`, like the GPU would draw it.
    fn color_at(layer: &Layer, p: [f32; 2]) -> Option<Color> {
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
        let mut layer = Layer::default();
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
    fn bevel_strip_is_one_pixel_wide_on_diagonals() {
        let mut layer = Layer::default();
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
