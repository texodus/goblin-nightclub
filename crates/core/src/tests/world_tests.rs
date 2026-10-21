use super::*;
use crate::layer::{Dir, Part};
use crate::level_file::Spawn;
use crate::prop::Prop;
use crate::rng::Rng;

/// A level file `cols` x `rows` x `depth` of random tiles: floor,
/// wall, void, splits, stairs.
fn file(seed: u32, cols: usize, rows: usize, depth: usize) -> LevelFile {
    let mut rng = Rng::new(seed);
    let parts = [Part::Floor(0), Part::Wall(0), Part::Void, Part::Liquid(0)];
    let layers: Vec<Vec<Tile>> = (0..depth)
        .map(|_| {
            (0..cols * rows)
                .map(|_| match rng.unit() {
                    r if r < 0.5 => Tile::FLOOR,
                    r if r < 0.55 => Tile::Stairs(Dir::Down, 0),
                    _ => Tile::split(
                        crate::layer::Corner::CLOCKWISE[rng.range(0, 3)],
                        parts[rng.range(0, 3)],
                        parts[rng.range(0, 3)],
                    ),
                })
                .collect()
        })
        .collect();
    let mut layers = layers;
    // Somewhere to stand.
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
    // And props here and there (not where the player starts).
    .with_props(
        (0..depth)
            .map(|_| {
                let mut props = Vec::new();
                for t in 1..cols * rows {
                    if rng.unit() < 0.05 {
                        let kind = rng.range(0, 1) as u8;
                        props.push(Prop {
                            at: [t % cols, t / cols],
                            kind,
                        });
                    }
                }
                props
            })
            .collect(),
    )
}

#[test]
fn builds_go_a_piece_at_a_time_and_wait_for_chunks() {
    struct Slow(
        FileSource,
        std::sync::Mutex<std::collections::HashSet<ChunkKey>>,
    );
    impl ChunkSource for Slow {
        fn size(&self) -> [usize; 3] {
            self.0.size()
        }
        fn tileset(&self) -> &Tileset {
            self.0.tileset()
        }
        /// Ready from the second time it's asked.
        fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
            let asked = !self.1.lock().unwrap().insert(key);
            asked.then(|| self.0.chunk(key)).flatten()
        }
        fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
            self.0.roof(cx, cy)
        }
    }
    let file = file(4, 80, 64, 3);
    let source = Rc::new(Slow(FileSource::new(&file), Default::default()));
    let w = Window::around(source.size(), [10, 10], 2);
    let mut build = Build::new(source, w, 2);
    let mut steps = 1;
    while !build.step() {
        steps += 1;
        assert!(steps < 100);
    }
    // The roofs, each layer, the level, then each layer seen; each
    // layer waiting once for its chunks, all asked for together.
    let seen = build.level.as_ref().unwrap().visible_layers(2).len();
    assert_eq!(steps, 1 + 2 * w.depth + 1 + seen);
    // What's drawn of those, to have ready: what the level draws.
    let made: Vec<u64> = build.meshes().map(|m| m.id()).collect();
    let (level, _) = build.finish();
    let seen = level.visible_layers(2);
    let mut drawn: Vec<u64> = seen
        .iter()
        .flat_map(|&k| level.meshes(k).all().map(|m| m.id()))
        .collect();
    let below = seen.iter().filter(|&&k| k < 2);
    drawn.extend(below.map(|&k| level.shaft_mesh(2, k).id()));
    assert_eq!(made, drawn);
    // And it comes apart: each layer, each to move by, and each seen
    // through.
    assert!(level.take_apart().len() >= 2 * w.depth);
}
