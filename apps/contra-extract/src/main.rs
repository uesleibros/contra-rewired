//! `contra-extract` — validates a user-supplied `baserom.nes` and reports
//! what it found. Actual asset decompression (graphics/audio) is not yet
//! implemented; see docs/ASSETS.md and ROADMAP.md. This tool exists now so
//! the legal/data-flow story (BYO-ROM, nothing copyrighted in this repo) is
//! real and testable from day one, even before the decoder lands.

use clap::Parser;
use contra_assets::{NesRom, RomIdentity};
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about = "Validate and (eventually) extract assets from your own legally-owned Contra (NES) ROM.")]
struct Args {
    /// Path to your own dump of the ROM, e.g. baserom.nes
    rom_path: PathBuf,

    /// Output directory for extracted assets (created if missing).
    #[arg(short, long, default_value = "assets")]
    out: PathBuf,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    log::info!("Loading {}", args.rom_path.display());
    let rom = NesRom::load(&args.rom_path)?;

    match rom.identity() {
        RomIdentity::ContraUs => {
            println!("Recognized: Contra (USA). MD5 {}", rom.md5_hex);
        }
        RomIdentity::Probotector => {
            println!("Recognized: Probotector. MD5 {}", rom.md5_hex);
        }
        RomIdentity::Unknown => {
            println!(
                "Warning: MD5 {} does not match the known Contra (USA) hash.\n\
                 This may still work if it's a Probotector/regional dump, but\n\
                 extraction has only been validated against the US release.",
                rom.md5_hex
            );
        }
    }

    println!(
        "PRG-ROM: {} KiB, CHR-ROM: {} KiB, mapper {}",
        rom.prg_rom.len() / 1024,
        rom.chr_rom.len() / 1024,
        rom.mapper
    );

    std::fs::create_dir_all(&args.out)?;
    println!(
        "\nROM validated. Graphics/audio decompression is not implemented yet\n\
         (tracked in ROADMAP.md, Phase 1). No files were written to {}.",
        args.out.display()
    );

    Ok(())
}
