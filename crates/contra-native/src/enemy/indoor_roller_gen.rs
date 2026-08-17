//! Native port of the indoor-family roller generator - `indoor_roller_
//! gen_routine_00`/`_01` (`src/bank0.asm`, `$95c8`/`$95cd`) - reads a
//! per-generator roller pattern from PRG-ROM (via a real pointer chase,
//! same "walk the real bytes" approach [`crate::enemy::indoor_soldier_gen`]
//! already uses) and can spawn *multiple* rollers in a single call, back
//! to back, whenever consecutive pattern entries have a `0` delay byte.
//!
//! ## Real byte format (`roller_gen_init_00`/`_01`, up to `$39` bytes
//! each)
//!
//! Pairs of `(position_and_attributes, delay)` bytes, `$ff` as the first
//! byte of a pair meaning "wrap back to the start of this table and keep
//! reading" - `position_and_attributes`' low nibble is the roller's
//! `ENEMY_ATTRIBUTES`, its high nibble is a horizontal segment/position
//! index (`0..7`) used *directly* as [`crate::enemy::indoor_soldier::create_roller_with_segment_a`]'s
//! own segment input - unlike every other real caller of that routine,
//! this one **doesn't** compute the segment via [`crate::enemy::find_far_segment::find_far_segment_for_x_pos`],
//! it's baked into the level data instead.
//!
//! ## Bounded loop, not a literal infinite one
//!
//! Real ASM's own loop (`@create_roller`, re-entered whenever the just-
//! set `ENEMY_ANIMATION_DELAY` comes back `0`) has no hard upper bound -
//! a pathological pattern of all-zero delays would spin forever on real
//! hardware too. This port caps at 64 iterations (same reasoning and
//! same cap [`crate::enemy::red_blue_soldier::red_blue_soldier_gen_routine_01`]
//! already uses for its own bounded spawn loop), comfortably above the
//! real data's own largest table (`$39` bytes, well under 64 roller
//! pairs).

use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::indoor_soldier::{create_roller_with_segment_a, CreatedIndoorEnemy};
use crate::enemy::indoor_soldier_gen::bank0_prg_offset;
use crate::enemy::update_enemy_pos::{remove_enemy, RemovedEnemy};

/// `roller_initial_x_pos_tbl` (`$9629`, 7 bytes) - the roller's starting
/// X position, indexed by the same horizontal segment baked into the
/// pattern data's high nibble.
const ROLLER_INITIAL_X_POS_TBL: [u8; 7] = [0x98, 0x90, 0x88, 0x80, 0x78, 0x70, 0x68];

/// Always `$70` - the real ASM's own hard-coded roller spawn Y position.
const ROLLER_SPAWN_Y_POS: u8 = 0x70;

/// `roller_gen_init_tbl`'s own CPU address (`$9630`, bank 0's switchable
/// window) - a small pointer table selected by `ENEMY_ATTRIBUTES & 7`
/// (real ASM: `and #$07; asl; tay`, i.e. a raw byte offset already
/// doubled) into the generator's own roller pattern (`roller_gen_init_
/// 00`/`_01`).
const ROLLER_GEN_INIT_TBL_ADDR: u16 = 0x9630;

fn roller_pattern_addr(prg_rom: &[u8], pattern_selector: u8) -> u16 {
    let off = bank0_prg_offset(ROLLER_GEN_INIT_TBL_ADDR) + pattern_selector as usize * 2;
    u16::from_le_bytes([prg_rom[off], prg_rom[off + 1]])
}

fn read_pattern_byte(prg_rom: &[u8], pattern_addr: u16, offset: u8) -> u8 {
    prg_rom[bank0_prg_offset(pattern_addr) + offset as usize]
}

/// One roller [`indoor_roller_gen_routine_01`] attempted to create this
/// call - `roller` is `None` if `ENEMY_ATTACK_FLAG` was clear or no
/// enemy slot was free (real ASM still consumes the pattern entry and
/// moves on regardless).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptedRollerSpawn {
    pub roller: Option<CreatedIndoorEnemy>,
    pub segment: u8,
}

/// The full result of one [`indoor_roller_gen_routine_01`] call once it
/// reached the roller-creation loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndoorRollerGenEntryResult {
    /// The generator's own `ENEMY_VAR_1` (pattern read offset) after the
    /// loop stops.
    pub var_1: u8,
    /// The delay byte that actually stopped the loop (nonzero, unless
    /// the 64-iteration safety cap was hit first).
    pub animation_delay: u8,
    pub spawns: Vec<AttemptedRollerSpawn>,
}

