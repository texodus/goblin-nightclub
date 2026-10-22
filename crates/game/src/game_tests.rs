//                                    ▗◡≝▔▔` ▔≝=◺▂             ∕        ▐▋  ▝▉
//                                  ╣▒◿▒≘≉◬_    _▒▆▒▀         ◞         ▐▋  ▐▌
//                                  ╔`     ▝◺◿▒▟▘╔╹ ▚◛                  ▐▌  ▐▏
//       G O B L I N               ▕▎_"▃▅≣P ▐▓▛▗▉▂_▂__◚◛_         __▂▂▂▂▟▙◓▀▀▀▀≝
//        N I G H T C L U B         ║`║║_▂▐╝ ▟◙██▛╬▔`_▂◙▒▖       ▕∏▃▃▃▃▂▐▌║▂▂▂▔▔
//                                  ▙⋡▜╥∺╝ ▗▟███▛Ƒ  ▐▛▔▔▜█▌      ▐╣ ▝▜███▋ ∭▔▔▔▔
//                                   ▁⋡▚▃▄▀▉Ƣ█▀▉╺▗▐∏▊   ╠█▎      ▐╣   ▁∭█▉ ▊ ∭▐█
//                          ▁▃◛◭◒≠◚▔▔╸ ╔▒∏▕▓▌ ▟▎ ▐▒██▖    ▜▎     ╠▎≕∏__  ◙∏▍ ╣█▌
//                     _◿▎`▂_ ▟▒║╣▂▂▂▂▞╜▔▜ ╬◙▏∏▎ ▐███◘⊠◚"` ▜     ╣ ▂▅▅▅_ ▐█▎▕▒█Ǩ
//              _▂_ _▂▂▒∭▒▒◛≝▔           ▕▶╣◚▔▔▔╠∏∏║▌ ┉━━≝▒▃▘   ▕▌▟█████▙ ∭ ▐▐▌▓
//    ▁▁▁▁_╓◛▋▝◠▬Ǟ◡▃╠◚≝≊■■■▆▆▤▀◚⊜⊛╥▄▃▃▂_║╳▞▗╝▟▛◚███████████▌    ▕▒████◚◚█▌▋ ∭▟▎╱
//   ▐▔▔"⋨∭◅▃╏◠≉╬▒▇▆▅┫ ▕▄▃∏◠━━◡▃▂║▃▃▃▄▙▄∭▟▟▙▓≝▬▬╩◘██████████    ▐∏██◘▜▁╔▕▒▎ ▒█▎╣
//   ▕╣◚◘▉▐▄▃▂▁▟▒╝◙▉◙Ů▒▒◚▜▛◙◘▒▒▌▍ ▔▔"▀▬ǡ∗▃∑◚▀▀◘◘▓████▀╣▟╢▒◙◙▙▖  ▐ ╬     ∭▐▏▐█▉▏▜
//    ĴĄ  ╲╲ ▔╠▛◘◘▓▍▅╣┇▐▒∭Ǭ▛╨◀∗▃▒▀Ň◘█▒⋯┳▂▃_▐█████████████████▌∑◚╣__%◺_⊋∛ ∭ ▐▐▉▌▝
//  ▔  ┗▌   ▔◚▜▍▝▀▀▜█▌▐▐▐██▙▄▃▂_ ▗╢▒▛◚▬∗▃▂◚▀╩╩╩◘◘◙╩◘Ů▔▔▔▔▔▔▔▔▔▝▜▒▒▒▒▒◘◘▣▤▌ ∭█▌▕▎
//  Copyright 2026, [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0).

use super::route::PLAYER_SPEED;
use super::sight::{VIEW_RADIUS_OUTDOORS, view_radius};
use super::*;
use goblin_nightclub_core::layer::Layer;
use goblin_nightclub_core::layer::Tile;
use goblin_nightclub_core::level_file::Spawn;
use goblin_nightclub_core::prop::Prop;
use std::sync::Arc;

const VIEW: [f32; 2] = [320.0, 200.0];
/// The test tileset's default zoom: 16-pixel cells over 10-unit tiles.
const ZOOM: f32 = 1.6;

fn tileset() -> Tileset {
    goblin_nightclub_core::tileset::testing::tileset()
}

fn sprites() -> Sprites {
    let t = tileset();
    Sprites {
        player: t.sprite("player").unwrap().cell,
        npc: t.sprite("npc").unwrap().cell,
    }
}

fn level(layers: &[&[&str]]) -> Level {
    Level::new(
        layers.iter().map(|rows| Layer::from_ascii(rows)).collect(),
        tileset(),
    )
}

/// A game on `level` with no NPCs, the player's box centered in tile
/// `at`.
fn game(level: Level, at: [i32; 2]) -> Game {
    let top = level.depth() - 1;
    game_on(level, top, at)
}

/// The same, on layer `k`.
fn game_on(level: Level, k: usize, at: [i32; 2]) -> Game {
    let c = path::center(at);
    let half = PLAYER_SIZE.map(|v| v / 2.0);
    let mut game = Game::with_level_at(
        level,
        k,
        [c[0] - half[0], c[1] - half[1]],
        sprites(),
        VIEW,
        1,
    );
    game.npcs.clear();
    game
}

/// Click world point `p`, through the camera.
fn click_world(game: &mut Game, p: [f32; 2]) {
    game.click([
        (p[0] - game.camera.pos[0]) * game.zoom(),
        (p[1] - game.camera.pos[1]) * game.zoom(),
    ]);
}

/// Click tile `t` and play until paused, up to `max` frames.
fn click_and_run(game: &mut Game, t: [i32; 2], max: usize) {
    click_world(game, path::center(t));
    for _ in 0..max {
        if game.paused {
            break;
        }
        game.update(1.0 / 60.0);
    }
}

fn player_tile(game: &Game) -> [i32; 2] {
    tile_of(center(game.player, PLAYER_SIZE))
}

