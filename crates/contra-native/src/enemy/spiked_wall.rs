//! Native port of the level-6 rising spiked wall and (plain) spiked
//! wall's own "activate, wait, get destroyed" states (`src/bank0.asm`,
//! `$afd6`-`$b025`, `$b103`-`$b0b1`, plus the shared `$b200` tail) - the
//! parts of this family that don't depend on the unported PPU graphics-
//! buffer subsystem. `rising_spiked_wall_routine_02`/`_04`/`_05` (the
//! actual rising/destruction animation) are **not ported** - all three
//! call `load_bank_3_update_nametable_supertile` directly.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;

/// Native port of `spiked_wall_set_collision_box` (`$b013`) - shared by
/// both `rising_spiked_wall_routine_00`'s own fallthrough and `spiked_
/// wall_routine_00`'s own tail `jmp`: sets `ENEMY_ATTRIBUTES = $c0` (a
/// dynamic collision-box configuration real ASM comments as "grow
/// upwards"), then advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpikedWallSetCollisionBoxResult {
    pub scroll: ScrolledEnemyPos,
    pub attributes: u8,
    pub routine_update: EnemyRoutineUpdate,
}

fn spiked_wall_set_collision_box(level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> SpikedWallSetCollisionBoxResult {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    SpikedWallSetCollisionBoxResult { scroll, attributes: 0xC0, routine_update: advance_enemy_routine(current_routine) }
}

/// `rising_spike_wall_trigger_dist_tbl` (`$b061`, 4 `(trigger_distance,
/// rising_delay)` pairs) - indexed by `ENEMY_ATTRIBUTES` bits 2-3.
const RISING_SPIKE_WALL_TRIGGER_DIST_TBL: [(u8, u8); 4] = [(0x30, 0x00), (0x50, 0x0F), (0x70, 0x1E), (0x40, 0x00)];
/// `rising_spike_wall_delay_tbl` (`$b069`, 4 bytes) - indexed by
/// `ENEMY_ATTRIBUTES` bits 0-1.
const RISING_SPIKE_WALL_DELAY_TBL: [u8; 4] = [0x0C, 0x08, 0x04, 0x02];

/// The full result of one [`rising_spiked_wall_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RisingSpikedWallRoutine00Result {
    pub var_3: u8,
    pub var_4: u8,
    pub attack_delay: u8,
    pub tail: SpikedWallSetCollisionBoxResult,
}

/// Native port of `rising_spiked_wall_routine_00` (`$afd6`).
pub fn rising_spiked_wall_routine_00(attributes: u8, level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> RisingSpikedWallRoutine00Result {
    let (var_3, var_4) = RISING_SPIKE_WALL_TRIGGER_DIST_TBL[((attributes & 0x0C) >> 2) as usize];
    let attack_delay = RISING_SPIKE_WALL_DELAY_TBL[(attributes & 0x03) as usize];
    let tail = spiked_wall_set_collision_box(level_scrolling_type, frame_scroll, x_pos, y_pos, current_routine);
    RisingSpikedWallRoutine00Result { var_3, var_4, attack_delay, tail }
}

/// One [`rising_spiked_wall_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RisingSpikedWallRoutine01Outcome {
    /// Closest player still farther than `ENEMY_VAR_3` (the trigger
    /// distance).
    Waiting,
    /// Close enough - enables collision, sets the initial emergence
    /// offset, and advances after `ENEMY_VAR_4`'s own delay.
    Triggered { var_2: u8, state_width: u8, delayed_routine: DelayedRoutineUpdate },
}

/// The full result of one [`rising_spiked_wall_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RisingSpikedWallRoutine01Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: RisingSpikedWallRoutine01Outcome,
}

/// Native port of `rising_spiked_wall_routine_01` (`$b00c`).
#[allow(clippy::too_many_arguments)]
pub fn rising_spiked_wall_routine_01(
    var_3: u8,
    var_4: u8,
    state_width: u8,
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> RisingSpikedWallRoutine01Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let closest = player_enemy_x_dist(sprite_x_pos, scroll.x_pos, player_state);

    let outcome = if closest.distance >= var_3 {
        RisingSpikedWallRoutine01Outcome::Waiting
    } else {
        RisingSpikedWallRoutine01Outcome::Triggered { var_2: 0x06, state_width: enable_enemy_collision(state_width), delayed_routine: set_enemy_delay_adv_routine(var_4, current_routine) }
    };

    RisingSpikedWallRoutine01Result { scroll, outcome }
}

