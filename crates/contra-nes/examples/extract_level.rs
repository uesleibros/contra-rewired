//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::supertile`: assembles an *entire level's*
//! nametable (which tile goes where) *and* attribute table (which palette
//! applies where) across every screen - entirely from PRG-ROM bytes
//! (graphics + palette + super-tile decoding, no emulation) - then checks
//! the two screens actually resident in live PPU VRAM at boot (the
//! current one and the prefetched next one - Contra double-buffers scroll
//! this way) against `contra-nes`'s live state after actually playing
//! into the level. Writes one wide, fully colored PNG of the whole level.
//!
//! ```text
//! cargo run -p contra-nes --release --example extract_level -- <rom> <out_dir>
//! ```

use std::io::BufWriter;

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};

// `level_1_graphic_data`'s own literal list, confirmed byte-for-byte from
// the real ROM (bank 7, $c8fd): `03,13,19,1a,14,16,05,ff`. Do NOT add
// `graphic_data_01` (HUD letters/numbers) here - its PPU range
// ($0ce0-$1f80) overlaps this list's own tile range (e.g. graphic_data_1a
// covers up to tile 0xdb), and applying it after these 7 overwrites
// correct level tiles with HUD glyph data. This exact 7-blob list is what
// `extract_graphics.rs` already proved byte-for-byte identical to live
// CHR-RAM across the full 8KB - adding an 8th blob on top of that broke
// it, which is exactly what led to finding this.
const LEVEL_1_GRAPHIC_DATA_PRG_OFFSETS: [usize; 7] = [0x10001, 0x107a1, 0x1631b, 0x16500, 0x16814, 0x1b15c, 0x14001];

/// Bank 3, CPU `$8001` -> `3*0x4000 + (0x8001-0x8000)`.
const LEVEL_1_SUPERTILE_DATA_PRG_OFFSET: usize = 0xC001;
/// Bank 3, CPU `$8671` (`docs/rom-symbols.txt`, defined in `src/bank3.asm`)
/// -> `3*0x4000 + (0x8671-0x8000)`.
const LEVEL_1_PALETTE_DATA_PRG_OFFSET: usize = 0xC671;
/// Bank 2, CPU `$8001` (`level_1_supertiles_screen_ptr_table`) ->
/// `2*0x4000 + (0x8001-0x8000)`. 14 little-endian 2-byte pointers
/// (bank-2-relative mem addresses) follow, one per screen - confirmed by
/// reading these raw bytes directly: the first 13 resolve exactly to
/// `level_1_supertiles_screen_00`..`_0c`'s known addresses
/// (`docs/rom-symbols.txt`), and the 14th duplicates the first (a
/// defensive wrap-around entry, never a real 14th screen - `level_2`'s
/// own table starts immediately after this level's real screen data).
const LEVEL_1_SCREEN_PTR_TABLE_PRG_OFFSET: usize = 0x8001;
const LEVEL_1_SCREEN_COUNT: usize = 13;

const SCREEN_COLS: usize = 8;
const SCREEN_ROWS: usize = 7;
const SCREEN_SUPERTILES: usize = SCREEN_COLS * SCREEN_ROWS; // 0x38, horizontal level
const SCREEN_TILES_W: usize = SCREEN_COLS * 4;
const SCREEN_TILES_H: usize = SCREEN_ROWS * 4;