/// A level too big for one window, `depth` layers of floor: here and
/// there walls (away from row `ROW`, kept clear to walk along), holes
/// in the layers above the spawn's (three from the top, so mostly
/// indoors, but not everywhere), and stairs down two tiles east of the
/// spawn on every layer but the bottom.
fn big_file(cols: usize, rows: usize, depth: usize) -> LevelFile {
    const ROW: usize = 60;
    let spawn = [20, ROW as i32];
    let top = depth - 3;
    let layers: Vec<Vec<Tile>> = (0..depth)
        .map(|k| {
            (0..cols * rows)
                .map(|i| {
                    let (x, y) = (i % cols, i / cols);
                    let clear = y.abs_diff(ROW) <= 2;
                    if k > 0 && [x as i32, y as i32] == [spawn[0] + 2, spawn[1]] {
                        Tile::Stairs(Dir::Down, 0)
                    } else if !clear && x % 37 == 0 && y % 5 == 0 {
                        Tile::WALL
                    } else if k > top && (x / 5 + y / 3) % 4 == 0 {
                        Tile::VOID
                    } else {
                        Tile::FLOOR
                    }
                })
                .collect()
        })
        .collect();
    LevelFile::new(
        cols,
        rows,
        &layers,
        Spawn::at_tile(top, spawn),
        tileset(),
        "player",
        "npc",
    )
}

/// Where the player's center is in the whole level, and their layer.
fn whereabouts(game: &Game) -> ([f32; 2], usize) {
    let s = game.stream.as_ref().unwrap();
    let o = s.window.tile_origin();
    let c = center(game.player, PLAYER_SIZE);
    (
        [c[0] + o[0] as f32 * TILE, c[1] + o[1] as f32 * TILE],
        game.layer(),
    )
}

/// Walk to `p` (a point in the whole level) on the player's layer, and
/// on until paused, checking the player moves smoothly, windows and
/// all, and `each` frame whatever else: how many times the window
/// moved.
fn walk_far(game: &mut Game, p: [f32; 2], each: &mut dyn FnMut(&Game)) -> usize {
    let o = game.stream.as_ref().unwrap().window.tile_origin();
    game.walk_to([p[0] - o[0] as f32 * TILE, p[1] - o[1] as f32 * TILE]);
    let (mut was, mut moves) = (whereabouts(game).0, 0);
    let mut window = game.stream.as_ref().unwrap().window;
    for _ in 0..4000 {
        if game.paused {
            break;
        }
        game.update(1.0 / 60.0);
        let (now, _) = whereabouts(game);
        let step = distance(was, now);
        assert!(
            step <= PLAYER_SPEED / 60.0 + 0.6,
            "jumped {step} to {now:?}"
        );
        was = now;
        let w = game.stream.as_ref().unwrap().window;
        if w != window {
            moves += 1;
            window = w;
        }
        each(game);
    }
    assert!(game.paused, "arrived");
    moves
}

#[test]
fn packed_levels_play_as_their_files_do() {
    use goblin_nightclub_core::packed::{Packed, pack_file};
    for file in [big_file(300, 120, 12), new_world(3, &Params::default())] {
        let packed = Packed::open(Arc::<[u8]>::from(pack_file(&file)))
            .unwrap()
            .unwrap();
        let meta = packed.header().level.clone();
        let from_pack = Game::from_source(Rc::new(packed), &meta, VIEW, 1).unwrap();
        let from_file = Game::from_file(&file, VIEW, 1);
        assert_eq!(from_pack.stream.is_some(), from_file.stream.is_some());
        assert_eq!(
            (from_pack.player, from_pack.top),
            (from_file.player, from_file.top)
        );
        assert_eq!(from_pack.npcs.len(), from_file.npcs.len());
        let at = |g: &Game| g.npcs.iter().map(|n| n.pos).collect::<Vec<_>>();
        assert_eq!(at(&from_pack), at(&from_file), "the same NPCs");
        assert_eq!(from_pack.outdoors(), from_file.outdoors());
    }
}

#[test]
fn openings_wait_for_a_slow_source() {
    use goblin_nightclub_core::chunk::{Chunk, ChunkKey};
    use goblin_nightclub_core::world::Roof;
    /// Each chunk and roof ready from the second time it's asked for.
    struct Slow(
        FileSource,
        std::sync::Mutex<std::collections::HashSet<(usize, i32, i32)>>,
    );
    impl Slow {
        fn asked(&self, key: (usize, i32, i32)) -> bool {
            !self.1.lock().unwrap().insert(key)
        }
    }
    impl ChunkSource for Slow {
        fn size(&self) -> [usize; 3] {
            self.0.size()
        }
        fn tileset(&self) -> &Tileset {
            self.0.tileset()
        }
        fn chunk(&self, key: ChunkKey) -> Option<Arc<Chunk>> {
            self.asked(key).then(|| self.0.chunk(key)).flatten()
        }
        fn roof(&self, cx: i32, cy: i32) -> Option<Roof> {
            self.asked((usize::MAX, cx, cy))
                .then(|| self.0.roof(cx, cy))
                .flatten()
        }
    }
    let file = big_file(300, 120, 12);
    let slow = Rc::new(Slow(FileSource::new(&file), Default::default()));
    let mut opening = Opening::new(slow, &file.meta(), VIEW, 1);
    let mut steps = 1;
    while !opening.step() {
        steps += 1;
    }
    assert!(steps > 12, "waited, a layer at a time: {steps}");
    let game = opening.finish().unwrap();
    assert_eq!(game.player, Game::from_file(&file, VIEW, 1).player);
}

#[test]
fn generated_worlds_start_from_their_regions() {
    use goblin_nightclub_generate::{Generated, Plan, generated_meta};
    // Small enough to play whole, but made a region at a time.
    let params = Params {
        underworld_layers: 3,
        ..Params::default()
    };
    let plan = Plan::new(4, 192, 128, &params).with_region(64);
    let meta = generated_meta(&plan);
    let game = Game::from_source(Rc::new(Generated::new(plan)), &meta, VIEW, 4).unwrap();
    assert!(game.stream.is_none());
    assert_eq!(game.layer(), meta.spawn.layer);
    assert!(game.outdoors(), "the spawn's on the plains");
    assert!(!game.frame().layers.is_empty());
}

