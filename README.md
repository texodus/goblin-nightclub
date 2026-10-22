# Goblin Nightclub

A rogue-like game engine in Rust via `wgpu`.

## Features

- **True line of sight.** Each frame, the game works out exactly what the
  player can see, through walls cut at 45° angles, half tiles, trees and
  rocks. Places you can see now are drawn in full color. Places you've seen
  before are drawn in dithered EGA grays, and places you've never seen are
  fog. Characters out of sight show as gray ghosts where you last saw them.
- **Parallax layers.** Up to eight layers of the world are drawn at once.
  Each one lower down is drawn a little smaller around the middle of the
  screen, so you look down through holes, pits and water into the levels
  below, with mist between them. Taking the stairs zooms and fades from one
  layer to the next, and the camera can move up and down layers on its own.
- **A practically endless world.** The default world is 4096×4096 tiles and
  9 layers deep. Web workers generate it a region at a time as you play,
  from overworld terrain with rivers, lakes and cliffs down to caves joined
  by stairs. You play in a window of 32-tile chunks that is quietly
  replaced as you move, so the size of the world doesn't slow you down.

## Development

You'll need stable Rust (the wasm target is set up by `rust-toolchain.toml`),
[wasm-pack](https://rustwasm.github.io/wasm-pack/), Node 20+, and a browser
that supports WebGPU.

```sh
npm run dev      # build, serve at http://localhost:8080, rebuild on changes
npm run build    # release build into dist/
npm run test     # native tests for every crate
```