/// Reads screen `index`'s 2-byte pointer (a bank-2-relative mem address,
/// little-endian) out of a screen pointer table, and converts it to a
/// PRG-ROM offset (`2*0x4000 + (mem_addr & 0x3FFF)`, UxROM's switchable
/// window starting at $8000).
fn screen_prg_offset(prg_rom: &[u8], ptr_table_offset: usize, index: usize) -> usize {
    let lo = prg_rom[ptr_table_offset + index * 2];
    let hi = prg_rom[ptr_table_offset + index * 2 + 1];
    let mem_addr = u16::from_le_bytes([lo, hi]);
    2 * 0x4000 + (mem_addr as usize & 0x3FFF)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: extract_level <rom> <out_dir>");
    let out_dir = args.get(2).expect("usage: extract_level <rom> <out_dir>");
    std::fs::create_dir_all(out_dir).unwrap();

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    // --- Offline decode: CHR tiles, straight from PRG-ROM. ---
    let mut chr = [0u8; 0x2000];
    for offset in LEVEL_1_GRAPHIC_DATA_PRG_OFFSETS {
        contra_native::graphics::apply_chr_writes(&rom.prg_rom[offset..], &mut chr);
    }

    // --- Offline decode: level 1's 4 background palettes, fully resolved. ---
    let bg_group_indexes = contra_native::palette::level_palette_group_indexes(&rom.prg_rom, 0);
    let bg_palettes: [[u32; 4]; 4] = std::array::from_fn(|i| contra_native::palette::resolve_palette_rgb(&rom.prg_rom, bg_group_indexes[i]));

    // --- Offline decode: every screen's super-tile layout, and the full
    // level's tile/palette grid assembled from them side by side. ---
    let mut all_screen_ids: Vec<Vec<u8>> = Vec::with_capacity(LEVEL_1_SCREEN_COUNT);
    for screen_index in 0..LEVEL_1_SCREEN_COUNT {
        let offset = screen_prg_offset(&rom.prg_rom, LEVEL_1_SCREEN_PTR_TABLE_PRG_OFFSET, screen_index);
        let ids = contra_native::supertile::decompress_screen(&rom.prg_rom[offset..], SCREEN_SUPERTILES);
        all_screen_ids.push(ids);
    }

    let level_tiles_w = SCREEN_TILES_W * LEVEL_1_SCREEN_COUNT;
    let mut tile_grid = vec![0u8; level_tiles_w * SCREEN_TILES_H];
    let mut palette_grid = vec![0u8; level_tiles_w * SCREEN_TILES_H];

    for (screen_index, screen_ids) in all_screen_ids.iter().enumerate() {
        let screen_x_offset = screen_index * SCREEN_TILES_W;
        for (i, &supertile_id) in screen_ids.iter().enumerate() {
            let super_col = i % SCREEN_COLS;
            let super_row = i / SCREEN_COLS;
            let tiles = contra_native::supertile::supertile_tiles(&rom.prg_rom[LEVEL_1_SUPERTILE_DATA_PRG_OFFSET..], supertile_id);
            let attr_byte = contra_native::supertile::supertile_attribute_byte(&rom.prg_rom[LEVEL_1_PALETTE_DATA_PRG_OFFSET..], supertile_id);
            let quadrants = contra_native::supertile::attribute_quadrants(attr_byte);

            for local in 0..16 {
                let local_col = local % 4;
                let local_row = local / 4;
                let tx = screen_x_offset + super_col * 4 + local_col;
                let ty = super_row * 4 + local_row;
                tile_grid[ty * level_tiles_w + tx] = tiles[local];
                let quadrant = (local_row / 2) * 2 + (local_col / 2); // 0=TL,1=TR,2=BL,3=BR
                palette_grid[ty * level_tiles_w + tx] = quadrants[quadrant];
            }
        }
    }

    // --- Render the whole level, fully colored, from extracted assets
    // alone. ---
    let img_w = level_tiles_w * 8;
    let img_h = SCREEN_TILES_H * 8;
    let mut buf = vec![0u8; img_w * img_h * 3];
    for ty in 0..SCREEN_TILES_H {
        for tx in 0..level_tiles_w {
            let tile = tile_grid[ty * level_tiles_w + tx] as usize;
            let palette = bg_palettes[palette_grid[ty * level_tiles_w + tx] as usize];
            let base = tile * 16;
            let plane0 = &chr[base..base + 8];
            let plane1 = &chr[base + 8..base + 16];
            for row in 0..8 {
                for col in 0..8 {
                    let bit = 7 - col;
                    let lo = (plane0[row] >> bit) & 1;
                    let hi = (plane1[row] >> bit) & 1;
                    let rgb = palette[(lo | (hi << 1)) as usize];
                    let px = (ty * 8 + row) * img_w + tx * 8 + col;
                    buf[px * 3] = ((rgb >> 16) & 0xFF) as u8;
                    buf[px * 3 + 1] = ((rgb >> 8) & 0xFF) as u8;
                    buf[px * 3 + 2] = (rgb & 0xFF) as u8;
                }
            }
        }
    }
    let path = std::path::Path::new(out_dir).join("level1_full.png");
    let file = std::fs::File::create(&path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, img_w as u32, img_h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buf).unwrap();
    println!("wrote {} ({} screens, {img_w}x{img_h}px)", path.display(), LEVEL_1_SCREEN_COUNT);

    // --- Ground truth: actually play into level 1, read live PPU state.
    // Contra double-buffers 2 screens' worth of nametable at a time (the
    // current one plus the prefetched next one), so both screen 0 and
    // screen 1 should already be resident in VRAM at boot without any
    // scrolling input needed. ---
    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let mut nes = Nes::new(rom.prg_rom.clone(), mirroring);
    let start_after = 120u32;
    for frame in 0..start_after + 900 {
        let buttons = if frame >= start_after && frame < start_after + 10 {
            BUTTON_START
        } else if frame >= start_after + 40 && frame < start_after + 50 {
            BUTTON_START
        } else {
            0
        };
        nes.set_controller(0, buttons);
        nes.run_frame();
    }

    let live_chr = nes.bus.ppu.chr_ram;
    let mut used_tiles: Vec<u8> = tile_grid.clone();
    used_tiles.sort_unstable();
    used_tiles.dedup();
    let chr_tile_diffs = used_tiles.iter().filter(|&&tile| chr[tile as usize * 16..tile as usize * 16 + 16] != live_chr[tile as usize * 16..tile as usize * 16 + 16]).count();
    if chr_tile_diffs == 0 {
        println!("MATCH: every tile used across all {LEVEL_1_SCREEN_COUNT} screens has CHR content identical to live CHR-RAM ({} distinct tiles checked).", used_tiles.len());
    } else {
        println!("MISMATCH: {chr_tile_diffs}/{} distinct tiles used across the level differ from live CHR-RAM.", used_tiles.len());
    }

    // Screen 0 at nametable base $2000; screen 1 empirically resolved
    // below by trying both plausible nametable bases and reporting which
    // one actually matches, rather than assuming.
    verify_screen_against_live(&nes, 0, &tile_grid, &all_screen_ids[0], &rom.prg_rom, 0x2000, 0x23C0);

    let screen1_base_2400_diffs = count_nametable_diffs(&nes, 1, &tile_grid, level_tiles_w, 0x2400);
    let screen1_base_2000_scrolled_diffs = count_nametable_diffs(&nes, 1, &tile_grid, level_tiles_w, 0x2000);
    if screen1_base_2400_diffs == 0 {
        verify_screen_against_live(&nes, 1, &tile_grid, &all_screen_ids[1], &rom.prg_rom, 0x2400, 0x27C0);
    } else if screen1_base_2000_scrolled_diffs == 0 {
        verify_screen_against_live(&nes, 1, &tile_grid, &all_screen_ids[1], &rom.prg_rom, 0x2000, 0x23C0);
    } else {
        println!(
            "screen 1: neither nametable-base guess matched live PPU ($2400 base: {screen1_base_2400_diffs} diffs, $2000 base: {screen1_base_2000_scrolled_diffs} diffs) - not verified this run."
        );
    }
}