#[test]
fn small_levels_are_played_whole() {
    let params = Params {
        underworld_layers: 3,
        ..Params::default()
    };
    let file = new_world(3, &params);
    assert!(Game::from_file(&file, VIEW, 3).stream.is_none());
}

#[test]
fn big_levels_are_played_a_window_at_a_time() {
    let file = big_file(300, 120, 12);
    let whole = file.level();
    let mut game = Game::from_file(&file, VIEW, 1);
    let (start, first) = whereabouts(&game);
    let w = game.stream.as_ref().expect("too big to play whole").window;
    assert!(w.cols < 300 && w.depth < 12);
    // The ground level is the bottom: outdoors only under holes all the
    // way up, as the whole level has it.
    let outdoors = |game: &Game| {
        let (p, top) = whereabouts(game);
        let [x, y] = tile_of(p);
        let open = |k: usize| {
            let layer = whole.layer(k);
            layer.is_void_at(p) || layer.tile(x, y).stairs().is_some()
        };
        (top + 1..12).all(open)
    };
    assert_eq!(game.outdoors(), outdoors(&game));
    // East along the clear row, out of sight of the start.
    let mut moves = 0;
    let mut was_outdoors = std::collections::HashSet::new();
    let mut check = |game: &Game| {
        assert_eq!(
            game.outdoors(),
            outdoors(game),
            "at {:?}",
            whereabouts(game)
        );
        was_outdoors.insert(game.outdoors());
    };
    for leg in 1..=4 {
        moves += walk_far(
            &mut game,
            [start[0] + leg as f32 * 450.0, start[1]],
            &mut check,
        );
        let ids: std::collections::HashSet<_> = game.npcs.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), game.npcs.len(), "NPCs are each awake once");
        assert!(game.npcs.iter().all(|n| n.layer < game.level.depth()));
    }
    assert!(moves >= 2, "the window moved on: {moves}");
    assert_eq!(was_outdoors.len(), 2, "in and out");
    assert!(whereabouts(&game).0[0] > start[0] + 1700.0);
    // Far from the start, the map has what was explored around it, as it
    // is.
    let map = &game.stream.as_ref().unwrap().map;
    for dx in -3..=3 {
        let t = [tile_of(start)[0] + dx, tile_of(start)[1]];
        assert!(map.is_explored(first, t), "{dx} from the start");
        let [cx, cy] = t.map(|v| v.div_euclid(CHUNK as i32));
        let i = (t[1] - cy * CHUNK as i32) * CHUNK as i32 + t[0] - cx * CHUNK as i32;
        let tiles = &map.chunk((first, cx, cy)).unwrap().tiles;
        assert_eq!(
            tiles[i as usize],
            map.code(whole.layer(first).tile(t[0], t[1]))
        );
    }
    // Back to the start: what was explored there still is.
    let mut back = 0;
    for leg in (0..4).rev() {
        back += walk_far(
            &mut game,
            [start[0] + leg as f32 * 450.0, start[1]],
            &mut |_| {},
        );
    }
    assert!(back >= 2);
    let s = game.stream.as_ref().unwrap();
    let o = s.window.tile_origin();
    let k = game.top;
    let explored = |dx: i32| {
        let t = [tile_of(start)[0] + dx - o[0], tile_of(start)[1] - o[1]];
        game.visions[k].is_explored(t[0] as usize, t[1] as usize)
    };
    assert!((-3..=3).all(explored), "around the start");
}

#[test]
fn going_down_moves_the_window_down() {
    let file = big_file(300, 120, 14);
    let mut game = Game::from_file(&file, VIEW, 1);
    let (start, top) = whereabouts(&game);
    assert_eq!(top, 11);
    let base = game.stream.as_ref().unwrap().window.base;
    // Onto the stairs, off them and back on, layer after layer.
    for down in 1..=6 {
        walk_far(&mut game, [start[0] + 2.0 * TILE, start[1]], &mut |_| {});
        assert_eq!(whereabouts(&game).1, 11 - down);
        walk_far(&mut game, [start[0] + TILE, start[1]], &mut |_| {});
    }
    let s = game.stream.as_ref().unwrap();
    assert!(s.window.base < base, "{} from {base}", s.window.base);
    assert!(game.top < game.level.depth());
    assert_eq!(game.layer(), s.window.base + game.top);
}

#[test]
fn clicking_walks_there_and_pauses() {
    let l = level(&[&["..........", "....#.....", "....#.....", ".........."]]);
    let mut game = game(l, [1, 2]);
    assert!(game.paused, "starts paused");
    click_and_run(&mut game, [8, 2], 600);
    assert!(game.paused);
    assert_eq!(player_tile(&game), [8, 2]);
    assert!(game.route.is_none());
}

#[test]
fn pause_button_and_freezing() {
    let l = level(&[&["..........", ".........."]]);
    let mut game = game(l, [1, 1]);
    let b = game.button_pos();
    game.click([b[0] + 2.0, b[1] + 2.0]);
    assert!(!game.paused, "button unpauses");
    game.click([b[0] + 2.0, b[1] + 2.0]);
    assert!(game.paused, "and pauses again");
    assert!(game.route.is_none(), "the button never starts a walk");
    let (time, pos) = (game.time, game.player);
    for _ in 0..30 {
        game.update(1.0 / 60.0);
    }
    assert_eq!(
        (game.time, game.player),
        (time, pos),
        "paused: nothing moves"
    );
}

#[test]
fn stops_when_the_route_runs_into_something_revealed() {
    // A long corridor, blocked far ahead, beyond the view radius.
    let wall = "#".repeat(64);
    let mut open = ".".repeat(64);
    open.replace_range(55..56, "#");
    let l = level(&[&[wall.as_str(), open.as_str(), wall.as_str()]]);
    let mut game = game(l, [0, 1]);
    assert!(!game.visions[0].is_explored(55, 1), "the block is unseen");
    click_and_run(&mut game, [62, 1], 4000);
    assert!(game.paused, "stopped");
    assert!(game.route.is_none());
    assert!(player_tile(&game)[0] < 55, "before reaching the block");
}

