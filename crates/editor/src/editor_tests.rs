use super::*;

const VIEW: [f32; 2] = [320.0, 200.0];

fn tileset() -> Tileset {
    goblin_nightclub_core::tileset::testing::tileset()
}

fn editor() -> Editor {
    let blank = LevelFile::blank(8, 6, 2, tileset(), "player", "npc");
    let mut e = Editor::from_file(&blank, VIEW);
    // The whole level in view, a tile 16 pixels across at the origin.
    e.camera.pos = [0.0, 0.0];
    e
}

/// View pixel at the center of tile `t`.
fn at(e: &Editor, t: [i32; 2]) -> [f32; 2] {
    let z = e.camera.zoom();
    [
        ((t[0] as f32 + 0.5) * TILE - e.camera.pos[0]) * z,
        ((t[1] as f32 + 0.5) * TILE - e.camera.pos[1]) * z,
    ]
}

fn tile(e: &Editor, t: [i32; 2]) -> Tile {
    e.layers[e.layer][e.index(t)]
}

fn brush(b: Brush) -> usize {
    palette(&tileset()).iter().position(|p| *p == b).unwrap()
}

fn click(e: &mut Editor, t: [i32; 2], button: u32) {
    let p = at(e, t);
    e.mouse_down(p, button);
    e.mouse_up(p, button);
}

#[test]
fn pencil_paints_clicks_and_drags_without_gaps() {
    let mut e = editor();
    click(&mut e, [1, 1], 0);
    assert_eq!(tile(&e, [1, 1]), Tile::WALL, "primary: wall");
    click(&mut e, [1, 1], 2);
    assert_eq!(tile(&e, [1, 1]), Tile::FLOOR, "secondary: floor");

    // A fast diagonal drag: every tile along it.
    e.mouse_down(at(&e, [0, 0]), 0);
    e.pointer(at(&e, [5, 5]));
    e.mouse_up(at(&e, [5, 5]), 0);
    for i in 0..=5 {
        assert_eq!(tile(&e, [i, i]), Tile::WALL, "{i}");
    }
    assert_eq!(tile(&e, [5, 0]), Tile::FLOOR);
}

#[test]
fn rectangles_fill_the_region_dragged() {
    let mut e = editor();
    e.set_tool(Tool::Rect);
    e.mouse_down(at(&e, [5, 4]), 0);
    e.pointer(at(&e, [2, 1]));
    assert_eq!(tile(&e, [3, 2]), Tile::FLOOR, "nothing until released");
    e.mouse_up(at(&e, [2, 1]), 0);
    for y in 0..6 {
        for x in 0..8 {
            let inside = (2..=5).contains(&x) && (1..=4).contains(&y);
            assert_eq!(tile(&e, [x, y]) == Tile::WALL, inside, "{x} {y}");
        }
    }
}

#[test]
fn fill_floods_like_tiles() {
    let mut e = editor();
    // A wall across column 3 splits the floor in two.
    e.set_tool(Tool::Rect);
    e.mouse_down(at(&e, [3, 0]), 0);
    e.mouse_up(at(&e, [3, 5]), 0);
    e.set_tool(Tool::Fill);
    e.set_brush(brush(Brush::Part(Part::Void)), false);
    click(&mut e, [0, 0], 0);
    assert_eq!(tile(&e, [2, 5]), Tile::VOID);
    assert_eq!(tile(&e, [3, 2]), Tile::WALL);
    assert_eq!(tile(&e, [4, 0]), Tile::FLOOR, "the other side");
}