fn count_nametable_diffs(nes: &Nes, screen_index: usize, tile_grid: &[u8], level_tiles_w: usize, nametable_base: u16) -> usize {
    let screen_x_offset = screen_index * SCREEN_TILES_W;
    let mut diffs = 0usize;
    for ty in 0..SCREEN_TILES_H {
        for tx in 0..SCREEN_TILES_W {
            let live = nes.peek_ppu(nametable_base + (ty * 32 + tx) as u16);
            let decoded = tile_grid[ty * level_tiles_w + screen_x_offset + tx];
            if live != decoded {
                diffs += 1;
            }
        }
    }
    diffs
}

#[allow(clippy::too_many_arguments)]
fn verify_screen_against_live(nes: &Nes, screen_index: usize, tile_grid: &[u8], screen_ids: &[u8], prg_rom: &[u8], nametable_base: u16, attr_base: u16) {
    let level_tiles_w = SCREEN_TILES_W * LEVEL_1_SCREEN_COUNT;
    let screen_x_offset = screen_index * SCREEN_TILES_W;
    let mut nametable_diffs = 0usize;
    for ty in 0..SCREEN_TILES_H {
        for tx in 0..SCREEN_TILES_W {
            let live = nes.peek_ppu(nametable_base + (ty * 32 + tx) as u16);
            let decoded = tile_grid[ty * level_tiles_w + screen_x_offset + tx];
            if live != decoded {
                nametable_diffs += 1;
            }
        }
    }

    let mut attr_diffs = 0usize;
    for super_row in 0..SCREEN_ROWS {
        for super_col in 0..SCREEN_COLS {
            let supertile_id = screen_ids[super_row * SCREEN_COLS + super_col];
            let decoded_attr = contra_native::supertile::supertile_attribute_byte(&prg_rom[LEVEL_1_PALETTE_DATA_PRG_OFFSET..], supertile_id);
            let live_attr = nes.peek_ppu(attr_base + (super_row * 8 + super_col) as u16);
            if decoded_attr != live_attr {
                attr_diffs += 1;
            }
        }
    }

    if nametable_diffs == 0 && attr_diffs == 0 {
        println!("MATCH: screen {screen_index} (nametable base ${nametable_base:04X}) - nametable ({} tiles) and attribute table ({SCREEN_SUPERTILES} bytes) both identical to live PPU.", SCREEN_TILES_W * SCREEN_TILES_H);
    } else {
        println!("MISMATCH: screen {screen_index} (nametable base ${nametable_base:04X}) - {nametable_diffs} nametable diffs, {attr_diffs} attribute diffs.");
    }
}
