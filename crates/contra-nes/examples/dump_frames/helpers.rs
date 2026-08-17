use std::io::BufWriter;

use contra_nes::Nes;

/// Draws a 1px red rectangle outline directly into a copy of the
/// framebuffer - used by `HITBOXES=1` to visually verify the same OAM
/// bounding-box math `contra-pc`'s hitbox overlay uses (see
/// `apps/contra-pc/src/main.rs::redraw`), without needing a GUI.
pub fn draw_rect_outline(buf: &mut [u32], w: usize, h: usize, x: i32, y: i32, bw: i32, bh: i32, color: u32) {
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

/// Reads one enemy slot's `enemy_clear`-relevant fields straight from real
/// RAM - shared by `VERIFY_ENEMY_CLEAR` and `VERIFY_INITIALIZE_ENEMY`.
pub fn read_enemy_clear_fields(bus: &contra_nes::bus::NesBus, x: usize) -> contra_native::enemy::enemy_clear::EnemyClearFields {
    contra_native::enemy::enemy_clear::EnemyClearFields {
        attributes: bus.ram[0x5A8 + x],
        y_pos: bus.ram[0x324 + x],
        x_pos: bus.ram[0x33E + x],
        y_vel_accum: bus.ram[0x4C8 + x],
        x_vel_accum: bus.ram[0x4D8 + x],
        sprites: bus.ram[0x30A + x],
        sprite_attr: bus.ram[0x358 + x],
        y_velocity_fract: bus.ram[0x4F8 + x],
        x_velocity_fract: bus.ram[0x518 + x],
        y_velocity_fast: bus.ram[0x4E8 + x],
        x_velocity_fast: bus.ram[0x508 + x],
        animation_delay: bus.ram[0x538 + x],
        var_a: bus.ram[0x548 + x],
        attack_delay: bus.ram[0x558 + x],
        frame: bus.ram[0x568 + x],
        state_width: bus.ram[0x598 + x],
        score_collision: bus.ram[0x588 + x],
        var_1: bus.ram[0x5B8 + x],
        var_2: bus.ram[0x5C8 + x],
        var_3: bus.ram[0x5D8 + x],
        var_4: bus.ram[0x5E8 + x],
    }
}

pub fn save_png(path: &std::path::Path, fb: &[u32], w: usize, h: usize) {
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

/// Measures `get_bg_collision`'s exact real cycle cost (entry `$e0bb` to
/// its own `rts`, inclusive) for one specific input, by directly driving
/// `contra-nes`'s cycle-accurate CPU through the real routine in isolation
/// - no gameplay needed, no sampling: pokes the routine's documented
/// inputs into RAM/registers, fakes a `jsr` (pushes a synthetic return
/// address), sets `pc` to the routine's entry, then single-steps
/// (`Cpu::step`) until `pc` reaches that synthetic address, summing every
/// instruction's real cost along the way. This is what
/// `EXHAUSTIVE_BG_COLLISION_CYCLES=1` uses to build a complete, exact
/// per-branch cost table - see that flag's own comment for why this
/// replaces the two earlier (both flawed) attempts at the same number.
pub fn measure_bg_collision_cycles(nes: &mut Nes, x: u8, y: u8, vertical_scroll: u8, horizontal_scroll: u8, ppuctrl_settings: u8) -> u64 {
    nes.poke_ram(0xFC, vertical_scroll);
    nes.poke_ram(0xFD, horizontal_scroll);
    nes.poke_ram(0xFF, ppuctrl_settings);
    for i in 0..0x80u16 {
        nes.poke_ram(0x0680 + i, 0); // BG_COLLISION_DATA - content doesn't affect cost, only the branches do
    }

    const FAKE_RETURN: u16 = 0x0002; // arbitrary unused RAM address, never actually executed
    let push_addr = FAKE_RETURN.wrapping_sub(1); // JSR convention: push (return_addr - 1)
    let mut sp = nes.cpu.sp;
    nes.poke_ram(0x0100 + sp as u16, (push_addr >> 8) as u8);
    sp = sp.wrapping_sub(1);
    nes.poke_ram(0x0100 + sp as u16, (push_addr & 0xFF) as u8);
    sp = sp.wrapping_sub(1);
    nes.cpu.sp = sp;
    nes.cpu.a = x;
    nes.cpu.y = y;
    nes.cpu.pc = 0xE0BB;

    let start = nes.cpu.cycles;
    for _ in 0..500 {
        // 500 is a generous cap - the real routine is a few dozen
        // instructions with no loops, so this can't legitimately run long.
        nes.cpu.step(&mut nes.bus);
        if nes.cpu.pc == FAKE_RETURN {
            return nes.cpu.cycles - start;
        }
    }
    panic!("get_bg_collision never returned within 500 instructions for x={x} y={y} vs={vertical_scroll} hs={horizontal_scroll} ppuctrl={ppuctrl_settings:02X} - real routine or harness is broken");
}
