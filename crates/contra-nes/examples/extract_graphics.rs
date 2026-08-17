//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::graphics`: decompresses Level 1's "graphic
//! data" blobs directly from PRG-ROM - no emulation involved, exactly the
//! way a real offline asset extractor would - and checks the result
//! byte-for-byte against `contra-nes`'s own CHR-RAM after actually playing
//! into level 1, which is populated by running the *real* 6502
//! `write_graphic_data_to_ppu` routine. Also writes both as PNG tile
//! sheets for a visual sanity check.
//!
//! ```text
//! cargo run -p contra-nes --release --example extract_graphics -- <rom> <out_dir>
//! ```

use std::io::BufWriter;

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};

/// `graphic_data_XX` -> exact PRG-ROM byte offset, taken directly from
/// `docs/Graphics Documentation.md`'s "Graphics Data Locations" table in
/// `vermiceli/nes-contra-us` (the "PRG ROM Address" column - already an
/// offset into raw PRG-ROM, no iNES header to add since
/// `contra_assets::NesRom` strips it). This is the set
/// `level_1_graphic_data` loads, in order: `#$03, #$13, #$19, #$1a, #$14,
/// #$16, #$05, #$ff`.
const LEVEL_1_GRAPHIC_DATA_PRG_OFFSETS: [(&str, usize); 7] = [
    ("graphic_data_03", 0x10001),
    ("graphic_data_13", 0x107a1),
    ("graphic_data_19", 0x1631b),
    ("graphic_data_1a", 0x16500),
    ("graphic_data_14", 0x16814),
    ("graphic_data_16", 0x1b15c),
    ("graphic_data_05", 0x14001),
];

fn save_chr_sheet(path: &std::path::Path, chr: &[u8; 0x2000]) {
    // 2 pattern tables side by side, each 16x16 tiles of 8x8px -> 256x128,
    // stacked: left table on top, right table below, for a 256x256 image.
    const TILES_PER_ROW: usize = 16;
    const SHEET_W: usize = TILES_PER_ROW * 8;
    const SHEET_H: usize = 32 * 8;
    let mut buf = vec![0u8; SHEET_W * SHEET_H];

    for tile in 0..512usize {
        let base = tile * 16;
        let plane0 = &chr[base..base + 8];
        let plane1 = &chr[base + 8..base + 16];
        let tile_x = (tile % TILES_PER_ROW) * 8;
        let tile_y = (tile / TILES_PER_ROW) * 8;
        for row in 0..8 {
            for col in 0..8 {
                let bit = 7 - col;
                let lo = (plane0[row] >> bit) & 1;
                let hi = (plane1[row] >> bit) & 1;
                let value = lo | (hi << 1); // 0..=3
                let gray = value * 85; // 0,85,170,255
                buf[(tile_y + row) * SHEET_W + tile_x + col] = gray;
            }
        }
    }

    let file = std::fs::File::create(path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, SHEET_W as u32, SHEET_H as u32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buf).unwrap();
}

fn save_chr_sheet_colored(path: &std::path::Path, chr: &[u8; 0x2000], palette: [u32; 4]) {
    const TILES_PER_ROW: usize = 16;
    const SHEET_W: usize = TILES_PER_ROW * 8;
    const SHEET_H: usize = 32 * 8;
    let mut buf = vec![0u8; SHEET_W * SHEET_H * 3];

    for tile in 0..512usize {
        let base = tile * 16;
        let plane0 = &chr[base..base + 8];
        let plane1 = &chr[base + 8..base + 16];
        let tile_x = (tile % TILES_PER_ROW) * 8;
        let tile_y = (tile / TILES_PER_ROW) * 8;
        for row in 0..8 {
            for col in 0..8 {
                let bit = 7 - col;
                let lo = (plane0[row] >> bit) & 1;
                let hi = (plane1[row] >> bit) & 1;
                let rgb = palette[(lo | (hi << 1)) as usize];
                let px = (tile_y + row) * SHEET_W + tile_x + col;
                buf[px * 3] = ((rgb >> 16) & 0xFF) as u8;
                buf[px * 3 + 1] = ((rgb >> 8) & 0xFF) as u8;
                buf[px * 3 + 2] = (rgb & 0xFF) as u8;
            }
        }
    }

    let file = std::fs::File::create(path).unwrap();
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, SHEET_W as u32, SHEET_H as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&buf).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: extract_graphics <rom> <out_dir>");
    let out_dir = args.get(2).expect("usage: extract_graphics <rom> <out_dir>");
    std::fs::create_dir_all(out_dir).unwrap();

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    // --- Offline decode: straight from PRG-ROM, no emulation. ---
    let mut decoded_chr = [0u8; 0x2000];
    for (name, offset) in LEVEL_1_GRAPHIC_DATA_PRG_OFFSETS {
        let blob = &rom.prg_rom[offset..];
        contra_native::world::graphics::apply_chr_writes(blob, &mut decoded_chr, false);
        eprintln!("decoded {name} @ prg[{offset:#06x}]");
    }
    save_chr_sheet(&std::path::Path::new(out_dir).join("decoded_chr.png"), &decoded_chr);

    // --- Ground truth: actually play into level 1 and read live CHR-RAM. ---
    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let mut nes = Nes::new(rom.prg_rom.clone(), mirroring);
    let start_after = 120u32;
    let total_frames = start_after + 900;
    for frame in 0..total_frames {
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
    save_chr_sheet(&std::path::Path::new(out_dir).join("live_chr.png"), &live_chr);

    // --- Compare. ---
    let mut diffs = 0usize;
    let mut first_diffs = Vec::new();
    for i in 0..0x2000 {
        if decoded_chr[i] != live_chr[i] {
            diffs += 1;
            if first_diffs.len() < 16 {
                first_diffs.push((i, decoded_chr[i], live_chr[i]));
            }
        }
    }

    if diffs == 0 {
        println!("MATCH: decoded CHR is byte-for-byte identical to live CHR-RAM ($0000-$1FFF, {} bytes checked).", 0x2000);
    } else {
        println!("MISMATCH: {diffs}/{} bytes differ.", 0x2000);
        for (addr, expected, actual) in &first_diffs {
            println!("  ${addr:04x}: decoded={expected:#04x} live={actual:#04x}");
        }
    }

    // --- Palette: cross-check the master palette copy, then verify level
    // 1's background palette group 0 (read from PRG-ROM, no compression)
    // against the PPU's actual live palette RAM after the same play session.
    assert_eq!(
        contra_native::world::palette::NES_MASTER_PALETTE,
        contra_nes::ppu::NES_PALETTE,
        "contra-native's standalone master-palette copy has drifted from contra-nes's"
    );
    let bg_group_indexes = contra_native::world::palette::level_palette_group_indexes(&rom.prg_rom, 0);
    let decoded_bg0 = contra_native::world::palette::resolve_palette_rgb(&rom.prg_rom, bg_group_indexes[0]);
    let live_palette = nes.bus.ppu.palette;
    let live_bg0: [u32; 4] = std::array::from_fn(|i| contra_native::world::palette::NES_MASTER_PALETTE[(live_palette[i] & 0x3F) as usize]);
    if decoded_bg0 == live_bg0 {
        println!("MATCH: decoded level 1 background palette 0 is identical to live PPU palette RAM ($3F00-$3F03).");
    } else {
        println!("MISMATCH: decoded level 1 background palette 0 {decoded_bg0:08X?} != live {live_bg0:08X?}");
    }
    save_chr_sheet_colored(&std::path::Path::new(out_dir).join("decoded_chr_colored.png"), &decoded_chr, decoded_bg0);
}