/// The real branch [`indoor_roller_gen_routine_01`] takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndoorRollerGenRoutine01Outcome {
    /// `INDOOR_ENEMY_ATTACK_COUNT` already reached its cap (`7`) - the
    /// generator removes itself without checking anything else.
    RoundsExhausted(RemovedEnemy),
    /// Real ASM only runs on odd `FRAME_COUNTER` values.
    EvenFrame,
    /// `ENEMY_ANIMATION_DELAY` hadn't reached `0` yet.
    StillWaiting { animation_delay: u8 },
    /// Reached the roller-creation loop.
    Entry(IndoorRollerGenEntryResult),
}

/// Native port of `indoor_roller_gen_routine_01` (`$95cd`).
#[allow(clippy::too_many_arguments)]
pub fn indoor_roller_gen_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    indoor_enemy_attack_count: u8,
    frame_counter: u8,
    enemy_animation_delay: u8,
    enemy_attributes: u8,
    var_1: u8,
) -> IndoorRollerGenRoutine01Outcome {
    if indoor_enemy_attack_count >= 0x07 {
        return IndoorRollerGenRoutine01Outcome::RoundsExhausted(remove_enemy());
    }
    if frame_counter & 0x01 == 0 {
        return IndoorRollerGenRoutine01Outcome::EvenFrame;
    }
    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return IndoorRollerGenRoutine01Outcome::StillWaiting { animation_delay: delay };
    }

    let pattern_addr = roller_pattern_addr(prg_rom, enemy_attributes & 0x07);

    let mut y = var_1;
    let mut occupancy = *enemy_routine;
    let mut spawns = Vec::new();
    let mut animation_delay = 0u8;

    for _ in 0..64 {
        let mut byte_a = read_pattern_byte(prg_rom, pattern_addr, y);
        if byte_a == 0xFF {
            y = 0;
            byte_a = read_pattern_byte(prg_rom, pattern_addr, y);
        }

        let attributes = byte_a & 0x0F;
        let segment = (byte_a >> 4) & 0x0F;

        y = y.wrapping_add(1);
        let delay_byte = read_pattern_byte(prg_rom, pattern_addr, y);
        y = y.wrapping_add(1);

        let x_pos = ROLLER_INITIAL_X_POS_TBL[segment as usize];
        let roller = create_roller_with_segment_a(prg_rom, &occupancy, current_level, enemy_attack_flag, segment, x_pos, ROLLER_SPAWN_Y_POS, attributes);
        if let Some(r) = &roller {
            occupancy[r.slot as usize] = 1;
        }
        spawns.push(AttemptedRollerSpawn { roller, segment });

        animation_delay = delay_byte;
        if delay_byte != 0 {
            break;
        }
    }

    IndoorRollerGenRoutine01Outcome::Entry(IndoorRollerGenEntryResult { var_1: y, animation_delay, spawns })
}

/// The full result of one [`indoor_roller_gen_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndoorRollerGenRoutine00Result {
    /// Always `$60`.
    pub animation_delay: u8,
    pub routine_update: crate::enemy::enemy_routine_transition::EnemyRoutineUpdate,
}

