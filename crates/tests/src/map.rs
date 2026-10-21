use goblin_nightclub_core::chunk::CHUNK;
use goblin_nightclub_core::layer::{Corner, Dir, Part, Tile};
use goblin_nightclub_core::map::*;
use goblin_nightclub_core::tileset::testing::tileset;

#[test]
fn quarters_have_codes_for_their_art() {
    let t = tileset();
    let map = Map::new(&t);
    let art = map.art();
    let of = |tile| map.code(tile).map(|c| &art[c as usize]);
    assert_eq!(map.code(Tile::Whole(Part::Void)), [VOID; 4]);
    assert_eq!(art[VOID as usize].alpha, 0.0, "void shows what's below");
    assert!(art[UNEXPLORED as usize].cells.is_empty());
    let floor = of(Tile::Whole(Part::Floor(1)));
    assert!(
        floor
            .iter()
            .all(|a| a.cells == t.floors[1].cells && a.alpha == 1.0)
    );
    let water = of(Tile::Whole(Part::Liquid(0)));
    assert_eq!(water[0].cells, t.liquids[0].cells);
    assert_eq!(water[0].alpha, t.liquids[0].alpha);
    // Stairs are drawn as stairs, whatever floor they're of.
    assert_eq!(of(Tile::Stairs(Dir::Down, 2))[0].cells, [t.stairs[1]]);
    // A split tile: the half at the corner, then the rest, by quarter
    // (N, E, S, W).
    let split = Tile::Split {
        corner: Corner::NE,
        at: Part::Wall(2),
        rest: Part::Floor(0),
    };
    let codes = map.code(split);
    assert_eq!(codes[0], codes[1]);
    assert_eq!(codes[2], codes[3]);
    assert_eq!(art[codes[0] as usize].cells, t.walls[2].cells);
    assert_eq!(art[codes[2] as usize].cells, t.floors[0].cells);
    // Every code differs, and none is unexplored.
    let mut all: Vec<u8> = [Part::Void, Part::Floor(3), Part::Wall(0), Part::Liquid(0)]
        .map(|p| map.code(Tile::Whole(p))[0])
        .to_vec();
    all.extend([Dir::Up, Dir::Down].map(|d| map.code(Tile::Stairs(d, 0))[0]));
    all.sort();
    all.dedup();
    assert_eq!(all.len(), 6);
    assert!(!all.contains(&UNEXPLORED));
}

#[test]
fn exploring_fills_in_chunks_and_changes_the_version() {
    let mut map = Map::new(&tileset());
    assert_eq!(map.bounds(0), None);
    let c = CHUNK as i32;
    let rock = Tile::Whole(Part::Wall(0));
    map.explore(2, [5, -1], rock);
    assert!(map.is_explored(2, [5, -1]));
    assert!(!map.is_explored(2, [6, -1]));
    assert!(!map.is_explored(1, [5, -1]), "another layer");
    assert_eq!(map.bounds(2), Some([[0, -1], [1, 0]]));
    let chunk = map.chunk((2, 0, -1)).unwrap();
    assert_eq!(chunk.tiles[((c - 1) * c + 5) as usize], map.code(rock));
    let version = chunk.version;
    assert_eq!(map.version(), version);

    map.explore(2, [5, -1], rock);
    assert_eq!(map.version(), version, "nothing new");
    map.explore(2, [3 * c, 0], rock);
    assert_eq!(map.bounds(2), Some([[0, -1], [4, 1]]));
    assert_eq!(map.chunk((2, 0, -1)).unwrap().version, version, "untouched");
    assert!(map.chunk((2, 3, 0)).unwrap().version > version);
}