#[test]
fn keeps_walking_when_an_npc_comes_into_view() {
    let l = level(&[&[
        "################################",
        "................................",
        "################################",
    ]]);
    let mut game = game(l, [0, 1]);
    game.npcs.push(Entity::npc(7, [292.0, 12.0]));
    click_and_run(&mut game, [31, 1], 2000);
    assert!(game.memories[0].is_visible(7), "seen on the way");
    assert_eq!(player_tile(&game), [31, 1], "walked all the way");
}

#[test]
fn routes_avoid_stairs_but_can_end_on_them() {
    let l = level(&[
        &["..........", "....<.....", ".........."],
        &["..........", "....>.....", ".........."],
    ]);
    let mut game = game(l, [1, 1]);
    click_and_run(&mut game, [8, 1], 600);
    assert_eq!(game.layer(), 1, "walked around the stairs");
    assert_eq!(player_tile(&game), [8, 1]);

    // Onto the stairs: down a layer, and paused there.
    click_and_run(&mut game, [4, 1], 600);
    assert_eq!(game.layer(), 0);
    assert!(game.paused);
    // Off them and back on: up again (no bouncing on arrival below).
    click_and_run(&mut game, [7, 1], 600);
    assert_eq!(game.layer(), 0);
    click_and_run(&mut game, [4, 1], 600);
    assert_eq!(game.layer(), 1);
}

#[test]
fn props_hide_whats_behind_them_and_are_walked_around() {
    let rows = ["..........", "..........", ".........."];
    let rock = tileset().prop("rock").unwrap();
    let with_rock = |rock: Option<u8>| {
        let props: Vec<Prop> = rock
            .map(|kind| Prop { at: [4, 1], kind })
            .into_iter()
            .collect();
        Level::new(vec![Layer::from_ascii(&rows).with_props(&props)], tileset())
    };
    // An NPC straight past the rock from the player: hidden by it.
    for (rock, seen) in [(Some(rock), false), (None, true)] {
        let mut game = game(with_rock(rock), [1, 1]);
        game.npcs.push(Entity::npc(7, [72.0, 12.0]));
        game.observed = None;
        game.observe();
        assert_eq!(game.memories[0].is_visible(7), seen, "rock {rock:?}");
        // The rock, seen, shown whole: its art, a quad.
        let shown = game.frame().layers[0].props_seen.vertices.len();
        assert_eq!(shown, if rock.is_some() { 6 } else { 0 });
    }
    // Walking through where it stands: around it.
    let mut game = game(with_rock(Some(rock)), [1, 1]);
    click_and_run(&mut game, [7, 1], 600);
    assert_eq!(player_tile(&game), [7, 1]);
    let route_ok = game.level.movement(0).solid(4, 1) == Solid::Full;
    assert!(route_ok, "solid to walking");
}

#[test]
fn torchlight_shows_what_it_lights_beyond_the_view_radius() {
    // Under a roof, so seen only `VIEW_RADIUS` far but for the torch.
    let floor = ".".repeat(30);
    let rows = [floor.as_str(), floor.as_str(), floor.as_str()];
    let torch = tileset().prop("torch").unwrap();
    let lit = |torch: Option<u8>| {
        let props: Vec<Prop> = torch
            .map(|kind| Prop { at: [12, 1], kind })
            .into_iter()
            .collect();
        let lower = Layer::from_ascii(&rows).with_props(&props);
        Level::new(vec![lower, Layer::from_ascii(&rows)], tileset())
    };
    for (torch, seen) in [(Some(torch), true), (None, false)] {
        let mut game = game_on(lit(torch), 0, [0, 1]);
        assert!(!game.outdoors());
        // Past the view radius, one in its light and one past it.
        game.npcs.push(Entity::npc(7, [150.0, 12.0]));
        game.npcs.push(Entity::npc(8, [190.0, 12.0]));
        game.observed = None;
        game.observe();
        assert_eq!(game.memories[0].is_visible(7), seen, "torch {torch:?}");
        assert!(!game.memories[0].is_visible(8), "torch {torch:?}");
        assert_eq!(game.visions[0].is_explored(15, 1), seen);
        assert!(!game.visions[0].is_explored(20, 1));
        // The frame has its light, to draw.
        let lights = game.frame().layers[0].lights.len();
        assert_eq!(lights, usize::from(seen));
    }
}

#[test]
fn changing_layers_fades_and_zooms_between_them() {
    let l = level(&[
        &["..........", "....<.....", ".........."],
        &["..........", "....>.....", ".........."],
    ]);
    let mut game = game(l, [3, 1]);
    // Onto the stairs down.
    game.player = [
        path::center([4, 1])[0] - PLAYER_SIZE[0] / 2.0,
        path::center([4, 1])[1] - PLAYER_SIZE[1] / 2.0,
    ];
    game.take_stairs([3, 1]);
    assert_eq!(game.top, 0);
    let at = |game: &mut Game, t: f32| {
        game.shift.as_mut().unwrap().t = t;
        let f = game.frame();
        let beneath = f.layers.iter().find(|l| l.index + 1 == f.top);
        (f.top, f.fade, f.sink, beneath.map(|l| l.shaft.is_none()))
    };
    let between = |v: f32, a: f32, b: f32| v > a && v < b;
    // Down: the layer left fading out and growing past the viewer,
    // over this one, shown whole, growing into place.
    let (top, fade, sink, beneath) = at(&mut game, 0.5);
    assert_eq!((top, beneath), (1, Some(true)));
    assert!(between(fade, 0.0, 1.0) && between(sink, -1.0, 0.0));
    // Up: this one fading in and shrinking into place, over the layer
    // left, shown whole, shrinking beneath it.
    game.top = 1;
    game.shift = Some(LayerShift { from: 0, t: 0.0 });
    let (top, fade, sink, beneath) = at(&mut game, 0.5);
    assert_eq!((top, beneath), (1, Some(true)));
    assert!(between(fade, 0.0, 1.0) && between(sink, -1.0, 0.0));
    // Each starts as the view from the layer left.
    let (top, fade, sink, _) = at(&mut game, 0.0);
    assert_eq!((top, fade, sink), (1, 0.0, -1.0));
    // And done, paused or not.
    game.paused = true;
    game.update(LAYER_SHIFT_TIME);
    assert!(game.shift.is_none());
    let f = game.frame();
    assert_eq!((f.top, f.fade, f.sink), (1, 1.0, 0.0));
}

