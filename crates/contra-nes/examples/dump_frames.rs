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
use contra_nes::{HookAction, Mirroring, Nes};

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

/// Reads one enemy slot's `enemy_clear`-relevant fields straight from real
/// RAM - shared by `VERIFY_ENEMY_CLEAR` and `VERIFY_INITIALIZE_ENEMY`.
fn read_enemy_clear_fields(bus: &contra_nes::bus::NesBus, x: usize) -> contra_native::enemy_clear::EnemyClearFields {
    contra_native::enemy_clear::EnemyClearFields {
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
fn measure_bg_collision_cycles(nes: &mut Nes, x: u8, y: u8, vertical_scroll: u8, horizontal_scroll: u8, ppuctrl_settings: u8) -> u64 {
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
    // VERIFY_INITIALIZE_ENEMY needs the raw PRG-ROM bytes itself (see
    // `contra_native::initialize_enemy`'s doc comment for why it reads
    // ROM bytes directly rather than a hand-transcribed table) - kept as
    // a separate owned copy since `Nes::new` below takes ownership of
    // `rom.prg_rom`.
    let prg_rom_copy = rom.prg_rom.clone();
    let mut nes = Nes::new(rom.prg_rom, mirroring);

    // EXHAUSTIVE_BG_COLLISION_CYCLES=1: replaces both earlier cycle-cost
    // measurement attempts (a flat guess, then a real-gameplay histogram
    // that only ever sampled whatever branch combinations the scripted
    // playthrough happened to hit) with a complete, exact table - see
    // `measure_bg_collision_cycles`'s doc comment for the harness. No
    // gameplay runs at all; this exits immediately after printing.
    if std::env::var("EXHAUSTIVE_BG_COLLISION_CYCLES").is_ok() {
        // vy (vertical) cases, chosen so each is reachable with row-guard
        // either on or off by pairing with the right `y`:
        //   "none"     - raw = y+vs stays < 0xf0, no overflow, no +0x10 adjust
        //   "cmp"      - no overflow, but raw >= 0xf0 (the `adc #$0f` is
        //                reached by falling through the `cmp`/`bcc` pair)
        //   "overflow" - y+vs genuinely overflows a byte (`bcs` taken directly)
        let vy_cases: [(&str, u8, u8); 3] = [("none", 0x10, 0x00), ("cmp", 0x10, 0xE0), ("overflow", 0x10, 0xF5)];
        let vy_cases_guard_on: [(&str, u8, u8); 3] = [("none", 0xE0, 0x00), ("cmp", 0xE0, 0x10), ("overflow", 0xE0, 0x20)];
        // (label, hs, [x for col0, col1, col2, col3]) - "overflow" needs a
        // *different* x per column (x+hs must overflow a byte AND still
        // land on that specific column afterward) instead of reusing the
        // no-overflow case's x list, which the first version of this
        // harness got wrong (an unused `_x_base` binding silently meant
        // the "overflow" row never actually overflowed anything, and both
        // rows came out identical - a symptom that should have been the
        // tell, not something to explain away).
        let hx_cases: [(&str, u8, [u8; 4]); 2] =
            [("no-overflow", 0x00, [0x00, 0x10, 0x20, 0x30]), ("overflow", 0xF0, [0x10, 0x20, 0x30, 0x40])];
        let col_labels = ["col0", "col1", "col2", "col3"];

        eprintln!("row guard OFF (y < 0xe0), by (vy case, hx case, column):");
        for (vy_label, y, vs) in vy_cases {
            for (hx_label, hs, xs) in hx_cases {
                for (col_label, x) in col_labels.iter().zip(xs) {
                    let cycles = measure_bg_collision_cycles(&mut nes, x, y, vs, hs, 0x00);
                    eprintln!("  vy={vy_label:9} hx={hx_label:11} {col_label}: x={x:#04x} y={y:#04x} vs={vs:#04x} hs={hs:#04x} -> {cycles} cycles");
                }
            }
        }
        eprintln!("row guard ON (y >= 0xe0), by (vy case, hx case) - column doesn't matter, that path is skipped:");
        for (vy_label, y, vs) in vy_cases_guard_on {
            for (hx_label, hs, xs) in hx_cases {
                let x = xs[0];
                let cycles = measure_bg_collision_cycles(&mut nes, x, y, vs, hs, 0x00);
                eprintln!("  vy={vy_label:9} hx={hx_label:11}: x={x:#04x} y={y:#04x} vs={vs:#04x} hs={hs:#04x} -> {cycles} cycles");
            }
        }
        return;
    }

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
    let verify_calc_bullet_velocities = std::env::var("VERIFY_CALC_BULLET_VELOCITIES").is_ok();

    // MEASURE_BG_COLLISION_CYCLES=1: a real, whole-run histogram of
    // `get_bg_collision`'s actual entry-to-`$e12a` cycle cost, replacing an
    // earlier attempt that declared its `HashSet` *inside* the frame loop -
    // silently resetting every frame, so it only ever showed whichever
    // costs happened to recur early in each frame rather than every
    // distinct cost the whole session produced. Declared here, outside the
    // loop, on purpose.
    let measure_bg_cycles = std::env::var("MEASURE_BG_COLLISION_CYCLES").is_ok();
    let mut bg_cycle_pending: Option<(u8, u8, u64)> = None;
    let mut bg_cycle_histogram: HashMap<u64, (u64, u8, u8)> = HashMap::new(); // cost -> (count, sample x, sample y)

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
            // VERIFY_CALC_BULLET_VELOCITIES only: also aim up periodically
            // to exercise more than the single straight-ahead aim_dir/
            // quadrant combo the default walk-and-shoot script produces -
            // gated behind the env var so every other verification mode's
            // exact frame timing (screenshots, other hooks) is unaffected.
            if verify_calc_bullet_velocities && frame % 90 < 30 {
                b |= BUTTON_UP;
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
                for addr in 0x0700..0x07C0u16 {
                    // CPU_GRAPHICS_BUFFER ($0700, 80 bytes) + the 112
                    // reserved bytes after it - stops short of
                    // PALETTE_CPU_BUFFER ($07c0) and the high-score bytes
                    // past that, which are real persistent state.
                    nes.poke_ram(addr, 0);
                }
                nes.poke_ram(0x21, 0); // GRAPHICS_BUFFER_OFFSET
                nes.poke_ram(0x23, 0); // GRAPHICS_BUFFER_MODE
                eprintln!(
                    "jump: game_routine=${:02X} level_routine=${:02X} current_level=${:02X}",
                    nes.bus.ram[0x18],
                    nes.bus.ram[0x2C],
                    nes.bus.ram[0x30],
                );
            }
        }
        // PC_TRACE_FRAME=N: tallies every instruction address the CPU
        // executes during frame N into a histogram and prints the most
        // frequent ones - for diagnosing a frame that produces no visible
        // RAM/PPU progress, where the question is "what is the CPU
        // actually doing" (see `Nes::run_frame_with_pc_trace`'s doc
        // comment). Used to track down the Base 1/Base 2 stage-select hang.
        // TRACE_ENEMY_SPAWN=1: verification hook for `apps/contra-pc`'s
        // `INITIALIZE_ENEMY_PC` - hooks the same address (`$ee47`,
        // `initialize_enemy`'s entry) and prints the enemy-slot register
        // plus a same-frame RAM read of that slot's type/position/HP, so
        // the hook address and the RAM layout it's paired with can both be
        // checked against real gameplay before trusting them in the actual
        // `enemy_spawn` mod event.
        let trace_spawn = std::env::var("TRACE_ENEMY_SPAWN").is_ok();
        let verify_bg_collision = std::env::var("VERIFY_BG_COLLISION").is_ok();
        if std::env::var("PC_TRACE_FRAME").ok().and_then(|s| s.parse::<u32>().ok()) == Some(frame) {
            let mut hist: HashMap<u16, u32> = HashMap::new();
            nes.run_frame_with_pc_trace(&mut |pc| *hist.entry(pc).or_insert(0) += 1);
            let mut counts: Vec<(u16, u32)> = hist.into_iter().collect();
            counts.sort_by(|a, b| b.1.cmp(&a.1));
            eprintln!("PC trace for frame {frame} - top addresses by instruction count:");
            for (pc, count) in counts.iter().take(15) {
                eprintln!("  ${pc:04X}: {count} instructions");
            }
        } else if measure_bg_cycles {
            nes.run_frame_with_hook(&mut |cpu, _bus| {
                if cpu.pc == 0xE0BB {
                    bg_cycle_pending = Some((cpu.a, cpu.y, cpu.cycles));
                } else if cpu.pc == 0xE12A {
                    if let Some((x, y, entry_cycles)) = bg_cycle_pending.take() {
                        let delta = cpu.cycles - entry_cycles;
                        let entry = bg_cycle_histogram.entry(delta).or_insert((0, x, y));
                        entry.0 += 1;
                    }
                }
                HookAction::Continue
            });
        } else if std::env::var("INTEGRATE_BG_COLLISION").is_ok() {
            // INTEGRATE_BG_COLLISION=1: the actual integration proof, not
            // just verification - see docs/NATIVE_PORT.md's "Integration
            // strategy". `get_bg_collision`'s entry (`$e0bb`) is hooked
            // with `HookAction::ReturnNow`: the real 6502 routine's body
            // *never executes at all* this run - `contra_native::collision
            // ::bg_collision` computes the answer instead, and the hook
            // writes it into the exact registers/flags the real routine's
            // documented contract promises (`a` = collision code, carry
            // set only for `Floor`) before simulating the `rts`. Compare a
            // `RAM_DUMP_FRAME` snapshot from a run with this flag set
            // against the same snapshot from a plain run (no flags) of the
            // same ROM/input script/frame count - identical bytes is the
            // actual proof this is safe to ship as a real replacement, not
            // just that the two implementations agree on inputs they were
            // both given (which `VERIFY_BG_COLLISION` already covers).
            nes.run_frame_with_hook(&mut |cpu, bus| {
                if cpu.pc == 0xE0BB {
                    let (x, y) = (cpu.a, cpu.y);
                    let mut data = [0u8; contra_native::collision::BG_COLLISION_DATA_LEN];
                    for (i, b) in data.iter_mut().enumerate() {
                        *b = bus.ram[0x0680 + i];
                    }
                    let code = contra_native::collision::bg_collision(x, y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF], &data);
                    let raw = code.to_raw_byte();
                    cpu.a = raw;
                    // Carry: set only for Floor (the real routine's own
                    // `lsr` on the collision code - bit 0 of `$01`/`$02`/
                    // `$80` is 1 only for Floor's `$01`).
                    if code == contra_native::collision::CollisionCode::Floor {
                        cpu.status |= contra_nes::cpu::FLAG_C;
                    } else {
                        cpu.status &= !contra_nes::cpu::FLAG_C;
                    }
                    // N/Z: the real routine's *last* instruction before its
                    // `rts` is `lda $14` (reloading the same collision code
                    // this hook is skipping) - a plain `LDA` always sets N
                    // to the loaded byte's bit 7 and Z to whether it's
                    // zero, same as any other load. Missing this was a
                    // real bug: at least one real caller (`get_bg_collision`
                    // return sites in `bank7.asm`, e.g. `jsr get_bg_collision;
                    // bpl @apply_gravity`) branches on N/Z immediately after
                    // the call, so leaving them stale from whatever
                    // instruction last touched them changed real control
                    // flow, not just an unread flag.
                    if raw & 0x80 != 0 {
                        cpu.status |= contra_nes::cpu::FLAG_N;
                    } else {
                        cpu.status &= !contra_nes::cpu::FLAG_N;
                    }
                    if raw == 0 {
                        cpu.status |= contra_nes::cpu::FLAG_Z;
                    } else {
                        cpu.status &= !contra_nes::cpu::FLAG_Z;
                    }
                    // Write back the real routine's zero-page scratch
                    // state too, not just its documented `a`/carry output -
                    // see `ScratchState`'s doc comment for why leaving
                    // these stale (shared, reused zero-page addresses some
                    // *other* routine may read expecting a fresh write) is
                    // a real, separate source of drift from cycle timing.
                    let scratch = contra_native::collision::bg_collision_scratch(x, y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF]);
                    bus.ram[0x10] = scratch.s10;
                    bus.ram[0x11] = scratch.s11;
                    bus.ram[0x12] = scratch.s12;
                    bus.ram[0x13] = scratch.s13;
                    bus.ram[0x14] = raw;
                    bus.ram[0x15] = scratch.s15;
                    // Exact, not averaged: `bg_collision_cycles` is derived
                    // from an exhaustive real-hardware measurement of every
                    // branch combination (`EXHAUSTIVE_BG_COLLISION_CYCLES=1`),
                    // not a sample of whatever a scripted playthrough
                    // happened to hit - see that function's doc comment and
                    // docs/NATIVE_PORT.md for the two earlier (both
                    // measurably wrong) attempts this replaced.
                    let real_cycles = contra_native::collision::bg_collision_cycles(x, y, bus.ram[0xFC], bus.ram[0xFD]);
                    HookAction::ReturnNow(real_cycles)
                } else {
                    HookAction::Continue
                }
            });
        } else if verify_bg_collision {
            // VERIFY_BG_COLLISION=1: the actual verification pass for
            // `contra_native::collision::bg_collision` (see that crate's
            // module docs for the methodology this implements). Hooks the
            // real ROM's `get_bg_collision` at its entry (`$e0bb`) to
            // capture every real call's inputs, and at its `rts`-adjacent
            // exit (`$e12a`, the `sta $14` right before the final `lda $14;
            // rts` - `a` already holds the answer there) to capture the
            // real answer, then calls the native Rust port with the same
            // inputs and asserts the two agree. Entry/exit hits are paired
            // in call order via `pending`, which only works because this
            // routine doesn't call itself recursively (true for the real
            // ROM - it's a short, self-contained leaf routine).
            let mut pending: Option<(u8, u8, u8, u8, u8, [u8; contra_native::collision::BG_COLLISION_DATA_LEN])> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                if cpu.pc == 0xE0BB {
                    let mut data = [0u8; contra_native::collision::BG_COLLISION_DATA_LEN];
                    for (i, b) in data.iter_mut().enumerate() {
                        *b = bus.ram[0x0680 + i];
                    }
                    pending = Some((cpu.a, cpu.y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF], data));
                } else if cpu.pc == 0xE12A {
                    if let Some((x, y, vs, hs, ppuctrl, data)) = pending.take() {
                        let expected = cpu.a;
                        let actual = contra_native::collision::bg_collision(x, y, vs, hs, ppuctrl, &data).to_raw_byte();
                        checked += 1;
                        if actual != expected {
                            eprintln!(
                                "MISMATCH frame={frame} x={x} y={y} vs={vs} hs={hs} ppuctrl=${ppuctrl:02X}: expected=${expected:02X} got=${actual:02X}"
                            );
                        }
                    }
                }
                HookAction::Continue
            });
            if frame % 200 == 0 && checked > 0 {
                eprintln!("frame={frame}: {checked} bg_collision calls verified this frame, no mismatches unless printed above");
            }
        } else if trace_spawn {
            let mut spawned_slots: Vec<u8> = Vec::new();
            nes.run_frame_with_hook(&mut |cpu, _bus| {
                if cpu.pc == 0xEE47 {
                    spawned_slots.push(cpu.x);
                }
                HookAction::Continue
            });
            for slot in spawned_slots {
                let o = slot as u16;
                eprintln!(
                    "frame={frame} enemy_spawn slot={slot} type=${:02X} x={} y={} hp={}",
                    nes.peek_ram(0x0528 + o),
                    nes.peek_ram(0x033E + o),
                    nes.peek_ram(0x0324 + o),
                    nes.peek_ram(0x0578 + o),
                );
            }
        } else if std::env::var("VERIFY_PLAYER_GRAVITY").is_ok() {
            // VERIFY_PLAYER_GRAVITY=1: verification pass for
            // `contra_native::player_physics`. `apply_gravity` and
            // `integrate_y_position` (= `player_jumping_set_y_pos`) are
            // hooked *independently* at their own entries/exits, not just
            // via the combined `apply_gravity_set_y_pos` entry - most real
            // jump processing (`set_jump_status_and_y_velocity`) calls
            // `apply_gravity` directly and conditionally calls
            // `player_jumping_set_y_pos` separately (skipping it on a
            // frame that scrolled vertically instead), so hooking only the
            // combined entry sees near zero real calls. Hooking both
            // routines' own entries/exits catches every real invocation
            // pattern, since even the combined-entry path's `jsr
            // apply_gravity` still passes through `apply_gravity`'s own
            // entry, and its `rts` returns straight into `player_jumping_
            // set_y_pos`'s own entry immediately after.
            use contra_native::player_physics::{apply_gravity, integrate_y_position, YPositionState, YVelocity};
            let mut pending_gravity: Option<(u8, YVelocity)> = None;
            let mut pending_integrate: Option<(u8, YVelocity, YPositionState)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xD9EC => {
                        pending_gravity = Some((cpu.x, YVelocity { fract: bus.ram[0xC4 + x], fast: bus.ram[0xC6 + x] }));
                    }
                    0xD9F9 => {
                        if let Some((px, v)) = pending_gravity.take() {
                            let px = px as usize;
                            let expected = apply_gravity(v);
                            let (real_fract, real_fast) = (bus.ram[0xC4 + px], bus.ram[0xC6 + px]);
                            checked += 1;
                            if expected.fract != real_fract || expected.fast != real_fast {
                                eprintln!(
                                    "MISMATCH(gravity) frame={frame} player={px} in={v:?}: expected fract=${:02X} fast=${:02X}, got fract=${real_fract:02X} fast=${real_fast:02X}",
                                    expected.fract, expected.fast
                                );
                            }
                        }
                    }
                    0xD9CB => {
                        let v = YVelocity { fract: bus.ram[0xC4 + x], fast: bus.ram[0xC6 + x] };
                        let state = YPositionState { y_pos: bus.ram[0x031A + x], jump_coefficient: bus.ram[0x94 + x], hidden: bus.ram[0xBA + x] };
                        pending_integrate = Some((cpu.x, v, state));
                    }
                    0xD9E9 => {
                        if let Some((px, v, state)) = pending_integrate.take() {
                            let px = px as usize;
                            let expected = integrate_y_position(v, state);
                            let real_jump_coeff = bus.ram[0x94 + px];
                            let real_y_pos = bus.ram[0x031A + px];
                            let real_hidden = cpu.a;
                            checked += 1;
                            if expected.jump_coefficient != real_jump_coeff || expected.y_pos != real_y_pos || expected.hidden != real_hidden {
                                eprintln!(
                                    "MISMATCH(integrate) frame={frame} player={px} in={v:?}/{state:?}: expected jump_coeff=${:02X} y_pos=${:02X} hidden=${:02X}, got jump_coeff=${real_jump_coeff:02X} y_pos=${real_y_pos:02X} hidden=${real_hidden:02X}",
                                    expected.jump_coefficient, expected.y_pos, expected.hidden
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if frame % 200 == 0 && checked > 0 {
                eprintln!("frame={frame}: {checked} player-gravity calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_BULLET_VELOCITY").is_ok() {
            // VERIFY_BULLET_VELOCITY=1: verification pass for
            // `contra_native::bullet_physics::adjust_bullet_velocity`.
            // The real routine ($f3a5) dispatches via `run_routine_from_
            // tbl_below`'s inline-jump-table trick, so every case handler's
            // own `rts` returns straight to *this* routine's caller, not to
            // `adjust_bullet_velocity` itself - hooking its own exit isn't
            // possible. Instead: hook entry ($f3a5) to capture inputs, and
            // hook the instruction right after each of the two real call
            // sites ($f342/$f356, i.e. $f345/$f359) to capture the result.
            let mut pending: Option<(u8, u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF3A5 => {
                        pending = Some((bus.ram[0x04], bus.ram[0x05], bus.ram[0x06]));
                    }
                    0xF345 | 0xF359 => {
                        if let Some((frac, fast, speed_code)) = pending.take() {
                            let expected = contra_native::bullet_physics::adjust_bullet_velocity(frac, fast, speed_code);
                            let real = (bus.ram[0x04], bus.ram[0x05]);
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(bullet_velocity) frame={frame} in=(frac=${frac:02X} fast=${fast:02X} speed=${speed_code:02X}): expected {expected:?}, got {real:?}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} bullet-velocity calls verified this frame, no mismatches unless printed above");
            }
        } else if verify_calc_bullet_velocities {
            // VERIFY_CALC_BULLET_VELOCITIES=1: verification pass for
            // `contra_native::bullet_physics::calc_bullet_velocities`.
            // Unlike `adjust_bullet_velocity`, this routine is a normal
            // `jsr`/`rts` call (not the inline-jump-table pattern) - its
            // one real call site is `set_bullet_velocities` ($f313), whose
            // first instruction is `jsr calc_bullet_velocities` ($f334),
            // so the return address right after it ($f316) is where the
            // real output ($04/$05/$0a/$0b) can be read.
            let mut pending: Option<(u8, u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF334 => {
                        pending = Some((cpu.a, bus.ram[0x06], bus.ram[0x07]));
                    }
                    0xF316 => {
                        if let Some((aim_dir, speed_code, quadrant)) = pending.take() {
                            let expected = contra_native::bullet_physics::calc_bullet_velocities(aim_dir, speed_code, quadrant);
                            let real = contra_native::bullet_physics::BulletVelocity {
                                frac_y: bus.ram[0x04],
                                fast_y: bus.ram[0x05],
                                frac_x: bus.ram[0x0a],
                                fast_x: bus.ram[0x0b],
                            };
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(calc_bullet_velocities) frame={frame} in=(aim_dir=${aim_dir:02X} speed=${speed_code:02X} quadrant=${quadrant:02X}): expected {expected:?}, got {real:?}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} calc_bullet_velocities calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_SLOT").is_ok() {
            // VERIFY_ENEMY_SLOT=1: verification pass for
            // `contra_native::enemy_slots::find_next_enemy_slot`/`_6_to_0`.
            // Normal jsr/rts, but with 13 real call sites across 3 banks -
            // rather than hook every site's return address, hook the one
            // shared internal exit label both entry points funnel through
            // (`find_enemy_routine_slot_exit`, $edd8) to read the real
            // result (x register + zero flag) directly, and hook both real
            // entry points ($edce full scan, $edca restricted-to-6 scan)
            // to snapshot ENEMY_ROUTINE ($04b8, 16 bytes) and which variant
            // was entered.
            use contra_native::enemy_slots::{find_next_enemy_slot, find_next_enemy_slot_6_to_0, ENEMY_SLOT_COUNT};
            let mut pending: Option<([u8; ENEMY_SLOT_COUNT], bool)> = None; // (snapshot, is_restricted_6to0)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xEDCE | 0xEDCA => {
                        let mut snapshot = [0u8; ENEMY_SLOT_COUNT];
                        snapshot.copy_from_slice(&bus.ram[0x04B8..0x04B8 + ENEMY_SLOT_COUNT]);
                        pending = Some((snapshot, cpu.pc == 0xEDCA));
                    }
                    0xEDD8 => {
                        if let Some((snapshot, restricted)) = pending.take() {
                            let expected =
                                if restricted { find_next_enemy_slot_6_to_0(&snapshot) } else { find_next_enemy_slot(&snapshot) };
                            let real_zero = cpu.status & contra_nes::cpu::FLAG_Z != 0;
                            let real = if real_zero { Some(cpu.x) } else { None };
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(enemy_slot) frame={frame} restricted={restricted} snapshot={snapshot:?}: expected {expected:?}, got {real:?}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-slot calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_CLEAR").is_ok() {
            // VERIFY_ENEMY_CLEAR=1: verification pass for
            // `contra_native::enemy_clear`'s 3 real, reachable entry
            // points. All funnel into one shared exit (the `rts` right
            // after `clear_enemy_pt_4`'s stores, $ee46) - hook entry to
            // snapshot every touched field's *pre* value (enough fields
            // to cover the widest entry point, `clear_enemy_pt_2`), hook
            // the shared exit to read the *post* state and compare
            // against applying the matching pure Rust function to the
            // snapshot.
            use contra_native::enemy_clear::{
                clear_enemy_custom_vars, clear_enemy_pt_2, clear_sprite_and_pt_3, EnemyClearFields,
            };
            #[derive(Clone, Copy, Debug)]
            enum EnemyClearEntry {
                SpriteAndPt3,
                CustomVars,
                Pt2,
            }
            let mut pending: Option<(EnemyClearFields, usize, EnemyClearEntry)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xEDF1 | 0xEDF8 | 0xEE0A => {
                        let x = cpu.x as usize;
                        let entry = match cpu.pc {
                            0xEDF1 => EnemyClearEntry::SpriteAndPt3,
                            0xEDF8 => EnemyClearEntry::CustomVars,
                            _ => EnemyClearEntry::Pt2,
                        };
                        pending = Some((read_enemy_clear_fields(bus, x), x, entry));
                    }
                    0xEE46 => {
                        if let Some((before, x, entry)) = pending.take() {
                            let mut expected = before;
                            match entry {
                                EnemyClearEntry::SpriteAndPt3 => clear_sprite_and_pt_3(&mut expected),
                                EnemyClearEntry::CustomVars => clear_enemy_custom_vars(&mut expected),
                                EnemyClearEntry::Pt2 => clear_enemy_pt_2(&mut expected),
                            }
                            let real = read_enemy_clear_fields(bus, x);
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(enemy_clear) frame={frame} x={x} before={before:?}: expected {expected:?}, got {real:?}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-clear calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INITIALIZE_ENEMY").is_ok() {
            // VERIFY_INITIALIZE_ENEMY=1: verification pass for
            // `contra_native::initialize_enemy::initialize_enemy`. Normal
            // jsr/rts with many real call sites - hook entry ($ee47) to
            // capture ENEMY_TYPE[x] (already set by *this* routine's own
            // caller) and CURRENT_LEVEL, and hook the routine's own single
            // internal `rts` ($ee8c, immediately before the
            // `enemy_prop_ptr_tbl` label - initialize_enemy has exactly
            // one exit, no per-call-site workaround needed) to compare
            // real ENEMY_ROUTINE/HP/the enemy_clear fields against
            // `initialize_enemy` applied to the real PRG-ROM bytes.
            let mut pending: Option<(usize, u8, u8)> = None; // (x, enemy_type, current_level)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xEE47 => {
                        let x = cpu.x as usize;
                        pending = Some((x, bus.ram[0x528 + x], bus.ram[0x30]));
                    }
                    0xEE8C => {
                        if let Some((x, enemy_type, current_level)) = pending.take() {
                            let expected = contra_native::initialize_enemy::initialize_enemy(&prg_rom_copy, enemy_type, current_level);
                            let real = contra_native::initialize_enemy::InitializedEnemy {
                                routine: bus.ram[0x4B8 + x],
                                hp: bus.ram[0x578 + x],
                                fields: read_enemy_clear_fields(bus, x),
                            };
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(initialize_enemy) frame={frame} x={x} enemy_type=${enemy_type:02X} level=${current_level:02X}: expected {expected:?}, got {real:?}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} initialize-enemy calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_CREATE_ENEMY_BULLET").is_ok() {
            // VERIFY_CREATE_ENEMY_BULLET=1: verification pass for
            // `contra_native::create_enemy_bullet::create_enemy_bullet`.
            // Real routine has 2 real exits (success: end of
            // `set_bullet_velocities`, `$f32e`, right before the
            // `bullet_gen_exit` label; failure: end of `bullet_gen_exit`
            // itself, `$f333`, right before `calc_bullet_velocities`) -
            // both funnel `x` back to `ENEMY_CURRENT_SLOT` as their last
            // step before `rts`, discarding the real found slot before
            // returning (see this module's own doc comment) - so rather
            // than trust `cpu.x` at either exit, the expected *slot* is
            // taken from applying the pure Rust function to the same
            // `ENEMY_ROUTINE` snapshot captured at entry (itself already
            // independently live-verified via `VERIFY_ENEMY_SLOT`).
            use contra_native::enemy_slots::ENEMY_SLOT_COUNT;
            let mut pending: Option<([u8; ENEMY_SLOT_COUNT], u8, u8, u8, u8, u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF2E4 => {
                        let mut snapshot = [0u8; ENEMY_SLOT_COUNT];
                        snapshot.copy_from_slice(&bus.ram[0x04B8..0x04B8 + ENEMY_SLOT_COUNT]);
                        pending = Some((
                            snapshot,
                            bus.ram[0x30],   // current_level
                            bus.ram[0x0a],   // bullet_type_and_angle
                            bus.ram[0x06],   // speed_code
                            bus.ram[0x07],   // quadrant
                            bus.ram[0x08],   // y_pos
                            bus.ram[0x09],   // x_pos
                        ));
                    }
                    0xF32E | 0xF333 => {
                        if let Some((snapshot, current_level, angle, speed, quadrant, y_pos, x_pos)) = pending.take() {
                            let expected = contra_native::create_enemy_bullet::create_enemy_bullet(
                                &prg_rom_copy,
                                &snapshot,
                                current_level,
                                angle,
                                speed,
                                quadrant,
                                y_pos,
                                x_pos,
                            );
                            let succeeded = cpu.pc == 0xF32E;
                            checked += 1;
                            match (expected, succeeded) {
                                (None, false) => {
                                    if cpu.a != 1 {
                                        eprintln!("MISMATCH(create_enemy_bullet) frame={frame}: expected failure a=1, got a={:02X}", cpu.a);
                                    }
                                }
                                (Some(b), true) => {
                                    let real_type = bus.ram[0x528 + b.slot as usize];
                                    let real_hp = bus.ram[0x578 + b.slot as usize];
                                    let real_fields = read_enemy_clear_fields(bus, b.slot as usize);
                                    if cpu.a != 0 || real_type != b.enemy_type || real_hp != b.hp || real_fields != b.fields {
                                        eprintln!(
                                            "MISMATCH(create_enemy_bullet) frame={frame} slot={}: expected {b:?}, got type={real_type:02X} hp={real_hp:02X} fields={real_fields:?} a={:02X}",
                                            b.slot, cpu.a
                                        );
                                    }
                                }
                                _ => {
                                    eprintln!(
                                        "MISMATCH(create_enemy_bullet) frame={frame}: pure fn disagreed with which real exit fired (expected={expected:?}, real_exit_succeeded={succeeded})"
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} create-enemy-bullet calls verified this frame, no mismatches unless printed above");
            }
        } else {
            nes.run_frame();
        }

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

    if measure_bg_cycles {
        let mut costs: Vec<(u64, (u64, u8, u8))> = bg_cycle_histogram.into_iter().collect();
        costs.sort_by_key(|(cost, _)| *cost);
        eprintln!("bg_collision cycle-cost histogram (whole run, entry $e0bb to $e12a):");
        for (cost, (count, sample_x, sample_y)) in costs {
            eprintln!("  {cost} cycles: {count} calls (e.g. x={sample_x} y={sample_y})");
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
