//! Props: things standing on tiles, like trees and rocks, at most one a
//! tile. What each kind looks like and does is the tileset's (see
//! [`crate::tileset::PropKind`]); which stands where is kept with the
//! tiles, in chunks (see [`crate::chunk`]).

/// A kind of prop: an index into the tileset's props.
pub type PropId = u8;

/// A prop standing on a tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Prop {
    /// Its tile: column and row.
    pub at: [usize; 2],
    pub kind: PropId,
}

use crate::draw::Canvas;
use crate::layer::{Layer, TILE};
use crate::palette;
use crate::tileset::{self, Tileset};

/// Draw the prop on tile `(x, y)` of `layer`, if there's one: its art (a
/// variant picked per tile, as tiles' are, by where it is in the whole
/// level), a tile a cell, placed about its tile as its kind says, as high
/// as its kind says.
pub fn draw_prop(layer: &Layer, tileset: &Tileset, canvas: &mut Canvas, x: usize, y: usize) {
    let Some(kind) = layer.prop(x as i32, y as i32) else {
        return;
    };
    let kind = &tileset.props[kind as usize];
    let [ax, ay] = layer.absolute(x, y);
    let [cx, cy] = tileset::variant(&kind.cells, ax, ay);
    let corner = [
        (x as f32 - kind.anchor[0]) * TILE,
        (y as f32 - kind.anchor[1]) * TILE,
    ];
    let start = canvas.vertices.len();
    for j in 0..kind.size[1] {
        for i in 0..kind.size[0] {
            let at = [
                corner[0] + f32::from(i) * TILE,
                corner[1] + f32::from(j) * TILE,
            ];
            let [x1, y1] = [at[0] + TILE, at[1] + TILE];
            let quad = [at, [x1, at[1]], [x1, y1], [at[0], y1]];
            canvas.textured(&quad, at, [cx + i, cy + j], palette::WHITE);
        }
    }
    for v in &mut canvas.vertices[start..] {
        v.z = kind.height;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Mesh;
    use crate::tileset::testing::tileset;

    #[test]
    fn props_are_drawn_about_their_tile_and_found_where_they_overhang() {
        let t = tileset();
        let tree = t.prop("tree").unwrap();
        // A tree at the top of the second row of mesh chunks: its art
        // reaches up into the first.
        let at = [3, crate::mesh::CHUNK];
        let rows: Vec<String> = (0..40).map(|_| ".".repeat(20)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let layer = Layer::from_ascii(&rows).with_props(&[Prop { at, kind: tree }]);
        let mut canvas = Canvas::new(&t);
        draw_prop(&layer, &t, &mut canvas, at[0], at[1]);
        // Two cells square: four quads, the trunk's tile its bottom middle.
        assert_eq!(canvas.vertices.len(), 4 * 6);
        let (xs, ys): (Vec<f32>, Vec<f32>) =
            canvas.vertices.iter().map(|v| (v.pos[0], v.pos[1])).unzip();
        let span = |v: &[f32]| {
            [
                v.iter().copied().fold(f32::MAX, f32::min),
                v.iter().copied().fold(f32::MIN, f32::max),
            ]
        };
        let [x, y] = at.map(|t| t as f32 * TILE);
        assert_eq!(span(&xs), [x - TILE / 2.0, x + 1.5 * TILE]);
        assert_eq!(span(&ys), [y - TILE, y + TILE]);
        assert!(
            canvas
                .vertices
                .iter()
                .all(|v| v.z == t.props[tree as usize].height)
        );
        // Nothing drawn where there's none.
        let mut empty = Canvas::new(&t);
        draw_prop(&layer, &t, &mut empty, 4, 4);
        assert!(empty.vertices.is_empty());

        // In a mesh, it's found from above its tile's chunk.
        let mesh = |reach: f32| {
            Mesh::build(20, 40, Canvas::new(&t), |xs, ys, canvas| {
                for y in ys {
                    for x in xs.clone() {
                        draw_prop(&layer, &t, canvas, x, y);
                    }
                }
            })
            .reaching(reach)
        };
        let above = [[x, y - 0.9 * TILE], [x + TILE, y - 0.8 * TILE]];
        let found = |m: &Mesh| m.ranges(above).iter().map(|r| r.len()).sum::<usize>();
        assert_eq!(found(&mesh(t.prop_reach())), 4 * 6);
        assert_eq!(
            found(&mesh(0.0)),
            0,
            "only half a tile past, it would be missed"
        );
    }
}