/// `=` and `-` move the view a layer up and down, as far as there are
/// layers, the player staying where they are, and following them again
/// once it's back on their layer; C brings it back to theirs.
/// Sight stays the player's: nothing is in sight on other layers, and
/// what's in sight is remembered whether its layer is shown or not.
#[test]
fn the_view_moves_between_layers_apart_from_the_player() {
    let l = level(&[
        &["........<.", "..........", "..........", ".........."],
        &["........>.", "..........", "..........", "~~........"],
        &["~~~~......", "~~~~......", "~~~~......", "~~~~......"],
    ]);
    let mut g = game_on(l, 1, [6, 1]);
    let settle = |g: &mut Game| g.update(LAYER_SHIFT_TIME);
    let shown = |f: &Frame| f.layers.iter().map(|l| l.index).collect::<Vec<_>>();
    let sighted = |f: &Frame| f.sighted.iter().map(|l| l.index).collect::<Vec<_>>();
    let f = g.frame();
    let both = vec![1, 0];
    assert_eq!(
        (f.top, f.sight, shown(&f), sighted(&f)),
        (1, 1, both.clone(), vec![])
    );

    // Up: the player's layer shows through the hole, in sight; the one
    // above isn't.
    g.key_pressed("Equal");
    assert!(g.shift.is_some(), "going there as the stairs do");
    settle(&mut g);
    let f = g.frame();
    assert_eq!((f.top, f.sight, shown(&f)), (2, 1, vec![2, 1, 0]));
    assert_eq!(g.top, 1, "the player stays");
    assert!(f.layers[0].polygon.is_empty());
    assert!(!f.layers[1].polygon.is_empty());
    assert!(f.layers[1].shaft.is_some() && f.layers[1].seen_through.is_none());
    g.key_pressed("Equal");
    assert!(g.shift.is_none() && g.frame().top == 2, "no further up");

    // Clicks walk the player to the point of their layer drawn there,
    // smaller about the view's center.
    let c = VIEW.map(|v| v / 2.0);
    let p = [c[0] + 30.0, c[1] + 8.0];
    g.click(p);
    let scale = g.parallax();
    let want = g.to_world([c[0] + 30.0 / scale, c[1] + 8.0 / scale]);
    let goal = *g.route.as_ref().unwrap().waypoints.last().unwrap();
    assert!(distance(goal, want) < 1e-3, "{goal:?} for {want:?}");
    g.route = None;

    // Back on the player's layer, following them again.
    g.key_pressed("Minus");
    assert!(g.view.is_none());

    // Down below the player: theirs isn't shown, but is still in sight,
    // as is this one through its hole, though with nothing over it, it's
    // shown as it's remembered.
    g.key_pressed("Minus");
    assert!(g.view.is_some());
    settle(&mut g);
    let f = g.frame();
    assert_eq!(
        (f.top, f.sight, shown(&f), sighted(&f)),
        (0, 1, vec![0], both)
    );
    assert!(f.layers[0].polygon.is_empty());
    assert!(f.sighted.iter().all(|l| !l.polygon.is_empty()));
    assert!(f.sighted[1].seen_through.is_some(), "seen through the hole");
    g.key_pressed("Minus");
    assert_eq!(g.frame().top, 0, "no further down");

    // The player taking the stairs leaves the view where it is, moved
    // off them.
    g.key_pressed("Equal");
    g.key_pressed("Equal");
    settle(&mut g);
    let to_stairs = |g: &mut Game| {
        g.player = [
            path::center([8, 0])[0] - PLAYER_SIZE[0] / 2.0,
            path::center([8, 0])[1] - PLAYER_SIZE[1] / 2.0,
        ];
        g.take_stairs([7, 0]);
    };
    to_stairs(&mut g);
    assert_eq!((g.top, g.frame().top), (0, 2));
    assert!(g.shift.is_none());

    // C: back to the player's layer, and following them again.
    g.key_pressed("KeyC");
    assert!(g.shift.is_some());
    settle(&mut g);
    let f = g.frame();
    assert_eq!((f.top, f.sight, sighted(&f)), (0, 0, vec![]));
    assert!(g.view.is_none());
    // Following them, the view goes with them.
    g.player[0] -= TILE;
    to_stairs(&mut g);
    settle(&mut g);
    assert_eq!((g.top, g.frame().top), (1, 1));
}

/// Along a chamfered shore the route steps diagonally over the
/// shore tiles' open halves instead of going around them.
#[test]
fn routes_cut_past_chamfered_corners() {
    // (0,0) is water in its top-left half: the step from (0,1) to (1,0)
    // clips its open corner.
    let shore = level(&[&["◜..", "...", "..."]]);
    let mut g = game(shore, [0, 1]);
    click_world(&mut g, path::center([1, 0]));
    assert_eq!(
        g.route.as_ref().unwrap().tiles.len(),
        2,
        "one diagonal step"
    );
    // Whole water there: around.
    let lake = level(&[&["≈..", "...", "..."]]);
    let mut g = game(lake, [0, 1]);
    click_world(&mut g, path::center([1, 0]));
    assert_eq!(g.route.as_ref().unwrap().tiles.len(), 3);
}

