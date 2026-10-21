//! Tests of the other crates through their public APIs (and the engine's
//! `test-util` helpers), a module per module tested. Tests that need a
//! crate's private items stay in that crate, in a `tests` module of their
//! own beside what they test.

#![cfg(test)]

mod chunk;
mod draw;
mod entity;
mod fringe;
mod generate;
mod generated_levels;
mod layer;
mod level;
mod level_file;
mod memory;
mod mesh;
mod packed;
mod path;
mod prop;
mod shaders;
mod tileset;
mod vision;
mod world;
