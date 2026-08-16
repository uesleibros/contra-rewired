//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::sound_engine`: runs real gameplay through
//! `contra-nes` (triggering real low-format sound effects on slots #$04/
//! #$05 naturally), and every time a *new* sound_code is triggered on
//! one of those slots, spins up a fresh `contra_native::sound_engine::
//! SoundSlot`, steps it in lockstep with real frames, and compares its
//! computed `cfg_low`/`cfg_high`/`period`/`cmd_length` against the real
//! RAM state at the same point - mechanical, exhaustive verification
//! against real hardware instead of trusting a single hand-picked
//! example.
//!
//! ```text
//! cargo run -p contra-nes --release --example verify_sound_engine -- <rom> [frames]
//! ```
//!
//! ## Known source of remaining mismatches: NMI reentrancy, not sound_code
//!
//! A real chunk of the mismatches this tool reports are **not**
//! `sound_engine`/`sound_code` bugs - they're an artifact of this tool
//! stepping the native engine exactly once per `Nes::run_frame()` call,
//! while real Contra's `handle_sound_code` can run *more than once* per
//! visual frame. Traced and confirmed directly: `NMI_CHECK` (`$001B`,
//! `src/ram.asm`) sits at `0x01` continuously during ordinary gameplay in
//! this test's movement/combat scenario - meaning `nmi_start` (`src/
//! bank7.asm`) is *still inside* a previous NMI's handler body (which
//! includes `exe_game_routine` - the actual player/enemy logic) when the
//! next vblank's NMI fires. Real 6502 NMI is edge-triggered and
//! non-maskable: it reenters `nmi_start` regardless, and `nmi_start`
//! itself detects this (`ldy NMI_CHECK; bne handle_sounds_set_ppu_
//! scroll_rti`) and takes an alternate path that *skips* `exe_game_
//! routine` but *still* calls `handle_sound_slots` (`src/bank7.asm:355-
//! 369`). `contra-nes` is cycle-accurate (`Nes::run_frame`'s scanline-
//! paced CPU budget), so this is real, hardware-accurate slowdown
//! behavior being faithfully reproduced - not an emulator bug. The
//! practical effect: `handle_sound_code` for an active slot can run
//! zero, one, or more than one time within what this tool calls "one
//! frame", so a strict 1:1 `run_frame()` <-> `step_low()` comparison
//! will show spurious mismatches during any lag-heavy stretch. A fully
//! accurate version of this tool would step the native engine once per
//! *actual* `handle_sound_slots` invocation (hookable via `Nes::
//! run_frame_with_hook` at that routine's CPU address) rather than once
//! per visual frame - not yet implemented. This doesn't affect the
//! eventual native PC port itself: a from-scratch game loop has no 6502
//! cycle budget to blow, so it never needs to replicate this reentrancy
//! at all.

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};
use contra_native::sound_engine::{SharedScratch, SoundSlot};

const SOUND_CODE: u16 = 0x106;
const SOUND_CMD_LENGTH: u16 = 0x100;
const SOUND_CFG_LOW: u16 = 0x142;
const SOUND_CFG_HIGH: u16 = 0x14e;
const PULSE_VOLUME: u16 = 0x160;
const INIT_SOUND_CODE: u16 = 0x0122;
const SOUND_CHNL_REG_OFFSET: u16 = 0x0123;

const SOUND_TABLE_00_PRG_OFFSET: usize = 0x48E8;

fn bank1_prg_offset(mem_addr: u16) -> usize {
    0x4000 + (mem_addr as usize & 0x3FFF)
}

/// Resolves a sound code's real starting PRG-ROM offset directly from
/// `sound_table_00` - unlike reading `SOUND_CMD_LOW/HIGH_ADDR` out of live
/// RAM, this isn't affected by the fact that triggering a sound causes an
/// *immediate* same-frame command read (`SOUND_CMD_LENGTH` starts at 1),
/// which already advances those RAM pointers past the start by the time
/// a caller can observe the trigger having happened.
fn sound_start_prg_offset(prg_rom: &[u8], sound_code: u8) -> usize {
    let base = SOUND_TABLE_00_PRG_OFFSET + sound_code as usize * 3;
    let mem_addr = u16::from_le_bytes([prg_rom[base + 1], prg_rom[base + 2]]);
    bank1_prg_offset(mem_addr)
}

struct Tracker {
    engine: Option<SoundSlot>,
    checks: usize,
    mismatches: usize,
    new_note_checks: usize,
    new_note_mismatches: usize,
}