/// A walk ends exactly where the click was, if the box fits there:
/// anywhere in a tile, or over the open half of a 45-degree tile.
#[test]
fn clicks_land_exactly_where_they_point() {
    let l = level(&[&["......", "......", "......", "......"]]);
    let mut g = game(l, [1, 1]);
    let p = [43.3, 27.7];
    click_world(&mut g, p);
    let waypoints = g.route.as_ref().unwrap().waypoints.clone();
    assert_eq!(
        waypoints.len(),
        1,
        "straight there, no tile center on the way: {waypoints:?}"
    );
    let last = waypoints[0];
    assert!(
        (last[0] - p[0]).abs() < 1e-3 && (last[1] - p[1]).abs() < 1e-3,
        "{last:?}"
    );
    // The destination marker (the outline drawn last: four strips of
    // two triangles) sits on the point, not its tile's center.
    let overlay = g.overlay();
    let marker = &overlay.vertices[overlay.vertices.len() - 24..];
    let xs: Vec<f32> = marker.iter().map(|v| v.pos[0]).collect();
    let (x0, x1) = (
        xs.iter().copied().fold(f32::MAX, f32::min),
        xs.iter().copied().fold(f32::MIN, f32::max),
    );
    assert!(
        ((x0 + x1) / 2.0 - p[0]).abs() < 1.0,
        "marker at {}..{}",
        x0,
        x1
    );
    for _ in 0..600 {
        if g.paused {
            break;
        }
        g.update(1.0 / 60.0);
    }
    assert!(g.paused && g.route.is_none());
    let c = center(g.player, PLAYER_SIZE);
    assert!(
        (c[0] - p[0]).abs() < 1e-3 && (c[1] - p[1]).abs() < 1e-3,
        "{c:?}"
    );
    // Over a shore tile's open half, where the box just fits (its far
    // corner touching the water's edge).
    let shore = level(&[&["......", "...◜..", "......"]]);
    let mut g = game(shore, [1, 1]);
    let p = [39.0, 19.0];
    click_world(&mut g, p);
    let last = *g.route.as_ref().unwrap().waypoints.last().unwrap();
    assert!(
        (last[0] - p[0]).abs() < 1e-3 && (last[1] - p[1]).abs() < 1e-3,
        "{last:?}"
    );
    for _ in 0..600 {
        if g.paused {
            break;
        }
        g.update(1.0 / 60.0);
    }
    let c = center(g.player, PLAYER_SIZE);
    assert!(
        (c[0] - p[0]).abs() < 1e-3 && (c[1] - p[1]).abs() < 1e-3,
        "{c:?}"
    );
    assert!(!g.level.movement(0).collides(g.player, PLAYER_SIZE));
}

/// Clicking where the box doesn't fit (the edge of a 45-degree tile)
/// walks as near as it gets; clicking void (or water) does nothing.
#[test]
fn clicking_a_half_tile_walks_to_its_edge() {
    let l = level(&[&["#######", "...◜...", "#######"]]);
    let mut g = game(l, [1, 1]);
    // (3,1) is water in its top-left half: no standing on it, but the
    // box gets past (2,1)'s center, until its side touches (3,1)'s.
    let touch = 30.0 - PLAYER_SIZE[0] / 2.0;
    click_world(&mut g, [38.0, 18.0]);
    let route = g.route.as_ref().unwrap();
    assert_eq!(
        *route.tiles.last().unwrap(),
        [2, 1],
        "the nearest whole tile"
    );
    let last = *route.waypoints.last().unwrap();
    assert!(last[0] > touch - 1.0 && last[0] <= touch, "{last:?}");
    for _ in 0..600 {
        if g.paused {
            break;
        }
        g.update(1.0 / 60.0);
    }
    assert!(g.paused);
    let c = center(g.player, PLAYER_SIZE);
    assert!(c[0] > touch - 0.5 && c[0] <= touch + 1e-3, "{c:?}");
    assert!(!g.level.movement(0).collides(g.player, PLAYER_SIZE));
    // The water half itself: ignored.
    click_world(&mut g, [31.0, 11.0]);
    assert!(g.route.is_none() && g.paused);
    // A rim: the hole's half is ignored, the floor's half walks there.
    let rim = level(&[&["....", "....", "...."], &["....", ".▛..", "...."]]);
    let mut g = game(rim, [3, 1]);
    click_world(&mut g, [18.0, 18.0]);
    assert!(g.route.is_none(), "clicked the hole");
    click_world(&mut g, [12.0, 12.0]);
    assert!(g.route.is_some(), "clicked the floor");
}

#[test]
fn camera_pans_independently_and_freely() {
    let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let l = Level::new(vec![Layer::from_ascii(&rows)], tileset());
    let mut game = game(l, [32, 20]);
    // The view is 320 x 200 screen pixels, so 200 x 125 world units.
    let view = [VIEW[0] / ZOOM, VIEW[1] / ZOOM];
    assert_eq!(view, [200.0, 125.0]);
    assert_eq!(
        game.camera.pos,
        [325.0 - view[0] / 2.0, 205.0 - view[1] / 2.0],
        "centered"
    );
    game.input.set("ArrowRight", true);
    for _ in 0..600 {
        game.update(1.0 / 60.0);
    }
    // Ten seconds of panning: well past the level's edge.
    let panned = game.camera.pos[0] - (325.0 - view[0] / 2.0);
    let expected = 10.0 * goblin_nightclub_core::camera::PAN_SPEED / ZOOM;
    assert!((panned - expected).abs() < 1.0, "{panned}");
    assert!(game.camera.pos[0] > 640.0, "no boundary");
    assert_eq!(game.player, game.player, "player didn't move");
    // Clicking maps through the camera.
    game.input.set("ArrowRight", false);
    click_world(&mut game, path::center([60, 20]));
    assert_eq!(
        *game.route.as_ref().unwrap().tiles.last().unwrap(),
        [60, 20]
    );
}

