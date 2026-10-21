use goblin_nightclub_core::draw::Canvas;
use goblin_nightclub_core::fringe::*;
use goblin_nightclub_core::layer::{Layer, Part, TILE, Tile};
use goblin_nightclub_core::palette::Color;
use goblin_nightclub_core::tileset::{self, Tileset};
use goblin_nightclub_core::tileset::{Edge, Trim};

/// The test tileset with floor 0 (`plain`) growing a 2-pixel fringe
/// over lower floors (the rest), and rock walls a 1-pixel one, and
/// `dirt` trimmed every edge, 3 pixels in.
fn tileset() -> Tileset {
    let mut t = tileset::testing::tileset();
    t.floors[0].edge = Edge {
        priority: 5,
        width: 2.0,
        rim: Some([0.0, 0.0, 0.0, 0.5]),
    };
    t.walls[0].edge = Edge {
        priority: 0,
        width: 1.0,
        rim: None,
    };
    t.floors[2].trim = Some(Trim {
        cells: vec![[8, 3]],
        density: 1.0,
        inset: 3.0,
    });
    t
}

fn fringes(layer: &Layer, x: usize, y: usize) -> Vec<goblin_nightclub_core::draw::Vertex> {
    let tileset = tileset();
    let mut canvas = Canvas::new(&tileset);
    draw_fringes(layer, &tileset, &mut canvas, x, y);
    canvas.vertices
}

fn area(vertices: &[goblin_nightclub_core::draw::Vertex]) -> f32 {
    vertices
        .chunks(3)
        .map(|t| {
            let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
        })
        .sum()
}

#[test]
fn higher_ranks_grow_over_lower_only() {
    // Floor beside void on the right and below; a wall above.
    let layer = Layer::from_ascii(&["##", ".~", "~~"]);
    let reach = 2.0 / tileset().zoom();
    let floor = fringes(&layer, 0, 1);
    // A strip right and one down, and the square between them, all
    // `reach` out; nothing up into the wall or left off the level.
    assert!((area(&floor) - (2.0 * TILE * reach + reach * reach)).abs() < 1e-3);
    assert!(floor.iter().all(|v| v.reach == reach));
    assert!(
        floor.iter().all(|v| v.pos[1] >= TILE - 1e-3),
        "not over the wall"
    );
    // The wall grows a 1-pixel strip down over the floor (the wall
    // beside it is its equal).
    let wall = fringes(&layer, 0, 0);
    assert!((area(&wall) - TILE / tileset().zoom()).abs() < 1e-3);
    // Void grows nothing.
    assert!(fringes(&layer, 1, 2).is_empty());
}

#[test]
fn walls_rim_by_the_light_floors_by_their_color() {
    let layer = Layer::from_ascii(&["~~~", "~#~", "~~~"]);
    let t = tileset();
    let wall = fringes(&layer, 1, 1);
    let rims: Vec<Color> = wall.iter().map(|v| v.color).collect();
    assert!(rims.contains(&t.walls[0].light) && rims.contains(&t.walls[0].dark));
    let floor = fringes(&Layer::from_ascii(&["~~~", "~.~", "~~~"]), 1, 1);
    assert!(floor.iter().all(|v| v.color == [0.0, 0.0, 0.0, 0.5]));
}

#[test]
fn diagonal_fringes_stay_inside_their_tile() {
    // Floor in the north-west half, void in the rest.
    let layer = Layer::from_ascii(&["▛"]);
    let v = fringes(&layer, 0, 0);
    assert!(!v.is_empty());
    for p in v.iter().map(|v| v.pos) {
        assert!(p.iter().all(|c| (-1e-3..=TILE + 1e-3).contains(c)), "{p:?}");
        // In the void half, at most `reach` from the diagonal.
        let d = (p[0] + p[1] - TILE) / std::f32::consts::SQRT_2;
        assert!(
            (-1e-3..=2.0 / tileset().zoom() + 1e-3).contains(&d),
            "{p:?}"
        );
    }
}

#[test]
fn trims_line_edges_with_other_surfaces() {
    let mut layer_tiles = Layer::from_ascii(&["....", "....", "...."]);
    // Dirt in the middle of plain floor.
    let dirt = Tile::Whole(Part::Floor(2));
    layer_tiles = {
        let mut tiles: Vec<Tile> = (0..12).map(|i| layer_tiles.tile(i % 4, i / 4)).collect();
        tiles[5] = dirt;
        tiles[6] = dirt;
        Layer::from_tiles(4, 3, tiles)
    };
    let t = tileset();
    let mut canvas = Canvas::new(&t);
    for y in 0..3 {
        for x in 0..4 {
            draw_trims(&layer_tiles, &t, &mut canvas, x, y);
        }
    }
    // Density 1: one per dirt edge with plain floor (not between the
    // two dirt tiles): 6 edges, a square sprite each.
    assert_eq!(canvas.vertices.len(), 6 * 6);
    // Each 3 pixels inside the dirt: sprite centers within the two
    // tiles, pulled in from the edges.
    let inset = 3.0 / t.zoom();
    for s in canvas.vertices.chunks(6) {
        let c = [
            s.iter().map(|v| v.pos[0]).sum::<f32>() / 6.0,
            s.iter().map(|v| v.pos[1]).sum::<f32>() / 6.0,
        ];
        assert!(
            c[0] >= TILE + inset - 0.1 && c[0] <= 3.0 * TILE - inset + 0.1,
            "{c:?}"
        );
        assert!(
            c[1] >= TILE + inset - 0.1 && c[1] <= 2.0 * TILE - inset + 0.1,
            "{c:?}"
        );
    }
    // The same every time.
    let mut again = Canvas::new(&t);
    for y in 0..3 {
        for x in 0..4 {
            draw_trims(&layer_tiles, &t, &mut again, x, y);
        }
    }
    assert_eq!(
        canvas.vertices.iter().map(|v| v.pos).collect::<Vec<_>>(),
        again.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()
    );
}