impl Tracker {
    fn new() -> Self {
        Self { engine: None, checks: 0, mismatches: 0, new_note_checks: 0, new_note_mismatches: 0 }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: verify_sound_engine <rom> [frames]");
    let frame_count: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(1200);

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let prg_rom = rom.prg_rom.clone();
    let mut nes = Nes::new(rom.prg_rom, mirroring);

    let mut trackers = [Tracker::new(), Tracker::new()]; // index 0 -> slot 4, index 1 -> slot 5
    let slot_ids = [4u16, 5u16];

    let start_after = 120u32;
    let move_after = start_after + 200;
    let mut prev_code = [0u8; 2];

    for frame in 0..frame_count {
        let buttons = if frame >= start_after && frame < start_after + 10 {
            BUTTON_START
        } else if frame >= start_after + 40 && frame < start_after + 50 {
            BUTTON_START
        } else if frame >= move_after {
            let hop = (frame - move_after) % 40 < 4;
            BUTTON_RIGHT | if hop { BUTTON_A } else { 0 } | BUTTON_B
        } else {
            0
        };
        nes.set_controller(0, buttons);
        nes.run_frame();

        // SOUND_VOL_ENV,4/,5's aliasing source - see sound_engine's
        // module doc comment. Sampled once per frame since it's shared
        // (not per-slot) real RAM.
        let scratch = SharedScratch {
            init_sound_code: nes.peek_ram(INIT_SOUND_CODE),
            sound_chnl_reg_offset: nes.peek_ram(SOUND_CHNL_REG_OFFSET),
        };

        for (i, &slot) in slot_ids.iter().enumerate() {
            let real_code = nes.peek_ram(SOUND_CODE + slot);
            if real_code != 0 && prev_code[i] == 0 {
                // Fresh trigger: resolve the sound's real start address
                // directly from sound_table_00 rather than trusting live
                // SOUND_CMD_LOW/HIGH_ADDR RAM - by the time we observe the
                // trigger (after run_frame() completed), real hardware has
                // already consumed the first command on this same frame
                // (SOUND_CMD_LENGTH starts at 1), so that RAM already
                // points past the start.
                let mut engine = SoundSlot::default();
                engine.trigger(slot as u8, real_code, sound_start_prg_offset(&prg_rom, real_code));
                trackers[i].engine = Some(engine);
            }

            if let Some(engine) = trackers[i].engine.as_mut() {
                if real_code == 0 {
                    trackers[i].engine = None;
                } else {
                    let out = engine.step_low(&prg_rom, scratch);
                    trackers[i].checks += 1;
                    let real_cfg_low = nes.peek_ram(SOUND_CFG_LOW + slot);
                    let real_cfg_high = nes.peek_ram(SOUND_CFG_HIGH + slot);
                    let real_cmd_length = nes.peek_ram(SOUND_CMD_LENGTH + slot);
                    let real_pulse_volume = nes.peek_ram(PULSE_VOLUME + slot);
                    let ok = out.is_some_and(|o| {
                        o.cfg_low == real_cfg_low
                            && o.cfg_high == real_cfg_high
                            && engine.cmd_length == real_cmd_length
                            && (o.new_note || engine.pulse_volume == real_pulse_volume)
                    });
                    if !ok {
                        trackers[i].mismatches += 1;
                        if trackers[i].mismatches <= 3 {
                            let cfg_ok = out.is_some_and(|o| o.cfg_low == real_cfg_low && o.cfg_high == real_cfg_high && engine.cmd_length == real_cmd_length);
                            if cfg_ok {
                                println!(
                                    "frame={frame} slot={slot} PULSE_VOLUME MISMATCH engine={:#04x} real={real_pulse_volume:#04x} code={real_code:#04x}",
                                    engine.pulse_volume
                                );
                            } else {
                                println!(
                                    "frame={frame} slot={slot} MISMATCH out={out:?} real cfg_low={real_cfg_low:#04x} cfg_high={real_cfg_high:#04x} cmd_length={real_cmd_length:#04x} code={real_code:#04x}"
                                );
                            }
                        }
                    }
                    if let Some(o) = out {
                        if o.new_note {
                            trackers[i].new_note_checks += 1;
                            if !ok {
                                trackers[i].new_note_mismatches += 1;
                            }
                        }
                    }
                }
            }
            prev_code[i] = real_code;
        }
    }

    for (i, &slot) in slot_ids.iter().enumerate() {
        let t = &trackers[i];
        println!(
            "slot {slot}: {}/{} frame checks matched ({} mismatches); new-note commands: {}/{} matched",
            t.checks - t.mismatches,
            t.checks,
            t.mismatches,
            t.new_note_checks - t.new_note_mismatches,
            t.new_note_checks
        );
    }
}
