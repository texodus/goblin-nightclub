use super::tile::{E, N, S, W};
use super::*;

/// Twice the signed area: positive clockwise on screen (around solid).
fn area2(points: &[[f32; 2]]) -> f32 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// Everything two layers know about their shape, compared.
#[track_caller]
fn assert_same_shape(a: &Layer, b: &Layer) {
    assert_eq!((a.cols, a.rows), (b.cols, b.rows));
    assert!(a.solids == b.solids, "solids");
    assert_eq!(a.corners, b.corners);
    assert_eq!(a.edges, b.edges);
    assert_eq!((&a.next, &a.prev), (&b.next, &b.prev));
    assert_eq!(a.concave, b.concave);
    for y in 0..a.rows as i32 {
        for x in 0..a.cols as i32 {
            assert!(identical_tile(a.tile(x, y), b.tile(x, y)), "({x}, {y})");
        }
    }
}

fn identical_tile(a: Tile, b: Tile) -> bool {
    crate::chunk::identical(a, b)
}

#[test]
fn layers_from_pieces_are_layers_from_tiles() {
    // Not whole chunks: the last are cut short, one to a single column.
    for (seed, [cols, rows]) in [(1, [100, 70]), (2, [64, 64]), (3, [97, 33]), (4, [20, 9])] {
        let soup = Layer::soup(seed, cols, rows);
        let tiles: Vec<Tile> = (0..cols * rows)
            .map(|i| soup.tile((i % cols) as i32, (i / cols) as i32))
            .collect();
        let chunks = Chunks::from_tiles(cols, rows, &tiles);
        let [across, down] = chunks.size();
        let size =
            |cx: usize, cy: usize| [CHUNK.min(cols - cx * CHUNK), CHUNK.min(rows - cy * CHUNK)];
        let pieces: Vec<Arc<Piece>> = (0..across * down)
            .map(|c| {
                let [cx, cy] = [c % across, c / across];
                Arc::new(Piece::new(chunks.chunk(cx, cy).clone(), size(cx, cy)))
            })
            .collect();
        let whole = Layer::from_chunks(chunks.clone());
        assert_same_shape(&Layer::from_pieces(cols, rows, &pieces), &whole);
        // And shaped by solids alone.
        let solids: Vec<Solid> = tiles.iter().map(|t| t.wall()).collect();
        let pieces: Vec<Arc<Piece>> = (0..across * down)
            .map(|c| {
                let [cx, cy] = [c % across, c / across];
                let [w, h] = size(cx, cy);
                let solids = (0..w * h)
                    .map(|i| solids[(cy * CHUNK + i / w) * cols + cx * CHUNK + i % w])
                    .collect();
                Arc::new(Piece::from_solids([w, h], solids))
            })
            .collect();
        assert_same_shape(
            &Layer::from_pieces(cols, rows, &pieces),
            &Layer::from_solids(cols, rows, solids),
        );
    }
}

#[test]
fn quarter_shapes_cover_their_quarters() {
    let area = |q: u8| -> f32 {
        quarter_shapes(1, 1, q)
            .iter()
            .map(|s| area2(s).abs() / 2.0)
            .sum()
    };
    assert_eq!(quarter_shapes(1, 1, N | E | S | W).len(), 1);
    assert_eq!(quarter_shapes(1, 1, S | E).len(), 1, "a half is one shape");
    assert!((area(N | E | S | W) - 100.0).abs() < 1e-3);
    assert!((area(S | E) - 50.0).abs() < 1e-3);
    assert!((area(N) - 25.0).abs() < 1e-3);
    assert!((area(N | S) - 50.0).abs() < 1e-3);
    assert!((area(N | E | S) - 75.0).abs() < 1e-3);
    assert!(quarter_shapes(1, 1, 0).is_empty());
}
