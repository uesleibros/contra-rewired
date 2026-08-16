//! Debug-only verification tool (not part of the library or any shipped
//! binary): assembles an *entire level's* nametable (which tile goes
//! where) *and* attribute table (which palette applies where) across
//! every screen, plus its CHR tiles and background palettes - entirely
//! from PRG-ROM bytes, no emulation, using only the general,
//! table-driven lookups in `contra_native::{level,graphics,supertile,
//! palette}` (no per-level hardcoded offsets - level 1's used to be
//! hand-verified constants; this now derives them the same way real
//! game code does, from `level_graphic_data_tbl`/`graphic_data_ptr_tbl`/
//! `level_headers`). Renders every level to a wide, fully colored PNG.
//!
//! Level 1 additionally gets checked against `contra-nes`'s live PPU
//! state after actually playing into it (the only level reachable
//! without scripted play past obstacles) - CHR content, screen 0's
//! nametable, and screen 0's attribute table.
//!
//! ```text
//! cargo run -p contra-nes --release --example extract_level -- <rom> <out_dir>
//! ```

use std::io::BufWriter;

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};

const LEVEL_COUNT: usize = 8;
const SCREEN_COLS: usize = 8;
const SCREEN_ROWS_HORIZONTAL: usize = 7;
const SCREEN_ROWS_VERTICAL: usize = 8;
const SCREEN_TILES_W: usize = SCREEN_COLS * 4;

fn screen_rows(scrolling_type: contra_native::level::ScrollingType) -> usize {
    match scrolling_type {
        contra_native::level::ScrollingType::Horizontal => SCREEN_ROWS_HORIZONTAL,
        contra_native::level::ScrollingType::Vertical => SCREEN_ROWS_VERTICAL,
    }
}

