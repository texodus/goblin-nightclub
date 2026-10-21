//! Opening a level too big to play whole: its first window made a piece
//! at a time, then the game on it.

use super::*;

/// A game on a level of chunks, starting: its first window being made, a
/// piece at a time, so a slow source (a server) needn't hold anything up.
pub struct Opening {
    build: Build,
    source: Rc<dyn ChunkSource>,
    meta: LevelMeta,
    view: [f32; 2],
    seed: u32,
}

impl Opening {
    /// Start a game on a level of chunks from `source`, `meta` saying the
    /// rest, seen through a view of size `view`.
    pub fn new(source: Rc<dyn ChunkSource>, meta: &LevelMeta, view: [f32; 2], seed: u32) -> Self {
        let size = [meta.cols, meta.rows, meta.depth];
        let window = Window::around(size, tile_of(meta.spawn.pos), meta.spawn.layer);
        Self {
            build: Build::new(source.clone(), window, meta.spawn.layer),
            source,
            meta: meta.clone(),
            view,
            seed,
        }
    }

    /// Make a piece more of the first window: whether it's all made.
    pub fn step(&mut self) -> bool {
        self.build.step()
    }

    /// The art the level is drawn with.
    pub fn tileset(&self) -> &Tileset {
        &self.meta.tileset
    }

    /// The game: once [`Opening::step`] says all's made, or right away for
    /// a source with every chunk ready. A level played whole is as if from
    /// a level file. Fails if the player doesn't fit at the spawn.
    pub fn finish(self) -> Result<Game, String> {
        let meta = &self.meta;
        let size = [meta.cols, meta.rows, meta.depth];
        let (window, pieces) = (self.build.window(), self.build.pieces());
        let (level, roofs) = self.build.finish();
        let o = window.tile_origin().map(|t| t as f32 * TILE);
        let [x, y] = meta.spawn.pos;
        let player = [
            x - o[0] - PLAYER_SIZE[0] / 2.0,
            y - o[1] - PLAYER_SIZE[1] / 2.0,
        ];
        let top = meta.spawn.layer - window.base;
        if level.movement(top).collides(player, PLAYER_SIZE) {
            return Err("the player doesn't fit at the spawn".into());
        }
        let sprites = Sprites::from_meta(meta);
        if window.is_whole(size) {
            let mut game = Game::with_level_at(level, top, player, sprites, self.view, self.seed);
            game.ground = meta.ground.min(size[2] - 1);
            return Ok(game);
        }
        let mut stream = Stream::new(self.source, window, roofs, pieces, self.seed);
        let avoid = center(player, PLAYER_SIZE);
        let npcs = stream.wake(&level, NPC_PER_CHUNK, avoid, NPC_SPAWN_DIST);
        let rng = Rng::new(self.seed ^ 0x9E37_79B9);
        let mut game = Game::assemble(level, top, player, sprites, self.view, rng, npcs);
        game.stream = Some(stream);
        game.ground = meta.ground.min(size[2] - 1);
        game.observed = None;
        game.observe();
        Ok(game)
    }
}
