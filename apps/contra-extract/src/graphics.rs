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

/// One-line descriptions for every `graphic_data_XX` index (0x00-0x1a),
/// taken from `docs/Graphics Documentation.md`'s "Graphics Data Locations"
/// table in `vermiceli/nes-contra-us` - documentation only. Offsets and
/// the horizontal-flip flag are *not* hardcoded here; both are resolved
/// from `graphic_data_ptr_tbl` at runtime by `contra_native::graphics`,
/// the same table the real game itself reads (see that module's doc
/// comment - an earlier version of this hardcoded offsets by hand from
/// the same doc table, which silently got `graphic_data_10`'s flip bit
/// wrong, decoding it unflipped).
/// `graphic_data_00`, `_02`, and `_18` write nametable/attribute data
/// (PPU `$2000+`), not pattern-table tiles, so they're included here for
/// completeness but produce no CHR tiles to dump.
pub const GRAPHIC_DATA_DESCRIPTIONS: [(u8, &str); 27] = [
    (0x00, "blanks both nametables + attribute tables"),
    (0x01, "intro/title/game-over: logo, Bill & Lance, letters, numbers, falcon"),
    (0x02, "intro screen nametable + attribute layout"),
    (0x03, "every level: Bill/Lance outdoor sprites, lives medals, power-ups, explosions"),
    (0x04, "indoor/base levels: player sprite tiles"),
    (0x05, "level 1: bridge/mountain/trees/water, player prone, flying capsule"),
    (0x06, "indoor/base: indoor player sprites, grenades, background"),
    (0x07, "level 3: background + sprite tiles, player prone, flying capsule"),
    (0x08, "indoor/base boss screen background + sprites"),
    (0x09, "level 4 boss screen sprites (3 tiles)"),
    (0x0a, "indoor/base tiles (same art as _10, unflipped)"),
    (0x0b, "level 5 pattern table tiles"),
    (0x0c, "level 6 pattern table tiles"),
    (0x0d, "level 7 pattern table tiles"),
    (0x0e, "level 8 pattern table tiles"),
    (0x0f, "indoor/base tiles (14 background tiles)"),
    (0x10, "indoor/base tiles (reuses _0a's art, horizontally flipped)"),
    (0x11, "indoor/base background tiles"),
    (0x12, "level 4 background tiles"),
    (0x13, "player aiming up/straight, laser bullets"),
    (0x14, "rotating gun + red turret tiles"),
    (0x15, "levels 5/6/7: turret man (basquez) sprites"),
    (0x16, "weapon box tiles"),
    (0x17, "ending scene: helicopter + island tiles"),
    (0x18, "ending scene nametable + attribute data"),
    (0x19, "player killed: recoil + lying on ground"),
    (0x1a, "enemy soldier sprite tiles"),
];

/// Decodes every `graphic_data_XX` blob (via `graphic_data_ptr_tbl`, so
/// offsets and the horizontal-flip flag come from the ROM itself, not a
/// hardcoded table) and writes one PNG per CHR-bound (pattern-table)
/// write segment into `out_dir`. Returns how many PNGs were written and
/// how many segments were skipped because they targeted nametable/
/// attribute memory instead (not CHR data, so there's no tile sheet to
/// render for them here).
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<(usize, usize)> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = 0usize;
    let mut skipped_non_chr = 0usize;

    for (index, description) in GRAPHIC_DATA_DESCRIPTIONS {
        let name = format!("graphic_data_{index:02x}");
        let entry = contra_native::graphics::graphic_data_prg_offset(prg_rom, index);
        let blob = prg_rom.get(entry.prg_offset..).ok_or_else(|| {
            anyhow::anyhow!("{name}: PRG offset {:#06x} is past the end of this ROM's PRG-ROM ({} bytes)", entry.prg_offset, prg_rom.len())
        })?;
        let segments = contra_native::graphics::decompress(blob, entry.flip);
        log::info!("{name} ({description}): {} segment(s){}", segments.len(), if entry.flip { ", flipped" } else { "" });

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
