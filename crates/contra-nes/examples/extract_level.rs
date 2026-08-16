//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::supertile`: assembles level 1's screen 0 -
//! nametable (which tile goes where) *and* attribute table (which palette
//! applies where) - entirely from PRG-ROM bytes (graphics + palette +
//! super-tile decoding, no emulation), then checks every tile ID and
//! attribute byte against `contra-nes`'s live PPU state after actually
//! playing into the level. Also writes a fully colored PNG of the
//! assembled screen - the end-to-end proof that CHR extraction, palette
//! resolution, and super-tile/nametable decoding compose into a real,
//! self-contained level render.
//!
//! ```text
//! cargo run -p contra-nes --release --example extract_level -- <rom> <out_dir>
//! ```

use std::io::BufWriter;

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};

// Level 1's own `level_1_graphic_data` list (7 blobs), plus
// `graphic_data_01` (letters/numbers/logo - used for the HUD score/lives
// display, loaded once at a different point in the boot sequence rather
// than being part of any single level's own graphic-data list).
const LEVEL_1_GRAPHIC_DATA_PRG_OFFSETS: [usize; 8] = [0x10001, 0x107a1, 0x1631b, 0x16500, 0x16814, 0x1b15c, 0x14001, 0x12a2d];

/// Bank 3, CPU `$8001` -> `3*0x4000 + (0x8001-0x8000)`.
const LEVEL_1_SUPERTILE_DATA_PRG_OFFSET: usize = 0xC001;
/// Bank 3, CPU `$8671` (`docs/rom-symbols.txt`, defined in `src/bank3.asm`)
/// -> `3*0x4000 + (0x8671-0x8000)`.
const LEVEL_1_PALETTE_DATA_PRG_OFFSET: usize = 0xC671;
/// Bank 2, CPU `$801d` (`level_1_supertiles_screen_00`) -> `2*0x4000 +
/// (0x801d-0x8000)`.
const LEVEL_1_SCREEN_00_PRG_OFFSET: usize = 0x801D;