/// Native port of `rising_spiked_wall_routine_03` (`$b200`) - real ASM:
/// `jmp add_scroll_to_enemy_pos`, a bare tail jump with no wrapping
/// `advance_enemy_routine` call at all (this routine index never
/// advances past here on its own). The same real address `immobile_
/// cart_generator_routine_01`'s own "not yet landed on" branch falls
/// through into (see that module's own doc comment).
pub fn rising_spiked_wall_routine_03(level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8) -> ScrolledEnemyPos {
    add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos)
}

/// `spiked_wall_destroyed_data_tbl` (`$b0ad`, 4 bytes) - real ASM reads
/// this as a flat, byte-offset (not doubled) index by `ENEMY_
/// ATTRIBUTES` directly, so `attributes` and `attributes + 1` read
/// *overlapping* windows for adjacent attribute values - ported exactly
/// as the raw byte-indexed access the ROM performs, not "corrected"
/// into a `[(u8, u8); 2]` pair table the real code doesn't actually use.
const SPIKED_WALL_DESTROYED_DATA_TBL: [u8; 4] = [0x04, 0x03, 0x00, 0x04];

/// The full result of one [`spiked_wall_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpikedWallRoutine00Result {
    pub var_1: u8,
    pub var_4: u8,
    pub animation_delay: u8,
    pub tail: SpikedWallSetCollisionBoxResult,
}

/// Native port of `spiked_wall_routine_00` (`$b103`).
pub fn spiked_wall_routine_00(attributes: u8, level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> SpikedWallRoutine00Result {
    let var_4 = SPIKED_WALL_DESTROYED_DATA_TBL[attributes as usize];
    let animation_delay = SPIKED_WALL_DESTROYED_DATA_TBL[attributes as usize + 1];
    let tail = spiked_wall_set_collision_box(level_scrolling_type, frame_scroll, x_pos, y_pos, current_routine);
    SpikedWallRoutine00Result { var_1: 0xB8, var_4, animation_delay, tail }
}

/// The full result of one [`spiked_wall_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpikedWallRoutine02Result {
    pub scroll: ScrolledEnemyPos,
    pub sound: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `spiked_wall_routine_02` (`$b091`).
pub fn spiked_wall_routine_02(level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> SpikedWallRoutine02Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    SpikedWallRoutine02Result { scroll, sound: 0x24, routine_update: advance_enemy_routine(current_routine) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rising_routine_00_loads_trigger_and_delay_from_attribute_bits() {
        // attrs = 0b0000_1001: bits2-3=0b10(idx2->(0x70,0x1e)), bits0-1=0b01(idx1->0x08)
        let r = rising_spiked_wall_routine_00(0b0000_1001, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.var_3, 0x70);
        assert_eq!(r.var_4, 0x1E);
        assert_eq!(r.attack_delay, 0x08);
        assert_eq!(r.tail.attributes, 0xC0);
        assert_eq!(r.tail.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn rising_routine_01_waits_when_player_is_far() {
        let r = rising_spiked_wall_routine_01(0x30, 0x0F, 0x00, [0x00, 0x00], [1, 1], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, RisingSpikedWallRoutine01Outcome::Waiting);
    }

    #[test]
    fn rising_routine_01_triggers_when_player_is_close() {
        let r = rising_spiked_wall_routine_01(0x30, 0x0F, 0x00, [0x55, 0x00], [1, 0], 0, 0x00, 0x50, 0x60, 3);
        match r.outcome {
            RisingSpikedWallRoutine01Outcome::Triggered { var_2, delayed_routine, .. } => {
                assert_eq!(var_2, 0x06);
                assert_eq!(delayed_routine, set_enemy_delay_adv_routine(0x0F, 3));
            }
            other => panic!("expected Triggered, got {other:?}"),
        }
    }

    #[test]
    fn rising_routine_03_is_a_bare_scroll_with_no_advance() {
        let r = rising_spiked_wall_routine_03(0, 0x02, 0x50, 0x60);
        assert_eq!(r, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
    }

    #[test]
    fn spiked_wall_routine_00_reads_the_raw_overlapping_byte_offsets() {
        let r0 = spiked_wall_routine_00(0x00, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!((r0.var_4, r0.animation_delay), (0x04, 0x03));
        let r2 = spiked_wall_routine_00(0x02, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!((r2.var_4, r2.animation_delay), (0x00, 0x04));
        assert_eq!(r0.var_1, 0xB8);
    }

    #[test]
    fn spiked_wall_routine_02_plays_the_explosion_sound_and_advances() {
        let r = spiked_wall_routine_02(0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.sound, 0x24);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }
}