#[test]
fn zooming_keeps_the_point_under_the_pointer() {
    let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let l = Level::new(vec![Layer::from_ascii(&rows)], tileset());
    let mut game = game(l, [32, 20]);
    let pointer = [100.0, 60.0];
    let anchor = game.to_world(pointer);
    game.zoom_by(1, pointer);
    assert_eq!(game.zoom(), goblin_nightclub_core::camera::ZOOM_STEP * ZOOM);
    let after = game.to_world(pointer);
    assert!((after[0] - anchor[0]).abs() < 1e-3 && (after[1] - anchor[1]).abs() < 1e-3);
    // Clicks map through the zoom too.
    click_world(&mut game, path::center([33, 21]));
    assert_eq!(
        *game.route.as_ref().unwrap().tiles.last().unwrap(),
        [33, 21]
    );
    // Zoom stops at either end (enough notches for any step size): at
    // 320 x 200, the default shows a pixel of the art per pixel of the
    // screen, and zoomed all the way out, `ZOOM_OUT` of them.
    game.zoom_by(10_000, pointer);
    assert_eq!(game.zoom(), goblin_nightclub_core::camera::ZOOM_MAX * ZOOM);
    game.zoom_by(-10_000, pointer);
    assert_eq!(game.zoom(), ZOOM / goblin_nightclub_core::camera::ZOOM_OUT);
    assert_eq!(game.frame().zoom, game.zoom());
}

#[test]
fn bigger_views_scale_up_and_zoom_out_further() {
    let rows: Vec<String> = (0..40).map(|_| ".".repeat(64)).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let l = Level::new(vec![Layer::from_ascii(&rows)], tileset());
    let mut game = game(l, [32, 20]);
    let center = game.to_world([160.0, 100.0]);
    // Four times the pixels each way (and a bit): the same view of the
    // level, everything four times the size.
    game.resize([1300.0, 820.0]);
    assert_eq!(game.ui_scale(), 4.0);
    assert_eq!(game.zoom(), 4.0 * ZOOM);
    let now = game.to_world([650.0, 410.0]);
    assert!((now[0] - center[0]).abs() < 1e-3 && (now[1] - center[1]).abs() < 1e-3);
    // The pause button scales with the interface.
    let b = game.button_pos();
    game.click([(b[0] + 2.0) * 4.0, (b[1] + 2.0) * 4.0]);
    assert!(!game.paused);
    // Zoomed all the way out, as far as at any size of view, and the
    // whole level fits, with room around it.
    let corner = game.to_world([0.0, 0.0]);
    game.zoom_by(-10_000, [0.0, 0.0]);
    assert_eq!(game.zoom(), ZOOM / goblin_nightclub_core::camera::ZOOM_OUT);
    let view = game.view_world();
    assert!(view[0] > 640.0 && view[1] > 400.0);
    assert_eq!(game.to_world([0.0, 0.0]), corner, "zoomed about the corner");
}

#[test]
fn looking_down_sees_past_the_lower_layers_walls() {
    // A wall below between the viewer and an NPC, all under an opening.
    let l = level(&[
        &["..........", "...#......", ".........."],
        &["~~~~~~~~~~", "~~~~~~~~~~", "~~~~~~~~~~"],
    ]);
    let mut game = game(l, [0, 1]);
    let mut behind = Entity::npc(100, [62.0, 12.0]);
    behind.layer = 0;
    game.npcs.push(behind);
    game.observe();
    assert!(game.memories[0].is_visible(100));
    assert!(game.visions[0].is_explored(8, 1), "floor behind the wall");
}

#[test]
fn lower_layers_are_seen_less_far() {
    // Open below, all void above: the player looks straight down, from
    // under the open sky.
    let (floor, void) = (".".repeat(50), "~".repeat(50));
    let l = level(&[
        &[floor.as_str(), floor.as_str()],
        &[void.as_str(), void.as_str()],
    ]);
    let mut game = game(l, [0, 0]);
    // From the player's center (5, 5): one NPC well inside the reduced
    // radius below, one inside the full radius but beyond the reduced.
    let near_r = VIEW_RADIUS_OUTDOORS - VIEW_RADIUS_STEP;
    let mut near = Entity::npc(1, [5.0 + near_r - 20.0, 2.0]);
    let mut far = Entity::npc(2, [5.0 + near_r + 6.0, 2.0]);
    near.layer = 0;
    far.layer = 0;
    game.npcs.extend([near, far]);
    game.observe();
    assert!(game.memories[0].is_visible(1));
    assert!(
        !game.memories[0].is_visible(2),
        "beyond the lower layer's radius"
    );
    assert_eq!(game.visions[0].radius(), near_r);
    assert_eq!(
        view_radius(VIEW_RADIUS, 1000, false),
        VIEW_RADIUS_MIN,
        "clamped"
    );
}

