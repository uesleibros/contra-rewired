//! `contra-extract` - validates a user-supplied ROM and tells you exactly
//! how to play it.
//!
//! Earlier in this project the plan was for this tool to decompress
//! Contra's graphics/audio into asset files. That's no longer needed:
//! `contra-pc` runs the ROM directly through `contra-nes` (a real NES
//! emulation core), so the original game code decompresses its own
//! graphics/audio at run time the same way it does on real hardware.
//! There is nothing to extract. See docs/ASSETS.md for the full story.
//!
//! What this tool actually does now: confirm the file is a valid ROM,
//! confirm `contra-nes` supports its mapper, and print the exact command
//! to launch it. `contra-pc` performs the same validation internally when
//! you point it at a ROM directly, so this is mainly useful for a quick
//! sanity check or scripting, not a required step before playing.

use clap::Parser;
use contra_assets::{NesRom, RomIdentity};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about = "Validate your own legally-dumped Contra (NES) ROM and print the command to play it.")]
struct Args {
    /// Path to your own ROM dump, e.g. baserom.nes
    rom_path: PathBuf,
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

    Ok(())
}
