//! The 16-color EGA palette as linear-ish RGBA floats (sRGB values / 255).

// Not every color is in use at any given time.
#![allow(dead_code)]

pub type Color = [f32; 4];

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

pub const BLACK: Color = rgb(0x00, 0x00, 0x00);
pub const BLUE: Color = rgb(0x00, 0x00, 0xAA);
pub const GREEN: Color = rgb(0x00, 0xAA, 0x00);
pub const CYAN: Color = rgb(0x00, 0xAA, 0xAA);
pub const RED: Color = rgb(0xAA, 0x00, 0x00);
pub const MAGENTA: Color = rgb(0xAA, 0x00, 0xAA);
pub const BROWN: Color = rgb(0xAA, 0x55, 0x00);
pub const LIGHT_GRAY: Color = rgb(0xAA, 0xAA, 0xAA);
pub const DARK_GRAY: Color = rgb(0x55, 0x55, 0x55);
pub const LIGHT_BLUE: Color = rgb(0x55, 0x55, 0xFF);
pub const LIGHT_GREEN: Color = rgb(0x55, 0xFF, 0x55);
pub const LIGHT_CYAN: Color = rgb(0x55, 0xFF, 0xFF);
pub const LIGHT_RED: Color = rgb(0xFF, 0x55, 0x55);
pub const LIGHT_MAGENTA: Color = rgb(0xFF, 0x55, 0xFF);
pub const YELLOW: Color = rgb(0xFF, 0xFF, 0x55);
pub const WHITE: Color = rgb(0xFF, 0xFF, 0xFF);

pub const ALL: [Color; 16] = [
    BLACK,
    BLUE,
    GREEN,
    CYAN,
    RED,
    MAGENTA,
    BROWN,
    LIGHT_GRAY,
    DARK_GRAY,
    LIGHT_BLUE,
    LIGHT_GREEN,
    LIGHT_CYAN,
    LIGHT_RED,
    LIGHT_MAGENTA,
    YELLOW,
    WHITE,
];