#[test]
fn shapes_split_tiles_and_stairs_sit_on_the_secondary_floor() {
    let mut e = editor();
    e.set_shape(BrushShape::Split(Corner::NW));
    e.set_brush(brush(Brush::Part(Part::Void)), true);
    click(&mut e, [2, 2], 0);
    assert_eq!(
        tile(&e, [2, 2]),
        Tile::split(Corner::NW, Part::Wall(0), Part::Void)
    );
    // The right button swaps them.
    click(&mut e, [3, 2], 2);
    assert_eq!(
        tile(&e, [3, 2]),
        Tile::split(Corner::NW, Part::Void, Part::Wall(0))
    );
    let grate = tileset().floor("grate").unwrap();
    e.set_brush(brush(Brush::Stairs(Dir::Down)), false);
    e.set_brush(brush(Brush::Part(Part::Floor(grate))), true);
    click(&mut e, [4, 2], 0);
    assert_eq!(tile(&e, [4, 2]), Tile::Stairs(Dir::Down, grate));

    // The picker takes it all back.
    e.set_tool(Tool::Pick);
    click(&mut e, [2, 2], 0);
    let s = e.state();
    assert_eq!(
        (s.shape, s.primary),
        ("NW", brush(Brush::Part(Part::Wall(0))))
    );
    assert_eq!(s.secondary, brush(Brush::Part(Part::Void)));
}

#[test]
fn undo_and_redo_whole_strokes() {
    let mut e = editor();
    assert!(!e.state().can_undo);
    e.mouse_down(at(&e, [0, 0]), 0);
    e.pointer(at(&e, [3, 0]));
    e.mouse_up(at(&e, [3, 0]), 0);
    click(&mut e, [0, 0], 0);
    assert_eq!(e.undo.len(), 1, "repainting the same tile changes nothing");
    e.undo();
    assert!(
        (0..=3).all(|x| tile(&e, [x, 0]) == Tile::FLOOR),
        "the whole drag"
    );
    e.redo();
    assert!((0..=3).all(|x| tile(&e, [x, 0]) == Tile::WALL));
    // A new change drops what could be redone.
    e.undo();
    click(&mut e, [5, 5], 0);
    assert!(!e.state().can_redo);
}

#[test]
fn layers_spawn_and_files() {
    let mut e = editor();
    assert_eq!(e.state().layer, 1, "starts on the top layer");
    e.set_layer(0);
    click(&mut e, [1, 1], 0);
    e.set_tool(Tool::Spawn);
    click(&mut e, [6, 4], 0);
    assert_eq!(e.layers[1][e.index([1, 1])], Tile::FLOOR);
    assert_eq!(e.layers[0][e.index([1, 1])], Tile::WALL);
    // Into the wall just painted: ignored. Off a tile's center: kept to
    // the art's pixel (16 to a tile).
    click(&mut e, [1, 1], 0);
    assert_eq!(e.spawn, Spawn::at_tile(0, [6, 4]));
    let z = e.camera.zoom();
    let p = [(23.3 - e.camera.pos[0]) * z, (41.1 - e.camera.pos[1]) * z];
    e.mouse_down(p, 0);
    e.mouse_up(p, 0);
    assert_eq!(e.spawn.pos, [23.125, 41.25]);
    e.undo();
    assert_eq!(e.spawn, Spawn::at_tile(0, [6, 4]));

    let file = LevelFile::from_json(&e.to_file().to_json()).unwrap();
    assert_eq!(file.spawn, Spawn::at_tile(0, [6, 4]));
    let loaded = Editor::from_file(&file, VIEW);
    assert_eq!(loaded.layers, e.layers);
    assert_eq!(loaded.state().layer, 0, "opens on the spawn's layer");
    assert_eq!(file.tileset, tileset(), "the tileset travels with it");
    assert_eq!(file.player.sprite, "player");
    // The palette offers everything in the tileset.
    let names: Vec<String> = e.swatches().into_iter().map(|s| s.name).collect();
    assert!(names.contains(&"grate floor".to_string()));
    assert!(names.contains(&"water".to_string()));
    assert!(names.contains(&"torch".to_string()));
    // Floors, walls, void, water, stairs, props and none.
    assert_eq!(e.swatches().len(), 4 + 3 + 1 + 1 + 2 + 3 + 1);
}

