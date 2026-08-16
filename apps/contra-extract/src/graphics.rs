//! Wires `contra_native::graphics` (the ported, byte-perfect-verified RLE
//! decompressor for Contra's `write_graphic_data_to_ppu` routine) into an
//! actual extraction command: decode every documented "graphic data" blob
//! directly out of PRG-ROM and write its pattern-table tiles to PNG.
//!
//! This is real asset extraction, not emulation: nothing here runs any
//! 6502 code or touches `contra-nes` at all - it reads raw ROM bytes and
//! decodes them with our own Rust port, the same way a Ship of
//! Harkinian-style asset pipeline would. See
//! `crates/contra-native/src/graphics.rs`'s doc comment for the format,
//! and `crates/contra-nes/examples/extract_graphics.rs` for the
//! cross-check that proved this decoder byte-for-byte identical to real
//! CHR-RAM after actually playing into level 1.

/// `(name, PRG-ROM byte offset, one-line description)` for every
/// `graphic_data_XX` blob, taken directly from
/// `docs/Graphics Documentation.md`'s "Graphics Data Locations" table in
/// `vermiceli/nes-contra-us` (its "PRG ROM Address" column - already a
/// plain offset into PRG-ROM, no iNES header to add since
/// `contra_assets::NesRom` strips it before we ever see the bytes).
/// `graphic_data_00`, `_02`, and `_18` write nametable/attribute data
/// (PPU `$2000+`), not pattern-table tiles, so they're included here for
/// completeness but produce no CHR tiles to dump.
pub const GRAPHIC_DATA_BLOBS: [(&str, usize, &str); 27] = [
    ("graphic_data_00", 0x1cb36, "blanks both nametables + attribute tables"),
    ("graphic_data_01", 0x12a2d, "intro/title/game-over: logo, Bill & Lance, letters, numbers, falcon"),
    ("graphic_data_02", 0x09097, "intro screen nametable + attribute layout"),
    ("graphic_data_03", 0x10001, "every level: Bill/Lance outdoor sprites, lives medals, power-ups, explosions"),
    ("graphic_data_04", 0x105ae, "indoor/base levels: player sprite tiles"),
    ("graphic_data_05", 0x14001, "level 1: bridge/mountain/trees/water, player prone, flying capsule"),
    ("graphic_data_06", 0x119fc, "indoor/base: indoor player sprites, grenades, background"),
    ("graphic_data_07", 0x14a61, "level 3: background + sprite tiles, player prone, flying capsule"),
    ("graphic_data_08", 0x1086c, "indoor/base boss screen background + sprites"),
    ("graphic_data_09", 0x119cd, "level 4 boss screen sprites (3 tiles)"),
    ("graphic_data_0a", 0x12005, "indoor/base tiles (same as _10, flipped)"),
    ("graphic_data_0b", 0x153e0, "level 5 pattern table tiles"),
    ("graphic_data_0c", 0x18001, "level 6 pattern table tiles"),
    ("graphic_data_0d", 0x18cdc, "level 7 pattern table tiles"),
    ("graphic_data_0e", 0x19bd6, "level 8 pattern table tiles"),
    ("graphic_data_0f", 0x12346, "indoor/base tiles (14 background tiles)"),
    ("graphic_data_10", 0x12003, "indoor/base tiles (same as _0a, flipped)"),
    ("graphic_data_11", 0x123e7, "indoor/base background tiles"),
    ("graphic_data_12", 0x12940, "level 4 background tiles"),
    ("graphic_data_13", 0x107a1, "player aiming up/straight, laser bullets"),
    ("graphic_data_14", 0x16814, "rotating gun + red turret tiles"),
    ("graphic_data_15", 0x1b07a, "levels 5/6/7: turret man (basquez) sprites"),
    ("graphic_data_16", 0x1b15c, "weapon box tiles"),
    ("graphic_data_17", 0x16ddf, "ending scene: helicopter + island tiles"),
    ("graphic_data_18", 0x1730d, "ending scene nametable + attribute data"),
    ("graphic_data_19", 0x1631b, "player killed: recoil + lying on ground"),
    ("graphic_data_1a", 0x16500, "enemy soldier sprite tiles"),
];

/// Decodes every blob in [`GRAPHIC_DATA_BLOBS`] and writes one PNG per
/// CHR-bound (pattern-table) write segment into `out_dir`. Returns how
/// many PNGs were written and how many segments were skipped because they
/// targeted nametable/attribute memory instead (not CHR data, so there's
/// no tile sheet to render for them here).
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<(usize, usize)> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = 0usize;
    let mut skipped_non_chr = 0usize;

    for (name, offset, description) in GRAPHIC_DATA_BLOBS {
        let blob = prg_rom.get(offset..).ok_or_else(|| {
            anyhow::anyhow!("{name}: PRG offset {offset:#06x} is past the end of this ROM's PRG-ROM ({} bytes)", prg_rom.len())
        })?;
        let segments = contra_native::graphics::decompress(blob);
        log::info!("{name} ({description}): {} segment(s)", segments.len());

        for (i, segment) in segments.iter().enumerate() {
            if segment.ppu_addr as usize >= 0x2000 || segment.bytes.len() % 16 != 0 {
                skipped_non_chr += 1;
                continue;
            }
            let path = out_dir.join(format!("{name}_seg{i}_ppu{:04x}.png", segment.ppu_addr));
            save_tile_strip(&path, &segment.bytes)?;
            written += 1;
        }
    }

    Ok((written, skipped_non_chr))
}

fn save_tile_strip(path: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tile_count = bytes.len() / 16;
    const TILES_PER_ROW: usize = 16;
    let rows = tile_count.div_ceil(TILES_PER_ROW);
    let cols = TILES_PER_ROW.min(tile_count.max(1));
    let w = cols * 8;
    let h = rows * 8;
    let mut buf = vec![0u8; w * h];

    for tile in 0..tile_count {
        let base = tile * 16;
        let plane0 = &bytes[base..base + 8];
        let plane1 = &bytes[base + 8..base + 16];
        let tile_x = (tile % TILES_PER_ROW) * 8;
        let tile_y = (tile / TILES_PER_ROW) * 8;
        for row in 0..8 {
            for col in 0..8 {
                let bit = 7 - col;
                let lo = (plane0[row] >> bit) & 1;
                let hi = (plane1[row] >> bit) & 1;
                let value = lo | (hi << 1);
                buf[(tile_y + row) * w + tile_x + col] = value * 85;
            }
        }
    }

    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, w as u32, h as u32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&buf)?;
    Ok(())
}
