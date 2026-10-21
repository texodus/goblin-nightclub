use goblin_nightclub_core::entity::PLAYER_SIZE;
use goblin_nightclub_core::layer::{Corner, Dir, Layer, Part, Tile};
use goblin_nightclub_core::level::Level;
use goblin_nightclub_core::level_file::*;
use goblin_nightclub_core::prop::Prop;
use goblin_nightclub_core::tileset::testing::tileset;

fn file(cols: usize, rows: usize, layers: &[Vec<Tile>], spawn: Spawn) -> LevelFile {
    LevelFile::new(cols, rows, layers, spawn, tileset(), "player", "npc")
}

#[test]
fn tiles_round_trip_as_text() {
    let t = tileset();
    let brick = Part::Wall(t.wall("brick").unwrap());
    let planks = Part::Floor(t.floor("planks").unwrap());
    let water = Part::Liquid(t.liquid("water").unwrap());
    for tile in [
        Tile::VOID,
        Tile::Whole(brick),
        Tile::Whole(planks),
        Tile::split(Corner::NW, brick, planks),
        Tile::split(Corner::SE, planks, Part::Void),
        Tile::Whole(water),
        Tile::split(Corner::NE, planks, water),
        Tile::Stairs(Dir::Down, t.floor("grate").unwrap()),
    ] {
        let text = tile.to_text(&t);
        assert_eq!(Tile::from_text(&text, &t), Ok(tile), "{text}");
    }
    assert_eq!(
        Tile::split(Corner::NW, brick, planks).to_text(&t),
        "NW/wall:brick/floor:planks"
    );
    assert_eq!(Tile::Whole(water).to_text(&t), "liquid:water");
    assert_eq!(
        Tile::Stairs(Dir::Up, t.floor("plain").unwrap()).to_text(&t),
        "up/plain"
    );
    assert!(Tile::from_text("wall:nonsense", &t).is_err());
    assert!(Tile::from_text("water", &t).is_err(), "format 1 spelling");
    assert!(Tile::from_text("XX/void/void", &t).is_err());
}

