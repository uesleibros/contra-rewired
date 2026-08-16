//! Wires `contra_native::{level,graphics,supertile,palette}` into a real
//! extraction command: renders each of the 8 levels' full nametable
//! (every screen, side by side) to a single colored PNG, straight from
//! PRG-ROM, no emulation. See `crates/contra-nes/examples/extract_level.rs`
//! for the live-PPU verification that proved this pipeline byte-perfect
//! for level 1 (CHR content, nametable, and attribute table all matching
//! `contra-nes` after actually playing into the level) - this command is
//! the same logic, generalized to all 8 levels via the ROM's own
//! `level_graphic_data_tbl`/`graphic_data_ptr_tbl`/`level_headers` tables
//! rather than level-1-only hardcoded offsets.

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

/// Renders all 8 levels to `<out_dir>/level{1..8}_full.png`. Returns how
/// many were written.
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<usize> {
    std::fs::create_dir_all(out_dir)?;
    for level_index in 0..8 {
        render_level(prg_rom, level_index, out_dir)?;
    }
    Ok(8)
}

fn render_level(prg_rom: &[u8], level_index: usize, out_dir: &std::path::Path) -> anyhow::Result<()> {
    let header = contra_native::level::level_header(prg_rom, level_index);
    let rows = screen_rows(header.scrolling_type);
    let screen_supertiles = SCREEN_COLS * rows;
    let screen_tiles_h = rows * 4;

    let mut chr = [0u8; 0x2000];
    for entry in contra_native::graphics::level_graphic_data_entries(prg_rom, level_index) {
        contra_native::graphics::apply_chr_writes(&prg_rom[entry.prg_offset..], &mut chr, entry.flip);
    }

    let bg_group_indexes = contra_native::palette::level_palette_group_indexes(prg_rom, level_index);
    let bg_palettes: [[u32; 4]; 4] = std::array::from_fn(|i| contra_native::palette::resolve_palette_rgb(prg_rom, bg_group_indexes[i]));

    let level_tiles_w = SCREEN_TILES_W * header.screen_count;
    let mut tile_grid = vec![0u8; level_tiles_w * screen_tiles_h];
    let mut palette_grid = vec![0u8; level_tiles_w * screen_tiles_h];

    for screen_index in 0..header.screen_count {
        let offset = contra_native::level::screen_prg_offset(prg_rom, &header, screen_index);
        let screen_ids = contra_native::supertile::decompress_screen(&prg_rom[offset..], screen_supertiles);
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
    let file = std::fs::File::create(&path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, img_w as u32, img_h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&buf)?;
    log::info!("wrote {} ({} screens, {img_w}x{img_h}px)", path.display(), header.screen_count);
    Ok(())
}
