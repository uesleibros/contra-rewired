//! Debug-only tool (not part of the library or any shipped binary): runs a
//! user-supplied ROM for a fixed number of frames, presses Start the way
//! Contra's title screen actually requires (see the comment below), and
//! writes PNG snapshots at intervals so emulator behavior can be inspected
//! without a GUI. `DEBUG_RAM=1` additionally traces a few well-known RAM
//! addresses every 10 frames (game routine index, controller state/diff,
//! demo-mode flag, PPU mask/ctrl); `DEBUG_OAM=1` dumps OAM occupancy every
//! 30 frames; `RAM_DUMP_FRAME=N` writes the full 2KB RAM to `ram_NNNN.bin`
//! in `out_dir` at that frame, for diffing two runs byte-for-byte (`cmp -l`)
//! when a RAM trace alone can't localize what's actually different. Not
//! built or run in CI; invoked manually during development:
//!
//! ```text
//! cargo run -p contra-nes --release --example dump_frames -- <rom> <out_dir> [frames] [start_after]
//! ```

use std::collections::HashMap;
use std::io::BufWriter;

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};

/// Draws a 1px red rectangle outline directly into a copy of the
/// framebuffer - used by `HITBOXES=1` to visually verify the same OAM
/// bounding-box math `contra-pc`'s hitbox overlay uses (see
/// `apps/contra-pc/src/main.rs::redraw`), without needing a GUI.
fn draw_rect_outline(buf: &mut [u32], w: usize, h: usize, x: i32, y: i32, bw: i32, bh: i32, color: u32) {
    for dx in 0..bw {
        for &dy in &[0, bh - 1] {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                buf[py as usize * w + px as usize] = color;
            }
        }
    }
    for dy in 0..bh {
        for &dx in &[0, bw - 1] {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && (px as usize) < w && (py as usize) < h {
                buf[py as usize * w + px as usize] = color;
            }
        }
    }
}

