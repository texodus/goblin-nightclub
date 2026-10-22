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

//! Pieces of layers made for one window onto a level and kept for the
//! next (see [`crate::world`]), which shares most of its chunks.

use std::collections::HashMap;
use std::sync::Arc;

use crate::art::MeshPiece;
use crate::chunk::ChunkKey;
use crate::layer::Piece;

/// [`Piece`]s kept for the next window: about two windows' worth.
const PIECES_KEPT: usize = 3000;
/// [`MeshPiece`]s kept for the next window: a window's worth (one a chunk
/// of each layer seen) and a column more, as they're big.
const MESH_PIECES_KEPT: usize = 450;

/// What a [`Piece`] is of: a layer, where it blocks movement, or where it
/// blocks the view of it from layer `top` (see [`crate::level::Level::view`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceOf {
    Tiles,
    Movement,
    View { top: usize },
}

/// Pieces of layers made for a window, kept for the next one, which
/// shares most of its chunks: of their geometry ([`Piece`]s) and what's
/// drawn of them ([`MeshPiece`]s), those used most recently. Each is of a
/// chunk of the level, the same in any window.
#[derive(Default)]
pub struct Pieces {
    geometry: Kept<(PieceOf, ChunkKey), Piece>,
    meshes: Kept<ChunkKey, MeshPiece>,
}

impl Pieces {
    /// The piece of `of` over chunk `key`, made by `make` unless it's kept.
    pub fn get(&mut self, of: PieceOf, key: ChunkKey, make: impl FnOnce() -> Piece) -> Arc<Piece> {
        self.geometry.get((of, key), PIECES_KEPT, make)
    }

    /// What's drawn of chunk `key`, made by `make` unless it's kept.
    pub fn meshes(&mut self, key: ChunkKey, make: impl FnOnce() -> MeshPiece) -> Arc<MeshPiece> {
        self.meshes.get(key, MESH_PIECES_KEPT, make)
    }

    /// How many pieces of geometry have been made, all told.
    pub fn made(&self) -> usize {
        self.geometry.made
    }

    /// How many [`MeshPiece`]s have been made, all told.
    pub fn meshes_made(&self) -> usize {
        self.meshes.made
    }
}

/// Things kept by key, the most recently used.
struct Kept<K, T> {
    kept: HashMap<K, (Arc<T>, u64)>,
    /// Counts uses, to tell the most recent.
    clock: u64,
    /// How many have been made.
    made: usize,
}

impl<K, T> Default for Kept<K, T> {
    fn default() -> Self {
        Self {
            kept: HashMap::new(),
            clock: 0,
            made: 0,
        }
    }
}

impl<K: std::hash::Hash + Eq, T> Kept<K, T> {
    /// The thing kept by `key`, made by `make` if there's none, keeping
    /// about `most`.
    fn get(&mut self, key: K, most: usize, make: impl FnOnce() -> T) -> Arc<T> {
        self.clock += 1;
        let clock = self.clock;
        if let Some((thing, used)) = self.kept.get_mut(&key) {
            *used = clock;
            return thing.clone();
        }
        let thing = Arc::new(make());
        self.made += 1;
        self.kept.insert(key, (thing.clone(), clock));
        // Down to the most recent, a batch at a time.
        if self.kept.len() > most + most / 4 {
            let mut uses: Vec<u64> = self.kept.values().map(|&(_, used)| used).collect();
            let gone = self.kept.len() - most;
            let cut = *uses.select_nth_unstable(gone).1;
            self.kept.retain(|_, &mut (_, used)| used >= cut);
        }
        thing
    }
}