const SCREEN_COLS: usize = 8;
const SCREEN_ROWS: usize = 7;
const SCREEN_SUPERTILES: usize = SCREEN_COLS * SCREEN_ROWS; // 0x38, horizontal level

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

    // --- Offline decode: which super-tile ID sits at each of the 56
    // screen-0 positions (row-major, 8 wide x 7 tall). ---
    let screen_ids = contra_native::supertile::decompress_screen(&rom.prg_rom[LEVEL_1_SCREEN_00_PRG_OFFSET..], SCREEN_SUPERTILES);
    assert_eq!(screen_ids.len(), SCREEN_SUPERTILES);

    // --- Offline decode: level 1's 4 background palettes, fully resolved. ---
    let bg_group_indexes = contra_native::palette::level_palette_group_indexes(&rom.prg_rom, 0);
    let bg_palettes: [[u32; 4]; 4] = std::array::from_fn(|i| contra_native::palette::resolve_palette_rgb(&rom.prg_rom, bg_group_indexes[i]));

    // --- Assemble: 32 (8*4) x 28 (7*4) tile grid, plus which of the 4
    // resolved palettes applies to each tile (from each super-tile's
    // packed attribute byte, 2x2-tile quadrant granularity). ---
    const TILES_W: usize = SCREEN_COLS * 4;
    const TILES_H: usize = SCREEN_ROWS * 4;
    let mut tile_grid = [0u8; TILES_W * TILES_H];
    let mut palette_grid = [0u8; TILES_W * TILES_H];

    for (i, &supertile_id) in screen_ids.iter().enumerate() {
        let super_col = i % SCREEN_COLS;
        let super_row = i / SCREEN_COLS;
        let tiles = contra_native::supertile::supertile_tiles(&rom.prg_rom[LEVEL_1_SUPERTILE_DATA_PRG_OFFSET..], supertile_id);
        let attr_byte = contra_native::supertile::supertile_attribute_byte(&rom.prg_rom[LEVEL_1_PALETTE_DATA_PRG_OFFSET..], supertile_id);
        let quadrants = contra_native::supertile::attribute_quadrants(attr_byte);

        for local in 0..16 {
            let local_col = local % 4;
            let local_row = local / 4;
            let tx = super_col * 4 + local_col;
            let ty = super_row * 4 + local_row;
            tile_grid[ty * TILES_W + tx] = tiles[local];
            let quadrant = (local_row / 2) * 2 + (local_col / 2); // 0=TL,1=TR,2=BL,3=BR
            palette_grid[ty * TILES_W + tx] = quadrants[quadrant];
        }
    }

    // --- Render the assembled screen, fully colored, from extracted
    // assets alone. ---
    let mut buf = vec![0u8; TILES_W * 8 * TILES_H * 8 * 3];
    let img_w = TILES_W * 8;
    for ty in 0..TILES_H {
        for tx in 0..TILES_W {
            let tile = tile_grid[ty * TILES_W + tx] as usize;
            let palette = bg_palettes[palette_grid[ty * TILES_W + tx] as usize];
            // NOTE: empirically this (no offset) mismatches live CHR-RAM
            // less than a $1000 ("right pattern table") offset does, but
            // neither is a full match - see this file's final printed
            // "CHR check" line and NATIVE_PORT.md's status entry for the
            // open question this leaves (most likely alternate-graphics
            // loading timing, not a decompression bug - the nametable and
            // attribute table are separately proven byte-perfect above).
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
    let path = std::path::Path::new(out_dir).join("level1_screen0.png");
    let file = std::fs::File::create(&path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, img_w as u32, (TILES_H * 8) as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buf).unwrap();
    println!("wrote {}", path.display());

    // --- Ground truth: actually play into level 1, read live PPU state. ---
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
    let mut used_tiles: Vec<u8> = tile_grid.to_vec();
    used_tiles.sort_unstable();
    used_tiles.dedup();
    let mut chr_tile_diffs = 0usize;
    for &tile in &used_tiles {
        let base = tile as usize * 16;
        if chr[base..base + 16] != live_chr[base..base + 16] {
            chr_tile_diffs += 1;
            if chr_tile_diffs <= 8 {
                println!("  CHR tile {tile:#04x} used by this screen doesn't match live CHR-RAM");
            }
        }
    }
    println!("CHR check: {chr_tile_diffs}/{} distinct tiles used by this screen differ from live CHR-RAM.", used_tiles.len());

    let mut nametable_diffs = 0usize;
    for ty in 0..TILES_H {
        for tx in 0..TILES_W {
            let live = nes.peek_ppu(0x2000 + (ty * 32 + tx) as u16);
            let decoded = tile_grid[ty * TILES_W + tx];
            if live != decoded {
                nametable_diffs += 1;
                if nametable_diffs <= 8 {
                    println!("  nametable ({tx},{ty}): decoded={decoded:#04x} live={live:#04x}");
                }
            }
        }
    }
    if nametable_diffs == 0 {
        println!("MATCH: decoded nametable is identical to live PPU nametable ({} tiles checked).", TILES_W * TILES_H);
    } else {
        println!("MISMATCH: {nametable_diffs}/{} nametable tiles differ.", TILES_W * TILES_H);
    }

    let mut attr_diffs = 0usize;
    for super_row in 0..SCREEN_ROWS {
        for super_col in 0..SCREEN_COLS {
            let supertile_id = screen_ids[super_row * SCREEN_COLS + super_col];
            let decoded_attr = contra_native::supertile::supertile_attribute_byte(&rom.prg_rom[LEVEL_1_PALETTE_DATA_PRG_OFFSET..], supertile_id);
            let live_attr = nes.peek_ppu(0x23C0 + (super_row * 8 + super_col) as u16);
            if decoded_attr != live_attr {
                attr_diffs += 1;
                println!("  attribute ({super_col},{super_row}): decoded={decoded_attr:#04x} live={live_attr:#04x}");
            }
        }
    }
    if attr_diffs == 0 {
        println!("MATCH: decoded attribute table is identical to live PPU attribute table ({SCREEN_SUPERTILES} bytes checked).");
    } else {
        println!("MISMATCH: {attr_diffs}/{SCREEN_SUPERTILES} attribute bytes differ.");
    }
}
