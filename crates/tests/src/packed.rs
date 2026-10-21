use goblin_nightclub_core::chunk::identical;
use goblin_nightclub_core::layer::{Corner, Dir, Part, Tile};
use goblin_nightclub_core::level_file::LevelFile;
use goblin_nightclub_core::level_file::Spawn;
use goblin_nightclub_core::packed::*;
use goblin_nightclub_core::rng::Rng;
use goblin_nightclub_core::world::{ChunkSource, FileSource};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

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
        goblin_nightclub_core::tileset::testing::tileset(),
        "player",
        "npc",
    )
}

fn same_tiles(a: &LevelFile, b: &LevelFile) -> bool {
    let (a, b) = (a.tiles(), b.tiles());
    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(a, b)| a.iter().zip(b).all(|(s, t)| identical(*s, *t)))
}

#[test]
fn tiles_round_trip_exactly() {
    let tileset = goblin_nightclub_core::tileset::testing::tileset();
    let parts = [Part::Floor(3), Part::Wall(1), Part::Void, Part::Liquid(0)];
    let mut tiles = vec![Tile::Stairs(Dir::Up, 0), Tile::Stairs(Dir::Down, 3)];
    for at in parts {
        tiles.push(Tile::Whole(at));
        for rest in parts.into_iter().filter(|r| *r != at) {
            tiles.extend(Corner::CLOCKWISE.map(|c| Tile::split(c, at, rest)));
        }
    }
    for t in tiles {
        let back = decode_tile(encode_tile(t), &tileset).unwrap();
        assert!(identical(back, t), "{t:?}");
    }
    // Nonsense, or ids the tileset hasn't.
    for bad in [
        [9, 0, 0, 0, 0, 0],
        [0, 1, 99, 0, 0, 0],
        [1, 2, 0, 2, 0, 0],
        [1, 1, 0, 2, 0, 7],
    ] {
        assert_eq!(decode_tile(bad, &tileset), None, "{bad:?}");
    }
}

#[test]
fn levels_round_trip_and_read_a_chunk_at_a_time() {
    let file = file(1, 70, 40, 3);
    let bytes: Arc<[u8]> = pack_file(&file).into();
    let packed = Packed::open(bytes.clone()).unwrap().unwrap();
    assert_eq!(packed.header().level, file.meta());
    assert_eq!(packed.header().chunks, 3 * 3 * 2);
    assert!(same_tiles(&packed.to_level_file().unwrap(), &file));
    // As a source, like the file itself.
    let source = FileSource::new(&file);
    assert_eq!(packed.size(), source.size());
    for key in [(0, 0, 0), (2, 2, 1)] {
        assert!(packed.chunk(key).unwrap().same(&source.chunk(key).unwrap()));
    }
    assert_eq!(packed.roof(1, 1).unwrap(), source.roof(1, 1).unwrap());
    assert!(packed.chunk((3, 0, 0)).is_none() && packed.roof(3, 0).is_none());
    // Packing is deterministic, and the id tells levels apart.
    assert_eq!(pack_file(&file), bytes.to_vec());
    assert_ne!(
        Packed::open(Arc::from(pack_file(&self::file(2, 70, 40, 3))))
            .unwrap()
            .unwrap()
            .header()
            .id,
        packed.header().id
    );
    // Uniform chunks pack small.
    let blank = LevelFile::new(
        320,
        320,
        &[vec![Tile::WALL; 320 * 320]],
        Spawn::at_tile(0, [0, 0]),
        goblin_nightclub_core::tileset::testing::tileset(),
        "player",
        "npc",
    );
    assert!(
        pack_file(&blank).len() < 12_000,
        "{}",
        pack_file(&blank).len()
    );
}

#[test]
fn slow_bytes_are_waited_for() {
    /// Ready from the second time they're asked for; a handle, so an
    /// open waiting on them can be tried again.
    #[derive(Clone)]
    struct Slow(
        Arc<[u8]>,
        Arc<Mutex<std::collections::HashSet<(u64, usize)>>>,
    );
    impl Bytes for Slow {
        fn read(&self, at: u64, len: usize) -> Option<Arc<[u8]>> {
            let asked = !self.1.lock().unwrap().insert((at, len));
            asked.then(|| self.0.read(at, len)).flatten()
        }
    }
    let slow = Slow(pack_file(&file(4, 40, 40, 2)).into(), Default::default());
    let mut tries = 1;
    let packed = loop {
        if let Some(packed) = Packed::open(slow.clone()).unwrap() {
            break packed;
        }
        tries += 1;
    };
    assert_eq!(
        tries, 4,
        "the start, the header, the index: each waited for"
    );
    assert!(packed.chunk((0, 0, 0)).is_none(), "not yet");
    assert!(packed.chunk((0, 0, 0)).is_some(), "now");
}

