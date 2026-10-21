use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::layer::{Layer, TILE};
use goblin_nightclub_core::mesh::Mesh;
use goblin_nightclub_core::prop::*;
use goblin_nightclub_core::tileset::testing::tileset;

#[test]
fn props_are_drawn_about_their_tile_and_found_where_they_overhang() {
    let t = tileset();
    let tree = t.prop("tree").unwrap();
    // A tree at the top of the second row of mesh chunks: its art
    // reaches up into the first.
    let at = [3, goblin_nightclub_core::mesh::CHUNK];
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
    let found = |m: &Mesh| m.ranges(above).map(|r| r.len()).sum::<usize>();
    assert_eq!(found(&mesh(t.prop_reach())), 4 * 6);
    assert_eq!(
        found(&mesh(0.0)),
        0,
        "only half a tile past, it would be missed"
    );
}
