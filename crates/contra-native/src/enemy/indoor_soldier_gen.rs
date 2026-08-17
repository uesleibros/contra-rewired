//! Native port of the indoor-family "green guys generator" - `indoor_
//! soldier_gen_routine_00`/`_01` (`src/bank0.asm`, `$8d1f`/`$8d28`) -
//! reads a level-and-screen-specific byte stream from PRG-ROM (via a
//! real pointer chase, same "don't hand-transcribe ambiguous data, walk
//! the real bytes" approach `initialize_enemy`/`enemy_spawn` already
//! use) to spawn indoor soldiers, jumping soldiers, grenade launchers,
//! and groups of four, up to `$07` "rounds" of attacks per screen.
//!
//! ## The stale-carry rotate, and why `enemy_type_code` is just
//! `byte0 >> 6`
//!
//! Real ASM decodes the enemy-type bits via `rol; rol; rol; and #$03` on
//! `byte0` rather than a plain shift - `rol` rotates *through the carry
//! flag*, and the carry going into the first `rol` here is whatever was
//! left over from a much earlier, unrelated instruction (`lda ENEMY_
//! ATTRIBUTES,x; asl` - the very same `asl` this port's [`indoor_
//! soldier_gen_routine_01`] uses to pick between the level-2 and
//! level-4 tables). The real ASM's own comment for that earlier `asl`
//! ("disregard bit 7 and double bit 0") documents that this generator's
//! own `ENEMY_ATTRIBUTES` never has bit 7 set - meaning the carry
//! flowing into `rol;rol;rol` is always `0`, which makes the 3 rotates
//! mathematically equivalent to a plain `(byte0 >> 6) & 3` (traced by
//! hand: after 3 rotates-through-a-zero-carry, the result's low 2 bits
//! are exactly `byte0`'s original bits 7 and 6). This port takes the
//! simpler, verified-equivalent form rather than replicating the literal
//! (and carry-dependent) instruction sequence.
//!
//! ## `play_sound`-style non-port
//!
//! Like the rest of this crate, nothing here plays a sound or writes to
//! the PPU - [`indoor_soldier_gen_routine_01`] returns which enemies (if
//! any) were spawned as plain data.

use crate::enemy::enemy_clear::EnemyClearFields;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::initialize_enemy;
use crate::enemy::update_enemy_pos::{remove_enemy, RemovedEnemy};

/// `ENEMY_TYPE` codes the generator can spawn.
pub const ENEMY_TYPE_INDOOR_SOLDIER: u8 = 0x15;
pub const ENEMY_TYPE_JUMPING_SOLDIER: u8 = 0x16;
pub const ENEMY_TYPE_GRENADE_LAUNCHER: u8 = 0x17;
pub const ENEMY_TYPE_FOUR_SOLDIERS: u8 = 0x18;

/// `indoor_enemy_gen_tbl`'s own CPU address (`$8dcf`, bank 0's
/// switchable window) - 2 pointers (level 2, level 4).
const INDOOR_ENEMY_GEN_TBL_ADDR: u16 = 0x8DCF;

/// Converts a bank-0 (switchable, currently-mapped) CPU address to a
/// PRG-ROM byte offset - valid only while `bank_select() == 0`, same
/// convention every bank0.asm-sourced port in this crate relies on.
pub(crate) fn bank0_prg_offset(addr: u16) -> usize {
    addr as usize - 0x8000
}

/// Walks `indoor_enemy_gen_tbl` -> `lvl_(2|4)_enemy_gen_tbl` -> per-
/// screen byte stream -> `(byte0, byte1)` at `read_offset`, exactly the
/// real pointer chase `indoor_soldier_gen_routine_01` performs. `wants_
/// level_4` is `ENEMY_ATTRIBUTES & 1` (real ASM: `asl; tay` - see this
/// module's doc comment for why only bit 0 is load-bearing here).
fn read_gen_entry_bytes(prg_rom: &[u8], wants_level_4: bool, level_screen_number: u8, read_offset: u8) -> (u8, u8) {
    let ptr_off = bank0_prg_offset(INDOOR_ENEMY_GEN_TBL_ADDR) + if wants_level_4 { 2 } else { 0 };
    let level_tbl_addr = u16::from_le_bytes([prg_rom[ptr_off], prg_rom[ptr_off + 1]]);

    let screen_ptr_off = bank0_prg_offset(level_tbl_addr) + level_screen_number as usize * 2;
    let screen_addr = u16::from_le_bytes([prg_rom[screen_ptr_off], prg_rom[screen_ptr_off + 1]]);

    let data_off = bank0_prg_offset(screen_addr) + read_offset as usize;
    (prg_rom[data_off], prg_rom[data_off + 1])
}