fn save_png(path: &std::path::Path, fb: &[u32], w: usize, h: usize) {
    let file = std::fs::File::create(path).unwrap();
    let w_buf = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w_buf, w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    let mut data = vec![0u8; w * h * 3];
    for (i, px) in fb.iter().enumerate() {
        data[i * 3] = ((px >> 16) & 0xFF) as u8;
        data[i * 3 + 1] = ((px >> 8) & 0xFF) as u8;
        data[i * 3 + 2] = (px & 0xFF) as u8;
    }
    writer.write_image_data(&data).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: dump_frames <rom> <out_dir> [frames] [start_after]");
    let out_dir = args.get(2).expect("usage: dump_frames <rom> <out_dir> [frames] [start_after]");
    let frame_count: u32 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(600);
    let start_after: u32 = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(120);
    let save_every: u32 = std::env::var("SAVE_EVERY").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let save_from: u32 = std::env::var("SAVE_FROM").ok().and_then(|s| s.parse().ok()).unwrap_or(0);

    std::fs::create_dir_all(out_dir).unwrap();

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);
    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let mut nes = Nes::new(rom.prg_rom, mirroring);
    if let Ok(px) = std::env::var("WIDE_PX") {
        nes.set_wide_width(px.parse().unwrap());
    } else if std::env::var("WIDE").is_ok() {
        nes.set_wide_width(contra_nes::EXTENDED_WIDTH);
    }
    if std::env::var("UNLIMITED_SPRITES").is_ok() {
        nes.set_unlimited_sprites(true);
    }

    let mut illegal_seen: HashMap<u8, u32> = HashMap::new();
    let mut saved = 0;

    for frame in 0..frame_count {
        // Real Contra's title screen needs Start pressed twice: once to
        // skip the scroll-in intro animation early (this alone does NOT
        // start a game - it just fast-forwards to the "PLAY SELECT" menu),
        // and once more, afterwards, to actually set GAME_ROUTINE_INDEX to
        // "start game" (see bank7.asm's dec_theme_delay_check_user_input).
        // A single press just lands on the menu and eventually times out
        // into the attract-mode demo, which is what earlier runs of this
        // tool were accidentally exercising. After the game truly starts,
        // walk right and hop/shoot periodically.
        let buttons = if frame >= start_after && frame < start_after + 10 {
            BUTTON_START
        } else if frame >= start_after + 40 && frame < start_after + 50 {
            BUTTON_START
        } else if frame >= start_after + 120 {
            let mut b = BUTTON_RIGHT | BUTTON_B;
            if frame % 40 < 4 {
                b |= BUTTON_A;
            }
            b
        } else {
            0
        };
        nes.set_controller(0, buttons);
        // JUMP_STAGE=N: verification hook for the stage-select feature
        // (`apps/contra-pc`'s Debug tab) - pokes the same two addresses
        // (CURRENT_LEVEL, LEVEL_ROUTINE_INDEX) once, partway through the
        // run, so a screenshot can confirm the level actually changed.
        if let Ok(stage) = std::env::var("JUMP_STAGE") {
            if frame == start_after + 700 {
                // Mirrors level_routine_05's own transition, not just the
                // two "which level" bytes: it clears $40-$f0 and $300-$5ff
                // (enemy/object/sprite-buffer state) before moving on, so
                // level_routine_00 starts from a clean slate instead of
                // whatever the previous level's entities left behind.
                for addr in 0x40..=0xF0u16 {
                    nes.poke_ram(addr, 0);
                }
                for addr in 0x300..0x600u16 {
                    nes.poke_ram(addr, 0);
                }
                nes.poke_ram(0x30, stage.parse().unwrap());
                nes.poke_ram(0x2C, 0);
                eprintln!(
                    "jump: game_routine=${:02X} level_routine=${:02X} current_level=${:02X}",
                    nes.bus.ram[0x18],
                    nes.bus.ram[0x2C],
                    nes.bus.ram[0x30],
                );
            }
        }
        nes.run_frame();

        if let Some(op) = nes.cpu.illegal_opcode_hit {
            *illegal_seen.entry(op).or_insert(0) += 1;
            nes.cpu.illegal_opcode_hit = None;
        }

        if frame >= save_from && (frame % save_every == 0 || frame == frame_count - 1) {
            let path = std::path::Path::new(out_dir).join(format!("frame_{frame:04}.png"));
            let wide = nes.wide_width() > contra_nes::SCREEN_W;
            let (w, h) = (if wide { nes.wide_width() } else { contra_nes::SCREEN_W }, contra_nes::SCREEN_H);
            let mut buf = if wide { nes.wide_framebuffer().to_vec() } else { nes.framebuffer().to_vec() };
            if std::env::var("HITBOXES").is_ok() {
                let x_offset = nes.wide_x_offset();
                let height = nes.sprite_height();
                for i in 0..64 {
                    let oam_y = nes.bus.ppu.oam[i * 4];
                    if oam_y >= 0xEF {
                        continue;
                    }
                    let x = nes.bus.ppu.oam[i * 4 + 3] as i32 + x_offset;
                    let y = oam_y as i32 + 1;
                    draw_rect_outline(&mut buf, w, h, x, y, 8, height, 0x00FF4040);
                }
            }
            save_png(&path, &buf, w, h);
            saved += 1;
        }

        if std::env::var("DEBUG_RAM").is_ok() && frame % 10 == 0 {
            eprintln!(
                "frame={frame} routine=${:02X} level_routine=${:02X} current_level=${:02X} ctrl_state=${:02X} ctrl_diff=${:02X} demo=${:02X} mask=${:02X} ctrl=${:02X}",
                nes.bus.ram[0x18],
                nes.bus.ram[0x2c],
                nes.bus.ram[0x30],
                nes.bus.ram[0xf1],
                nes.bus.ram[0xf5],
                nes.bus.ram[0x1c],
                nes.bus.ppu.mask,
                nes.bus.ppu.ctrl,
            );
        }
        if let Ok(target) = std::env::var("RAM_DUMP_FRAME") {
            if frame == target.parse().unwrap() {
                std::fs::write(std::path::Path::new(out_dir).join(format!("ram_{frame:04}.bin")), &nes.bus.ram[..]).unwrap();
            }
        }
        if std::env::var("DEBUG_OAM").is_ok() && frame % 30 == 0 {
            let mut visible = 0;
            for i in 0..64 {
                let y = nes.bus.ppu.oam[i * 4];
                let tile = nes.bus.ppu.oam[i * 4 + 1];
                let x = nes.bus.ppu.oam[i * 4 + 3];
                if y < 0xEF {
                    visible += 1;
                    if visible <= 5 {
                        eprintln!("  frame={frame} sprite[{i}] y={y} tile={tile:02X} x={x}");
                    }
                }
            }
            eprintln!("frame={frame} visible_sprites={visible}");
        }
    }

    eprintln!("saved {saved} frames to {out_dir}");
    if !illegal_seen.is_empty() {
        eprintln!("illegal opcodes hit: {illegal_seen:?}");
    } else {
        eprintln!("no illegal opcodes hit");
    }
    eprintln!(
        "final cpu state: pc={:04X} a={:02X} x={:02X} y={:02X} sp={:02X} status={:08b} cycles={}",
        nes.cpu.pc, nes.cpu.a, nes.cpu.x, nes.cpu.y, nes.cpu.sp, nes.cpu.status, nes.cpu.cycles
    );
}
