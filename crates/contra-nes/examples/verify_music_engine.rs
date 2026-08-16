//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::sound_engine::MusicSlot`: same methodology
//! as `verify_sound_engine.rs`, but for slots #$00-#$03 (music/high
//! format + percussion) instead of the two sound-effect slots. Runs real
//! gameplay through `contra-nes`, and on every fresh trigger spins up a
//! matching `MusicSlot`, stepping it in lockstep with real frames and
//! comparing computed `cfg_low`/`cfg_high`/`cmd_length` against real RAM.
//!
//! ```text
//! cargo run -p contra-nes --release --example verify_music_engine -- <rom> [frames]
//! ```
//!
//! See `verify_sound_engine.rs`'s doc comment for the two real findings
//! that apply equally here: (1) a sound's start address must be resolved
//! directly from `sound_table_00`, not sampled from live `SOUND_CMD_LOW/
//! HIGH_ADDR` RAM after the trigger frame's own immediate first-command
//! read has already advanced it, and (2) `handle_sound_code` can run more
//! than once per visual frame during real NMI-reentrancy/lag, which this
//! tool's strict one-step-per-`run_frame()` model can't represent -
//! expect mismatches from that, not necessarily from `MusicSlot` itself.

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};
use contra_native::sound_code::Slot;
use contra_native::sound_engine::MusicSlot;

const SOUND_CODE: u16 = 0x106;
const SOUND_CMD_LENGTH: u16 = 0x100;
const SOUND_CFG_LOW: u16 = 0x142;
const SOUND_CFG_HIGH: u16 = 0x14e;

const SOUND_TABLE_00_PRG_OFFSET: usize = 0x48E8;

fn bank1_prg_offset(mem_addr: u16) -> usize {
    0x4000 + (mem_addr as usize & 0x3FFF)
}

/// A multi-slot sound (e.g. code `0x26`'s TITLE theme, which spans slots
/// #$00-#$03 via 4 consecutive `sound_table_00` entries `0x26`-`0x29`)
/// sets `SOUND_CODE,x = INIT_SOUND_CODE` for *every* slot it touches -
/// the original triggering code, not that slot's own table entry index
/// (`load_sound_code_entry`, `src/bank1.asm:1655-1656`). So peeking
/// `SOUND_CODE` for slot 1 during TITLE reads back `0x26`, not `0x27`,
/// even though slot 1 is actually running `sound_27`'s data. Real Contra
/// resolves this by walking consecutive `sound_table_00` entries starting
/// at `INIT_SOUND_CODE` (`play_sound`'s `$eb`/`$ea` loop) - each entry's
/// own byte 0 embeds which slot *it* is for, so this does the same walk
/// and picks the first entry (within the sound's own entry count) whose
/// embedded slot matches.
fn sound_start_prg_offset_for_slot(prg_rom: &[u8], sound_code: u8, slot_index: u16) -> usize {
    let first_base = SOUND_TABLE_00_PRG_OFFSET + sound_code as usize * 3;
    let entry_count = ((prg_rom[first_base] >> 3) & 0x03) + 1;
    for k in 0..entry_count as usize {
        let base = SOUND_TABLE_00_PRG_OFFSET + (sound_code as usize + k) * 3;
        if (prg_rom[base] & 0x07) as u16 == slot_index {
            let mem_addr = u16::from_le_bytes([prg_rom[base + 1], prg_rom[base + 2]]);
            return bank1_prg_offset(mem_addr);
        }
    }
    panic!("no sound_table_00 entry starting at {sound_code:#04x} matches slot {slot_index}");
}

fn slot_for(slot_index: u16) -> Slot {
    match slot_index {
        0 => Slot::Pulse1,
        1 => Slot::Pulse2,
        2 => Slot::Triangle,
        3 => Slot::Noise,
        _ => unreachable!("music slots are only 0-3"),
    }
}

struct Tracker {
    engine: Option<MusicSlot>,
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
    let rom_path = args.get(1).expect("usage: verify_music_engine <rom> [frames]");
    let frame_count: u32 = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(1200);

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let prg_rom = rom.prg_rom.clone();
    let mut nes = Nes::new(rom.prg_rom, mirroring);

    let mut trackers = [Tracker::new(), Tracker::new(), Tracker::new(), Tracker::new()];
    let slot_ids = [0u16, 1, 2, 3];

    let start_after = 120u32;
    let move_after = start_after + 200;
    let mut prev_code = [0u8; 4];

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

        for (i, &slot) in slot_ids.iter().enumerate() {
            let real_code = nes.peek_ram(SOUND_CODE + slot);
            if real_code != 0 && prev_code[i] == 0 {
                let mut engine = MusicSlot::default();
                engine.trigger(slot_for(slot), real_code, sound_start_prg_offset_for_slot(&prg_rom, real_code, slot));
                trackers[i].engine = Some(engine);
            }

            if let Some(engine) = trackers[i].engine.as_mut() {
                if real_code == 0 {
                    trackers[i].engine = None;
                } else {
                    let out = engine.step_high(&prg_rom);
                    trackers[i].checks += 1;
                    let real_cfg_low = nes.peek_ram(SOUND_CFG_LOW + slot);
                    let real_cfg_high = nes.peek_ram(SOUND_CFG_HIGH + slot);
                    let real_cmd_length = nes.peek_ram(SOUND_CMD_LENGTH + slot);
                    let ok = out.is_some_and(|o| {
                        (slot == 2 || (o.cfg_low == real_cfg_low && o.cfg_high == real_cfg_high)) && engine.cmd_length == real_cmd_length
                    });
                    if !ok {
                        trackers[i].mismatches += 1;
                        if trackers[i].mismatches <= 3 {
                            println!(
                                "frame={frame} slot={slot} MISMATCH out={out:?} real cfg_low={real_cfg_low:#04x} cfg_high={real_cfg_high:#04x} cmd_length={real_cmd_length:#04x} code={real_code:#04x}"
                            );
                        }
                    }
                    if let Some(o) = out {
                        if o.note_source.is_some() {
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