/// One spawned enemy's full real field set - shared shape for all 4
/// real spawn paths (mirrors [`crate::enemy::create_enemy_bullet::CreatedBullet`]'s
/// own convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedIndoorGenSpawn {
    pub slot: u8,
    pub enemy_type: u8,
    pub hp: u8,
    pub fields: EnemyClearFields,
}

fn spawn_one(prg_rom: &[u8], enemy_routine: &[u8; ENEMY_SLOT_COUNT], current_level: u8, enemy_type: u8, attributes: u8) -> Vec<CreatedIndoorGenSpawn> {
    match find_next_enemy_slot(enemy_routine) {
        None => Vec::new(),
        Some(slot) => {
            let init = initialize_enemy(prg_rom, enemy_type, current_level);
            let mut fields = init.fields;
            fields.attributes = attributes;
            vec![CreatedIndoorGenSpawn { slot, enemy_type, hp: init.hp, fields }]
        }
    }
}

/// Native port of `@create_group_of_4`/`@green_guy_creation_loop` - up
/// to 4 spawns, soldier index counting *down* from `3` to `0` (real
/// ASM's own loop order), stopping early (real: `bne indoor_soldier_
/// gen_routine_exit`, exits the whole routine, not just this loop) the
/// first time no enemy slot is free - same "thread a local mutable
/// occupancy copy through the loop" approach `red_blue_soldier_gen_
/// routine_01` already uses, since a slot claimed earlier in *this same
/// call* must be seen as occupied by the next iteration.
fn spawn_group_of_four(prg_rom: &[u8], enemy_routine: &[u8; ENEMY_SLOT_COUNT], current_level: u8, attributes: u8) -> Vec<CreatedIndoorGenSpawn> {
    let mut occupancy = *enemy_routine;
    let mut spawns = Vec::new();
    for soldier_index in (0..=3u8).rev() {
        let Some(slot) = find_next_enemy_slot(&occupancy) else { break };
        occupancy[slot as usize] = 1;
        let init = initialize_enemy(prg_rom, ENEMY_TYPE_FOUR_SOLDIERS, current_level);
        let mut fields = init.fields;
        fields.attributes = attributes;
        fields.var_1 = soldier_index;
        spawns.push(CreatedIndoorGenSpawn { slot, enemy_type: ENEMY_TYPE_FOUR_SOLDIERS, hp: init.hp, fields });
    }
    spawns
}

/// The full result of one [`indoor_soldier_gen_routine_01`] call once it
/// reached the actual spawn decision (real ASM's `@create_enemy`).
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct IndoorSoldierGenEntryResult {
    pub animation_delay: u8,
    /// The generator's own `ENEMY_VAR_1` (byte-stream read offset) after
    /// advancing past the 2 bytes just consumed.
    pub var_1: u8,
    pub spawns: Vec<CreatedIndoorGenSpawn>,
}

/// The real branch [`indoor_soldier_gen_routine_01`] takes.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum IndoorSoldierGenRoutine01Outcome {
    /// Real ASM only runs on odd `FRAME_COUNTER` values.
    EvenFrame,
    /// A grenade launcher is already on screen (`GRENADE_LAUNCHER_FLAG`).
    GrenadeLauncherOnScreen,
    /// `ENEMY_ANIMATION_DELAY` hadn't reached `0` yet.
    StillWaiting { animation_delay: u8 },
    /// This entry's delay byte had bit 7 set, and the incremented
    /// `INDOOR_ENEMY_ATTACK_COUNT` reached its cap (`7`) - the generator
    /// removes *itself* instead of spawning anything.
    RoundsExhausted { indoor_enemy_attack_count: u8, removed: RemovedEnemy },
    /// Reached the spawn decision.
    Entry {
        /// `Some(new_count)` only when this entry's delay byte had bit 7
        /// set (and didn't hit the cap above).
        indoor_enemy_attack_count: Option<u8>,
        result: IndoorSoldierGenEntryResult,
    },
}

