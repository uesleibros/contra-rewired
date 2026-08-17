//! Wires `contra_native::palette` into a real extraction command: renders
//! every `game_palettes` group as a small color swatch, straight from
//! PRG-ROM, no emulation. See `crates/contra-native/src/palette.rs`'s doc
//! comment for the format, and
//! `crates/contra-nes/examples/extract_graphics.rs` for the cross-check
//! that proved it byte-for-byte identical to live PPU palette RAM for
//! level 1's background palette 0.

const GROUP_COUNT: usize = contra_native::world::palette::GAME_PALETTES_LEN / 3;

/// Renders every `game_palettes` group as a 32x32 swatch (4 stacked 8px
/// stripes: hard-coded black, then the group's 3 colors, top to bottom),
/// arranged in an 11-column grid, into `<out_dir>/game_palettes.png`.
/// Returns how many groups were rendered.
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<usize> {
    std::fs::create_dir_all(out_dir)?;

    const COLS: usize = 11;
    const CELL: usize = 32;
    let rows = GROUP_COUNT.div_ceil(COLS);
    let w = COLS * CELL;
    let h = rows * CELL;
    let mut buf = vec![0u8; w * h * 3];

    for group in 0..GROUP_COUNT {
        let colors = contra_native::world::palette::resolve_palette_rgb(prg_rom, group as u8);
        let cell_x = (group % COLS) * CELL;
        let cell_y = (group / COLS) * CELL;
        for (stripe, &rgb) in colors.iter().enumerate() {
            for y in 0..8 {
                for x in 0..CELL {
                    let px = (cell_y + stripe * 8 + y) * w + cell_x + x;
                    buf[px * 3] = ((rgb >> 16) & 0xFF) as u8;
                    buf[px * 3 + 1] = ((rgb >> 8) & 0xFF) as u8;
                    buf[px * 3 + 2] = (rgb & 0xFF) as u8;
                }
            }
        }
    }

    let path = out_dir.join("game_palettes.png");
    let file = std::fs::File::create(&path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&buf)?;

    Ok(GROUP_COUNT)
}