#[test]
fn props_round_trip_and_are_checked() {
    let t = tileset();
    let (tree, rock) = (t.prop("tree").unwrap(), t.prop("rock").unwrap());
    let layers = vec![vec![Tile::FLOOR; 5 * 4]; 2];
    let file = LevelFile::new(5, 4, &layers, Spawn::at_tile(1, [0, 0]), t, "player", "npc")
        .with_props(vec![
            vec![Prop {
                at: [1, 1],
                kind: tree,
            }],
            vec![
                Prop {
                    at: [4, 3],
                    kind: rock,
                },
                Prop {
                    at: [2, 0],
                    kind: tree,
                },
            ],
        ]);
    let json = file.to_json();
    assert!(json.contains(r#"{"at":[4,3],"kind":"rock"}"#), "{json}");
    assert_eq!(LevelFile::from_json(&json).unwrap(), file);
    // And on a layer, and in its chunks.
    let level = file.level();
    assert_eq!(level.layer(1).prop(4, 3), Some(rock));
    assert_eq!(level.layer(0).prop(1, 1), Some(tree));
    assert_eq!(level.layer(0).prop(4, 3), None);
    assert_eq!(level.layer(1).props().count(), 2);
    // A level without props says nothing of them.
    let bare = LevelFile::new(
        5,
        4,
        &layers,
        Spawn::at_tile(1, [0, 0]),
        tileset(),
        "player",
        "npc",
    );
    assert!(!bare.to_json().contains("\"props\": ["));

    let refused = |from: &str, to: &str| LevelFile::from_json(&json.replace(from, to)).unwrap_err();
    assert!(refused(r#""kind":"rock""#, r#""kind":"shrub""#).contains("no prop"));
    assert!(refused("[4,3]", "[5,3]").contains("outside"));
    // Where the player starts, a prop in the way.
    assert!(refused("[2,0]", "[0,0]").contains("doesn't fit"));
}

#[test]
fn levels_round_trip_as_json() {
    // Every kind of tile: wall, floor, void, liquid, splits over floor
    // and void, and stairs.
    let level = Level::new(
        vec![
            Layer::from_ascii(&["#..◸", "..>.", "###≈"]),
            Layer::from_ascii(&["~~◤▟", "~.<.", "...."]),
        ],
        tileset(),
    );
    let (cols, rows) = (4, 3);
    let layers: Vec<Vec<Tile>> = (0..level.depth())
        .map(|k| {
            (0..rows)
                .flat_map(|y| (0..cols).map(move |x| (x, y)))
                .map(|(x, y)| level.layer(k).tile(x, y))
                .collect()
        })
        .collect();
    // A spawn between tiles, to the pixel.
    let spawn = Spawn {
        layer: level.depth() - 1,
        pos: [27.5, 24.25],
    };
    let file = file(cols as usize, rows as usize, &layers, spawn).with_ground(1);
    let json = file.to_json();
    let back = LevelFile::from_json(&json).unwrap();
    assert_eq!(back, file);
    assert_eq!(back.spawn.pos, [27.5, 24.25]);
    assert_eq!(back.ground, 1);
    assert_eq!(back.tiles(), layers);
    assert_eq!(back.player.sprite, "player");
    let loaded = back.level();
    assert_eq!(loaded.depth(), level.depth());
    assert_eq!(loaded.layer(1).tile(3, 0), level.layer(1).tile(3, 0));
    assert_eq!(loaded.tileset(), &tileset());
    // Readable: a row of tiles and a tileset entry per line.
    assert!(json.contains(
        "\n      [\"wall:rock\",\"floor:plain\",\"floor:plain\",\"NW/wall:rock/floor:plain\"],\n"
    ));
    assert!(json.contains(
        "\n      \"brick\": {\"cells\":[[0,2],[1,2]],\"light\":\"#ff5555\",\"dark\":\"#000000\"}"
    ));
    assert!(json.contains("\n    \"cell\": 16,\n"));
}

#[test]
fn bad_levels_are_refused() {
    let file = file(
        2,
        1,
        &[vec![Tile::FLOOR, Tile::FLOOR]],
        Spawn::at_tile(0, [0, 0]),
    );
    let mut wrong = file.clone();
    wrong.spawn.pos = [55.0, 5.0];
    assert!(LevelFile::from_json(&wrong.to_json()).is_err(), "outside");
    let mut tight = file.clone();
    // The player's box over the level's left edge, by a pixel.
    tight.spawn.pos = [PLAYER_SIZE[0] / 2.0 - 1.0, 5.0];
    assert!(
        LevelFile::from_json(&tight.to_json())
            .unwrap_err()
            .contains("fit"),
        "against the level's edge"
    );
    let mut walled = file.clone();
    walled.layers[0][0][0] = Tile::WALL;
    assert!(
        LevelFile::from_json(&walled.to_json()).is_err(),
        "in a wall"
    );
    walled.spawn.pos = [15.0, 5.0];
    assert!(LevelFile::from_json(&walled.to_json()).is_ok(), "beside it");
    let mut ragged = file.clone();
    ragged.layers[0][0].pop();
    assert!(LevelFile::from_json(&ragged.to_json()).is_err());
    let mut naked = file.clone();
    naked.player.sprite = "ghost".into();
    assert!(LevelFile::from_json(&naked.to_json()).is_err());
    assert!(LevelFile::from_json("{}").is_err());
    let good = file.to_json();
    assert!(LevelFile::from_json(&good).is_ok());
    // Names the tileset doesn't have, and the old format.
    let unknown = good.replacen("floor:plain", "floor:lava", 1);
    assert!(LevelFile::from_json(&unknown).unwrap_err().contains("lava"));
    let old = good.replacen("\"format\": 3", "\"format\": 2", 1);
    assert!(LevelFile::from_json(&old).unwrap_err().contains("format 2"));
}
