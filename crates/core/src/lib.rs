//! The engine: tile grids and their geometry, levels of stacked layers,
//! sight, movement, drawing lists, and level files. Nothing here touches
//! the browser or GPU, so it compiles natively and is unit tested with
//! `cargo test`. Nothing here knows any material by name either: what
//! the tiles look like is data a level carries (its tileset) and the sheet
//! image is loaded at runtime.
//!
//! The `test-util` feature exposes test helpers (levels from ASCII art,
//! random tile soups, invariant checks) for other crates' tests.

pub mod camera;
pub mod draw;
pub mod entity;
pub mod frame;
pub mod input;
pub mod layer;
pub mod level;
pub mod level_file;
pub mod memory;
pub mod palette;
pub mod path;
pub mod rng;
pub mod tileset;
pub mod vision;

/// The screen the interface is laid out for, and the view shows about as
/// much of the level as: the real screen (the canvas at its own
/// resolution) is a whole number of times this, more or less.
pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 200;
