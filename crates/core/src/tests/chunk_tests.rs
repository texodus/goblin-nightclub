use super::*;
use crate::layer::Part;

#[test]
fn more_than_256_tiles_take_two_bytes() {
    let tile = |x: usize, y: usize| {
        let i = y * CHUNK + x;
        if i < 256 {
            Tile::Whole(Part::Floor(i as u8))
        } else {
            Tile::Whole(Part::Wall((i % 256) as u8))
        }
    };
    let chunk = Chunk::from_fn(tile);
    assert_eq!(chunk.palette().len(), 512);
    assert!(matches!(chunk.cells, Cells::Wide(_)));
    assert_eq!(chunk.get(5, 20), tile(5, 20));
}