/// Native port of `indoor_soldier_gen_routine_01` (`$8d28`) - "generates
/// indoor soldier enemies: running, jumping, grenade launcher, and group
/// of 4". See this module's doc comment for the enemy-type decode and
/// the level-2-vs-level-4 table selector.
#[allow(clippy::too_many_arguments)]
pub fn indoor_soldier_gen_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    frame_counter: u8,
    grenade_launcher_flag: u8,
    enemy_animation_delay: u8,
    enemy_attributes: u8,
    level_screen_number: u8,
    var_1: u8,
    indoor_enemy_attack_count: u8,
) -> IndoorSoldierGenRoutine01Outcome {
    if frame_counter & 0x01 == 0 {
        return IndoorSoldierGenRoutine01Outcome::EvenFrame;
    }
    if grenade_launcher_flag != 0 {
        return IndoorSoldierGenRoutine01Outcome::GrenadeLauncherOnScreen;
    }

    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return IndoorSoldierGenRoutine01Outcome::StillWaiting { animation_delay: delay };
    }

    let wants_level_4 = enemy_attributes & 0x01 != 0;
    let (byte0, byte1) = read_gen_entry_bytes(prg_rom, wants_level_4, level_screen_number, var_1);
    let attributes = byte0 & 0x3F;
    let enemy_type_code = (byte0 >> 6) & 0x03;

    let indoor_enemy_attack_count = if byte1 & 0x80 != 0 { Some(indoor_enemy_attack_count.wrapping_add(1)) } else { None };

    if let Some(count) = indoor_enemy_attack_count {
        if count >= 0x07 {
            return IndoorSoldierGenRoutine01Outcome::RoundsExhausted { indoor_enemy_attack_count: count, removed: remove_enemy() };
        }
    }

    let animation_delay = byte1 & 0x7F;
    let new_var_1 = var_1.wrapping_add(2);

    let spawns = match enemy_type_code {
        0 => spawn_one(prg_rom, enemy_routine, current_level, ENEMY_TYPE_INDOOR_SOLDIER, attributes),
        1 => spawn_one(prg_rom, enemy_routine, current_level, ENEMY_TYPE_JUMPING_SOLDIER, attributes),
        2 => spawn_group_of_four(prg_rom, enemy_routine, current_level, attributes),
        _ => spawn_one(prg_rom, enemy_routine, current_level, ENEMY_TYPE_GRENADE_LAUNCHER, attributes),
    };

    IndoorSoldierGenRoutine01Outcome::Entry {
        indoor_enemy_attack_count,
        result: IndoorSoldierGenEntryResult { animation_delay, var_1: new_var_1, spawns },
    }
}

/// The full result of one [`indoor_soldier_gen_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndoorSoldierGenRoutine00Result {
    /// Always `$40`.
    pub animation_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `indoor_soldier_gen_routine_00` (`$8d1f`).
