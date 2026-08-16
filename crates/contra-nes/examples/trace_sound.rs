//! Debug-only tool (not part of the library or any shipped binary):
//! captures real hardware ground truth for Contra's sound engine,
//! frame by frame, by snapshotting all 6 sound slots' RAM state and the
//! APU's raw register writes (`Apu::last_write`, added for this purpose)
//! around each `Nes::run_frame()` call during real gameplay. Used to
//! build and verify `contra_native`'s native sound engine against real
//! behavior instead of hand-deriving every rule from reading assembly
//! alone (which the rest of this project's sound_code work already
//! showed is slow and error-prone even with careful reading).
//!
//! ```text
//! cargo run -p contra-nes --release --example trace_sound -- <rom> <out.txt> [frames]
//! ```

use contra_nes::controller::*;
use contra_nes::{Mirroring, Nes};

const SLOT_COUNT: usize = 6;
const SOUND_CMD_LENGTH: u16 = 0x100;
const SOUND_CODE: u16 = 0x106;
const SOUND_PULSE_LENGTH: u16 = 0x10c;
const SOUND_CMD_LOW_ADDR: u16 = 0x112;
const SOUND_CMD_HIGH_ADDR: u16 = 0x118;
const SOUND_VOL_ENV: u16 = 0x11e;
const SOUND_FLAGS: u16 = 0x124;
const PULSE_VOL_DURATION: u16 = 0x12a;
const DECRESCENDO_END_PAUSE: u16 = 0x130;
const SOUND_CFG_LOW: u16 = 0x142;
const SOUND_REPEAT_COUNT: u16 = 0x148;
const SOUND_CFG_HIGH: u16 = 0x14e;
const SOUND_LENGTH_MULTIPLIER: u16 = 0x154;
const PULSE_VOLUME: u16 = 0x160;
const SOUND_PULSE_PERIOD: u16 = 0x172;

#[derive(Clone, PartialEq, Eq)]
struct SlotSnapshot {
    cmd_length: u8,
    code: u8,
    pulse_length: u8,
    cmd_low: u8,
    cmd_high: u8,
    vol_env: u8,
    flags: u8,
    pulse_vol_duration: u8,
    decrescendo_end_pause: u8,
    cfg_low: u8,
    repeat_count: u8,
    cfg_high: u8,
    length_multiplier: u8,
    pulse_volume: u8,
    pulse_period: u8,
}

fn snapshot(nes: &Nes, slot: usize) -> SlotSnapshot {
    let s = slot as u16;
    SlotSnapshot {
        cmd_length: nes.peek_ram(SOUND_CMD_LENGTH + s),
        code: nes.peek_ram(SOUND_CODE + s),
        pulse_length: nes.peek_ram(SOUND_PULSE_LENGTH + s),
        cmd_low: nes.peek_ram(SOUND_CMD_LOW_ADDR + s),
        cmd_high: nes.peek_ram(SOUND_CMD_HIGH_ADDR + s),
        vol_env: nes.peek_ram(SOUND_VOL_ENV + s),
        flags: nes.peek_ram(SOUND_FLAGS + s),
        pulse_vol_duration: nes.peek_ram(PULSE_VOL_DURATION + s),
        decrescendo_end_pause: nes.peek_ram(DECRESCENDO_END_PAUSE + s),
        cfg_low: nes.peek_ram(SOUND_CFG_LOW + s),
        repeat_count: nes.peek_ram(SOUND_REPEAT_COUNT + s),
        cfg_high: nes.peek_ram(SOUND_CFG_HIGH + s),
        length_multiplier: nes.peek_ram(SOUND_LENGTH_MULTIPLIER + s),
        pulse_volume: nes.peek_ram(PULSE_VOLUME + s),
        pulse_period: nes.peek_ram(SOUND_PULSE_PERIOD + s),
    }
}

fn fmt_snapshot(s: &SlotSnapshot) -> String {
    format!(
        "code={:#04x} len={:#04x} plen={:#04x} cmd={:#04x}{:02x} vol_env={:#04x} flags={:#04x} pvd={:#04x} dep={:#04x} cfglo={:#04x} rep={:#04x} cfghi={:#04x} lmul={:#04x} pvol={:#04x} pper={:#04x}",
        s.code, s.cmd_length, s.pulse_length, s.cmd_high, s.cmd_low, s.vol_env, s.flags, s.pulse_vol_duration, s.decrescendo_end_pause, s.cfg_low, s.repeat_count, s.cfg_high, s.length_multiplier, s.pulse_volume, s.pulse_period
    )
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: trace_sound <rom> <out.txt> [frames]");
    let out_path = args.get(2).expect("usage: trace_sound <rom> <out.txt> [frames]");
    let frame_count: u32 = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(1200);

    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let mut nes = Nes::new(rom.prg_rom, mirroring);

    let mut out = String::new();
    let start_after = 120u32;
    let move_after = start_after + 200;
    let mut prev: Vec<SlotSnapshot> = (0..SLOT_COUNT).map(|s| snapshot(&nes, s)).collect();

    for frame in 0..frame_count {
        let buttons = if frame >= start_after && frame < start_after + 10 {
            BUTTON_START
        } else if frame >= start_after + 40 && frame < start_after + 50 {
            BUTTON_START
        } else if frame >= move_after {
            // Walk right and hop periodically to naturally trigger a
            // variety of real sound effects (footsteps landing, jump,
            // shooting) without hand-picking specific ones.
            let hop = (frame - move_after) % 40 < 4;
            BUTTON_RIGHT | if hop { BUTTON_A } else { 0 } | BUTTON_B
        } else {
            0
        };
        nes.set_controller(0, buttons);
        nes.run_frame();

        let now: Vec<SlotSnapshot> = (0..SLOT_COUNT).map(|s| snapshot(&nes, s)).collect();
        for slot in 0..SLOT_COUNT {
            if now[slot].code != 0 || prev[slot].code != 0 {
                if now[slot] != prev[slot] {
                    out.push_str(&format!("frame={frame} slot={slot} PRE  {}\n", fmt_snapshot(&prev[slot])));
                    out.push_str(&format!("frame={frame} slot={slot} POST {}\n", fmt_snapshot(&now[slot])));
                }
            }
        }
        prev = now;
    }

    std::fs::write(out_path, &out).unwrap();
    println!("wrote trace to {out_path} ({} lines)", out.lines().count());
}
