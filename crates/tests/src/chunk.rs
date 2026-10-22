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

use goblin_nightclub_core::chunk::*;
use goblin_nightclub_core::layer::Tile;
use goblin_nightclub_core::layer::{Corner, Part};

#[test]
fn uniform_chunks_hold_one_tile() {
    let rock = Chunk::filled(Tile::WALL);
    assert_eq!(rock.uniform(), Some(Tile::WALL));
    assert_eq!(rock.get(31, 31), Tile::WALL);
    let made = Chunk::from_fn(|_, _| Tile::VOID);
    assert_eq!(made.uniform(), Some(Tile::VOID));
    assert_eq!(made.palette(), &[Tile::VOID]);
}

#[test]
fn tiles_come_back_as_they_went_in() {
    let tile = |x: usize, y: usize| match (x + 2 * y) % 5 {
        0 => Tile::FLOOR,
        1 => Tile::WALL,
        2 => Tile::slope(Corner::CLOCKWISE[(x + y) % 4]),
        3 => Tile::VOID,
        _ => Tile::Whole(Part::Floor(((x * y) % 7) as u8)),
    };
    let chunk = Chunk::from_fn(tile);
    assert!(chunk.uniform().is_none());
    for y in 0..CHUNK {
        for x in 0..CHUNK {
            assert_eq!(chunk.get(x, y), tile(x, y), "({x}, {y})");
        }
    }
    // A split given from either corner is one tile to `==`, but comes
    // back as it was given.
    let given = |x: usize, _| {
        if x.is_multiple_of(2) {
            Tile::split(Corner::NW, Part::Wall(0), Part::Floor(0))
        } else {
            Tile::split(Corner::SE, Part::Floor(0), Part::Wall(0))
        }
    };
    let same = Chunk::from_fn(given);
    assert_eq!(same.palette().len(), 2);
    for x in 0..2 {
        assert!(identical(same.get(x, 0), given(x, 0)));
    }
}

#[test]
fn grids_of_any_size_read_back() {
    let (cols, rows) = (70, 33);
    let tiles: Vec<Tile> = (0..cols * rows)
        .map(|i| {
            if (i * 7) % 11 < 3 {
                Tile::WALL
            } else {
                Tile::FLOOR
            }
        })
        .collect();
    let grid = Chunks::from_tiles(cols, rows, &tiles);
    assert_eq!(grid.size(), [3, 2]);
    for y in 0..rows {
        for x in 0..cols {
            assert_eq!(grid.get(x, y), tiles[y * cols + x]);
        }
    }
    // The last chunk down is mostly past the edge; all one tile.
    let solid = Chunks::from_tiles(40, 40, &vec![Tile::WALL; 1600]);
    assert!((0..2).all(|c| solid.chunk(c, 1).uniform() == Some(Tile::WALL)));
}