pub fn indoor_soldier_gen_routine_00(current_routine: u8) -> IndoorSoldierGenRoutine00Result {
    IndoorSoldierGenRoutine00Result { animation_delay: 0x40, routine_update: advance_enemy_routine(current_routine) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Level-4's per-level property table (all 4 spawnable types, `$15`-
    /// `$18`), plus a level-2/level-4 byte-stream fixture at screen 0
    /// with 4 entries (one of each enemy type) - matches the real data's
    /// own `(byte0, byte1)` shape.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];

        // enemy_prop_ptr_tbl (level 4's own per-level property table).
        let prop_ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let level4_prop_addr: u16 = 0xF000;
        rom[prop_ptr_tbl_off + 4 * 2..prop_ptr_tbl_off + 4 * 2 + 2].copy_from_slice(&level4_prop_addr.to_le_bytes());
        let level4_prop_off = 7 * 0x4000 + (level4_prop_addr as usize - 0xC000);
        for (i, ty) in [ENEMY_TYPE_INDOOR_SOLDIER, ENEMY_TYPE_JUMPING_SOLDIER, ENEMY_TYPE_GRENADE_LAUNCHER, ENEMY_TYPE_FOUR_SOLDIERS].into_iter().enumerate() {
            let off = level4_prop_off + ty as usize * 4;
            rom[off..off + 4].copy_from_slice(&[0x10, 0x20, 0x30 + i as u8, 0x40]);
        }

        // indoor_enemy_gen_tbl -> lvl_4_enemy_gen_tbl -> screen 0's byte stream.
        let gen_tbl_off = bank0_prg_offset(INDOOR_ENEMY_GEN_TBL_ADDR);
        let lvl4_tbl_addr: u16 = 0x8F00;
        rom[gen_tbl_off + 2..gen_tbl_off + 4].copy_from_slice(&lvl4_tbl_addr.to_le_bytes());
        let lvl4_tbl_off = bank0_prg_offset(lvl4_tbl_addr);
        let screen0_addr: u16 = 0x8F10;
        rom[lvl4_tbl_off..lvl4_tbl_off + 2].copy_from_slice(&screen0_addr.to_le_bytes());

        let screen0_off = bank0_prg_offset(screen0_addr);
        // offset 0: type 0 (indoor soldier), attrs=0x01, delay=0x30 (no attack-count bit)
        rom[screen0_off..screen0_off + 2].copy_from_slice(&[0b00_00_0001, 0x30]);
        // offset 2: type 1 (jumping soldier), attrs=0x02, delay=0x10, bit7 set (counts as a round)
        rom[screen0_off + 2..screen0_off + 4].copy_from_slice(&[0b01_00_0010, 0x90]);
        // offset 4: type 2 (group of four), attrs=0x03, delay=0x18
        rom[screen0_off + 4..screen0_off + 6].copy_from_slice(&[0b10_00_0011, 0x18]);
        // offset 6: type 3 (grenade launcher), attrs=0x05, delay=0x20
        rom[screen0_off + 6..screen0_off + 8].copy_from_slice(&[0b11_00_0101, 0x20]);

        rom
    }

    #[test]
    fn even_frame_never_runs() {
        let rom = synthetic_prg_rom();
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x02, 0, 0x01, 0x01, 0, 0, 0);
        assert_eq!(r, IndoorSoldierGenRoutine01Outcome::EvenFrame);
    }

    #[test]
    fn grenade_launcher_on_screen_blocks_generation() {
        let rom = synthetic_prg_rom();
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x01, 1, 0x01, 0x01, 0, 0, 0);
        assert_eq!(r, IndoorSoldierGenRoutine01Outcome::GrenadeLauncherOnScreen);
    }

    #[test]
    fn waits_while_delay_has_not_elapsed() {
        let rom = synthetic_prg_rom();
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x01, 0, 0x05, 0x01, 0, 0, 0);
        assert_eq!(r, IndoorSoldierGenRoutine01Outcome::StillWaiting { animation_delay: 0x04 });
    }

    #[test]
    fn spawns_indoor_soldier_for_type_code_0() {
        let rom = synthetic_prg_rom();
        // wants_level_4=true (bit0 set), read_offset=0 -> first entry (type 0).
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 4, 0x01, 0, 0x01, 0x01, 0, 0, 0);
        match r {
            IndoorSoldierGenRoutine01Outcome::Entry { indoor_enemy_attack_count: None, result } => {
                assert_eq!(result.animation_delay, 0x30);
                assert_eq!(result.var_1, 2);
                assert_eq!(result.spawns.len(), 1);
                let spawn = &result.spawns[0];
                assert_eq!(spawn.enemy_type, ENEMY_TYPE_INDOOR_SOLDIER);
                assert_eq!(spawn.fields.attributes, 0x01);
                assert_eq!(spawn.hp, 0x30);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn spawns_jumping_soldier_and_increments_attack_count_for_type_code_1() {
        let rom = synthetic_prg_rom();
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 4, 0x01, 0, 0x01, 0x01, 0, 2, 3);
        match r {
            IndoorSoldierGenRoutine01Outcome::Entry { indoor_enemy_attack_count: Some(4), result } => {
                assert_eq!(result.animation_delay, 0x10); // 0x90 & 0x7f
                assert_eq!(result.var_1, 4);
                assert_eq!(result.spawns[0].enemy_type, ENEMY_TYPE_JUMPING_SOLDIER);
                assert_eq!(result.spawns[0].fields.attributes, 0x02);
            }
            other => panic!("expected Entry with incremented count, got {other:?}"),
        }
    }

    #[test]
    fn rounds_exhausted_removes_the_generator_instead_of_spawning() {
        let rom = synthetic_prg_rom();
        // indoor_enemy_attack_count starts at 6 -> becomes 7, hits the cap.
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 4, 0x01, 0, 0x01, 0x01, 0, 2, 6);
        assert_eq!(r, IndoorSoldierGenRoutine01Outcome::RoundsExhausted { indoor_enemy_attack_count: 7, removed: remove_enemy() });
    }

    #[test]
    fn spawns_all_4_group_of_four_soldiers_with_descending_var_1() {
        let rom = synthetic_prg_rom();
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 4, 0x01, 0, 0x01, 0x01, 0, 4, 0);
        match r {
            IndoorSoldierGenRoutine01Outcome::Entry { result, .. } => {
                assert_eq!(result.spawns.len(), 4);
                let var_1s: Vec<u8> = result.spawns.iter().map(|s| s.fields.var_1).collect();
                assert_eq!(var_1s, vec![3, 2, 1, 0]);
                for s in &result.spawns {
                    assert_eq!(s.enemy_type, ENEMY_TYPE_FOUR_SOLDIERS);
                    assert_eq!(s.fields.attributes, 0x03);
                }
                // all 4 in distinct slots
                let mut slots: Vec<u8> = result.spawns.iter().map(|s| s.slot).collect();
                slots.sort();
                slots.dedup();
                assert_eq!(slots.len(), 4);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn group_of_four_stops_early_when_slots_run_out() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT]; // full
        routine[0] = 0;
        routine[1] = 0; // only 2 free slots
        let r = indoor_soldier_gen_routine_01(&rom, &routine, 4, 0x01, 0, 0x01, 0x01, 0, 4, 0);
        match r {
            IndoorSoldierGenRoutine01Outcome::Entry { result, .. } => assert_eq!(result.spawns.len(), 2),
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn spawns_grenade_launcher_for_type_code_3() {
        let rom = synthetic_prg_rom();
        let r = indoor_soldier_gen_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 4, 0x01, 0, 0x01, 0x01, 0, 6, 0);
        match r {
            IndoorSoldierGenRoutine01Outcome::Entry { result, .. } => {
                assert_eq!(result.spawns[0].enemy_type, ENEMY_TYPE_GRENADE_LAUNCHER);
                assert_eq!(result.spawns[0].fields.attributes, 0x05);
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn no_free_slot_still_updates_delay_and_var_1_but_spawns_nothing() {
        let rom = synthetic_prg_rom();
        let full = [1u8; ENEMY_SLOT_COUNT];
        let r = indoor_soldier_gen_routine_01(&rom, &full, 4, 0x01, 0, 0x01, 0x01, 0, 0, 0);
        match r {
            IndoorSoldierGenRoutine01Outcome::Entry { result, .. } => {
                assert_eq!(result.animation_delay, 0x30);
                assert_eq!(result.var_1, 2);
                assert!(result.spawns.is_empty());
            }
            other => panic!("expected Entry, got {other:?}"),
        }
    }

    #[test]
    fn routine_00_sets_fixed_delay_and_advances() {
        let r = indoor_soldier_gen_routine_00(5);
        assert_eq!(r.animation_delay, 0x40);
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }
}