#[test]
fn files_open_as_a_level_or_a_pack_and_its_diffs() {
    let file = file(7, 60, 40, 2);
    let base: Arc<[u8]> = pack_file(&file).into();
    let id = Packed::open(base.clone())
        .unwrap()
        .unwrap()
        .header()
        .id
        .clone();
    let packed = Packed::open(base.clone()).unwrap().unwrap();
    let edit = |x: usize, spawn: f32| {
        let mut edited = file.clone();
        edited.layers[0][3][x] = Tile::VOID;
        edited.spawn.pos = [spawn, 5.0];
        Arc::<[u8]>::from(diff(&packed, &id, &edited).unwrap())
    };
    let (one, two) = (edit(10, 15.0), edit(50, 25.0));
    let json: Arc<[u8]> = file.to_json().into_bytes().into();
    assert!(matches!(
        open_files(vec![json.clone()]),
        Ok(Opened::File(_))
    ));
    assert!(
        open_files(vec![json, base.clone()]).is_err(),
        "a level file has no diffs"
    );
    // Diffs in the order given, the base anywhere.
    let Ok(Opened::Chunks {
        source,
        meta,
        base: b,
    }) = open_files(vec![one.clone(), base.clone(), two.clone()])
    else {
        panic!("opens");
    };
    assert_eq!(b.header().id, id);
    assert_eq!(meta.spawn.pos, [25.0, 5.0], "the last diff's");
    let level = to_level_file(&meta, source.as_ref()).unwrap();
    assert_eq!(level.layers[0][3][10], Tile::VOID);
    assert_eq!(level.layers[0][3][50], Tile::VOID);
    assert!(open_files(vec![one.clone()]).is_err(), "no base");
    assert!(open_files(vec![base.clone(), base.clone()]).is_err(), "two");
    let other: Arc<[u8]> = pack_file(&self::file(8, 60, 40, 2)).into();
    let Err(e) = open_files(vec![other, one]) else {
        panic!("refused");
    };
    assert!(e.contains("isn't of that level"), "{e}");
}

#[test]
fn diffs_hold_just_whats_changed() {
    let file = file(5, 100, 70, 3);
    let base: Arc<[u8]> = pack_file(&file).into();
    let packed = Rc::new(Packed::open(base).unwrap().unwrap());
    let id = packed.header().id.clone();
    // A hole knocked through every layer at one spot, the spawn moved.
    let mut edited = file.clone();
    for layer in &mut edited.layers {
        layer[50][90] = Tile::VOID;
    }
    edited.spawn.pos = [15.0, 15.0];
    let diff: Arc<[u8]> = diff(packed.as_ref(), &id, &edited).unwrap().into();
    let header = Packed::open(diff.clone())
        .unwrap()
        .unwrap()
        .header()
        .clone();
    assert_eq!(
        (header.chunks, header.roofs),
        (3, 1),
        "one chunk a layer, one roof"
    );
    assert_eq!(header.base.as_deref(), Some(id.as_str()));
    let whole = pack_file(&edited).len();
    assert!(diff.len() * 5 < whole, "{} of {whole}", diff.len());
    // Over the base, it's the edited level.
    let overlay = Overlay::new(packed.clone(), &id, diff.clone()).unwrap();
    assert_eq!(overlay.meta().spawn.pos, [15.0, 15.0]);
    let edited_source = FileSource::new(&edited);
    for k in 0..3 {
        for (x, y) in [(0, 0), (2, 1), (3, 2)] {
            assert!(
                overlay
                    .chunk((k, x, y))
                    .unwrap()
                    .same(&edited_source.chunk((k, x, y)).unwrap())
            );
        }
    }
    assert_eq!(
        overlay.roof(2, 1).unwrap(),
        edited_source.roof(2, 1).unwrap()
    );
    // Not over another level, nor resized.
    assert!(Overlay::new(packed.clone(), "0123", diff).is_err());
    let mut bigger = edited.clone();
    bigger.cols += 1;
    for layer in &mut bigger.layers {
        for row in layer {
            row.push(Tile::FLOOR);
        }
    }
    assert!(
        goblin_nightclub_core::packed::diff(packed.as_ref(), &id, &bigger)
            .unwrap_err()
            .contains("resized")
    );
}
