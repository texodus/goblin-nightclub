//! Decodes the tile sheet at compile time, so the game embeds raw RGBA and
//! needs no PNG decoder: writes `$OUT_DIR/tiles.rgba` (width and height as
//! little-endian u32s, then the pixels row by row).

use std::path::Path;

fn main() {
    let src = "assets/tiles.png";
    println!("cargo::rerun-if-changed={src}");
    let file = std::fs::File::open(src).expect("assets/tiles.png (run `node scripts/tiles.mjs`)");
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().expect("tile sheet header");
    let mut rgba = vec![0; reader.output_buffer_size().expect("tile sheet size")];
    let info = reader.next_frame(&mut rgba).expect("tile sheet pixels");
    rgba.truncate(info.buffer_size());
    assert_eq!(
        info.color_type,
        png::ColorType::Rgba,
        "tile sheet must decode to RGBA"
    );

    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend(info.width.to_le_bytes());
    out.extend(info.height.to_le_bytes());
    out.extend(rgba);
    let dir = std::env::var("OUT_DIR").unwrap();
    std::fs::write(Path::new(&dir).join("tiles.rgba"), out).unwrap();
}