#[test]
fn indoors_sees_less_far_than_outdoors() {
    // Three layers: the player on the middle one, under a roof with a
    // hole in it. The top layer is all void over the hole only.
    let l = level(&[
        &["................"],
        &["................"],
        &["....~~~~........"],
    ]);
    let mut inside = game_on(l, 1, [1, 0]);
    assert!(!inside.outdoors(), "under the roof");
    assert_eq!(inside.visions[1].radius(), VIEW_RADIUS);
    // Step under the hole: outdoors, and the radius grows.
    click_and_run(&mut inside, [5, 0], 600);
    assert_eq!(player_tile(&inside), [5, 0]);
    assert!(inside.outdoors());
    assert_eq!(inside.visions[1].radius(), VIEW_RADIUS_OUTDOORS);
    // Stairs overhead (the top of a ramp whose foot the player is on)
    // are no roof; a floor beside them is.
    let ramp = level(&[&["....<..."], &["....>~~~"]]);
    let mut foot = game_on(ramp, 0, [4, 0]);
    assert!(foot.outdoors(), "under the ramp's top");
    assert_eq!(foot.visions[0].radius(), VIEW_RADIUS_OUTDOORS);
    click_and_run(&mut foot, [2, 0], 600);
    assert_eq!(player_tile(&foot), [2, 0]);
    assert!(!foot.outdoors(), "under the floor beside it");
    // On the top layer, always outdoors.
    let top = game(level(&[&["...."], &["...."]]), [1, 0]);
    assert!(top.outdoors());
    assert_eq!(top.visions[1].radius(), VIEW_RADIUS_OUTDOORS);
    // From outdoors, what's below ground is seen a fifth as far: the
    // player on the plains (ground level) looking down a hole.
    let pit = level(&[&["...."], &["~~~~"]]);
    let mut above = game_on(pit, 1, [1, 0]);
    above.ground = 1;
    above.observe();
    assert!(above.outdoors());
    let expected =
        ((VIEW_RADIUS_OUTDOORS - VIEW_RADIUS_STEP) * UNDERGROUND_FROM_ABOVE).max(VIEW_RADIUS_MIN);
    assert_eq!(above.visions[0].radius(), expected);
    assert_eq!(above.frame().radius_underground, UNDERGROUND_FROM_ABOVE);
    // Underground, a shaft open to the sky is still underground.
    let shaft = level(&[&["...."], &["~~~~"], &["~~~~"]]);
    let mut below_ground = game_on(shaft, 0, [1, 0]);
    assert!(
        below_ground.outdoors(),
        "ground level 0: the bottom is outdoors"
    );
    below_ground.ground = 1;
    below_ground.observe();
    assert!(!below_ground.outdoors());
    assert_eq!(below_ground.visions[0].radius(), VIEW_RADIUS);
    // The renderer is told, so it can put light mist over the layers
    // below, down to the ground level.
    assert!(top.frame().outdoors);
    assert_eq!(
        top.frame().ground,
        0,
        "a level with no file: all above ground"
    );
    let file = LevelFile::new(
        4,
        1,
        &vec![vec![Tile::FLOOR; 4]; 3],
        Spawn {
            layer: 2,
            pos: [12.5, 5.25],
        },
        tileset(),
        "player",
        "npc",
    )
    .with_ground(1);
    let from_file = Game::from_file(&file, VIEW, 1);
    assert_eq!(from_file.frame().ground, 1);
    assert_eq!(
        center(from_file.player, PLAYER_SIZE),
        [12.5, 5.25],
        "spawn to the pixel"
    );
    assert_eq!(from_file.sprites, sprites(), "sprites by name");
    assert_eq!(from_file.frame().cell, 16);
    assert!(
        !game_on(level(&[&["..."], &["..."]]), 0, [1, 0])
            .frame()
            .outdoors
    );
}

#[test]
fn only_characters_drawn_cover_the_void() {
    let (floor, void) = (".".repeat(60), "~".repeat(60));
    let l = level(&[&[&floor, &floor], &[&floor, &void]]);
    let mut game = game(l, [0, 0]);
    // Along the edge of the void: one NPC in view, one beyond the
    // (outdoor) view radius.
    game.npcs.push(Entity::npc(1, [32.0, 2.0]));
    game.npcs
        .push(Entity::npc(2, [VIEW_RADIUS_OUTDOORS + 60.0, 2.0]));
    for n in &mut game.npcs {
        n.layer = 1;
    }
    game.observe();
    let frame = game.frame();
    let top = &frame.layers[0];
    // Player and the visible NPC: a square each.
    assert_eq!(top.standing.vertices.len(), 2 * 6);
    // The void row, a rectangle in each chunk it crosses.
    let voids = top.meshes.pieces.iter().flat_map(|p| &p.voids.vertices);
    let voids: Vec<_> = voids.collect();
    let area: f32 = (voids.chunks(3))
        .map(|t| {
            let [a, b, c] = [t[0].pos, t[1].pos, t[2].pos];
            ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() / 2.0
        })
        .sum();
    assert!((area - 60.0 * TILE * TILE).abs() < 1e-2, "{area}");
}

#[test]
fn standing_over_stacked_walls_still_sees_to_the_bottom() {
    // The player stands on floor over a wall over a wall, beside a hole
    // down through both upper layers.
    let l = level(&[
        &["#.....", "#....."], //
        &["#~~~~~", "#~~~~~"],
        &[".~~~~~", ".~~~~~"],
    ]);
    let game = game(l, [0, 0]);
    // The wall under the wall under the player shows at the hole's
    // edge, and so does the floor at the bottom of the hole.
    assert!(game.visions[0].is_explored(0, 0));
    assert!(game.visions[0].is_explored(4, 1));
    assert!(game.visions[1].is_explored(0, 0));
}

#[test]
fn floors_above_hide_what_is_under_them() {
    // Two down, a hole at x 4..5; beside it, under the middle layer's
    // floor, a wall at x 3 and floor at x 1..2 (the roof of a room).
    let l = level(&[
        &["...#.#..", "...#.#..", "...#.#.."],
        &["....~~..", "....~~..", "....~~.."],
        &["~~~~~~~~", "~~~~~~~~", "~~~~~~~~"],
    ]);
    let game = game(l, [6, 1]);
    assert!(game.visions[0].is_explored(4, 1), "the bottom of the hole");
    assert!(game.visions[0].is_explored(5, 1), "a wall in the hole");
    assert!(
        game.visions[0].is_explored(3, 1),
        "a wall at the hole's edge"
    );
    assert!(!game.visions[0].is_explored(2, 1), "under the floor");
    assert!(!game.visions[0].is_explored(1, 1), "under the floor");
    // An NPC under that floor isn't seen.
    let mut game = game;
    let mut hidden = Entity::npc(9, [12.0, 12.0]);
    hidden.layer = 0;
    game.npcs.push(hidden);
    game.observe();
    assert!(!game.memories[0].is_visible(9));
}

#[test]
fn npcs_below_are_seen_only_through_holes() {
    let l = level(&[
        &["..........", "..........", ".........."],
        &["..........", "....~~....", "....~~...."],
    ]);
    let mut game = game(l, [0, 0]);
    // One below the hole, one below the deck's floor.
    let mut under_hole = Entity::npc(100, [42.0, 12.0]);
    let mut under_floor = Entity::npc(101, [82.0, 12.0]);
    under_hole.layer = 0;
    under_floor.layer = 0;
    game.npcs.extend([under_hole, under_floor]);
    game.observe();
    assert!(game.memories[0].is_visible(100));
    assert!(!game.memories[0].is_visible(101));
    let frame = game.frame();
    assert_eq!(
        frame.layers.iter().map(|l| l.index).collect::<Vec<_>>(),
        vec![1, 0]
    );
}
