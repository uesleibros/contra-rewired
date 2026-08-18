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
//!
//! Split across this directory: `helpers.rs` (small shared utilities) and
//! `verify/` (one file per `VERIFY_*` mode's captured-context struct and
//! its comparison function - `main` below still owns each mode's actual
//! hook-registration/dispatch logic, just calling into these on exit).

use std::collections::HashMap;

use contra_nes::controller::*;
use contra_nes::{HookAction, Mirroring, Nes};

mod helpers;
mod verify;

use helpers::*;
use verify::*;

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
            // VERIFY_INDOOR_ENEMY_SPAWN only: once the jumped-to level has
            // had time to settle into its "in-level" routine, force
            // ENEMY_SCREEN_READ_OFFSET back to 0 to guarantee a fresh,
            // real `load_enemy_indoor_level` pass triggers - the jump
            // itself only clears it once, and by the time the level
            // settles real gameplay may have already advanced it past 0.
            if std::env::var("VERIFY_INDOOR_ENEMY_SPAWN").is_ok() && frame == start_after + 750 {
                nes.poke_ram(0x82, 0);
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
                    let mut data = [0u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN];
                    for (i, b) in data.iter_mut().enumerate() {
                        *b = bus.ram[0x0680 + i];
                    }
                    let code = contra_native::physics::collision::bg_collision(x, y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF], &data);
                    let raw = code.to_raw_byte();
                    cpu.a = raw;
                    // Carry: set only for Floor (the real routine's own
                    // `lsr` on the collision code - bit 0 of `$01`/`$02`/
                    // `$80` is 1 only for Floor's `$01`).
                    if code == contra_native::physics::collision::CollisionCode::Floor {
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
                    let scratch = contra_native::physics::collision::bg_collision_scratch(x, y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF]);
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
                    let real_cycles = contra_native::physics::collision::bg_collision_cycles(x, y, bus.ram[0xFC], bus.ram[0xFD]);
                    HookAction::ReturnNow(real_cycles)
                } else {
                    HookAction::Continue
                }
            });
        } else if verify_bg_collision {
            // VERIFY_BG_COLLISION=1: the actual verification pass for
            // `contra_native::physics::collision::bg_collision` (see that crate's
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
            let mut pending: Option<(u8, u8, u8, u8, u8, [u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN])> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                if cpu.pc == 0xE0BB {
                    let mut data = [0u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN];
                    for (i, b) in data.iter_mut().enumerate() {
                        *b = bus.ram[0x0680 + i];
                    }
                    pending = Some((cpu.a, cpu.y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF], data));
                } else if cpu.pc == 0xE12A {
                    if let Some((x, y, vs, hs, ppuctrl, data)) = pending.take() {
                        let expected = cpu.a;
                        let actual = contra_native::physics::collision::bg_collision(x, y, vs, hs, ppuctrl, &data).to_raw_byte();
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
            use contra_native::physics::player_physics::{apply_gravity, integrate_y_position, YPositionState, YVelocity};
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
            // `contra_native::physics::bullet_physics::adjust_bullet_velocity`.
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
                            let expected = contra_native::physics::bullet_physics::adjust_bullet_velocity(frac, fast, speed_code);
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
            // `contra_native::physics::bullet_physics::calc_bullet_velocities`.
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
                            let expected = contra_native::physics::bullet_physics::calc_bullet_velocities(aim_dir, speed_code, quadrant);
                            let real = contra_native::physics::bullet_physics::BulletVelocity {
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
            // `contra_native::enemy::enemy_slots::find_next_enemy_slot`/`_6_to_0`.
            // Normal jsr/rts, but with 13 real call sites across 3 banks -
            // rather than hook every site's return address, hook the one
            // shared internal exit label both entry points funnel through
            // (`find_enemy_routine_slot_exit`, $edd8) to read the real
            // result (x register + zero flag) directly, and hook both real
            // entry points ($edce full scan, $edca restricted-to-6 scan)
            // to snapshot ENEMY_ROUTINE ($04b8, 16 bytes) and which variant
            // was entered.
            use contra_native::enemy::enemy_slots::{find_next_enemy_slot, find_next_enemy_slot_6_to_0, ENEMY_SLOT_COUNT};
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
            use contra_native::enemy::enemy_clear::{
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
            // `contra_native::enemy::initialize_enemy::initialize_enemy`. Normal
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
                            let expected = contra_native::enemy::initialize_enemy::initialize_enemy(&prg_rom_copy, enemy_type, current_level);
                            let real = contra_native::enemy::initialize_enemy::InitializedEnemy {
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
            // `contra_native::enemy::create_enemy_bullet::create_enemy_bullet`.
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
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;
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
                            let expected = contra_native::enemy::create_enemy_bullet::create_enemy_bullet(
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
        } else if std::env::var("VERIFY_CREATE_ENEMY_BULLET_ANGLE_A").is_ok() {
            // VERIFY_CREATE_ENEMY_BULLET_ANGLE_A=1: verification pass for
            // `contra_native::enemy::create_enemy_bullet::create_enemy_bullet_
            // angle_a`. Entry ($f2bf) takes its inputs in registers a/y
            // (bullet_type_and_angle/speed), stored to $0a/$06 by the
            // routine's own first two instructions - hook *before* those
            // run to read the real registers. Both real failure paths
            // (attack-flag gate declined, or no free slot once
            // `create_enemy_bullet` itself runs) funnel to the same
            // shared exit as `create_enemy_bullet`'s own failure case
            // ($f333, end of `bullet_gen_exit`) - no need to distinguish
            // which one fired, the pure function returns `None` either
            // way for the same real inputs.
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;
            let mut pending: Option<([u8; ENEMY_SLOT_COUNT], u8, u8, u8, u8, u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF2BF => {
                        let mut snapshot = [0u8; ENEMY_SLOT_COUNT];
                        snapshot.copy_from_slice(&bus.ram[0x04B8..0x04B8 + ENEMY_SLOT_COUNT]);
                        pending = Some((
                            snapshot,
                            bus.ram[0x30], // current_level
                            bus.ram[0x8E], // enemy_attack_flag
                            cpu.a,         // bullet_type_and_angle
                            cpu.y,         // speed_code
                            bus.ram[0x08], // y_pos
                            bus.ram[0x09], // x_pos
                        ));
                    }
                    0xF32E | 0xF333 => {
                        if let Some((snapshot, current_level, attack_flag, angle, speed, y_pos, x_pos)) = pending.take() {
                            let expected = contra_native::enemy::create_enemy_bullet::create_enemy_bullet_angle_a(
                                &prg_rom_copy,
                                &snapshot,
                                current_level,
                                attack_flag,
                                angle,
                                speed,
                                y_pos,
                                x_pos,
                            );
                            let succeeded = cpu.pc == 0xF32E;
                            checked += 1;
                            match (expected, succeeded) {
                                (None, false) => {
                                    if cpu.a != 1 {
                                        eprintln!(
                                            "MISMATCH(create_enemy_bullet_angle_a) frame={frame}: expected failure a=1, got a={:02X}",
                                            cpu.a
                                        );
                                    }
                                }
                                (Some(b), true) => {
                                    let real_type = bus.ram[0x528 + b.slot as usize];
                                    let real_hp = bus.ram[0x578 + b.slot as usize];
                                    let real_fields = read_enemy_clear_fields(bus, b.slot as usize);
                                    if cpu.a != 0 || real_type != b.enemy_type || real_hp != b.hp || real_fields != b.fields {
                                        eprintln!(
                                            "MISMATCH(create_enemy_bullet_angle_a) frame={frame} slot={}: expected {b:?}, got type={real_type:02X} hp={real_hp:02X} fields={real_fields:?} a={:02X}",
                                            b.slot, cpu.a
                                        );
                                    }
                                }
                                _ => {
                                    eprintln!(
                                        "MISMATCH(create_enemy_bullet_angle_a) frame={frame}: pure fn disagreed with which real exit fired (expected={expected:?}, real_exit_succeeded={succeeded})"
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
                eprintln!("frame={frame}: {checked} create-enemy-bullet-angle-a calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_QUADRANT_AIM_DIR").is_ok() {
            // VERIFY_QUADRANT_AIM_DIR=1: verification pass for
            // `contra_native::enemy::quadrant_aim_dir::get_quadrant_aim_dir`.
            // Normal jsr/rts, single real exit (the `and #$0f; rts` at
            // the very end, `$f5ab`, right before the `quadrant_aim_dir_
            // lookup_ptr_tbl` label).
            use contra_native::enemy::quadrant_aim_dir::{get_quadrant_aim_dir, QUADRANT_AIM_DIR_00, QUADRANT_AIM_DIR_01, QUADRANT_AIM_DIR_02};
            let mut pending: Option<(u8, u8, u8, u8, u8)> = None; // (source_y, source_x, target_y, target_x, table_index)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF55E => {
                        pending = Some((bus.ram[0x08], bus.ram[0x09], bus.ram[0x0a], bus.ram[0x0b], bus.ram[0x0f]));
                    }
                    0xF5AB => {
                        if let Some((source_y, source_x, target_y, target_x, table_index)) = pending.take() {
                            let table = match table_index {
                                0 => &QUADRANT_AIM_DIR_00,
                                1 => &QUADRANT_AIM_DIR_01,
                                _ => &QUADRANT_AIM_DIR_02,
                            };
                            let expected = get_quadrant_aim_dir(source_y, source_x, target_y, target_x, table);
                            let real_aim_dir = cpu.a;
                            let real_quadrant = bus.ram[0x07];
                            checked += 1;
                            if expected.aim_dir != real_aim_dir || expected.quadrant != real_quadrant {
                                eprintln!(
                                    "MISMATCH(quadrant_aim_dir) frame={frame} src=({source_x:02X},{source_y:02X}) tgt=({target_x:02X},{target_y:02X}) tbl={table_index}: expected {expected:?}, got aim_dir={real_aim_dir:02X} quadrant={real_quadrant:02X}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} quadrant-aim-dir calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_QUADRANT_AIM_DIR_FOR_PLAYER").is_ok() {
            // VERIFY_QUADRANT_AIM_DIR_FOR_PLAYER=1: verification pass for
            // `contra_native::enemy::quadrant_aim_dir::get_quadrant_aim_dir_
            // for_player`. Entry ($f52c) takes `player_index` in `a`;
            // this routine has no `rts` of its own - it falls straight
            // into `get_quadrant_aim_dir`'s shared exit ($f5ab), same as
            // that routine's own verification pass above.
            use contra_native::enemy::quadrant_aim_dir::{
                get_quadrant_aim_dir_for_player, QUADRANT_AIM_DIR_00, QUADRANT_AIM_DIR_01, QUADRANT_AIM_DIR_02,
            };
            let mut pending: Option<(u8, u8, u8, [u8; 2], [u8; 2], [u8; 2], u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF52C => {
                        pending = Some((
                            bus.ram[0x08],
                            bus.ram[0x09],
                            cpu.a, // player_index
                            [bus.ram[0x90], bus.ram[0x91]],
                            [bus.ram[0x31A], bus.ram[0x31B]],
                            [bus.ram[0x334], bus.ram[0x335]],
                            bus.ram[0x40],
                            bus.ram[0x0f],
                        ));
                    }
                    0xF5AB => {
                        if let Some((source_y, source_x, player_index, player_state, sprite_y, sprite_x, level_loc, table_index)) =
                            pending.take()
                        {
                            let table = match table_index {
                                0 => &QUADRANT_AIM_DIR_00,
                                1 => &QUADRANT_AIM_DIR_01,
                                _ => &QUADRANT_AIM_DIR_02,
                            };
                            let expected = get_quadrant_aim_dir_for_player(
                                source_y,
                                source_x,
                                player_index,
                                player_state,
                                sprite_y,
                                sprite_x,
                                level_loc,
                                table,
                            );
                            let real_aim_dir = cpu.a;
                            let real_quadrant = bus.ram[0x07];
                            checked += 1;
                            if expected.aim_dir != real_aim_dir || expected.quadrant != real_quadrant {
                                eprintln!(
                                    "MISMATCH(quadrant_aim_dir_for_player) frame={frame} player_idx={player_index:02X} states={player_state:?} tbl={table_index}: expected {expected:?}, got aim_dir={real_aim_dir:02X} quadrant={real_quadrant:02X}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} quadrant-aim-dir-for-player calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_AIM_AND_CREATE_ENEMY_BULLET").is_ok() {
            // VERIFY_AIM_AND_CREATE_ENEMY_BULLET=1: verification pass for
            // `contra_native::enemy::create_enemy_bullet::aim_and_create_enemy_
            // bullet`. Entry ($f29e) takes bullet_type/speed_code in a/y;
            // real exits are the same two `create_enemy_bullet` itself
            // uses ($f32e success, $f333 failure), since this routine's
            // own control flow funnels into that same shared tail.
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;
            #[allow(clippy::type_complexity)]
            let mut pending: Option<(
                [u8; ENEMY_SLOT_COUNT],
                u8, // current_level
                u8, // enemy_attack_flag
                u8, // bullet_type
                u8, // speed_code
                u8, // source_y
                u8, // source_x
                u8, // aim_target
                u8, // direct_target_x
                u8, // direct_target_y
                [u8; 2], // player_state
                [u8; 2], // sprite_y_pos
                [u8; 2], // sprite_x_pos
                u8,      // level_location_type
            )> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF29E => {
                        let mut snapshot = [0u8; ENEMY_SLOT_COUNT];
                        snapshot.copy_from_slice(&bus.ram[0x04B8..0x04B8 + ENEMY_SLOT_COUNT]);
                        pending = Some((
                            snapshot,
                            bus.ram[0x30],
                            bus.ram[0x8E],
                            cpu.a,
                            cpu.y,
                            bus.ram[0x08],
                            bus.ram[0x09],
                            bus.ram[0x0a],
                            bus.ram[0x0b],
                            bus.ram[0x0c],
                            [bus.ram[0x90], bus.ram[0x91]],
                            [bus.ram[0x31A], bus.ram[0x31B]],
                            [bus.ram[0x334], bus.ram[0x335]],
                            bus.ram[0x40],
                        ));
                    }
                    0xF32E | 0xF333 => {
                        if let Some((
                            snapshot,
                            current_level,
                            attack_flag,
                            bullet_type,
                            speed_code,
                            source_y,
                            source_x,
                            aim_target,
                            direct_target_x,
                            direct_target_y,
                            player_state,
                            sprite_y,
                            sprite_x,
                            level_loc,
                        )) = pending.take()
                        {
                            let expected = contra_native::enemy::create_enemy_bullet::aim_and_create_enemy_bullet(
                                &prg_rom_copy,
                                &snapshot,
                                current_level,
                                attack_flag,
                                bullet_type,
                                speed_code,
                                source_y,
                                source_x,
                                aim_target,
                                direct_target_x,
                                direct_target_y,
                                player_state,
                                sprite_y,
                                sprite_x,
                                level_loc,
                            );
                            let succeeded = cpu.pc == 0xF32E;
                            checked += 1;
                            match (expected, succeeded) {
                                (None, false) => {
                                    if cpu.a != 1 {
                                        eprintln!(
                                            "MISMATCH(aim_and_create_enemy_bullet) frame={frame}: expected failure a=1, got a={:02X}",
                                            cpu.a
                                        );
                                    }
                                }
                                (Some(b), true) => {
                                    let real_type = bus.ram[0x528 + b.slot as usize];
                                    let real_hp = bus.ram[0x578 + b.slot as usize];
                                    let real_fields = read_enemy_clear_fields(bus, b.slot as usize);
                                    if cpu.a != 0 || real_type != b.enemy_type || real_hp != b.hp || real_fields != b.fields {
                                        eprintln!(
                                            "MISMATCH(aim_and_create_enemy_bullet) frame={frame} slot={}: expected {b:?}, got type={real_type:02X} hp={real_hp:02X} fields={real_fields:?} a={:02X}",
                                            b.slot, cpu.a
                                        );
                                    }
                                }
                                _ => {
                                    eprintln!(
                                        "MISMATCH(aim_and_create_enemy_bullet) frame={frame}: pure fn disagreed with which real exit fired (expected={expected:?}, real_exit_succeeded={succeeded})"
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
                eprintln!("frame={frame}: {checked} aim-and-create-enemy-bullet calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_PLAYER_ENEMY_DIST").is_ok() {
            // VERIFY_PLAYER_ENEMY_DIST=1: verification pass for
            // `contra_native::enemy::player_enemy_distance::player_enemy_x_dist`/
            // `player_enemy_y_dist`. Both real routines share one exit
            // (`lda_closer_distance`'s own `rts`, $ed4b, right before the
            // `find_far_segment_for_x_pos` label) - hook both real
            // entries ($ecf5 X, $ed0e Y) to snapshot inputs and which
            // axis was requested, and that one shared exit for the result.
            use contra_native::enemy::player_enemy_distance::{player_enemy_x_dist, player_enemy_y_dist};
            #[derive(Clone, Copy)]
            enum Axis {
                X,
                Y,
            }
            let mut pending: Option<(Axis, [u8; 2], u8, [u8; 2])> = None; // (axis, sprite_pos, enemy_pos, player_state)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xECF5 => {
                        pending = Some((
                            Axis::X,
                            [bus.ram[0x334], bus.ram[0x335]], // SPRITE_X_POS
                            bus.ram[0x33E + x],                // ENEMY_X_POS
                            [bus.ram[0x90], bus.ram[0x91]],
                        ));
                    }
                    0xED0E => {
                        pending = Some((
                            Axis::Y,
                            [bus.ram[0x31A], bus.ram[0x31B]], // SPRITE_Y_POS
                            bus.ram[0x324 + x],                // ENEMY_Y_POS
                            [bus.ram[0x90], bus.ram[0x91]],
                        ));
                    }
                    0xED4B => {
                        if let Some((axis, sprite_pos, enemy_pos, player_state)) = pending.take() {
                            let expected = match axis {
                                Axis::X => player_enemy_x_dist(sprite_pos, enemy_pos, player_state),
                                Axis::Y => player_enemy_y_dist(sprite_pos, enemy_pos, player_state),
                            };
                            let real_index = cpu.y;
                            let real_distance = cpu.a;
                            checked += 1;
                            if expected.player_index != real_index || expected.distance != real_distance {
                                eprintln!(
                                    "MISMATCH(player_enemy_dist) frame={frame} sprite_pos={sprite_pos:?} enemy_pos={enemy_pos:02X} states={player_state:?}: expected {expected:?}, got index={real_index:02X} distance={real_distance:02X}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} player-enemy-dist calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ADD_SCROLL_TO_ENEMY_POS").is_ok() {
            // VERIFY_ADD_SCROLL_TO_ENEMY_POS=1: verification pass for
            // `contra_native::enemy::add_scroll_to_enemy_pos::add_scroll_to_
            // enemy_pos`. Real routine has 3 real exits: vertical/no-
            // removal ($e8b8), horizontal/no-removal ($e8c6, right
            // before the dead-code `bank_7_unused_label_02`), and the
            // shared "removed" tail both branches' `remove_enemy_far`
            // jumps into (`remove_enemy`/`set_sprite_0`'s own rts,
            // $e813, right before `shared_enemy_routine_clear_sprite`).
            // Position is written before the removal decision either
            // way, so all 3 exits can be checked the same way; which
            // exit actually fired is itself compared against this
            // port's own `should_remove` prediction.
            use contra_native::enemy::add_scroll_to_enemy_pos::add_scroll_to_enemy_pos;
            let mut pending: Option<(usize, u8, u8, u8, u8)> = None; // (x, scroll_type, frame_scroll, enemy_x, enemy_y)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE8A7 => {
                        let x = cpu.x as usize;
                        pending = Some((x, bus.ram[0x41], bus.ram[0x68], bus.ram[0x33E + x], bus.ram[0x324 + x]));
                    }
                    0xE8B8 | 0xE8C6 | 0xE813 => {
                        if let Some((x, scroll_type, frame_scroll, enemy_x, enemy_y)) = pending.take() {
                            let expected = add_scroll_to_enemy_pos(scroll_type, frame_scroll, enemy_x, enemy_y);
                            let real_x = bus.ram[0x33E + x];
                            let real_y = bus.ram[0x324 + x];
                            let real_removed = cpu.pc == 0xE813;
                            checked += 1;
                            if expected.x_pos != real_x || expected.y_pos != real_y || expected.should_remove != real_removed {
                                eprintln!(
                                    "MISMATCH(add_scroll_to_enemy_pos) frame={frame} x={x} scroll_type={scroll_type:02X} frame_scroll={frame_scroll:02X} in=({enemy_x:02X},{enemy_y:02X}): expected {expected:?}, got x_pos={real_x:02X} y_pos={real_y:02X} should_remove={real_removed}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} add-scroll-to-enemy-pos calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_UPDATE_ENEMY_POS").is_ok() {
            // VERIFY_UPDATE_ENEMY_POS=1: verification pass for
            // `contra_native::enemy::update_enemy_pos::update_enemy_pos`. Real
            // routine has 2 real exits: success/no-removal
            // (`apply_vel_exit`'s own rts, $e849, shared by both the
            // horizontal and vertical branches' full-success paths), and
            // the same shared "removed" tail `add_scroll_to_enemy_pos`
            // uses ($e813).
            use contra_native::enemy::update_enemy_pos::update_enemy_pos;
            #[allow(clippy::type_complexity)]
            let mut pending: Option<(usize, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE837 => {
                        let x = cpu.x as usize;
                        pending = Some((
                            x,
                            bus.ram[0x41],       // level_scrolling_type
                            bus.ram[0x68],       // frame_scroll
                            bus.ram[0x33E + x],  // x_pos
                            bus.ram[0x4D8 + x],  // x_vel_accum
                            bus.ram[0x518 + x],  // x_vel_fract
                            bus.ram[0x508 + x],  // x_vel_fast
                            bus.ram[0x324 + x],  // y_pos
                            bus.ram[0x4C8 + x],  // y_vel_accum
                            bus.ram[0x4F8 + x],  // y_vel_fract
                            bus.ram[0x4E8 + x],  // y_vel_fast
                        ));
                    }
                    0xE849 | 0xE813 => {
                        if let Some((
                            x,
                            scroll_type,
                            frame_scroll,
                            x_pos,
                            x_vel_accum,
                            x_vel_fract,
                            x_vel_fast,
                            y_pos,
                            y_vel_accum,
                            y_vel_fract,
                            y_vel_fast,
                        )) = pending.take()
                        {
                            let expected = update_enemy_pos(
                                scroll_type,
                                frame_scroll,
                                x_pos,
                                x_vel_accum,
                                x_vel_fract,
                                x_vel_fast,
                                y_pos,
                                y_vel_accum,
                                y_vel_fract,
                                y_vel_fast,
                            );
                            let real_x_pos = bus.ram[0x33E + x];
                            let real_x_accum = bus.ram[0x4D8 + x];
                            let real_y_pos = bus.ram[0x324 + x];
                            let real_y_accum = bus.ram[0x4C8 + x];
                            let real_removed = cpu.pc == 0xE813;
                            checked += 1;
                            let mismatch = expected.x.pos != real_x_pos
                                || expected.x.vel_accum != real_x_accum
                                || expected.y.pos != real_y_pos
                                || expected.y.vel_accum != real_y_accum
                                || expected.removed.is_some() != real_removed;
                            if mismatch {
                                eprintln!(
                                    "MISMATCH(update_enemy_pos) frame={frame} x={x} scroll_type={scroll_type:02X} frame_scroll={frame_scroll:02X}: expected {expected:?}, got x_pos={real_x_pos:02X} x_accum={real_x_accum:02X} y_pos={real_y_pos:02X} y_accum={real_y_accum:02X} removed={real_removed}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} update-enemy-pos calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ADD_WITH_ENEMY_POS").is_ok() {
            // VERIFY_ADD_WITH_ENEMY_POS=1: verification pass for
            // `contra_native::add_with_enemy_pos`. Two real entries
            // share one exit: `set_08_09_to_enemy_pos` ($eb2f, always
            // offset 0/0) and `add_with_enemy_pos` ($eb32, offsets in
            // a/y) both funnel into the same rts ($eb3f).
            use contra_native::enemy::add_with_enemy_pos::{add_with_enemy_pos, set_08_09_to_enemy_pos};
            let mut pending: Option<(usize, u8, u8)> = None; // (x, x_offset, y_offset)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xEB2F => {
                        pending = Some((cpu.x as usize, 0, 0));
                    }
                    0xEB32 => {
                        pending = Some((cpu.x as usize, cpu.a, cpu.y));
                    }
                    0xEB3F => {
                        if let Some((x, x_offset, y_offset)) = pending.take() {
                            let enemy_x = bus.ram[0x33E + x];
                            let enemy_y = bus.ram[0x324 + x];
                            let expected = if x_offset == 0 && y_offset == 0 {
                                set_08_09_to_enemy_pos(enemy_x, enemy_y)
                            } else {
                                add_with_enemy_pos(x_offset, y_offset, enemy_x, enemy_y)
                            };
                            let real = (bus.ram[0x09], bus.ram[0x08]);
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(add_with_enemy_pos) frame={frame} x={x} offsets=({x_offset:02X},{y_offset:02X}) enemy_pos=({enemy_x:02X},{enemy_y:02X}): expected {expected:?}, got {real:?}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} add-with-enemy-pos calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_COLLISION_FLAGS").is_ok() {
            // VERIFY_ENEMY_COLLISION_FLAGS=1: verification pass for
            // `contra_native::enemy_collision_flags`'s 5 real entry
            // points, all funneling into one shared exit
            // (`set_enemy_state_width_to_a`'s own rts, $eb1e).
            use contra_native::enemy::enemy_collision_flags::{
                disable_bullet_enemy_collision, disable_enemy_collision, enable_bullet_enemy_collision,
                enable_enemy_collision, enable_enemy_player_collision_check,
            };
            #[derive(Clone, Copy, Debug)]
            enum Toggle {
                DisableBullet,
                DisableAll,
                EnablePlayerCheck,
                EnableBullet,
                EnableAll,
            }
            let mut pending: Option<(usize, u8, Toggle)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xEB03 => pending = Some((x, bus.ram[0x598 + x], Toggle::DisableBullet)),
                    0xEB07 => pending = Some((x, bus.ram[0x598 + x], Toggle::DisableAll)),
                    0xEB0E => pending = Some((x, bus.ram[0x598 + x], Toggle::EnablePlayerCheck)),
                    0xEB12 => pending = Some((x, bus.ram[0x598 + x], Toggle::EnableBullet)),
                    0xEB16 => pending = Some((x, bus.ram[0x598 + x], Toggle::EnableAll)),
                    0xEB1E => {
                        if let Some((x, before, toggle)) = pending.take() {
                            let expected = match toggle {
                                Toggle::DisableBullet => disable_bullet_enemy_collision(before),
                                Toggle::DisableAll => disable_enemy_collision(before),
                                Toggle::EnablePlayerCheck => enable_enemy_player_collision_check(before),
                                Toggle::EnableBullet => enable_bullet_enemy_collision(before),
                                Toggle::EnableAll => enable_enemy_collision(before),
                            };
                            let real = bus.ram[0x598 + x];
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(enemy_collision_flags) frame={frame} x={x} toggle={toggle:?} before={before:02X}: expected {expected:02X}, got {real:02X}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-collision-flags calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_ENEMY_SPAWN").is_ok() {
            // VERIFY_INDOOR_ENEMY_SPAWN=1 (use with JUMP_STAGE=1 or 3 to
            // reach an indoor level): verification pass for
            // `contra_native::enemy::enemy_spawn::decompress_indoor_enemy_screen`.
            // The resolved screen-data pointer is already sitting in
            // $0a/$0b (bank2.asm's own `load_screen_enemy_data` prefix
            // resolves it before `load_enemy_indoor_level` is even
            // called), so the real 2-level pointer-table indirection this
            // project already trusts doesn't need to be redone here.
            // Reads the actual bytes via `bus.mapper.read` (the
            // emulator's own live bank mapping) rather than independently
            // recomputing a PRG-ROM file offset - the switchable-bank
            // number active for bank2.asm's own code isn't a fixed
            // constant to assume, and guessing it wrong silently reads
            // the wrong bank's bytes.
            let mut pending: Option<Vec<u8>> = None; // 64 real bytes read via the live mapper
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    // $b4c6 = the `cmp #$ff` right after the real `lda
                    // ($0a),y` (verified by counting instruction lengths
                    // from $b4af: 2+2+2+2+2+2+2+2+3+2+2 = 23 bytes, all
                    // confirmed zero-page/immediate/jsr-abs modes via
                    // rom-symbols.txt's own addresses) - hooking $b4af
                    // itself and reading $0a/$0b there looked identical
                    // to this in testing but produced a real mismatch:
                    // `cpu.a` here is the actual byte the CPU's own `cmp`
                    // just compared, so this is the ground truth, not an
                    // assumption about timing.
                    0xB4C6 if bus.ram[0x82] == 1 => {
                        let addr = u16::from_le_bytes([bus.ram[0x0a], bus.ram[0x0b]]);
                        let data: Vec<u8> = (0..64u16).map(|i| bus.mapper.read(addr.wrapping_add(i))).collect();
                        pending = Some(data);
                    }
                    // Both real exits land here for the same reason: the
                    // mid-loop "no more enemies" check (position byte ==
                    // $ff, the terminator every real screen with fewer
                    // than 16 enemies actually hits) branches to the same
                    // $b4ae `load_screen_enemy_data_exit` the "no data at
                    // all" (cores == $ff) check uses - $b512 (this
                    // routine's own local `rts`) is reached only in the
                    // edge case of exactly 16 enemies with no terminator
                    // at all, which real data may never exercise. Both
                    // exits get the identical real-RAM comparison.
                    0xB4AE | 0xB512 => {
                        if let Some(data) = pending.take() {
                            let expected = contra_native::enemy::enemy_spawn::decompress_indoor_enemy_screen(&data);
                            checked += 1;
                            if let Some(screen) = &expected {
                                let real_cores = bus.ram[0x86];
                                let mut mismatch = real_cores != screen.cores_to_destroy;
                                let mut real_spawns = Vec::new();
                                for (i, expected_spawn) in screen.spawns.iter().enumerate() {
                                    let slot = 15 - i;
                                    let real_spawn = contra_native::enemy::enemy_spawn::EnemySpawn {
                                        x: bus.ram[0x33E + slot],
                                        y: bus.ram[0x324 + slot],
                                        enemy_type: bus.ram[0x528 + slot],
                                        attribute: bus.ram[0x5A8 + slot],
                                    };
                                    if real_spawn != *expected_spawn {
                                        mismatch = true;
                                    }
                                    real_spawns.push(real_spawn);
                                }
                                if mismatch {
                                    eprintln!(
                                        "MISMATCH(indoor_enemy_spawn) frame={frame} real_cores={real_cores:02X}: expected {screen:?}, got real_spawns={real_spawns:?}"
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
                eprintln!("frame={frame}: {checked} indoor-enemy-spawn calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_POSITION_UTILS").is_ok() {
            // VERIFY_ENEMY_POSITION_UTILS=1: verification pass for
            // `contra_native::enemy_position_utils`'s 5 real entry
            // points, each with its own real exit (no shared tail here,
            // unlike most of this file's other verify blocks).
            use contra_native::enemy::enemy_position_utils::{
                add_10_to_enemy_y_fract_vel, add_a_to_enemy_x_pos, add_a_to_enemy_y_fract_vel, add_a_to_enemy_y_pos,
                reverse_enemy_x_direction,
            };
            #[derive(Clone, Copy, Debug)]
            enum Op {
                AddYPos(u8, u8),                // (a, enemy_y_pos)
                AddXPos(u8, u8),                // (a, enemy_x_pos)
                Add10YFractVel(u8, u8),         // (y_vel_fract, y_vel_fast)
                AddAYFractVel(u8, u8, u8),       // (a, y_vel_fract, y_vel_fast)
                ReverseXDir(u8, u8),             // (x_vel_fract, x_vel_fast)
            }
            let mut pending: Option<Op> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xEB1F => pending = Some(Op::AddYPos(cpu.a, bus.ram[0x324 + x])),
                    0xEB27 => pending = Some(Op::AddXPos(cpu.a, bus.ram[0x33E + x])),
                    0xEB40 => pending = Some(Op::Add10YFractVel(bus.ram[0x4F8 + x], bus.ram[0x4E8 + x])),
                    0xEB42 => pending = Some(Op::AddAYFractVel(cpu.a, bus.ram[0x4F8 + x], bus.ram[0x4E8 + x])),
                    0xE91E => pending = Some(Op::ReverseXDir(bus.ram[0x518 + x], bus.ram[0x508 + x])),
                    0xEB26 | 0xEB2E | 0xEB51 | 0xE92F => {
                        if let Some(op) = pending.take() {
                            checked += 1;
                            match (cpu.pc, op) {
                                (0xEB26, Op::AddYPos(a, before)) => {
                                    let expected = add_a_to_enemy_y_pos(a, before);
                                    let real = bus.ram[0x324 + x];
                                    if expected != real {
                                        eprintln!("MISMATCH(enemy_position_utils AddYPos) frame={frame} a={a:02X} before={before:02X}: expected {expected:02X}, got {real:02X}");
                                    }
                                }
                                (0xEB2E, Op::AddXPos(a, before)) => {
                                    let expected = add_a_to_enemy_x_pos(a, before);
                                    let real = bus.ram[0x33E + x];
                                    if expected != real {
                                        eprintln!("MISMATCH(enemy_position_utils AddXPos) frame={frame} a={a:02X} before={before:02X}: expected {expected:02X}, got {real:02X}");
                                    }
                                }
                                (0xEB51, Op::Add10YFractVel(fract, fast)) => {
                                    let expected = add_10_to_enemy_y_fract_vel(fract, fast);
                                    let real = (bus.ram[0x4F8 + x], bus.ram[0x4E8 + x]);
                                    if expected != real {
                                        eprintln!("MISMATCH(enemy_position_utils Add10YFractVel) frame={frame} before=({fract:02X},{fast:02X}): expected {expected:?}, got {real:?}");
                                    }
                                }
                                (0xEB51, Op::AddAYFractVel(a, fract, fast)) => {
                                    let expected = add_a_to_enemy_y_fract_vel(a, fract, fast);
                                    let real = (bus.ram[0x4F8 + x], bus.ram[0x4E8 + x]);
                                    if expected != real {
                                        eprintln!("MISMATCH(enemy_position_utils AddAYFractVel) frame={frame} a={a:02X} before=({fract:02X},{fast:02X}): expected {expected:?}, got {real:?}");
                                    }
                                }
                                (0xE92F, Op::ReverseXDir(fract, fast)) => {
                                    let expected = reverse_enemy_x_direction(fract, fast);
                                    let real = (bus.ram[0x518 + x], bus.ram[0x508 + x]);
                                    if expected != real {
                                        eprintln!("MISMATCH(enemy_position_utils ReverseXDir) frame={frame} before=({fract:02X},{fast:02X}): expected {expected:?}, got {real:?}");
                                    }
                                }
                                _ => {
                                    checked -= 1; // wrong exit for the pending op - not a real match, don't count it
                                }
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-position-utils calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_ROUTINE_TRANSITION").is_ok() {
            // VERIFY_ENEMY_ROUTINE_TRANSITION=1: verification pass for
            // `contra_native::enemy_routine_transition`'s 3 real entry
            // points - by far the most-reused routines found in this
            // crate (75 real call sites for `advance_enemy_routine`
            // alone). `set_enemy_delay_adv_routine` ($e78b) is a real ASM
            // fallthrough straight into `advance_enemy_routine` ($e78e) -
            // only set `pending` at $e78e if nothing is already pending
            // from that same fallthrough, so the delay comparison isn't
            // silently dropped.
            use contra_native::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a};
            #[derive(Clone, Copy, Debug)]
            enum Op {
                Advance(u8),
                SetToA(u8, u8),
                DelayedAdvance(u8, u8),
            }
            let mut pending: Option<Op> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xE78B => pending = Some(Op::DelayedAdvance(cpu.a, bus.ram[0x4B8 + x])),
                    0xE78E => {
                        if pending.is_none() {
                            pending = Some(Op::Advance(bus.ram[0x4B8 + x]));
                        }
                    }
                    0xE81A => pending = Some(Op::SetToA(bus.ram[0x4B8 + x], cpu.a)),
                    0xE796 | 0xE822 | 0xE813 => {
                        if let Some(op) = pending.take() {
                            checked += 1;
                            let real_routine = bus.ram[0x4B8 + x];
                            let real_sprites = bus.ram[0x30A + x];
                            match op {
                                Op::Advance(before) => {
                                    let expected = advance_enemy_routine(before);
                                    if expected.routine != real_routine || (expected.sprites.is_some() && expected.sprites != Some(real_sprites)) {
                                        eprintln!(
                                            "MISMATCH(enemy_routine_transition Advance) frame={frame} before={before:02X}: expected {expected:?}, got routine={real_routine:02X} sprites={real_sprites:02X}"
                                        );
                                    }
                                }
                                Op::SetToA(before, a) => {
                                    let expected = set_enemy_routine_to_a(before, a);
                                    if expected.routine != real_routine || (expected.sprites.is_some() && expected.sprites != Some(real_sprites)) {
                                        eprintln!(
                                            "MISMATCH(enemy_routine_transition SetToA) frame={frame} before={before:02X} a={a:02X}: expected {expected:?}, got routine={real_routine:02X} sprites={real_sprites:02X}"
                                        );
                                    }
                                }
                                Op::DelayedAdvance(a, before) => {
                                    let expected = set_enemy_delay_adv_routine(a, before);
                                    let real_delay = bus.ram[0x538 + x];
                                    let mismatch = expected.animation_delay != real_delay
                                        || expected.routine_update.routine != real_routine
                                        || (expected.routine_update.sprites.is_some() && expected.routine_update.sprites != Some(real_sprites));
                                    if mismatch {
                                        eprintln!(
                                            "MISMATCH(enemy_routine_transition DelayedAdvance) frame={frame} a={a:02X} before={before:02X}: expected {expected:?}, got routine={real_routine:02X} sprites={real_sprites:02X} delay={real_delay:02X}"
                                        );
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-routine-transition calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_VERT_SCROLL_Y_ADD").is_ok() {
            // VERIFY_VERT_SCROLL_Y_ADD=1: verification pass for
            // `contra_native::enemy::enemy_position_utils::add_a_with_vert_
            // scroll_to_enemy_y_pos`/`add_4_to_enemy_y_pos`. Both real
            // entries ($eb88 preset a=4, $eb8a general) share one real
            // exit ($eba3, right before `update_nametable_tiles_set_
            // delay`).
            use contra_native::enemy::enemy_position_utils::add_a_with_vert_scroll_to_enemy_y_pos;
            let mut pending: Option<(u8, u8, u8)> = None; // (a, vertical_scroll, enemy_y_pos)
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xEB88 => pending = Some((0x04, bus.ram[0xFC], bus.ram[0x324 + x])),
                    0xEB8A => {
                        if pending.is_none() {
                            pending = Some((cpu.a, bus.ram[0xFC], bus.ram[0x324 + x]));
                        }
                    }
                    0xEBA3 => {
                        if let Some((a, vertical_scroll, before)) = pending.take() {
                            let expected = add_a_with_vert_scroll_to_enemy_y_pos(a, vertical_scroll, before);
                            let real = bus.ram[0x324 + x];
                            checked += 1;
                            if expected != real {
                                eprintln!(
                                    "MISMATCH(vert_scroll_y_add) frame={frame} a={a:02X} vscroll={vertical_scroll:02X} before={before:02X}: expected {expected:02X}, got {real:02X}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} vert-scroll-y-add calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_00").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_00` - this
            // crate's first *composed* enemy AI state port. Real entry
            // $861e; real exits are the same 2 shared ones `enemy_
            // routine_transition`'s own verification pass uses ($e796
            // success, $e813 guard-rejected/removed), since this routine
            // ends with a real `jmp set_enemy_delay_adv_routine`.
            use contra_native::enemy::soldier::soldier_routine_00;
            let mut pending: Option<(usize, u8, u8, u8, u8, u8, u8, u8)> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    // See `VERIFY_SOLDIER_ROUTINE_03`'s comment below for
                    // why this bank gate matters: $8000-$bfff is a
                    // switchable window, and `bank0.asm` (this routine's
                    // real home) is only actually mapped there when
                    // `bank_select() == 0`.
                    0x861E if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some((
                            x,
                            bus.ram[0x41],       // level_scrolling_type
                            bus.ram[0x68],       // frame_scroll
                            bus.ram[0xFC],       // vertical_scroll
                            bus.ram[0x33E + x],  // enemy_x_pos
                            bus.ram[0x324 + x],  // enemy_y_pos
                            bus.ram[0x5A8 + x],  // enemy_attributes
                            bus.ram[0x4B8 + x],  // current_routine
                        ));
                    }
                    0xE796 | 0xE813 => {
                        if let Some((x, scroll_type, frame_scroll, vscroll, x_pos, y_pos, attrs, routine)) = pending.take() {
                            let expected = soldier_routine_00(scroll_type, frame_scroll, vscroll, x_pos, y_pos, attrs, routine);
                            let real_x = bus.ram[0x33E + x];
                            let real_y = bus.ram[0x324 + x];
                            let real_routine = bus.ram[0x4B8 + x];
                            let real_delay = bus.ram[0x538 + x];
                            checked += 1;
                            let mismatch = expected.scroll.x_pos != real_x
                                || expected.y_pos_after_offset != real_y
                                || expected.delayed_routine.routine_update.routine != real_routine
                                || expected.delayed_routine.animation_delay != real_delay;
                            if mismatch {
                                eprintln!(
                                    "MISMATCH(soldier_routine_00) frame={frame} in=(scroll_type={scroll_type:02X} frame_scroll={frame_scroll:02X} vscroll={vscroll:02X} x={x_pos:02X} y={y_pos:02X} attrs={attrs:02X} routine={routine:02X}): expected {expected:?}, got x={real_x:02X} y={real_y:02X} routine={real_routine:02X} delay={real_delay:02X}"
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_BG_COLLISION_FAR").is_ok() {
            // VERIFY_BG_COLLISION_FAR=1: verification pass for
            // `contra_native::physics::collision::get_bg_collision_far`. Real
            // entry $e087; real exit is `floor_get_next_row_bg_
            // collision`'s own shared rts at $e0ba (right before
            // `get_bg_collision` begins at $e0bb).
            use contra_native::physics::collision::{get_bg_collision_far, BG_COLLISION_DATA_LEN};
            let mut pending: Option<(u8, u8, u8, u8, u8, [u8; BG_COLLISION_DATA_LEN])> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE087 => {
                        let mut data = [0u8; BG_COLLISION_DATA_LEN];
                        for (i, b) in data.iter_mut().enumerate() {
                            *b = bus.ram[0x0680 + i];
                        }
                        pending = Some((cpu.a, cpu.y, bus.ram[0xFC], bus.ram[0xFD], bus.ram[0xFF], data));
                    }
                    0xE0BA => {
                        if let Some((x, y, vscroll, hscroll, ppuctrl, data)) = pending.take() {
                            let expected = get_bg_collision_far(x, y, vscroll, hscroll, ppuctrl, &data);
                            let real_raw = cpu.a;
                            checked += 1;
                            if expected.to_raw_byte() != real_raw {
                                eprintln!(
                                    "MISMATCH(get_bg_collision_far) frame={frame} x={x:02X} y={y:02X} vscroll={vscroll:02X} hscroll={hscroll:02X}: expected {expected:?} ({:02X}), got {real_raw:02X}",
                                    expected.to_raw_byte()
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} bg-collision-far calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ADD_Y_POS_BG_COLLISION").is_ok() {
            // VERIFY_ADD_Y_POS_BG_COLLISION=1: verification pass for
            // `contra_native::physics::collision::add_a_y_to_enemy_pos_get_bg_
            // collision`/`add_y_to_y_pos_get_bg_collision`. Two real
            // entries ($ec33 zero-x-offset, $ec35 general) and two real
            // exits: the early Y-overflow "$exit" ($ec48) and the shared
            // success exit `get_bg_collision`/`bg_collision`'s own
            // verification already relies on ($e12f, the `read_bg_
            // collision_byte`/`@set_code_exit` chain's own rts, confirmed
            // by counting instruction lengths from `$e12a` against
            // `level_screen_mem_offset_tbl_01`'s real address at `$e130`).
            use contra_native::physics::collision::{add_a_y_to_enemy_pos_get_bg_collision, BG_COLLISION_DATA_LEN};
            let mut pending: Option<(u8, u8, u8, u8, u8, u8, u8, [u8; BG_COLLISION_DATA_LEN])> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                let x = cpu.x as usize;
                match cpu.pc {
                    0xEC33 | 0xEC35 => {
                        let x_offset = if cpu.pc == 0xEC33 { 0 } else { cpu.a };
                        let mut data = [0u8; BG_COLLISION_DATA_LEN];
                        for (i, b) in data.iter_mut().enumerate() {
                            *b = bus.ram[0x0680 + i];
                        }
                        pending = Some((
                            x_offset,
                            cpu.y,
                            bus.ram[0x33E + x],
                            bus.ram[0x324 + x],
                            bus.ram[0xFC],
                            bus.ram[0xFD],
                            bus.ram[0xFF],
                            data,
                        ));
                    }
                    0xEC48 | 0xE12F => {
                        if let Some((x_offset, y_offset, ex, ey, vscroll, hscroll, ppuctrl, data)) = pending.take() {
                            let expected = add_a_y_to_enemy_pos_get_bg_collision(x_offset, y_offset, ex, ey, vscroll, hscroll, ppuctrl, &data);
                            let real_raw = cpu.a;
                            checked += 1;
                            if expected.to_raw_byte() != real_raw {
                                eprintln!(
                                    "MISMATCH(add_y_pos_bg_collision) frame={frame} x_off={x_offset:02X} y_off={y_offset:02X} ex={ex:02X} ey={ey:02X}: expected {expected:?} ({:02X}), got {real_raw:02X}",
                                    expected.to_raw_byte()
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} add-y-pos-bg-collision calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_01").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_01`. Real entry
            // $8665. Real exits: `soldier_routine_exit` ($865c, the
            // NoDecrement/DelayNotYetZero outcomes), and the two shared
            // exits `soldier_routine_00`'s own verification already uses
            // ($e796 normal advance, $e813 guard-rejected advance *and*
            // `remove_enemy`, since `remove_enemy` shares that tail).
            //
            // Subtlety: $865c is *also* hit mid-flight on the Advanced
            // path - it's the address of `soldier_set_x_velocity`'s own
            // `rts`, reached via a real nested `jsr soldier_set_x_velocity`
            // inside `soldier_stop_y_set_x_velocity` ($8638). That inner
            // return isn't our exit; disambiguated by peeking the return
            // address on the 6502 stack ($0100+sp+1/+2) - the inner call's
            // return address is always $863a (last byte of the `jsr` at
            // $8638), which a genuine soldier_routine_01 exit via branch
            // can't coincidentally match (it returns to whatever called
            // soldier_routine_01 itself).
            let mut pending: Option<SoldierRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    // See `VERIFY_SOLDIER_ROUTINE_03`'s comment below for
                    // why this bank gate matters: $8000-$bfff is a
                    // switchable window, and `bank0.asm` (this routine's
                    // real home) is only actually mapped there when
                    // `bank_select() == 0`.
                    0x8665 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut data = [0u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN];
                        for (i, b) in data.iter_mut().enumerate() {
                            *b = bus.ram[0x0680 + i];
                        }
                        pending = Some(SoldierRoutine01Ctx {
                            x,
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            frame_counter: bus.ram[0x1A],
                            attrs: bus.ram[0x5A8 + x],
                            delay: bus.ram[0x538 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            state_width: bus.ram[0x598 + x],
                            vscroll: bus.ram[0xFC],
                            hscroll: bus.ram[0xFD],
                            ppuctrl: bus.ram[0xFF],
                            data,
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x865C => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if ret == 0x863A {
                            // Nested return from `soldier_set_x_velocity`
                            // inside `soldier_stop_y_set_x_velocity` - not
                            // our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_soldier_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_soldier_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_02_JUMPING").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_02_JUMPING=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_02_jumping` - the
            // jumping sub-path only (see that function's doc comment for
            // why the walking sub-path isn't ported yet). Real entry
            // $86af, but only proceeds if `ENEMY_VAR_3 != 0` there (the
            // walking sub-path shares the same entry).
            //
            // Real exits: $e849 (`apply_vel_exit`, `update_enemy_pos`'s
            // own success rts) and the 2 shared exits `soldier_routine_
            // 00`/`01` already use ($e796/$e813 - reached here either via
            // `soldier_apply_vel_check_solid_collision`'s own solid-ahead
            // early exit, or via `update_enemy_pos`'s off-screen removal
            // path, both real `jmp`s into the same `set_enemy_routine_to_
            // a`/`remove_enemy` machinery).
            //
            // Subtlety, the same shape as `soldier_routine_01`'s: the
            // water-landing case's own `jsr set_enemy_routine_to_a` (a
            // real nested call, not a tail jump) also returns through
            // $e796/$e813 mid-flight, before this routine's genuine
            // exit. Disambiguated by peeking the stack's return address -
            // that nested call's return address always lands back inside
            // `soldier_routine_02`'s own un-labeled body (province of
            // this composition: strictly below `soldier_apply_vel_check_
            // solid_collision`'s own address, $8794, since that routine
            // is only ever *tail*-jumped into here, never `jsr`'d, so no
            // return address pointing past $8794 can come from one of
            // our own nested calls).
            use contra_native::physics::collision::BG_COLLISION_DATA_LEN;

            let mut pending: Option<SoldierRoutine02JumpingCtx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    // See `VERIFY_SOLDIER_ROUTINE_03`'s comment below for
                    // why this bank gate matters: $8000-$bfff is a
                    // switchable window, and `bank0.asm` (this routine's
                    // real home) is only actually mapped there when
                    // `bank_select() == 0`.
                    0x86AF if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let var_3 = bus.ram[0x5D8 + x];
                        if var_3 != 0 {
                            let mut data = [0u8; BG_COLLISION_DATA_LEN];
                            for (i, b) in data.iter_mut().enumerate() {
                                *b = bus.ram[0x0680 + i];
                            }
                            pending = Some(SoldierRoutine02JumpingCtx {
                                x,
                                var_3,
                                y_vel_fast: bus.ram[0x4E8 + x],
                                x_pos: bus.ram[0x33E + x],
                                y_pos: bus.ram[0x324 + x],
                                var_4: bus.ram[0x5E8 + x],
                                var_2: bus.ram[0x5C8 + x],
                                var_1: bus.ram[0x5B8 + x],
                                vscroll: bus.ram[0xFC],
                                hscroll: bus.ram[0xFD],
                                ppuctrl: bus.ram[0xFF],
                                data,
                                scroll_type: bus.ram[0x41],
                                frame_scroll: bus.ram[0x68],
                                x_accum: bus.ram[0x4D8 + x],
                                x_fract: bus.ram[0x518 + x],
                                x_fast: bus.ram[0x508 + x],
                                y_accum: bus.ram[0x4C8 + x],
                                y_fract: bus.ram[0x4F8 + x],
                                y_fast: bus.ram[0x4E8 + x],
                                routine: bus.ram[0x4B8 + x],
                            });
                        }
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if ret < 0x8794 {
                            // Nested return from the water-landing case's
                            // own `jsr set_enemy_routine_to_a`, still
                            // inside `soldier_routine_02`'s own body -
                            // not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_soldier_routine_02_jumping(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE849 => {
                        if let Some(ctx) = pending.take() {
                            verify_soldier_routine_02_jumping(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-02-jumping calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_03").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_03=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_03`. Real entry
            // $8803. Real exits for the Waiting/Fired outcomes (reached
            // via a pure tail-call chain all the way through `set_
            // soldier_sprite_add_scroll_01`/`add_scroll_to_enemy_pos`,
            // no nested returns): `$e8b8` (`scroll_enemy_pos_exit`,
            // vertical level, no removal), `$e8c6` (`add_horizontal_
            // scroll`'s own rts, horizontal level, no removal), and the
            // shared `remove_enemy` exit `$e813`.
            //
            // The `AllFired` outcome (`soldier_fired_all_bullets`) is
            // different: it `jsr`s (not tail-jumps) both `set_soldier_
            // sprite` and `add_scroll_to_enemy_pos`, then makes its own
            // final `set_enemy_routine_to_a` call - so `$e796`/`$e813`
            // can *also* fire as an intermediate nested return (if `add_
            // scroll_to_enemy_pos`'s own removal check happens to trip)
            // before the genuine final exit. Disambiguated the same way
            // as `soldier_routine_01`/`02`'s hooks: a nested return here
            // always lands back inside `soldier_fired_all_bullets`'s own
            // un-labeled body ($886a up to the next label, `soldier_
            // bullet_y_offset`, at $8882) - only a return address outside
            // that narrow range is treated as the genuine exit.
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<SoldierRoutine03Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    // $8000-$bfff is the switchable bank window - `bank0.asm`
                    // (where `soldier_routine_03` and everything it calls
                    // lives) is only actually mapped there when `bank_select()
                    // == 0`; any other bank number reuses these same numeric
                    // addresses for completely unrelated code (confirmed by
                    // capturing raw bytes at $8803 without this gate: real
                    // bytes were `85 ea a5` - `sta $ea`, not `soldier_routine_
                    // 03`'s real `bd a8 05` - `lda ENEMY_ATTRIBUTES,x` - every
                    // one of 212 "matches" from an earlier, ungated version of
                    // this hook was a false positive against unrelated code).
                    0x8803 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(SoldierRoutine03Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attack_flag: bus.ram[0x8E],
                            attributes: bus.ram[0x5A8 + x],
                            attack_delay: bus.ram[0x558 + x],
                            var_3: bus.ram[0x5D8 + x],
                            var_2: bus.ram[0x5C8 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            var_1: bus.ram[0x5B8 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine,
                        });
                    }
                    0xE8B8 | 0xE8C6 => {
                        if let Some(ctx) = pending.take() {
                            verify_soldier_routine_03(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x886A..0x8882).contains(&ret) {
                            // Nested return from `soldier_fired_all_
                            // bullets`'s own `jsr set_soldier_sprite`/
                            // `jsr add_scroll_to_enemy_pos` - not our
                            // exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_soldier_routine_03(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-03 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_04").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_04=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_04`. Real entry
            // $88c3 (gated on bank_select()==0 - see `VERIFY_SOLDIER_
            // ROUTINE_03`'s comment for why). Real exits: the 2 shared
            // exits earlier soldier routines already use ($e796/$e813),
            // reached via `jmp set_enemy_delay_adv_routine` at the very
            // end. That final tail is preceded by a real *nested* `jsr
            // add_scroll_to_enemy_pos` (`$88f8`), so - same shape as
            // `soldier_routine_03`'s `AllFired` path - $e796/$e813 can
            // also fire as an intermediate nested return first;
            // disambiguated by checking the stack's return address is
            // outside this routine's own body (`$88c3`-`$8900`, the next
            // real label, `soldier_routine_05`).
            let mut pending: Option<SoldierRoutine04Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x88C3 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SoldierRoutine04Ctx {
                            x,
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            var_2: bus.ram[0x5C8 + x],
                            var_1: bus.ram[0x5B8 + x],
                            state_width: bus.ram[0x598 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x88C3..0x8900).contains(&ret) {
                            // Nested return from the `jsr add_scroll_to_
                            // enemy_pos` inside this routine's own body -
                            // not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_soldier_routine_04(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-04 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_05").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_05=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_05`. Real entry
            // $8900 (gated on bank_select()==0). Real exits: `$8939`
            // (`soldier_routine_05_exit`, the `StillWaiting` outcome's
            // plain `rts`), and the 2 shared exits ($e796/$e813, reached
            // via `jmp advance_enemy_routine` for both `OffTopAdvance`
            // and `Advanced`). The on-screen path's real nested `jsr
            // update_enemy_pos` means $e796/$e813 can also fire as an
            // intermediate nested return; disambiguated the same way as
            // `soldier_routine_04`'s hook - a nested return here lands
            // inside this routine's own body (`$8900`-`$8939`).
            let mut pending: Option<SoldierRoutine05Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8900 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SoldierRoutine05Ctx {
                            x,
                            frame: bus.ram[0x568 + x],
                            var_2: bus.ram[0x5C8 + x],
                            var_1: bus.ram[0x5B8 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            y_fract: bus.ram[0x4F8 + x],
                            y_fast: bus.ram[0x4E8 + x],
                            x_accum: bus.ram[0x4D8 + x],
                            x_fract: bus.ram[0x518 + x],
                            x_fast: bus.ram[0x508 + x],
                            y_accum: bus.ram[0x4C8 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            animation_delay: bus.ram[0x538 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x8939 => {
                        if let Some(ctx) = pending.take() {
                            verify_soldier_routine_05(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x8900..0x8939).contains(&ret) {
                            // Nested return from `soldier_routine_05`'s
                            // own `jsr update_enemy_pos` - not our exit,
                            // keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_soldier_routine_05(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-05 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_09").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_09=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_09`. Real entry
            // $888c (gated on bank_select()==0). This routine's own port
            // exists specifically to test a surprising real-ASM reading:
            // it calls `set_soldier_sprite`/`add_scroll_to_enemy_pos`
            // *twice* (see the port's own doc comment) - if that reading
            // is wrong, this hook is exactly what would catch it. Real
            // exits: the 2 shared exits ($e796/$e813) reached via `jmp
            // set_enemy_delay_adv_routine` at the very end, disambiguated
            // from the 3 nested `jsr`s earlier in the call (`soldier_set_
            // y_pos_sprite_add_scroll`, `set_soldier_sprite`, `add_
            // scroll_to_enemy_pos`) the same way as every other soldier
            // routine's hook - a nested return lands inside this
            // routine's own body (`$888c`-`$88a1`, the next real label,
            // `soldier_routine_0a`).
            let mut pending: Option<SoldierRoutine09Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x888C if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SoldierRoutine09Ctx {
                            x,
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            var_2: bus.ram[0x5C8 + x],
                            var_1: bus.ram[0x5B8 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x888C..0x88A1).contains(&ret) {
                            // Nested return from one of this routine's own
                            // 3 nested `jsr`s - not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_soldier_routine_09(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-09 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SOLDIER_ROUTINE_0A").is_ok() {
            // VERIFY_SOLDIER_ROUTINE_0A=1: verification pass for
            // `contra_native::enemy::soldier::soldier_routine_0a`. Real entry
            // $88a1 (gated on bank_select()==0). Real exits: `$e8b8`/
            // `$e8c6` (`add_scroll_to_enemy_pos`'s own success exits,
            // reached via a real tail-call chain the whole way for both
            // the `Waiting` and `StillSplashing` outcomes - the `jsr
            // set_soldier_sprite` inside that chain returns through its
            // own separate address, not these, so no disambiguation is
            // needed here), and the shared `remove_enemy` exit `$e813`
            // (the `Removed` outcome, or either other outcome's own
            // off-screen removal).
            let mut pending: Option<SoldierRoutine0aCtx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x88A1 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SoldierRoutine0aCtx {
                            x,
                            animation_delay: bus.ram[0x538 + x],
                            frame: bus.ram[0x568 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            var_2: bus.ram[0x5C8 + x],
                            var_1: bus.ram[0x5B8 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE8B8 | 0xE8C6 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_soldier_routine_0a(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} soldier-routine-0a calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_ROUTINE_REMOVE_ENEMY").is_ok() {
            // VERIFY_ENEMY_ROUTINE_REMOVE_ENEMY=1: verification pass for
            // `contra_native::enemy::update_enemy_pos::enemy_routine_remove_
            // enemy`. Real entry $e806 (fixed bank, always mapped - no
            // bank_select() gate needed, unlike the soldier_routine_0N
            // hooks). Real (single) exit: `remove_enemy`'s own rts,
            // `$e813` - reached both as a genuine final exit (this
            // routine falls straight through into `remove_enemy`'s own
            // body after its one real `jsr add_scroll_to_enemy_pos`) and,
            // confusingly, as a *nested* return from that same `jsr` if
            // its own scroll happens to trigger its own internal removal
            // path first. Disambiguated by checking the stack's return
            // address: the nested case always returns to exactly `$e808`
            // (the last byte of the 3-byte `jsr add_scroll_to_enemy_pos`
            // at $e806-$e808, immediately followed by `remove_enemy`'s
            // own code at $e809) - any other return address is genuine.
            let mut pending: Option<EnemyRoutineRemoveEnemyCtx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE806 => {
                        let x = cpu.x as usize;
                        pending = Some(EnemyRoutineRemoveEnemyCtx {
                            x,
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                        });
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if ret == 0xE808 {
                            // Nested return from `jsr add_scroll_to_enemy_
                            // pos`'s own internal removal - not our exit,
                            // keep waiting (execution falls through into
                            // `remove_enemy`'s own body next anyway).
                        } else if let Some(ctx) = pending.take() {
                            verify_enemy_routine_remove_enemy(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-routine-remove-enemy calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_ROUTINE_INIT_EXPLOSION").is_ok() {
            // VERIFY_ENEMY_ROUTINE_INIT_EXPLOSION=1: verification pass
            // for `contra_native::enemy::enemy_explosion::enemy_routine_init_
            // explosion`. Real entry $e74b (fixed bank, no bank gate
            // needed). While `pending` is armed, also watches `play_
            // sound`'s own real entry ($c16b) to capture whether (and
            // with what code, via `cpu.a`) it actually fired - `play_
            // sound` itself isn't ported (it's a bank-switch wrapper, not
            // a pure RAM transform), so this is the only way to verify
            // the `sound` field against real hardware. Real exits: the 2
            // shared exits `enemy_routine_remove_enemy` already uses
            // ($e796/$e813), disambiguated from the real nested `jsr
            // add_scroll_to_enemy_pos` the same way as every other hook
            // this session - a nested return lands inside this routine's
            // own body+tail (`$e74b`-`$e796`).
            let mut pending: Option<EnemyRoutineInitExplosionCtx> = None;
            let mut sound_seen: Option<u8> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE74B => {
                        let x = cpu.x as usize;
                        sound_seen = None;
                        pending = Some(EnemyRoutineInitExplosionCtx {
                            x,
                            state_width: bus.ram[0x598 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            sprites: bus.ram[0x30A + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xC16B if pending.is_some() => {
                        sound_seen = Some(cpu.a);
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xE74B..0xE796).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos` - not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_enemy_routine_init_explosion(ctx, sound_seen, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-routine-init-explosion calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_ROUTINE_EXPLOSION").is_ok() {
            // VERIFY_ENEMY_ROUTINE_EXPLOSION=1: verification pass for
            // `contra_native::enemy::enemy_explosion::enemy_routine_
            // explosion`. Real entry $e7b0 (fixed bank, no bank gate
            // needed). Real exits: `enemy_routine_explosion_exit` ($e805,
            // `show_explosion_a`'s own dedicated rts - the `NoRoutine`/
            // `Waiting`/`Animating` outcomes all reach it via a real
            // *tail* jmp/branch chain the whole way, no nested-return
            // ambiguity there), and the 2 shared exits ($e796/$e813,
            // the `Advanced` outcome, reached via `bcs advance_enemy_
            // routine`). $e813 specifically can *also* fire as a nested
            // return from this routine's own early `jsr add_scroll_to_
            // enemy_pos` (if its own scroll happens to trigger its own
            // internal removal) - disambiguated the usual way: a nested
            // return lands inside `show_explosion_a`'s own body
            // (`$e7bc`-`$e805`).
            let mut pending: Option<EnemyRoutineExplosionCtx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE7B0 => {
                        let x = cpu.x as usize;
                        pending = Some(EnemyRoutineExplosionCtx {
                            x,
                            state_width: bus.ram[0x598 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            routine: bus.ram[0x4B8 + x],
                            animation_delay: bus.ram[0x538 + x],
                            frame: bus.ram[0x568 + x],
                        });
                    }
                    0xE805 => {
                        if let Some(ctx) = pending.take() {
                            verify_enemy_routine_explosion(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xE7BC..0xE805).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos` - not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_enemy_routine_explosion(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-routine-explosion calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_RED_BLUE_SOLDIER_ROUTINE_00").is_ok() {
            // VERIFY_RED_BLUE_SOLDIER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::red_blue_soldier::red_blue_soldier_
            // routine_00`. Real entry $a157 (switchable bank - gated on
            // bank_select()==0, same reasoning as every soldier_routine_0N
            // hook). Real exit: the 2 shared exits ($e796/$e813, reached
            // via `jmp advance_enemy_routine` at the very end) - no
            // nested `jsr` anywhere in this routine, so no disambiguation
            // needed.
            let mut pending: Option<RedBlueSoldierRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA157 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(RedBlueSoldierRoutine00Ctx { x, attributes: bus.ram[0x5A8 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_red_blue_soldier_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} red-blue-soldier-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_BLUE_SOLDIER_ROUTINE_01").is_ok() {
            // VERIFY_BLUE_SOLDIER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::red_blue_soldier::blue_soldier_
            // routine_01`. Real entry $a18a (switchable bank - gated on
            // bank_select()==0). Real exits: `blue_soldier_routine_01_
            // exit` ($a1da, the `StillRunning` outcome), and the 2 shared
            // exits ($e796/$e813, the `CloseToPlayer` outcome, reached
            // via `jmp set_anim_delay_adv_enemy_routine_01` which itself
            // falls into `advance_enemy_routine`). Both need
            // disambiguation: $a1da is *also* this routine's own nested
            // `jsr red_blue_soldier_set_run_frame`'s return address (that
            // helper shares its real rts with this routine's exit
            // label), and $e813 can *also* fire as a nested return from
            // the real nested `jsr update_enemy_pos`'s own off-screen
            // removal. Both disambiguated the usual way: a nested return
            // lands inside this routine's own body (`$a18a`-`$a1da`).
            let mut pending: Option<BlueSoldierRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA18A if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(BlueSoldierRoutine01Ctx {
                            x,
                            frame: bus.ram[0x568 + x],
                            frame_counter: bus.ram[0x1A],
                            attributes: bus.ram[0x5A8 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_accum: bus.ram[0x4D8 + x],
                            x_fract: bus.ram[0x518 + x],
                            x_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_accum: bus.ram[0x4C8 + x],
                            y_fract: bus.ram[0x4F8 + x],
                            y_fast: bus.ram[0x4E8 + x],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xA1DA | 0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xA18A..0xA1DA).contains(&ret) {
                            // Nested return from this routine's own body
                            // - not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_blue_soldier_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} blue-soldier-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_BLUE_SOLDIER_ROUTINE_02").is_ok() {
            // VERIFY_BLUE_SOLDIER_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::red_blue_soldier::blue_soldier_
            // routine_02`. Real entry $a1f7 (switchable bank, gated).
            // Real exits: `blue_soldier_routine_02_exit` ($a1f6, the
            // `Waiting`/`Animating` outcomes, both reached via a direct
            // branch with no nested `jsr` in between - no ambiguity), and
            // the 2 shared exits ($e796/$e813, the `JumpStart` outcome,
            // reached via `jmp set_anim_delay_adv_enemy_routine_01` after
            // a real nested `jsr enable_enemy_collision` that resolves
            // well before reaching them and doesn't itself touch those
            // addresses - no ambiguity there either).
            let mut pending: Option<BlueSoldierRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA1F7 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(BlueSoldierRoutine02Ctx {
                            x,
                            animation_delay: bus.ram[0x538 + x],
                            frame: bus.ram[0x568 + x],
                            attributes: bus.ram[0x5A8 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            state_width: bus.ram[0x598 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xA1F6 | 0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_blue_soldier_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} blue-soldier-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_BLUE_SOLDIER_ROUTINE_03").is_ok() {
            // VERIFY_BLUE_SOLDIER_ROUTINE_03=1: verification pass for
            // `contra_native::enemy::red_blue_soldier::blue_soldier_
            // routine_03`. Real entry $a245 (switchable bank, gated).
            // Real exits: `$e849` (`apply_vel_exit`, `update_enemy_pos`'s
            // own success rts, reached via a real tail `jmp` - no nested-
            // return ambiguity, since nothing runs after it) and the
            // shared `remove_enemy` exit `$e813`.
            let mut pending: Option<BlueSoldierRoutine03Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA245 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(BlueSoldierRoutine03Ctx {
                            x,
                            animation_delay: bus.ram[0x538 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_accum: bus.ram[0x4D8 + x],
                            x_fract: bus.ram[0x518 + x],
                            x_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_accum: bus.ram[0x4C8 + x],
                            y_fract: bus.ram[0x4F8 + x],
                            y_fast: bus.ram[0x4E8 + x],
                        });
                    }
                    0xE849 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_blue_soldier_routine_03(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} blue-soldier-routine-03 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_RED_SOLDIER_ROUTINE_01").is_ok() {
            // VERIFY_RED_SOLDIER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::red_blue_soldier::red_soldier_
            // routine_01`. Real entry $a266 (switchable bank, gated).
            // Real exits: `red_soldier_routine_exit` ($a2cf, the
            // `AlreadyFired`/`StillRunning` outcomes - reached only via
            // direct branches here, no nested `jsr` shares this address
            // for this specific routine, so no ambiguity), and the 2
            // shared exits ($e796/$e813, the `Attack` outcome). $e813 can
            // still fire early as a nested return from this routine's own
            // real `jsr update_enemy_pos`'s off-screen removal;
            // disambiguated the usual way - a nested return lands inside
            // this routine's own body (`$a266`-`$a2cf`).
            let mut pending: Option<RedSoldierRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA266 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(RedSoldierRoutine01Ctx {
                            x,
                            frame: bus.ram[0x568 + x],
                            frame_counter: bus.ram[0x1A],
                            attributes: bus.ram[0x5A8 + x],
                            var_2: bus.ram[0x5C8 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_accum: bus.ram[0x4D8 + x],
                            x_fract: bus.ram[0x518 + x],
                            x_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_accum: bus.ram[0x4C8 + x],
                            y_fract: bus.ram[0x4F8 + x],
                            y_fast: bus.ram[0x4E8 + x],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xA2CF | 0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xA266..0xA2CF).contains(&ret) {
                            // Nested return from this routine's own body
                            // - not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_red_soldier_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} red-soldier-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_RED_SOLDIER_ROUTINE_02").is_ok() {
            // VERIFY_RED_SOLDIER_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::red_blue_soldier::red_soldier_
            // routine_02`. Real entry $a2bb (switchable bank, gated).
            // Real exits: `red_soldier_routine_exit` ($a2cf, the
            // `Waiting` outcome - no nested `jsr` before it, no
            // ambiguity), the 2 shared exits ($e796/$e813, the
            // `AllFired` outcome via `jmp set_enemy_routine_to_a` -
            // also no nested `jsr` before it in that specific path, no
            // ambiguity), and `bullet_gen_exit` ($f32f, the shared real
            // exit for the entire `create_enemy_bullet_if_attack_
            // enabled` chain `aim_and_create_enemy_bullet` tail-jumps
            // into - the `Fired` outcome).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<RedSoldierRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA2BB if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(RedSoldierRoutine02Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attack_flag: bus.ram[0x8E],
                            attack_delay: bus.ram[0x558 + x],
                            var_1: bus.ram[0x5B8 + x],
                            var_2: bus.ram[0x5C8 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            sprite_y_pos: [bus.ram[0x31A], bus.ram[0x31B]],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            level_location_type: bus.ram[0x40],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine,
                        });
                    }
                    0xA2CF | 0xE796 | 0xE813 | 0xF32F => {
                        if let Some(ctx) = pending.take() {
                            verify_red_soldier_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} red-soldier-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_RED_BLUE_SOLDIER_GEN_ROUTINE_00").is_ok() {
            // VERIFY_RED_BLUE_SOLDIER_GEN_ROUTINE_00=1: verification pass
            // for `contra_native::enemy::red_blue_soldier::red_blue_
            // soldier_gen_routine_00`. Real entry $a304 (switchable bank,
            // gated). Real exit: the 2 shared exits ($e796/$e813), no
            // nested `jsr` anywhere in this trivial routine.
            let mut pending: Option<u8> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA304 if bus.mapper.bank_select() == 0 => pending = Some(bus.ram[0x4B8 + cpu.x as usize]),
                    0xE796 | 0xE813 => {
                        if let Some(routine) = pending.take() {
                            use contra_native::enemy::red_blue_soldier::red_blue_soldier_gen_routine_00;
                            let x = cpu.x as usize;
                            let expected = red_blue_soldier_gen_routine_00(routine);
                            checked += 1;
                            let real_delay = bus.ram[0x538 + x];
                            let real_routine = bus.ram[0x4B8 + x];
                            let real_sprites = bus.ram[0x30A + x];
                            let mismatch = real_delay != expected.animation_delay
                                || real_routine != expected.routine_update.routine
                                || expected.routine_update.sprites.map(|s| real_sprites != s).unwrap_or(false);
                            if mismatch {
                                eprintln!(
                                    "MISMATCH(red_blue_soldier_gen_routine_00) frame={frame} pc={:04X} in=(routine={routine:02X}): expected {expected:?}, got delay={real_delay:02X} routine={real_routine:02X} sprites={real_sprites:02X}",
                                    cpu.pc
                                );
                            }
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} red-blue-soldier-gen-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_RED_BLUE_SOLDIER_GEN_ROUTINE_01").is_ok() {
            // VERIFY_RED_BLUE_SOLDIER_GEN_ROUTINE_01=1: verification pass
            // for `contra_native::enemy::red_blue_soldier::red_blue_
            // soldier_gen_routine_01`. Real entry $a309 (switchable
            // bank, gated). Real exits: the shared `remove_enemy` exit
            // `$e813` (`Removed` - no nested `jsr` before the wall-
            // plating check, unambiguous), `red_blue_soldier_gen_
            // routine_01_exit` ($a367, `OddFrame`/`StillWaiting` - but
            // *also* the real nested return address for every real `jsr
            // @find_slot_init_red_blue_soldier` call the spawn loop
            // makes, since that helper's own control flow falls straight
            // into this same shared rts; disambiguated the usual way, a
            // nested return lands inside this routine's own body,
            // `$a309`-`$a367`), and `@set_delay_exit`'s own dedicated
            // rts ($a34b, the `Spawned` outcome - reached by straight-
            // line code with nothing after it, no ambiguity).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<RedBlueSoldierGenRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xA309 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(RedBlueSoldierGenRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            wall_plating_destroyed_count: bus.ram[0x87],
                            frame_counter: bus.ram[0x1A],
                            animation_delay: bus.ram[0x538 + x],
                            var_1: bus.ram[0x5B8 + x],
                            enemy_routine,
                        });
                    }
                    0xA34B => {
                        if let Some(ctx) = pending.take() {
                            verify_red_blue_soldier_gen_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xA367 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xA309..0xA367).contains(&ret) {
                            // Nested return from `jsr @find_slot_init_
                            // red_blue_soldier` - not our exit, keep
                            // waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_red_blue_soldier_gen_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} red-blue-soldier-gen-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_SOLDIER_ROUTINE_00").is_ok() {
            // VERIFY_INDOOR_SOLDIER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::indoor_soldier::indoor_soldier_
            // routine_00`. Real entry $92c8 (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // advance_enemy_routine` - the real nested `jsr init_indoor_
            // enemy_pos_and_vel` (and its own further nested `jsr
            // reverse_enemy_x_direction`) resolve well before that tail
            // and don't themselves touch those addresses, so no
            // disambiguation is needed.
            let mut pending: Option<IndoorSoldierRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x92C8 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(IndoorSoldierRoutine00Ctx { x, attributes: bus.ram[0x5A8 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_soldier_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} indoor-soldier-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_SOLDIER_ROUTINE_01").is_ok() {
            // VERIFY_INDOOR_SOLDIER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::indoor_soldier::indoor_soldier_
            // routine_01`. Real entry $92d5 (switchable bank, gated).
            // Real exits: this routine's own local `@exit` ($9315, the
            // `StillWaiting`/`OutOfRange`/`GrenadeSkipped` outcomes - all
            // 3 share this one address), plus the 3 sub-routines'
            // `@exit`s it tail-jumps into for the other outcomes:
            // `create_indoor_bullet`'s ($97c3, `Bullet`), `enemy_launch_
            // grenade`'s ($9773, `Grenade` fired), and `create_roller_
            // with_segment_a`'s ($9732, `Roller`). None of the nested
            // `jsr`s along the way (`init_sprite_from_frame`,
            // `apply_enemy_velocity_set_bg_priority`, `add_with_enemy_
            // pos`, `find_next_enemy_slot`, `initialize_enemy`, `find_
            // far_segment_for_x_pos`) revisit any of these 4 addresses,
            // so no disambiguation is needed.
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<IndoorSoldierRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x92D5 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(IndoorSoldierRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            frame_counter: bus.ram[0x1A],
                            enemy_frame: bus.ram[0x568 + x],
                            enemy_sprite_attr: bus.ram[0x358 + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            attack_delay: bus.ram[0x558 + x],
                            attributes: bus.ram[0x5A8 + x],
                            var_1: bus.ram[0x5B8 + x],
                            attack_flag: bus.ram[0x8E],
                            attributes_scratch: bus.ram[0x0A],
                            enemy_routine,
                        });
                    }
                    0x9315 | 0x9732 | 0x9773 | 0x97C3 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_soldier_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} indoor-soldier-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SHARED_ENEMY_ROUTINE_00").is_ok() {
            // VERIFY_SHARED_ENEMY_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::indoor_soldier::shared_enemy_
            // routine_00`. Real entry $9346 (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // set_anim_delay_adv_enemy_routine_00` -> `jmp advance_
            // enemy_routine` - the nested `jsr disable_enemy_collision`/
            // `jsr set_enemy_x_velocity_to_0` both resolve in bank7's
            // fixed region, well away from either address, so no
            // disambiguation is needed.
            let mut pending: Option<SharedEnemyRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9346 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SharedEnemyRoutine00Ctx { x, state_width: bus.ram[0x598 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_shared_enemy_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} shared-enemy-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SHARED_ENEMY_ROUTINE_01").is_ok() {
            // VERIFY_SHARED_ENEMY_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::indoor_soldier::shared_enemy_
            // routine_01`. Real entry $9360 (switchable bank, gated).
            // Real exits: this routine's own local exit ($9345, real
            // label `shared_enemy_routine_01_exit` - the `Waiting`
            // outcome), plus the 2 shared exits ($e796/$e813, the
            // `Advanced` outcome via `jmp advance_enemy_routine`). $9345
            // is also `init_sprite_from_frame`'s own natural `rts` (a
            // real, shared exit) - harmless here since this routine
            // never calls `init_sprite_from_frame` itself, so within one
            // tracked call $9345 can only be reached via this routine's
            // own `bne`.
            let mut pending: Option<SharedEnemyRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9360 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SharedEnemyRoutine01Ctx {
                            x,
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            animation_delay: bus.ram[0x538 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x9345 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_shared_enemy_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_shared_enemy_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} shared-enemy-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SHARED_ENEMY_ROUTINE_03").is_ok() {
            // VERIFY_SHARED_ENEMY_ROUTINE_03=1: verification pass for
            // `contra_native::enemy::enemy_explosion::shared_enemy_
            // routine_03`. Real entry $e7aa (fixed bank, no gate needed) -
            // falls straight into `show_explosion_a` via `bne`, so this
            // hook mirrors `VERIFY_ENEMY_ROUTINE_EXPLOSION`'s own hook
            // exactly (same real exits: `enemy_routine_explosion_exit`
            // $e805, the 2 shared exits $e796/$e813 disambiguated the
            // same way against a nested return from `add_scroll_to_
            // enemy_pos`).
            let mut pending: Option<SharedEnemyRoutine03Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE7AA => {
                        let x = cpu.x as usize;
                        pending = Some(SharedEnemyRoutine03Ctx {
                            x,
                            state_width: bus.ram[0x598 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            scroll_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            routine: bus.ram[0x4B8 + x],
                            animation_delay: bus.ram[0x538 + x],
                            frame: bus.ram[0x568 + x],
                        });
                    }
                    0xE805 => {
                        if let Some(ctx) = pending.take() {
                            verify_shared_enemy_routine_03(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xE7BC..0xE805).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos` - not our exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_shared_enemy_routine_03(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} shared-enemy-routine-03 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_JUMPING_SOLDIER_ROUTINE_00").is_ok() {
            // VERIFY_JUMPING_SOLDIER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::jumping_soldier::jumping_soldier_
            // routine_00`. Real entry $9380 (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // advance_enemy_routine` - the nested `jsr init_indoor_
            // enemy_pos_and_vel` (and its own nested `jsr reverse_enemy_
            // x_direction`, `$e91e`, fixed bank) never revisits either
            // address, so no disambiguation is needed.
            let mut pending: Option<JumpingSoldierRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9380 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(JumpingSoldierRoutine00Ctx {
                            x,
                            attributes: bus.ram[0x5A8 + x],
                            indoor_red_soldier_created: bus.ram[0x89],
                            indoor_enemy_attack_count: bus.ram[0x88],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_jumping_soldier_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} jumping-soldier-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_JUMPING_SOLDIER_ROUTINE_01").is_ok() {
            // VERIFY_JUMPING_SOLDIER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::jumping_soldier::jumping_soldier_
            // routine_01`. Real entry $93a5 (switchable bank, gated).
            // Real exits: this routine's own local `@exit` ($9422, the
            // `Waiting` outcome and both `Jumping` sub-cases), plus the
            // shared bullet-chain exit `bullet_gen_exit` ($f32f, the
            // `Fired` outcome, same shared tail `red_soldier_routine_02`
            // already reaches the same way).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<JumpingSoldierRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x93A5 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(JumpingSoldierRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attack_flag: bus.ram[0x8E],
                            animation_delay: bus.ram[0x538 + x],
                            attributes: bus.ram[0x5A8 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            var_1: bus.ram[0x5B8 + x],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            sprite_y_pos: [bus.ram[0x31A], bus.ram[0x31B]],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            level_location_type: bus.ram[0x40],
                            enemy_routine,
                        });
                    }
                    0x9422 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_jumping_soldier_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xF32F => {
                        if let Some(ctx) = pending.take() {
                            verify_jumping_soldier_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} jumping-soldier-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_JUMPING_SOLDIER_ROUTINE_04").is_ok() {
            // VERIFY_JUMPING_SOLDIER_ROUTINE_04=1: verification pass for
            // `contra_native::enemy::jumping_soldier::jumping_soldier_
            // routine_04`. Real entry $9437 (switchable bank, gated).
            // Real exits: the 2 shared exits ($e796/$e813, the
            // `AdvancedOnly` outcome via `jmp advance_enemy_routine`),
            // and `weapon_box_exit_1` ($82f4, switchable bank, gated -
            // the `ExplodedAndDroppedWeapon` outcome via `jmp play_
            // explosion_sound`). No nested `jsr`s revisit either address
            // along the way, so no disambiguation is needed.
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<JumpingSoldierRoutine04Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9437 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(JumpingSoldierRoutine04Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attributes: bus.ram[0x5A8 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine,
                        });
                    }
                    0x82F4 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_jumping_soldier_routine_04(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_jumping_soldier_routine_04(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} jumping-soldier-routine-04 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_GRENADE_LAUNCHER_ROUTINE_00").is_ok() {
            // VERIFY_GRENADE_LAUNCHER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::grenade_launcher::grenade_launcher_
            // routine_00`. Real entry $9468 (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // set_anim_delay_adv_enemy_routine_00` -> `jmp advance_
            // enemy_routine` - the nested `jsr player_enemy_x_dist`
            // ($ecf5, fixed bank) and `jsr init_indoor_enemy_pos_and_vel`
            // (switchable, own address range) never revisit either
            // shared exit, so no disambiguation is needed.
            let mut pending: Option<GrenadeLauncherRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9468 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(GrenadeLauncherRoutine00Ctx {
                            x,
                            attributes: bus.ram[0x5A8 + x],
                            x_pos: bus.ram[0x33E + x],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_grenade_launcher_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} grenade-launcher-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_GRENADE_LAUNCHER_ROUTINE_01").is_ok() {
            // VERIFY_GRENADE_LAUNCHER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::grenade_launcher::grenade_launcher_
            // routine_01`. Real entry $9479 (switchable bank, gated).
            // Real exits (all switchable bank, all gated): `@exit`
            // ($94b0, the `Cooldown/Redirected` outcome), `@launch_
            // grenade_exit` ($94c6, `Cooldown/LaunchCheck`'s `NotReady`/
            // `Waiting` cases), `enemy_launch_grenade`'s own `@exit`
            // ($9773, `Cooldown/LaunchCheck`'s `Launched` case - same
            // shared tail `indoor_soldier_routine_01`'s own grenade
            // branch reaches), and `grenade_launcher_exit` ($9515, both
            // `ApplyVelAim` sub-cases).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<GrenadeLauncherRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9479 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(GrenadeLauncherRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attack_flag: bus.ram[0x8E],
                            var_3: bus.ram[0x5D8 + x],
                            animation_delay: bus.ram[0x538 + x],
                            attack_delay: bus.ram[0x558 + x],
                            var_1: bus.ram[0x5B8 + x],
                            attributes: bus.ram[0x5A8 + x],
                            frame_counter: bus.ram[0x1A],
                            frame: bus.ram[0x568 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            var_2: bus.ram[0x5C8 + x],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            enemy_routine,
                        });
                    }
                    0x94B0 | 0x94C6 | 0x9773 | 0x9515 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_grenade_launcher_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} grenade-launcher-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_FOUR_SOLDIERS_ROUTINE_00").is_ok() {
            // VERIFY_FOUR_SOLDIERS_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::four_soldiers::four_soldiers_
            // routine_00`. Real entry $9541 (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // advance_enemy_routine` - the nested `jsr init_indoor_
            // enemy_pos_and_vel`/`jsr four_soldiers_set_firing_delay`
            // never revisit either address, so no disambiguation is
            // needed.
            let mut pending: Option<FourSoldiersRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9541 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(FourSoldiersRoutine00Ctx {
                            x,
                            attributes: bus.ram[0x5A8 + x],
                            soldier_index: bus.ram[0x5B8 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_four_soldiers_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} four-soldiers-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_FOUR_SOLDIERS_ROUTINE_01").is_ok() {
            // VERIFY_FOUR_SOLDIERS_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::four_soldiers::four_soldiers_
            // routine_01`. Real entry $954c (switchable bank, gated).
            // Real exits (all switchable, all gated): `four_soldiers_
            // exit` ($9575, the `Waiting` outcome), `create_indoor_
            // bullet`'s own `@exit` ($97c3, the `Fired` outcome, same
            // shared tail `indoor_soldier_routine_01`'s own bullet
            // branch reaches), and the 2 shared exits ($e796/$e813, the
            // `Advanced` outcome via `jmp advance_enemy_routine`).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<FourSoldiersRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x954C if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(FourSoldiersRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attack_flag: bus.ram[0x8E],
                            animation_delay: bus.ram[0x538 + x],
                            soldier_index: bus.ram[0x5B8 + x],
                            times_fired: bus.ram[0x5C8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine,
                        });
                    }
                    0x9575 | 0x97C3 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_four_soldiers_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_four_soldiers_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} four-soldiers-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_FOUR_SOLDIERS_ROUTINE_02").is_ok() {
            // VERIFY_FOUR_SOLDIERS_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::four_soldiers::four_soldiers_
            // routine_02`. Real entry $9582 (switchable bank, gated).
            // Real exits: `four_soldiers_exit` ($9575, switchable,
            // gated, the `StillMoving` outcome - same shared address
            // `four_soldiers_routine_01`'s own `Waiting` outcome
            // reaches), plus the 2 shared exits ($e796/$e813, the
            // `Fired` outcome via `jmp set_enemy_routine_to_a`).
            let mut pending: Option<FourSoldiersRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x9582 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(FourSoldiersRoutine02Ctx {
                            x,
                            frame_counter: bus.ram[0x1A],
                            enemy_frame: bus.ram[0x568 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            x_pos: bus.ram[0x33E + x],
                            animation_delay: bus.ram[0x538 + x],
                            soldier_index: bus.ram[0x5B8 + x],
                            times_fired: bus.ram[0x5C8 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x9575 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_four_soldiers_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_four_soldiers_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} four-soldiers-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_SOLDIER_GEN_ROUTINE_00").is_ok() {
            // VERIFY_INDOOR_SOLDIER_GEN_ROUTINE_00=1: verification pass
            // for `contra_native::enemy::indoor_soldier_gen::indoor_
            // soldier_gen_routine_00`. Real entry $8d1f (switchable
            // bank, gated). Real exit: the 2 shared exits ($e796/$e813)
            // via `jmp advance_enemy_routine` - no nested calls in this
            // routine at all, so no disambiguation is needed.
            let mut pending: Option<IndoorSoldierGenRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8D1F if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(IndoorSoldierGenRoutine00Ctx { x, routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_soldier_gen_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} indoor-soldier-gen-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_SOLDIER_GEN_ROUTINE_01").is_ok() {
            // VERIFY_INDOOR_SOLDIER_GEN_ROUTINE_01=1: verification pass
            // for `contra_native::enemy::indoor_soldier_gen::indoor_
            // soldier_gen_routine_01`. Real entry $8d28 (switchable
            // bank, gated). Real exits: `indoor_soldier_gen_exit` ($8d27,
            // switchable, gated - the `EvenFrame`/`GrenadeLauncherOn
            // Screen`/`StillWaiting` outcomes), `indoor_soldier_gen_
            // routine_exit` ($8dcc, switchable, gated - the `Entry`
            // outcome, both the single-enemy and group-of-4 spawn paths),
            // and `remove_enemy`'s own address ($e809, fixed bank - the
            // `RoundsExhausted` outcome via `jmp remove_enemy`).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<IndoorSoldierGenRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8D28 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(IndoorSoldierGenRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            frame_counter: bus.ram[0x1A],
                            grenade_launcher_flag: bus.ram[0x8A],
                            animation_delay: bus.ram[0x538 + x],
                            attributes: bus.ram[0x5A8 + x],
                            level_screen_number: bus.ram[0x64],
                            var_1: bus.ram[0x5B8 + x],
                            indoor_enemy_attack_count: bus.ram[0x88],
                            enemy_routine,
                        });
                    }
                    0x8D27 | 0x8DCC if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_soldier_gen_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE809 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_soldier_gen_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} indoor-soldier-gen-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_ROLLER_GEN_ROUTINE_00").is_ok() {
            // VERIFY_INDOOR_ROLLER_GEN_ROUTINE_00=1: verification pass
            // for `contra_native::enemy::indoor_roller_gen::indoor_
            // roller_gen_routine_00`. Real entry $95c8 (switchable bank,
            // gated). Real exit: the 2 shared exits ($e796/$e813) via
            // `jmp advance_enemy_routine` - no nested calls at all, so
            // no disambiguation is needed.
            let mut pending: Option<IndoorRollerGenRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x95C8 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(IndoorRollerGenRoutine00Ctx { x, routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_roller_gen_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} indoor-roller-gen-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_INDOOR_ROLLER_GEN_ROUTINE_01").is_ok() {
            // VERIFY_INDOOR_ROLLER_GEN_ROUTINE_01=1: verification pass
            // for `contra_native::enemy::indoor_roller_gen::indoor_
            // roller_gen_routine_01`. Real entry $95cd (switchable bank,
            // gated). Real exits: `@exit` ($9628, switchable, gated -
            // the `EvenFrame`/`StillWaiting`/`Entry` outcomes; every
            // `jsr create_roller_with_segment_a` inside the roller-
            // creation loop returns to a point *before* this address, so
            // no nested-return ambiguity), and `remove_enemy`'s own
            // address ($e809, fixed bank - the `RoundsExhausted`
            // outcome via `jmp remove_enemy`, reached with no nested
            // `jsr`s along the way at all).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<IndoorRollerGenRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x95CD if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(IndoorRollerGenRoutine01Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            attack_flag: bus.ram[0x8E],
                            indoor_enemy_attack_count: bus.ram[0x88],
                            frame_counter: bus.ram[0x1A],
                            animation_delay: bus.ram[0x538 + x],
                            attributes: bus.ram[0x5A8 + x],
                            var_1: bus.ram[0x5B8 + x],
                            enemy_routine,
                        });
                    }
                    0x9628 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_roller_gen_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE809 => {
                        if let Some(ctx) = pending.take() {
                            verify_indoor_roller_gen_routine_01(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} indoor-roller-gen-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_WEAPON_ITEM_ROUTINE_00").is_ok() {
            // VERIFY_WEAPON_ITEM_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::weapon_item::weapon_item_routine_00`.
            // Real entry $8007 (switchable bank, gated). Real exit: the
            // 2 shared exits ($e796/$e813) via `weapon_item_advance_
            // enemy_routine` ($8059) -> `jmp advance_enemy_routine` -
            // both the indoor and outdoor branches funnel through this
            // one tail-jump, no other real exit exists. The nested `jsr
            // set_weapon_item_indoor_velocity` (`$ed9d`, fixed bank,
            // itself calling `jsr find_far_segment_for_a`) never
            // revisits either shared exit, so no disambiguation is
            // needed.
            let mut pending: Option<WeaponItemRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8007 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(WeaponItemRoutine00Ctx {
                            x,
                            level_location_type: bus.ram[0x40],
                            y_pos: bus.ram[0x324 + x],
                            x_pos: bus.ram[0x33E + x],
                            level_scrolling_type: bus.ram[0x41],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_weapon_item_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} weapon-item-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_BULLET_ROUTINE_00").is_ok() {
            // VERIFY_ENEMY_BULLET_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::enemy_bullet::enemy_bullet_
            // routine_00`. Real entry $814f (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // advance_enemy_routine` - no nested calls at all in this
            // trivial routine, so no disambiguation is needed.
            let mut pending: Option<EnemyBulletRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x814F if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(EnemyBulletRoutine00Ctx { x, bullet_type: bus.ram[0x5B8 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_enemy_bullet_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-bullet-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_BULLET_ROUTINE_01").is_ok() {
            // VERIFY_ENEMY_BULLET_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::enemy_bullet::enemy_bullet_
            // routine_01`. Real entry $8161 (switchable bank, gated).
            // Real exits: `enemy_bullet_routine_01_exit` ($81a5,
            // switchable, gated - the `Exited`/`DragonOrbAnimated`
            // outcomes), `enemy_bullet_exit` ($8201, switchable, gated -
            // `StillFalling`/`IndoorOnScreen`), the 2 shared exits
            // ($e796/$e813, `Exploded`, via `adv_bullet_routine`'s own
            // `jmp advance_enemy_routine` - no nested calls reach these
            // 2 addresses so no disambiguation needed there), and
            // `remove_enemy`'s own address ($e809, fixed bank -
            // `RemovedBySolidCollision`/`IndoorRemoved`, **with**
            // disambiguation: `update_enemy_pos`'s own internal removal
            // (real ASM: `jmp remove_enemy`, a tail-jump) also reaches
            // $e809 as a *nested* occurrence that returns straight back
            // into this routine's own body (since `jsr update_enemy_pos`
            // pushed the return address, not the nested jmp) - if the
            // stack's return address falls inside this routine's own
            // $8161-$81a5 range, it's that nested case, not our real
            // exit; keep waiting).
            use contra_native::physics::collision::BG_COLLISION_DATA_LEN;

            let mut pending: Option<EnemyBulletRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8161 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut bg_collision_data = [0u8; BG_COLLISION_DATA_LEN];
                        for (i, b) in bg_collision_data.iter_mut().enumerate() {
                            *b = bus.ram[0x0680 + i];
                        }
                        pending = Some(EnemyBulletRoutine01Ctx {
                            x,
                            bullet_type: bus.ram[0x5B8 + x],
                            current_level: bus.ram[0x30],
                            level_solid_bg_collision_check: bus.ram[0x59],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            vertical_scroll: bus.ram[0xFC],
                            horizontal_scroll: bus.ram[0xFD],
                            ppuctrl_settings: bus.ram[0xFF],
                            bg_collision_data,
                            frame_counter: bus.ram[0x1A],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x81A5 | 0x8201 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_enemy_bullet_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_enemy_bullet_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE809 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x8161..0x81A6).contains(&ret) {
                            // Nested return from `jsr update_enemy_pos`'s
                            // own internal removal - not our exit, keep
                            // waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_enemy_bullet_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-bullet-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ENEMY_BULLET_ROUTINE_02").is_ok() {
            // VERIFY_ENEMY_BULLET_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::enemy_bullet::enemy_bullet_
            // routine_02`. Real entry $81e4 (switchable bank, gated).
            // Real exits: `enemy_bullet_exit` ($8201, switchable, gated -
            // the `Waiting`/`Animating` outcomes, same shared address
            // `enemy_bullet_routine_01`'s own `StillFalling`/
            // `IndoorOnScreen` outcomes reach - harmless since these are
            // separate, mutually-exclusive `VERIFY_` modes), and the 2
            // shared exits ($e796/$e813, `Advanced`, via `jmp advance_
            // enemy_routine`). The nested `jsr add_scroll_to_enemy_pos`
            // can itself trigger a nested removal (`$e809`), but that
            // address isn't one of this routine's own real exits, so no
            // disambiguation is needed here.
            let mut pending: Option<EnemyBulletRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x81E4 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(EnemyBulletRoutine02Ctx {
                            x,
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            animation_delay: bus.ram[0x538 + x],
                            frame: bus.ram[0x568 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x8201 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_enemy_bullet_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_enemy_bullet_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} enemy-bullet-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_FLYING_CAPSULE_ROUTINE_00").is_ok() {
            // VERIFY_FLYING_CAPSULE_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::flying_capsule::flying_capsule_
            // routine_00`. Real entry $830b (switchable bank, gated).
            // Real exit: the 2 shared exits ($e796/$e813) via `jmp
            // advance_enemy_routine` - no nested calls reach either
            // address, so no disambiguation is needed.
            let mut pending: Option<FlyingCapsuleRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x830B if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(FlyingCapsuleRoutine00Ctx {
                            x,
                            level_scrolling_type: bus.ram[0x41],
                            y_pos: bus.ram[0x324 + x],
                            x_pos: bus.ram[0x33E + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_flying_capsule_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} flying-capsule-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_FLYING_CAPSULE_ROUTINE_01").is_ok() {
            // VERIFY_FLYING_CAPSULE_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::flying_capsule::flying_capsule_
            // routine_01`. Real entry $835d (switchable bank, gated).
            // This routine's own body ends in a tail `jmp update_enemy_
            // pos`, so its real exits are that routine's own: `apply_
            // vel_exit` ($e849, fixed bank, success) and `remove_enemy`'s
            // own address ($e809, fixed bank, off-screen removal). The
            // nested `jsr set_flying_capsule_y_vel`/`_x_vel` (pure
            // arithmetic, no further calls) never reach either address,
            // so no disambiguation is needed.
            let mut pending: Option<FlyingCapsuleRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x835D if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(FlyingCapsuleRoutine01Ctx {
                            x,
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            var_2: bus.ram[0x5C8 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            var_1: bus.ram[0x5B8 + x],
                        });
                    }
                    0xE849 | 0xE809 => {
                        if let Some(ctx) = pending.take() {
                            verify_flying_capsule_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} flying-capsule-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_FLYING_CAPSULE_ROUTINE_02").is_ok() {
            // VERIFY_FLYING_CAPSULE_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::flying_capsule::flying_capsule_
            // routine_02`. Real entry $8376 (switchable bank, gated) - a
            // one-line `jmp play_explosion_sound`, so its own real exit
            // is that routine's own `weapon_box_exit_1` ($82f4,
            // switchable bank, gated). Nothing nested reaches that
            // address, so no disambiguation is needed.
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<FlyingCapsuleRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8376 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(FlyingCapsuleRoutine02Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            attributes: bus.ram[0x5A8 + x],
                            enemy_routine,
                        });
                    }
                    0x82F4 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_flying_capsule_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} flying-capsule-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SNIPER_ROUTINE_00").is_ok() {
            // VERIFY_SNIPER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::sniper::sniper_routine_00`. Real
            // entry $8958 (switchable bank, gated). Real exit: the 2
            // shared exits ($e796/$e813) via `jmp advance_enemy_
            // routine` - the nested `jsr add_4_to_enemy_y_pos`/`jsr
            // add_a_to_enemy_y_pos` (both pure position arithmetic, no
            // further calls) never revisit either address, so no
            // disambiguation is needed.
            let mut pending: Option<SniperRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8958 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SniperRoutine00Ctx {
                            x,
                            sniper_type: bus.ram[0x5A8 + x],
                            y_pos: bus.ram[0x324 + x],
                            vertical_scroll: bus.ram[0xFC],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_sniper_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} sniper-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SNIPER_ROUTINE_01").is_ok() {
            // VERIFY_SNIPER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::sniper::sniper_routine_01`. Real
            // entry $8982 (switchable bank, gated). Real exits:
            // `sniper_routine_exit` ($89cb, switchable, gated - the
            // `Waiting`/`CrouchCycling` outcomes), and the 2 shared
            // exits ($e796/$e813, the `Activated` outcome via `jmp
            // advance_enemy_routine`). The nested `jsr sniper_set_
            // sprite`/`jsr add_scroll_to_enemy_pos`/`jsr enable_enemy_
            // collision` never revisit either address (their own
            // internal removal, if any, goes through `remove_enemy`'s
            // own unrelated address instead), so no disambiguation is
            // needed.
            let mut pending: Option<SniperRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8982 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(SniperRoutine01Ctx {
                            x,
                            sniper_type: bus.ram[0x5A8 + x],
                            frame: bus.ram[0x568 + x],
                            var_2: bus.ram[0x5C8 + x],
                            var_3: bus.ram[0x5D8 + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            animation_delay: bus.ram[0x538 + x],
                            state_width: bus.ram[0x598 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0x89CB if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_sniper_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_sniper_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} sniper-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_TURRET_MAN_ROUTINE_00").is_ok() {
            // VERIFY_TURRET_MAN_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::turret_man::turret_man_routine_00`.
            // Real entry $f0c9 (fixed bank, no gate needed). Real exit:
            // the 2 shared exits ($e796/$e813) via `jmp set_anim_delay_
            // adv_enemy_routine`. No nested calls, so no disambiguation
            // is needed.
            let mut pending: Option<TurretManRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF0C9 => {
                        let x = cpu.x as usize;
                        pending = Some(TurretManRoutine00Ctx { x, enemy_attributes: bus.ram[0x5A8 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_turret_man_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} turret-man-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_TURRET_MAN_ROUTINE_01").is_ok() {
            // VERIFY_TURRET_MAN_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::turret_man::turret_man_routine_01`.
            // Real entry $f0db (fixed bank). Real exits: `turret_man_
            // exit` ($f0eb, `Waiting`), and the 2 shared exits
            // ($e796/$e813, `RecoilStarted`, via `jmp set_anim_delay_adv_
            // enemy_routine`). The nested `jsr add_scroll_to_enemy_pos`
            // can itself trigger a nested removal that also reaches
            // $e813 (falling through `remove_enemy`/`set_sprite_0`) -
            // disambiguated via stack-peek: if the return address falls
            // inside this routine's own $f0db-$f0ec range, it's that
            // nested case, not our real exit.
            let mut pending: Option<TurretManRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF0DB => {
                        let x = cpu.x as usize;
                        pending = Some(TurretManRoutine01Ctx {
                            x,
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            animation_delay: bus.ram[0x538 + x],
                            sprite: bus.ram[0x30A + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xF0EB | 0xE796 => {
                        if let Some(ctx) = pending.take() {
                            verify_turret_man_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xF0DB..0xF0EC).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos`'s own internal removal - not our
                            // exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_turret_man_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} turret-man-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_TURRET_MAN_ROUTINE_02").is_ok() {
            // VERIFY_TURRET_MAN_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::turret_man::turret_man_routine_02`.
            // Real entry $f0ec (fixed bank). Real exits: `turret_man_
            // exit` ($f0eb, `Waiting` - same address `_01`'s own
            // `Waiting` exit uses, harmless since these are separate,
            // mutually-exclusive `VERIFY_` modes), `set_enemy_routine_
            // to_a`'s own success rts ($e822, `Fired`), and the shared
            // guard-rejected rts ($e813, `Fired`) - disambiguated the
            // same way as `_01` (nested `jsr add_scroll_to_enemy_pos`
            // removal vs. this routine's own $f0ec-$f119 range).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<TurretManRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF0EC => {
                        let x = cpu.x as usize;
                        let mut enemy_routine_slots = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine_slots.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(TurretManRoutine02Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            enemy_attributes: bus.ram[0x5A8 + x],
                            enemy_sprites: bus.ram[0x30A + x],
                            animation_delay: bus.ram[0x538 + x],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine_slots,
                        });
                    }
                    0xF0EB | 0xE822 => {
                        if let Some(ctx) = pending.take() {
                            verify_turret_man_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xF0EC..0xF119).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos`'s own internal removal - not our
                            // exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_turret_man_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} turret-man-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_TURRET_MAN_BULLET_ROUTINE_00").is_ok() {
            // VERIFY_TURRET_MAN_BULLET_ROUTINE_00=1: verification pass
            // for `contra_native::enemy::turret_man::turret_man_bullet_
            // routine_00`. Real entry $f11f (fixed bank). Real exit: the
            // 2 shared exits ($e796/$e813) via `turret_man_adv_routine`'s
            // own `jmp advance_enemy_routine`. No nested calls, so no
            // disambiguation is needed.
            let mut pending: Option<TurretManBulletRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF11F => {
                        let x = cpu.x as usize;
                        pending = Some(TurretManBulletRoutine00Ctx { x, routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_turret_man_bullet_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} turret-man-bullet-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_TURRET_MAN_BULLET_ROUTINE_01").is_ok() {
            // VERIFY_TURRET_MAN_BULLET_ROUTINE_01=1: verification pass
            // for `contra_native::enemy::turret_man::turret_man_bullet_
            // routine_01`. Real entry $f131 (fixed bank). Real exits: the
            // 2 shared exits ($e796/$e813, `Advanced`, via `turret_man_
            // adv_routine`'s own tail `jmp advance_enemy_routine` -
            // reached directly, no intervening calls), and `update_
            // enemy_pos`'s own real exits ($e849/$e809, `Position`, via
            // this routine's own tail `jmp update_enemy_pos`). No
            // disambiguation needed either way since both jumps are real
            // tail calls, not nested `jsr`s.
            let mut pending: Option<TurretManBulletRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF131 => {
                        let x = cpu.x as usize;
                        pending = Some(TurretManBulletRoutine01Ctx {
                            x,
                            x_pos: bus.ram[0x33E + x],
                            routine: bus.ram[0x4B8 + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                        });
                    }
                    0xE796 | 0xE813 | 0xE849 | 0xE809 => {
                        if let Some(ctx) = pending.take() {
                            verify_turret_man_bullet_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} turret-man-bullet-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SCUBA_SOLDIER_ROUTINE_00").is_ok() {
            // VERIFY_SCUBA_SOLDIER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::scuba_soldier::scuba_soldier_
            // routine_00`. Real entry $f147 (fixed bank). Real exit: the
            // 2 shared exits ($e796/$e813) via `jmp set_anim_delay_adv_
            // enemy_routine`. No nested calls, so no disambiguation is
            // needed.
            let mut pending: Option<ScubaSoldierRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF147 => {
                        let x = cpu.x as usize;
                        pending = Some(ScubaSoldierRoutine00Ctx { x, routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_scuba_soldier_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} scuba-soldier-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SCUBA_SOLDIER_ROUTINE_01").is_ok() {
            // VERIFY_SCUBA_SOLDIER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::scuba_soldier::scuba_soldier_
            // routine_01`. Real entry $f14c (fixed bank). Real exits:
            // `@exit` ($f175, `Waiting`/`NotYetHighEnough`), and the 2
            // shared exits ($e796/$e813, `Activated`, via `jmp set_anim_
            // delay_adv_enemy_routine`) - $e813 disambiguated via
            // stack-peek the same way as `turret_man_routine_01` (nested
            // `jsr add_scroll_to_enemy_pos` removal vs. this routine's
            // own $f14c-$f183 range).
            let mut pending: Option<ScubaSoldierRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF14C => {
                        let x = cpu.x as usize;
                        pending = Some(ScubaSoldierRoutine01Ctx {
                            x,
                            animation_delay: bus.ram[0x538 + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xF175 | 0xE796 => {
                        if let Some(ctx) = pending.take() {
                            verify_scuba_soldier_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xF14C..0xF183).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos`'s own internal removal - not our
                            // exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_scuba_soldier_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} scuba-soldier-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_SCUBA_SOLDIER_ROUTINE_02").is_ok() {
            // VERIFY_SCUBA_SOLDIER_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::scuba_soldier::scuba_soldier_
            // routine_02`. Real entry $f183 (fixed bank). Real exits:
            // `@add_scroll_exit` ($f1b1, `Firing`/`Fired` - this
            // routine's own unique `jmp add_scroll_to_enemy_pos`
            // instruction address, captured *before* it executes so no
            // ambiguity with the shared target), `set_enemy_routine_to_
            // a`'s own success rts ($e822, `Submerging`), and the shared
            // guard-rejected rts ($e813, `Submerging`) - disambiguated
            // via stack-peek (nested `jsr add_scroll_to_enemy_pos`
            // removal vs. this routine's own $f183-$f1c4 range).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<ScubaSoldierRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF183 => {
                        let x = cpu.x as usize;
                        let mut enemy_routine_slots = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine_slots.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(ScubaSoldierRoutine02Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            var_1: bus.ram[0x5B8 + x],
                            animation_delay: bus.ram[0x538 + x],
                            attack_delay: bus.ram[0x558 + x],
                            state_width: bus.ram[0x598 + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine_slots,
                        });
                    }
                    0xF1B1 | 0xE822 => {
                        if let Some(ctx) = pending.take() {
                            verify_scuba_soldier_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xF183..0xF1C4).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos`'s own internal removal - not our
                            // exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_scuba_soldier_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} scuba-soldier-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_MORTAR_SHOT_ROUTINE_00").is_ok() {
            // VERIFY_MORTAR_SHOT_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::mortar_shot::mortar_shot_
            // routine_00`. Real entry $f1d6 (fixed bank). Real exit: the
            // 2 shared exits ($e796/$e813) via `mortar_shot_adv_
            // routine`'s own `jmp advance_enemy_routine`. No nested
            // calls, so no disambiguation is needed.
            let mut pending: Option<MortarShotRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF1D6 => {
                        let x = cpu.x as usize;
                        pending = Some(MortarShotRoutine00Ctx { x, enemy_attributes: bus.ram[0x5A8 + x], enemy_var_1: bus.ram[0x5B8 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_mortar_shot_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} mortar-shot-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_MORTAR_SHOT_ROUTINE_01").is_ok() {
            // VERIFY_MORTAR_SHOT_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::mortar_shot::mortar_shot_
            // routine_01`. Real entry $f237 (fixed bank). Real exits:
            // `@mortar_shot_routine_01_exit` ($f24e, `StillRising`/
            // `SplitStillRising`/`SplitAboveClosestPlayer`/
            // `SplitNoBgCollision` - all 4 converge here), `advance_
            // enemy_routine`'s own success rts ($e796, `Advanced`),
            // `set_enemy_routine_to_a`'s own success rts ($e822,
            // `SplitCollided`), and the shared guard-rejected rts
            // ($e813, either `Advanced` or `SplitCollided`) - $e813
            // disambiguated via stack-peek (this routine's own `jsr
            // update_enemy_pos` can itself trigger a nested removal that
            // also reaches $e813; genuine hits return outside this
            // routine's own $f237-$f26e range).
            let mut pending: Option<MortarShotRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF237 => {
                        let x = cpu.x as usize;
                        let mut bg_collision_data = [0u8; contra_native::physics::collision::BG_COLLISION_DATA_LEN];
                        for (i, b) in bg_collision_data.iter_mut().enumerate() {
                            *b = bus.ram[0x0680 + i];
                        }
                        pending = Some(MortarShotRoutine01Ctx {
                            x,
                            enemy_attributes: bus.ram[0x5A8 + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            sprite_x_pos: [bus.ram[0x334], bus.ram[0x335]],
                            sprite_y_pos: [bus.ram[0x31A], bus.ram[0x31B]],
                            player_state: [bus.ram[0x90], bus.ram[0x91]],
                            vertical_scroll: bus.ram[0xFC],
                            horizontal_scroll: bus.ram[0xFD],
                            ppuctrl_settings: bus.ram[0xFF],
                            bg_collision_data,
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xF24E | 0xE796 | 0xE822 => {
                        if let Some(ctx) = pending.take() {
                            verify_mortar_shot_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xF237..0xF26E).contains(&ret) {
                            // Nested return from `jsr update_enemy_pos`'s
                            // own internal removal - not our exit, keep
                            // waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_mortar_shot_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} mortar-shot-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_MORTAR_SHOT_ROUTINE_02").is_ok() {
            // VERIFY_MORTAR_SHOT_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::mortar_shot::mortar_shot_
            // routine_02`. Real entry $f26e (fixed bank). Real exits:
            // `advance_enemy_routine`'s own success rts ($e796), and the
            // shared guard-rejected rts ($e813) - disambiguated via
            // stack-peek the same way as `_01` (nested `jsr update_
            // enemy_pos` removal vs. this routine's own $f26e-$f29e
            // range, which also covers its own local `@advance_enemy_
            // routine` slot-restore helper).
            use contra_native::enemy::enemy_slots::ENEMY_SLOT_COUNT;

            let mut pending: Option<MortarShotRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xF26E => {
                        let x = cpu.x as usize;
                        let mut enemy_routine_slots = [0u8; ENEMY_SLOT_COUNT];
                        for (i, b) in enemy_routine_slots.iter_mut().enumerate() {
                            *b = bus.ram[0x4B8 + i];
                        }
                        pending = Some(MortarShotRoutine02Ctx {
                            x,
                            current_level: bus.ram[0x30],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            y_pos: bus.ram[0x324 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            routine: bus.ram[0x4B8 + x],
                            enemy_routine_slots,
                        });
                    }
                    0xE796 => {
                        if let Some(ctx) = pending.take() {
                            verify_mortar_shot_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0xF26E..0xF29E).contains(&ret) {
                            // Nested return from `jsr update_enemy_pos`'s
                            // own internal removal - not our exit, keep
                            // waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_mortar_shot_routine_02(ctx, &prg_rom_copy, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} mortar-shot-routine-02 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_MORTAR_SHOT_ROUTINE_03").is_ok() {
            // VERIFY_MORTAR_SHOT_ROUTINE_03=1: verification pass for
            // `contra_native::enemy::mortar_shot::mortar_shot_
            // routine_03`. Real entry $e752 (fixed bank - also reused by
            // ice grenades, so hits aren't exclusively from split
            // mortars). Real exits: `remove_enemy`'s own address ($e809,
            // `Removed`, direct tail `jmp remove_enemy` from entry - no
            // intervening calls), and the 2 shared exits ($e796/$e813,
            // `Hidden`, via `jmp set_enemy_delay_adv_routine`). This
            // routine's own `jsr add_scroll_to_enemy_pos` (inside the
            // `Hidden` path only) can itself trigger a nested removal
            // that also reaches $e809/$e813 - both disambiguated via
            // stack-peek against this routine's own $e752-$e78b range
            // (genuine hits, reached via a real tail `jmp`, return
            // outside that range - this routine's own caller, not
            // anything inside it).
            let mut pending: Option<MortarShotRoutine03Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0xE752 => {
                        let x = cpu.x as usize;
                        pending = Some(MortarShotRoutine03Ctx {
                            x,
                            state_width: bus.ram[0x598 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            sprites: bus.ram[0x30A + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            y_pos: bus.ram[0x324 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE809 | 0xE796 | 0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if cpu.pc != 0xE796 && (0xE752..0xE78B).contains(&ret) {
                            // Nested return from `jsr add_scroll_to_
                            // enemy_pos`'s own internal removal (only
                            // possible at $e809/$e813 - $e796 is only
                            // ever reached via advance_enemy_routine's
                            // own real tail jmp) - not our exit, keep
                            // waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_mortar_shot_routine_03(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} mortar-shot-routine-03 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ROLLER_ROUTINE_00").is_ok() {
            // VERIFY_ROLLER_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::roller::roller_routine_00`. Real
            // entry $8f8c (switchable bank, gated). Real exit: the 2
            // shared exits ($e796/$e813) via `jmp advance_enemy_
            // routine`. No nested calls, so no disambiguation is needed.
            let mut pending: Option<RollerRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8F8C if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(RollerRoutine00Ctx { x, routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_roller_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} roller-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_ROLLER_ROUTINE_01").is_ok() {
            // VERIFY_ROLLER_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::roller::roller_routine_01`. Real
            // entry $8f94 (switchable bank, gated). Real exits: `@exit`
            // ($8fc2, `NotCloseEnough`), the shared collision-toggle rts
            // ($eb1e, `CollisionEnabled`, via `jmp enable_enemy_
            // collision`), and `remove_enemy`'s own address ($e809,
            // `Removed`, via this routine's own tail `jmp remove_enemy`)
            // - $e809 disambiguated via stack-peek: this routine's own
            // `jsr update_enemy_pos` can itself trigger a nested removal
            // that also reaches $e809; genuine hits return outside this
            // routine's own $8f94-$8fc6 range.
            let mut pending: Option<RollerRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8F94 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(RollerRoutine01Ctx {
                            x,
                            y_pos: bus.ram[0x324 + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            state_width: bus.ram[0x598 + x],
                        });
                    }
                    0x8FC2 if bus.mapper.bank_select() == 0 => {
                        if let Some(ctx) = pending.take() {
                            verify_roller_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xEB1E => {
                        if let Some(ctx) = pending.take() {
                            verify_roller_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE809 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x8F94..0x8FC6).contains(&ret) {
                            // Nested return from `jsr update_enemy_pos`'s
                            // own internal removal - not our exit, keep
                            // waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_roller_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} roller-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_GRENADE_ROUTINE_00").is_ok() {
            // VERIFY_GRENADE_ROUTINE_00=1: verification pass for
            // `contra_native::enemy::grenade::grenade_routine_00`. Real
            // entry $8fd5 (switchable bank, gated). Real exit: the 2
            // shared exits ($e796/$e813) via `grenade_adv_routine`'s own
            // `jmp advance_enemy_routine`. No nested calls, so no
            // disambiguation is needed.
            let mut pending: Option<GrenadeRoutine00Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8FD5 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(GrenadeRoutine00Ctx { x, y_pos: bus.ram[0x324 + x], routine: bus.ram[0x4B8 + x] });
                    }
                    0xE796 | 0xE813 => {
                        if let Some(ctx) = pending.take() {
                            verify_grenade_routine_00(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} grenade-routine-00 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_GRENADE_ROUTINE_01").is_ok() {
            // VERIFY_GRENADE_ROUTINE_01=1: verification pass for
            // `contra_native::enemy::grenade::grenade_routine_01`. Real
            // entry $8fe8 (switchable bank, gated). This mode only
            // verifies the `Some(routine_update)` outcome (real ASM's
            // `bpl grenade_adv_routine`, reaching the 2 shared exits
            // $e796/$e813) - the `None`/"stayed" outcome's own `rts` has
            // no exported label to hook (unlike `roller_routine_01`'s
            // `@exit` or `turret_man_routine_01`'s `turret_man_exit`),
            // so it isn't independently exercised here; every real field
            // this routine touches (sprite/frame/arc vars) still gets
            // checked on whichever call happens to reach the advance
            // path. $e813 disambiguated via stack-peek: this routine's
            // own `jsr set_enemy_falling_arc_pos` can itself trigger a
            // nested removal that also reaches $e813 (falling through
            // `remove_enemy`/`set_sprite_0`) and returns into this
            // routine's own remaining code (same call shape `jsr
            // update_enemy_pos` nested removals use elsewhere) - genuine
            // hits return outside this routine's own $8fe8-$9049 range.
            let mut pending: Option<GrenadeRoutine01Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x8FE8 if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(GrenadeRoutine01Ctx {
                            x,
                            var_1: bus.ram[0x5B8 + x],
                            var_2: bus.ram[0x5C8 + x],
                            var_3: bus.ram[0x5D8 + x],
                            var_4: bus.ram[0x5E8 + x],
                            var_b: bus.ram[0x558 + x],
                            enemy_frame: bus.ram[0x568 + x],
                            frame_counter: bus.ram[0x1A],
                            attack_delay: bus.ram[0x558 + x],
                            y_vel_accum: bus.ram[0x4C8 + x],
                            y_vel_fract: bus.ram[0x4F8 + x],
                            y_vel_fast: bus.ram[0x4E8 + x],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            x_vel_accum: bus.ram[0x4D8 + x],
                            x_vel_fract: bus.ram[0x518 + x],
                            x_vel_fast: bus.ram[0x508 + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 => {
                        if let Some(ctx) = pending.take() {
                            verify_grenade_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x8FE8..0x9049).contains(&ret) {
                            // Nested return from `jsr set_enemy_falling_
                            // arc_pos`'s own internal removal - not our
                            // exit, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_grenade_routine_01(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} grenade-routine-01 calls verified this frame, no mismatches unless printed above");
            }
        } else if std::env::var("VERIFY_GRENADE_ROUTINE_02").is_ok() {
            // VERIFY_GRENADE_ROUTINE_02=1: verification pass for
            // `contra_native::enemy::grenade::grenade_routine_02`. Real
            // entry $907c (switchable bank, gated). Real exit: the 2
            // shared exits ($e796/$e813) via this routine's own trailing
            // `jmp advance_enemy_routine` - reached unconditionally,
            // always *after* `jsr mortar_shot_routine_03` already
            // returns (see this crate's `grenade` module doc comment for
            // why that `jsr`, not the usual tail `jmp`, means the enemy
            // routine index really does advance twice on the "still had
            // a sprite" path). `$e813 disambiguated via stack-peek: `
            // `mortar_shot_routine_03`'s own internal removal can itself
            // reach $e813 first (a real, *expected* nested hit here, not
            // a bug) before this routine's own final tail jump reaches
            // it again - genuine (final) hits return outside this
            // routine's own $907c-$908c range.
            let mut pending: Option<GrenadeRoutine02Ctx> = None;
            let mut checked = 0u64;
            nes.run_frame_with_hook(&mut |cpu, bus| {
                match cpu.pc {
                    0x907C if bus.mapper.bank_select() == 0 => {
                        let x = cpu.x as usize;
                        pending = Some(GrenadeRoutine02Ctx {
                            x,
                            state_width: bus.ram[0x598 + x],
                            sprite_attr: bus.ram[0x358 + x],
                            sprites: bus.ram[0x30A + x],
                            level_scrolling_type: bus.ram[0x41],
                            frame_scroll: bus.ram[0x68],
                            x_pos: bus.ram[0x33E + x],
                            routine: bus.ram[0x4B8 + x],
                        });
                    }
                    0xE796 => {
                        if let Some(ctx) = pending.take() {
                            verify_grenade_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    0xE813 => {
                        let sp = cpu.sp as usize;
                        let ret_lo = bus.ram[0x100 + ((sp + 1) & 0xFF)] as u16;
                        let ret_hi = bus.ram[0x100 + ((sp + 2) & 0xFF)] as u16;
                        let ret = ret_lo | (ret_hi << 8);
                        if (0x907C..0x908C).contains(&ret) {
                            // Nested return from `jsr mortar_shot_
                            // routine_03`'s own internal removal - not
                            // our final exit yet, keep waiting.
                        } else if let Some(ctx) = pending.take() {
                            verify_grenade_routine_02(ctx, cpu, bus, frame, &mut checked);
                        }
                    }
                    _ => {}
                }
                HookAction::Continue
            });
            if checked > 0 {
                eprintln!("frame={frame}: {checked} grenade-routine-02 calls verified this frame, no mismatches unless printed above");
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
