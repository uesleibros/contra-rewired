//! `contra-extract` - validates a user-supplied ROM and, per
//! `docs/NATIVE_PORT.md`'s "actual end state", is the one place the real
//! PC port is allowed to touch the ROM at all: a one-time extraction step
//! that decodes assets into plain files so `contra-pc` never needs to
//! again. See docs/ASSETS.md for the full story of how this project's
//! stance on that changed.
//!
//! What this tool does today: confirm the file is a valid, supported ROM
//! and print the exact command to launch it (unchanged, still useful on
//! its own for a quick sanity check); with `--dump-graphics <dir>`, decode
//! every documented `graphic_data_XX` blob (see
//! `crates/contra-native/src/graphics.rs`, ported from the real
//! `write_graphic_data_to_ppu` routine and proven byte-for-byte identical
//! to live CHR-RAM - see `crates/contra-nes/examples/extract_graphics.rs`)
//! straight out of PRG-ROM into pattern-table tile-sheet PNGs; and with
//! `--dump-palettes <dir>`, render every `game_palettes` group (see
//! `crates/contra-native/src/palette.rs`) as a color swatch. No emulation
//! involved in either.

mod graphics;
mod palette;

use clap::Parser;
use contra_assets::{NesRom, RomIdentity};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about = "Validate your own legally-dumped Contra (NES) ROM, print the command to play it, and (optionally) extract its graphics.")]
struct Args {
    /// Path to your own ROM dump, e.g. baserom.nes
    rom_path: PathBuf,

    /// Decode every documented graphic-data blob straight from PRG-ROM
    /// into pattern-table tile-sheet PNGs in this directory. No
    /// emulation involved - this reads raw ROM bytes and decodes them
    /// with contra-native's own ported decompressor.
    #[arg(long, value_name = "DIR")]
    dump_graphics: Option<PathBuf>,

    /// Render every `game_palettes` group to a color-swatch PNG in this
    /// directory.
    #[arg(long, value_name = "DIR")]
    dump_palettes: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    log::info!("Loading {}", args.rom_path.display());
    let rom = NesRom::load(&args.rom_path)?;

    match rom.identity() {
        RomIdentity::ContraUs => println!("Recognized: Contra (USA). MD5 {}", rom.md5_hex),
        RomIdentity::Probotector => println!("Recognized: Probotector. MD5 {}", rom.md5_hex),
        RomIdentity::Unknown => println!(
            "Warning: MD5 {} does not match the known Contra (USA) hash.\n\
             It may still work (e.g. a Probotector/regional dump), but only\n\
             the US release has been tested against this emulator core.",
            rom.md5_hex
        ),
    }

    println!(
        "PRG-ROM: {} KiB, CHR-ROM: {} KiB, mapper {}",
        rom.prg_rom.len() / 1024,
        rom.chr_rom.len() / 1024,
        rom.mapper
    );

    let path_display = args.rom_path.display();

    if rom.mapper == 2 {
        println!(
            "\nMapper 2 (UxROM) - contra-nes supports this. Nothing else to do; play it with:\n\n    cargo run -p contra-pc --release -- \"{path_display}\"\n"
        );
    } else {
        println!(
            "\nMapper {} - contra-nes currently only emulates mapper 2 (UxROM), which is\n\
             what Contra (USA) uses. contra-pc will refuse this ROM and fall back to its\n\
             placeholder demo instead of playing it. Adding another mapper is real, scoped\n\
             work - see ROADMAP.md.",
            rom.mapper
        );
    }

    if let Some(out_dir) = &args.dump_graphics {
        let (written, skipped_non_chr) = graphics::dump_all(&rom.prg_rom, out_dir)?;
        println!(
            "\nWrote {written} tile-sheet PNG(s) to {}. ({skipped_non_chr} segment(s) targeted nametable/attribute data, not CHR - not dumped as tile sheets.)",
            out_dir.display()
        );
    }

    if let Some(out_dir) = &args.dump_palettes {
        let count = palette::dump_all(&rom.prg_rom, out_dir)?;
        println!("\nWrote {count} palette swatches to {}/game_palettes.png.", out_dir.display());
    }

    Ok(())
}