/// Decodes and renders level `level_index` (0-based) entirely from
/// PRG-ROM, writing `level{N}_full.png` to `out_dir`. Returns the
/// assembled tile grid, screen width in tiles, and each screen's decoded
/// super-tile ID list (for the live-PPU check level 1 gets).
fn extract_and_render_level(prg_rom: &[u8], level_index: usize, out_dir: &std::path::Path) -> (Vec<u8>, usize, usize, Vec<Vec<u8>>) {
    let header = contra_native::level::level_header(prg_rom, level_index);
    let screen_rows = screen_rows(header.scrolling_type);
    let screen_supertiles = SCREEN_COLS * screen_rows;
    let screen_tiles_h = screen_rows * 4;

    let mut chr = [0u8; 0x2000];
    for entry in contra_native::graphics::level_graphic_data_entries(prg_rom, level_index) {
        contra_native::graphics::apply_chr_writes(&prg_rom[entry.prg_offset..], &mut chr, entry.flip);
    }

    let bg_group_indexes = contra_native::palette::level_palette_group_indexes(prg_rom, level_index);
    let bg_palettes: [[u32; 4]; 4] = std::array::from_fn(|i| contra_native::palette::resolve_palette_rgb(prg_rom, bg_group_indexes[i]));

    let mut all_screen_ids: Vec<Vec<u8>> = Vec::with_capacity(header.screen_count);
    for screen_index in 0..header.screen_count {
        let offset = contra_native::level::screen_prg_offset(prg_rom, &header, screen_index);
        all_screen_ids.push(contra_native::supertile::decompress_screen(&prg_rom[offset..], screen_supertiles));
    }

    let level_tiles_w = SCREEN_TILES_W * header.screen_count;
    let mut tile_grid = vec![0u8; level_tiles_w * screen_tiles_h];
    let mut palette_grid = vec![0u8; level_tiles_w * screen_tiles_h];

    for (screen_index, screen_ids) in all_screen_ids.iter().enumerate() {
        let screen_x_offset = screen_index * SCREEN_TILES_W;
        for (i, &supertile_id) in screen_ids.iter().enumerate() {
            let super_col = i % SCREEN_COLS;
            let super_row = i / SCREEN_COLS;
            let tiles = contra_native::supertile::supertile_tiles(&prg_rom[header.supertile_data_prg_offset..], supertile_id);
            let attr_byte = contra_native::supertile::supertile_attribute_byte(&prg_rom[header.palette_data_prg_offset..], supertile_id);
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

    let img_w = level_tiles_w * 8;
    let img_h = screen_tiles_h * 8;
    let mut buf = vec![0u8; img_w * img_h * 3];
    for ty in 0..screen_tiles_h {
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
    let path = out_dir.join(format!("level{}_full.png", level_index + 1));
    let file = std::fs::File::create(&path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, img_w as u32, img_h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buf).unwrap();
    println!("wrote {} ({} screens, {img_w}x{img_h}px)", path.display(), header.screen_count);

    (tile_grid, level_tiles_w, screen_tiles_h, all_screen_ids)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: extract_level <rom> <out_dir>");
    let out_dir = args.get(2).expect("usage: extract_level <rom> <out_dir>");
    std::fs::create_dir_all(out_dir).unwrap();

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    // --- Offline decode + render every level, straight from PRG-ROM,
    // fully generally (no per-level hardcoding). ---
    let mut level1_tile_grid = Vec::new();
    let mut level1_tiles_w = 0usize;
    let mut level1_screen_ids = Vec::new();
    for level_index in 0..LEVEL_COUNT {
        let (tile_grid, tiles_w, _tiles_h, screen_ids) = extract_and_render_level(&rom.prg_rom, level_index, std::path::Path::new(out_dir));
        if level_index == 0 {
            level1_tile_grid = tile_grid;
            level1_tiles_w = tiles_w;
            level1_screen_ids = screen_ids;
        }
    }

    // --- Ground truth: actually play into level 1 (the only level
    // reachable without scripted play past obstacles), read live PPU
    // state. ---
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
    let mut level1_chr = [0u8; 0x2000];
    for entry in contra_native::graphics::level_graphic_data_entries(&rom.prg_rom, 0) {
        contra_native::graphics::apply_chr_writes(&rom.prg_rom[entry.prg_offset..], &mut level1_chr, entry.flip);
    }
    let mut used_tiles: Vec<u8> = level1_tile_grid.clone();
    used_tiles.sort_unstable();
    used_tiles.dedup();
    let chr_tile_diffs = used_tiles.iter().filter(|&&tile| live_chr[tile as usize * 16..tile as usize * 16 + 16] != level1_chr[tile as usize * 16..tile as usize * 16 + 16]).count();
    if chr_tile_diffs == 0 {
        println!("MATCH: level 1 - every tile used across all its screens has CHR content identical to live CHR-RAM ({} distinct tiles checked).", used_tiles.len());
    } else {
        println!("MISMATCH: level 1 - {chr_tile_diffs}/{} distinct tiles used differ from live CHR-RAM.", used_tiles.len());
    }

    let screen_tiles_h = SCREEN_ROWS_HORIZONTAL * 4;
    let mut nametable_diffs = 0usize;
    for ty in 0..screen_tiles_h {
        for tx in 0..SCREEN_TILES_W {
            let live = nes.peek_ppu(0x2000 + (ty * 32 + tx) as u16);
            let decoded = level1_tile_grid[ty * level1_tiles_w + tx];
            if live != decoded {
                nametable_diffs += 1;
            }
        }
    }
    let mut attr_diffs = 0usize;
    let header = contra_native::level::level_header(&rom.prg_rom, 0);
    for super_row in 0..SCREEN_ROWS_HORIZONTAL {
        for super_col in 0..SCREEN_COLS {
            let supertile_id = level1_screen_ids[0][super_row * SCREEN_COLS + super_col];
            let decoded_attr = contra_native::supertile::supertile_attribute_byte(&rom.prg_rom[header.palette_data_prg_offset..], supertile_id);
            let live_attr = nes.peek_ppu(0x23C0 + (super_row * 8 + super_col) as u16);
            if decoded_attr != live_attr {
                attr_diffs += 1;
            }
        }
    }
    if nametable_diffs == 0 && attr_diffs == 0 {
        println!("MATCH: level 1 screen 0 - nametable ({} tiles) and attribute table (56 bytes) both identical to live PPU.", SCREEN_TILES_W * screen_tiles_h);
    } else {
        println!("MISMATCH: level 1 screen 0 - {nametable_diffs} nametable diffs, {attr_diffs} attribute diffs.");
    }
}
