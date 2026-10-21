use super::*;
use crate::level_file::Spawn;
use crate::prop::Prop;
use crate::rng::Rng;

/// A level of random tiles, every kind, `cols` x `rows` x `depth`.
fn file(seed: u32, cols: usize, rows: usize, depth: usize) -> LevelFile {
    let mut rng = Rng::new(seed);
    let parts = [Part::Floor(1), Part::Wall(2), Part::Void, Part::Liquid(0)];
    let mut layers: Vec<Vec<Tile>> = (0..depth)
        .map(|_| {
            (0..cols * rows)
                .map(|_| match rng.unit() {
                    r if r < 0.4 => Tile::FLOOR,
                    r if r < 0.45 => Tile::Stairs(Dir::Up, 2),
                    r if r < 0.5 => Tile::Stairs(Dir::Down, 0),
                    _ => Tile::split(
                        Corner::CLOCKWISE[rng.range(0, 3)],
                        parts[rng.range(0, 3)],
                        parts[rng.range(0, 3)],
                    ),
                })
                .collect()
        })
        .collect();
    layers[depth - 1][0] = Tile::FLOOR;
    LevelFile::new(
        cols,
        rows,
        &layers,
        Spawn::at_tile(depth - 1, [0, 0]),
        crate::tileset::testing::tileset(),
        "player",
        "npc",
    )
}

#[test]
fn props_pack_and_older_packs_read_without() {
    let file = file(8, 70, 40, 2);
    let (tree, rock) = (
        file.tileset.prop("tree").unwrap(),
        file.tileset.prop("rock").unwrap(),
    );
    let props = vec![
        vec![
            Prop {
                at: [1, 2],
                kind: tree,
            },
            Prop {
                at: [69, 39],
                kind: rock,
            },
        ],
        vec![Prop {
            at: [33, 5],
            kind: rock,
        }],
    ];
    let file = file.with_props(props.clone());
    let packed = Packed::open(Arc::<[u8]>::from(pack_file(&file)))
        .unwrap()
        .unwrap();
    assert_eq!(packed.to_level_file().unwrap().props, props);
    assert_eq!(packed.chunk((1, 1, 0)).unwrap().prop(1, 5), Some(rock));
    // Version 1's chunks end with their tiles: read, they've no props.
    let chunk = Chunk::filled(Tile::FLOOR);
    let mut v1 = 1u16.to_le_bytes().to_vec();
    v1.extend(encode_tile(Tile::FLOOR));
    let v1 = compress_to_vec(&v1, 6);
    let read = decode_chunk(&v1, &file.tileset, 1).unwrap();
    assert!(read.same(&chunk) && read.props().is_empty());
    // Version 2's must say how many props they have; and no more.
    assert!(decode_chunk(&v1, &file.tileset, 2).is_none());
    let with = chunk.with_props(vec![(7, rock)]);
    assert!(
        decode_chunk(&encode_chunk(&with), &file.tileset, 2)
            .unwrap()
            .same(&with)
    );
    assert!(decode_chunk(&encode_chunk(&with), &file.tileset, 1).is_none());
}

#[test]
fn bad_files_are_refused_and_bad_chunks_reported() {
    let file = file(3, 40, 40, 2);
    let bytes = pack_file(&file);
    let refused = |b: Vec<u8>| Packed::open(Arc::<[u8]>::from(b)).err().unwrap();
    assert!(refused(b"not a level at all".to_vec()).contains("isn't a packed level"));
    let mut newer = bytes.clone();
    newer[4] = 3;
    assert!(refused(newer).contains("version 3"));
    assert!(refused(bytes[..40].to_vec()).contains("header"));
    // A chunk's bytes spoiled: a wall stands in, and it's reported.
    let packed = Packed::open(Arc::<[u8]>::from(bytes.clone()))
        .unwrap()
        .unwrap();
    let (at, len) = packed.chunks[&(1, 1, 1)];
    let mut spoiled = bytes;
    spoiled[at as usize..(at + len as u64) as usize].fill(0xff);
    let packed = Packed::open(Arc::<[u8]>::from(spoiled)).unwrap().unwrap();
    assert!(packed.problem().is_none());
    assert_eq!(packed.chunk((1, 1, 1)).unwrap().uniform(), Some(Tile::WALL));
    assert!(packed.problem().unwrap().contains("(1, 1, 1)"));
}