/// Native port of `indoor_roller_gen_routine_00` (`$95c8`).
pub fn indoor_roller_gen_routine_00(current_routine: u8) -> IndoorRollerGenRoutine00Result {
    IndoorRollerGenRoutine00Result {
        animation_delay: 0x60,
        routine_update: crate::enemy::enemy_routine_transition::advance_enemy_routine(current_routine),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Roller property table (`$11 >= $10`) plus a small roller pattern
    /// at `roller_gen_init_tbl`'s pattern index `0`: entry 0 = segment 2,
    /// attrs 3, delay 5 (stops immediately); a second fixture pattern at
    /// index `1` has a `0`-delay first entry so the loop creates a
    /// second roller before stopping.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];

        let prop_ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let level0_prop_addr: u16 = 0xF000;
        rom[prop_ptr_tbl_off..prop_ptr_tbl_off + 2].copy_from_slice(&level0_prop_addr.to_le_bytes());
        let level0_prop_off = 7 * 0x4000 + (level0_prop_addr as usize - 0xC000);
        let roller_prop_off = level0_prop_off + 0x11 * 4;
        rom[roller_prop_off..roller_prop_off + 4].copy_from_slice(&[0x10, 0x20, 0x0A, 0x30]);

        let tbl_off = bank0_prg_offset(ROLLER_GEN_INIT_TBL_ADDR);
        let pattern0_addr: u16 = 0x9700;
        let pattern1_addr: u16 = 0x9720;
        rom[tbl_off..tbl_off + 2].copy_from_slice(&pattern0_addr.to_le_bytes());
        rom[tbl_off + 2..tbl_off + 4].copy_from_slice(&pattern1_addr.to_le_bytes());

        let p0_off = bank0_prg_offset(pattern0_addr);
        // segment=2 (high nibble), attrs=3 (low nibble); delay=5 -> stops after 1 roller.
        rom[p0_off..p0_off + 2].copy_from_slice(&[0x23, 0x05]);

        let p1_off = bank0_prg_offset(pattern1_addr);
        // entry 0: segment=1, attrs=2, delay=0 -> immediately creates a second roller.
        rom[p1_off..p1_off + 2].copy_from_slice(&[0x12, 0x00]);
        // entry 1: segment=4, attrs=6, delay=7 -> stops.
        rom[p1_off + 2..p1_off + 4].copy_from_slice(&[0x46, 0x07]);
        // wraparound sentinel + one more entry, to exercise $ff handling.
        rom[p1_off + 4..p1_off + 6].copy_from_slice(&[0xFF, 0x00]);

        rom
    }

    #[test]
    fn rounds_exhausted_removes_the_generator() {
        let rom = synthetic_prg_rom();
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 7, 0x01, 0x01, 0x00, 0);
        assert_eq!(r, IndoorRollerGenRoutine01Outcome::RoundsExhausted(remove_enemy()));
    }

    #[test]
    fn even_frame_never_runs() {
        let rom = synthetic_prg_rom();
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0, 0x02, 0x01, 0x00, 0);
        assert_eq!(r, IndoorRollerGenRoutine01Outcome::EvenFrame);
    }

    #[test]
    fn waits_while_delay_has_not_elapsed() {
        let rom = synthetic_prg_rom();
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0, 0x01, 0x05, 0x00, 0);
        assert_eq!(r, IndoorRollerGenRoutine01Outcome::StillWaiting { animation_delay: 0x04 });
    }

    #[test]
    fn creates_a_single_roller_when_the_first_delay_is_nonzero() {
        let rom = synthetic_prg_rom();
        // enemy_attributes & 7 = 0 -> pattern 0.
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0, 0x01, 0x01, 0x00, 0);
        match r {
            IndoorRollerGenRoutine01Outcome::Entry(result) => {
                assert_eq!(result.spawns.len(), 1);
                assert_eq!(result.animation_delay, 0x05);
                assert_eq!(result.var_1, 2);
                let spawn = &result.spawns[0];
                assert_eq!(spawn.segment, 2);
                let roller = spawn.roller.as_ref().unwrap();
                assert_eq!(roller.fields.attributes, 3);
                assert_eq!(roller.fields.x_pos, ROLLER_INITIAL_X_POS_TBL[2]);
                assert_eq!(roller.fields.y_pos, ROLLER_SPAWN_Y_POS);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn creates_multiple_rollers_back_to_back_when_delay_is_zero() {
        let rom = synthetic_prg_rom();
        // enemy_attributes & 7 = 1 -> pattern 1.
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0, 0x01, 0x01, 0x01, 0);
        match r {
            IndoorRollerGenRoutine01Outcome::Entry(result) => {
                assert_eq!(result.spawns.len(), 2);
                assert_eq!(result.animation_delay, 0x07);
                assert_eq!(result.var_1, 4);
                assert_eq!(result.spawns[0].segment, 1);
                assert_eq!(result.spawns[1].segment, 4);
                // both rollers landed in distinct slots
                let a = result.spawns[0].roller.as_ref().unwrap().slot;
                let b = result.spawns[1].roller.as_ref().unwrap().slot;
                assert_ne!(a, b);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn wraparound_sentinel_restarts_the_pattern_from_offset_0() {
        let rom = synthetic_prg_rom();
        // var_1=4 -> reads the $ff sentinel at p1_off+4, wraps to offset 0
        // (segment=1, attrs=2, delay=0), then continues to entry 1 (delay=7, stops).
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0, 0x01, 0x01, 0x01, 4);
        match r {
            IndoorRollerGenRoutine01Outcome::Entry(result) => {
                assert_eq!(result.spawns.len(), 2);
                assert_eq!(result.spawns[0].segment, 1); // wrapped back to offset 0
                assert_eq!(result.animation_delay, 0x07);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn no_attack_flag_still_advances_the_pattern_but_spawns_nothing() {
        let rom = synthetic_prg_rom();
        let r = indoor_roller_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0, 0, 0x01, 0x01, 0x00, 0);
        match r {
            IndoorRollerGenRoutine01Outcome::Entry(result) => {
                assert_eq!(result.spawns.len(), 1);
                assert!(result.spawns[0].roller.is_none());
                assert_eq!(result.var_1, 2);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn routine_00_sets_fixed_delay_and_advances() {
        let r = indoor_roller_gen_routine_00(5);
        assert_eq!(r.animation_delay, 0x60);
        assert_eq!(r.routine_update, crate::enemy::enemy_routine_transition::advance_enemy_routine(5));
    }
}