#[test]
fn props_are_painted_picked_undone_and_saved() {
    let mut e = editor();
    let tree = tileset().prop("tree").unwrap();
    e.set_brush(brush(Brush::Prop(tree)), false);
    e.set_tool(Tool::Rect);
    let (a, b) = (at(&e, [1, 1]), at(&e, [3, 2]));
    e.mouse_down(a, 0);
    e.pointer(b);
    e.mouse_up(b, 0);
    let on = |e: &Editor, t: [i32; 2]| e.props[e.state().layer][e.index(t)];
    assert_eq!(on(&e, [2, 2]), Some(tree));
    assert_eq!(on(&e, [4, 2]), None);
    // The tiles under them stay as they were.
    assert_eq!(e.layers[1][e.index([2, 2])], Tile::FLOOR);
    // Picked from where one stands; erased with none.
    e.set_brush(brush(Brush::NoProp), false);
    e.set_tool(Tool::Pick);
    click(&mut e, [1, 1], 0);
    assert_eq!(e.state().primary, brush(Brush::Prop(tree)));
    e.set_brush(brush(Brush::NoProp), false);
    e.set_tool(Tool::Pencil);
    click(&mut e, [1, 1], 0);
    assert_eq!(on(&e, [1, 1]), None);
    e.undo();
    assert_eq!(on(&e, [1, 1]), Some(tree));
    // Saved and loaded: there, and drawn.
    let file = LevelFile::from_json(&e.to_file().to_json()).unwrap();
    assert_eq!(file.props[1].len(), 6);
    let loaded = Editor::from_file(&file, VIEW);
    assert_eq!(loaded.props, e.props);
    e.update(0.0);
    let pieces = &e.level.meshes(1).pieces;
    assert!(pieces.iter().any(|p| !p.props.vertices.is_empty()));
    // The spawn can't be put where a prop's in the way.
    e.set_tool(Tool::Spawn);
    click(&mut e, [2, 2], 0);
    assert_ne!(e.spawn, Spawn::at_tile(1, [2, 2]));
}

#[test]
fn layers_can_be_added_and_removed() {
    let mut e = editor();
    assert_eq!(e.state().depth, 2);
    e.set_layer(0);
    click(&mut e, [1, 1], 0);
    // A new layer above the bottom one, between the two: the spawn (on
    // the old top) moves up with it, and the wall stays below.
    e.add_layer(true);
    let s = e.state();
    assert_eq!((s.depth, s.layer), (3, 1));
    assert_eq!(e.spawn.layer, 2);
    assert_eq!(e.layers[0][e.index([1, 1])], Tile::WALL);
    assert!(e.layers[1].iter().all(|t| t.is_floor()), "plain floor");
    // Below the current one: the spawn moves again.
    e.add_layer(false);
    assert_eq!((e.state().depth, e.state().layer), (4, 1));
    assert_eq!(e.spawn.layer, 3);
    // Undo both.
    e.undo();
    e.undo();
    assert_eq!((e.state().depth, e.spawn.layer), (2, 1));
    // Remove the spawn's layer: the spawn drops to the one below.
    e.set_layer(1);
    e.remove_layer();
    assert_eq!((e.state().depth, e.state().layer, e.spawn.layer), (1, 0, 0));
    assert!(!e.state().can_remove_layer);
    e.remove_layer();
    assert_eq!(e.state().depth, 1, "never the last");
    e.redo();
    e.undo();
    assert_eq!(e.state().depth, 2);
    // Up to the most a level may have.
    for _ in 0..MAX_LAYERS {
        e.add_layer(true);
    }
    assert_eq!(e.state().depth, MAX_LAYERS);
    assert!(!e.state().can_add_layer);
    let file = LevelFile::from_json(&e.to_file().to_json()).unwrap();
    assert_eq!(file.layers.len(), MAX_LAYERS);
}

#[test]
fn frames_show_everything_on_the_layer_and_below() {
    let mut e = editor();
    e.set_brush(brush(Brush::Part(Part::Void)), false);
    click(&mut e, [2, 2], 0);
    e.update(0.0);
    let frame = e.frame();
    assert_eq!(
        frame.layers.iter().map(|l| l.index).collect::<Vec<_>>(),
        [1, 0]
    );
    let voids = &frame.layers[0].meshes.pieces[0].voids;
    assert_eq!(voids.vertices.len(), 6, "one void tile");
    assert_eq!(frame.layers[0].polygon.len(), 4, "the whole level in view");
    assert!(!frame.overlay.vertices.is_empty(), "the grid");
}
