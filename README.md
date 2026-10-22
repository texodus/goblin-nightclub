# Goblin Nightclub

A rogue-like game engine in Rust via `wgpu`.

## Features

- True line-of-sight.
- Parallax multi-planar rendering.
- A practically infinite, lazily generated world.

## Development

You'll need stable Rust (the wasm target is set up by `rust-toolchain.toml`),
[wasm-pack](https://rustwasm.github.io/wasm-pack/), Node 20+, and a browser
that supports WebGPU.

```sh
npm run dev      # build, serve at http://localhost:8080, rebuild on changes
npm run build    # release build into dist/
npm run test     # native tests for every crate
```
