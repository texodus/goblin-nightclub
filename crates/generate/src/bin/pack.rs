//! Levels packed in chunks (see `goblin_nightclub_core::packed`), from the
//! command line:
//!
//!   pack LEVEL.json OUT.pack          a level file, packed
//!   pack --generate SEED COLS ROWS OUT.pack [PARAMS.json]
//!                                     a newly generated world that size:
//!                                     made whole if it fits in a region,
//!                                     else a region at a time, on every
//!                                     core
//!   pack --unpack IN.pack OUT.json    a packed level (small enough) as a
//!                                     level file
//!
//! Run with `cargo run --release -p goblin-nightclub-generate --bin pack --`.

use std::process::ExitCode;
use std::sync::Arc;

use goblin_nightclub_core::layer::TILE;
use goblin_nightclub_core::level_file::LevelFile;
use goblin_nightclub_core::packed::{Packed, pack_file};
use goblin_nightclub_generate::{Generated, Params, Plan, REGION, world};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(done) => {
            eprintln!("{done}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("pack: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<String, String> {
    let read = |path: &str| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
    let write = |path: &str, bytes: &[u8]| {
        std::fs::write(path, bytes).map_err(|e| format!("{path}: {e}"))?;
        Ok(format!("wrote {path} ({} bytes)", bytes.len()))
    };
    let number = |s: &str| {
        s.parse::<usize>()
            .map_err(|_| format!("{s:?} isn't a number"))
    };
    match args {
        [flag, seed, cols, rows, out, rest @ ..] if flag == "--generate" && rest.len() <= 1 => {
            let params: Params = match rest.first() {
                Some(path) => serde_json::from_slice(&read(path)?).map_err(|e| format!("{path}: {e}"))?,
                None => Params::default(),
            };
            let (cols, rows, seed) = (number(cols)?, number(rows)?, number(seed)? as u32);
            if cols.max(rows) > REGION {
                return write(out, &Generated::new(Plan::new(seed, cols, rows, &params)).bake());
            }
            let spawn = [cols as f32 / 2.0 * TILE, rows as f32 / 2.0 * TILE];
            write(out, &pack_file(&world(seed, cols, rows, spawn, &params)))
        }
        [flag, input, out] if flag == "--unpack" => {
            let bytes: Arc<[u8]> = read(input)?.into();
            let packed = Packed::open(bytes)?.ok_or("it's cut short")?;
            if packed.header().base.is_some() {
                return Err(format!("{input} is a diff: unpack the level it's over"));
            }
            write(out, packed.to_level_file()?.to_json().as_bytes())
        }
        [input, out] if !input.starts_with("--") => {
            let json = String::from_utf8(read(input)?).map_err(|_| format!("{input} isn't text"))?;
            let file = LevelFile::from_json(&json).map_err(|e| format!("{input}: {e}"))?;
            write(out, &pack_file(&file))
        }
        _ => Err("usage: pack LEVEL.json OUT.pack | pack --generate SEED COLS ROWS OUT.pack [PARAMS.json] | pack --unpack IN.pack OUT.json".into()),
    }
}
