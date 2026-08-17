//! Native port of the "group of four soldiers" enemy type's ($18) own
//! `_00`/`_01`/`_02` table entries (`src/bank0.asm`, `$9541`/`$954c`/
//! `$9582`) - the other 5 entries of `four_soldiers_routine_ptr_tbl` are
//! the same shared routines every indoor-family type reuses, already
//! ported.
//!
//! ## The 3-state cycle
//!
//! `_00` initializes one soldier of the group (`ENEMY_VAR_1` is which of
//! the 4, `0..3`, set by whatever spawner creates them - not itself part
//! of this module) and sets its first firing delay, then advances to
//! `_01`. `_01` walks until its running delay elapses, decides whether to
//! reverse direction (only the *second* pair of soldiers, and only after
//! their first shot), computes the next standing-still delay, and jumps
//! straight to `_02` (real ASM: `set_anim_delay_adv_enemy_routine_00`,
//! not a return) - but while its OWN delay is still running, it fires a
//! bullet on the exact frame the decremented delay equals `$04` (real
//! ASM gives no explanation for that specific value beyond the comment
//! "fire if animation delay is #$04"). `_02` applies velocity/sprite
//! while standing still, and once its own delay elapses, sets the firing
//! sprite, counts the shot, computes the *next* running delay, and jumps
//! back to `_01` (via `set_enemy_routine_to_a`, not `advance_enemy_
//! routine` - a direct jump back rather than a linear advance).
//!
//! [`four_soldiers_get_delay_offset`]/[`four_soldiers_set_firing_delay`]
//! are shared by all 3 real entry points (`_00`/`_01`/`_02`) to index the
//! same two 12-byte, 4-soldiers-by-3-rounds tables by `(times fired,
//! soldier index)`.

use crate::enemy::enemy_clear::EnemyClearFields;
use crate::enemy::enemy_position_utils::reverse_enemy_x_direction;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::indoor_soldier::{
    apply_enemy_velocity_set_bg_priority, create_indoor_bullet, init_indoor_enemy_pos_and_vel, init_sprite_from_frame, ApplyEnemyVelocityResult,
    InitIndoorEnemyPosAndVelResult, InitSpriteFromFrameResult,
};

/// `four_soldiers_delay_running_tbl` (`$9576`, 12 bytes) - the walking
/// delay before the *next* stop, `(times fired, soldier index)`-indexed
/// via [`four_soldiers_get_delay_offset`]. Row 2 (`$ff` for all 4
/// soldiers) is real ASM's own "shouldn't happen" tail - `ENEMY_VAR_2`
/// never legitimately reaches `2` through this module's own control flow
/// (only `_02` increments it, and only up to `1` before `_01`'s own
/// `cmp #$01` branch stops treating it specially).
const FOUR_SOLDIERS_DELAY_RUNNING_TBL: [u8; 12] = [0x3F, 0x39, 0x33, 0x2D, 0x18, 0x10, 0x10, 0x18, 0xFF, 0xFF, 0xFF, 0xFF];

/// `four_soldiers_firing_delay_tbl` (`$95b6`, 12 bytes) - the standing-
/// still delay before firing, same `(times fired, soldier index)`
/// indexing.
const FOUR_SOLDIERS_FIRING_DELAY_TBL: [u8; 12] = [0x01, 0x07, 0x0D, 0x13, 0x18, 0x18, 0x18, 0x18, 0x10, 0x18, 0x18, 0x10];

/// Native port of `four_soldiers_get_delay_offset` (`$95a7`) - `times_
/// fired * 4 + soldier_index`, the shared row/column index into both
/// tables above.
pub fn four_soldiers_get_delay_offset(soldier_index: u8, times_fired: u8) -> u8 {
    times_fired.wrapping_mul(4).wrapping_add(soldier_index)
}

/// Native port of `four_soldiers_set_firing_delay` (`$959d`).
pub fn four_soldiers_set_firing_delay(soldier_index: u8, times_fired: u8) -> u8 {
    FOUR_SOLDIERS_FIRING_DELAY_TBL[four_soldiers_get_delay_offset(soldier_index, times_fired) as usize]
}

/// The full result of one [`four_soldiers_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FourSoldiersRoutine00Result {
    pub init: InitIndoorEnemyPosAndVelResult,
    pub animation_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `four_soldiers_routine_00` (`$9541`) - "initialize
/// soldier". Always initializes with [`init_indoor_enemy_pos_and_vel`]'s
/// logical index `2` (real ASM: `ldy #$04`, a raw byte offset -
/// `4/2=2`, the table's "group of 4" row).
pub fn four_soldiers_routine_00(enemy_attributes: u8, soldier_index: u8, current_routine: u8) -> FourSoldiersRoutine00Result {
    let init = init_indoor_enemy_pos_and_vel(2, enemy_attributes);
    let animation_delay = four_soldiers_set_firing_delay(soldier_index, 0);
    let routine_update = advance_enemy_routine(current_routine);
    FourSoldiersRoutine00Result { init, animation_delay, routine_update }
}

/// The real branch [`four_soldiers_routine_01`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FourSoldiersRoutine01Outcome {
    /// Decremented delay is still nonzero and isn't the fire frame.
    Waiting { animation_delay: u8 },
    /// Decremented delay hit exactly `$04` - fires a regular indoor
    /// bullet from the soldier's own position.
    Fired { animation_delay: u8, bullet: Option<EnemyClearFields> },
    /// Decremented delay reached `0` - done walking for this round,
    /// transitions to `four_soldiers_routine_02`.
    Advanced {
        /// `Some(new_x_velocity)` only for soldiers `2`/`3` on their
        /// *second* round (`ENEMY_VAR_2 == 1`) - real ASM's own "split
        /// soldiers so some go left, some go right".
        x_velocity: Option<(u8, u8)>,
        delayed_routine: DelayedRoutineUpdate,
    },
}

/// Native port of `four_soldiers_routine_01` (`$954c`) - "walk until
/// timer elapses, begin firing, move to `four_soldiers_routine_02`".
#[allow(clippy::too_many_arguments)]
pub fn four_soldiers_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_animation_delay: u8,
    soldier_index: u8,
    times_fired: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> FourSoldiersRoutine01Outcome {
    let delay = enemy_animation_delay.wrapping_sub(1);

    if delay != 0 {
        if delay == 0x04 {
            let bullet = create_indoor_bullet(prg_rom, enemy_routine, current_level, enemy_attack_flag, x_pos, y_pos).map(|b| b.fields);
            FourSoldiersRoutine01Outcome::Fired { animation_delay: delay, bullet }
        } else {
            FourSoldiersRoutine01Outcome::Waiting { animation_delay: delay }
        }
    } else {
        let should_reverse = times_fired == 1 && soldier_index >= 2;
        let x_velocity = if should_reverse { Some(reverse_enemy_x_direction(x_vel_fract, x_vel_fast)) } else { None };

        let offset = four_soldiers_get_delay_offset(soldier_index, times_fired);
        let new_delay = FOUR_SOLDIERS_DELAY_RUNNING_TBL[offset as usize];
        let delayed_routine = set_enemy_delay_adv_routine(new_delay, current_routine);

        FourSoldiersRoutine01Outcome::Advanced { x_velocity, delayed_routine }
    }
}

/// The real branch [`four_soldiers_routine_02`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FourSoldiersRoutine02Outcome {
    /// Decremented delay is still nonzero.
    StillMoving { animation_delay: u8 },
    /// Decremented delay hit `0` - sets the firing sprite, counts the
    /// shot, computes the next running delay, and jumps back to `four_
    /// soldiers_routine_01` directly (`set_enemy_routine_to_a`, not a
    /// linear advance).
    Fired {
        /// Always `$96`.
        sprites: u8,
        /// `ENEMY_VAR_2` after incrementing.
        times_fired: u8,
        animation_delay: u8,
        routine_update: EnemyRoutineUpdate,
    },
}

/// The full result of one [`four_soldiers_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FourSoldiersRoutine02Result {
    pub sprite: InitSpriteFromFrameResult,
    pub velocity: ApplyEnemyVelocityResult,
    pub outcome: FourSoldiersRoutine02Outcome,
}

/// Native port of `four_soldiers_routine_02` (`$9582`) - "waits for
/// delay, get into firing position, set new delay, go back to `four_
/// soldiers_routine_01`".
#[allow(clippy::too_many_arguments)]
pub fn four_soldiers_routine_02(
    frame_counter: u8,
    enemy_frame: u8,
    enemy_sprite_attr: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    x_pos: u8,
    enemy_animation_delay: u8,
    soldier_index: u8,
    times_fired: u8,
    current_routine: u8,
) -> FourSoldiersRoutine02Result {
    let sprite = init_sprite_from_frame(frame_counter, enemy_frame, enemy_sprite_attr, x_vel_fast);
    let velocity = apply_enemy_velocity_set_bg_priority(x_vel_accum, x_vel_fract, x_vel_fast, x_pos, sprite.sprite_attr);

    let delay = enemy_animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        FourSoldiersRoutine02Outcome::StillMoving { animation_delay: delay }
    } else {
        let times_fired = times_fired.wrapping_add(1);
        let animation_delay = four_soldiers_set_firing_delay(soldier_index, times_fired);
        let routine_update = set_enemy_routine_to_a(current_routine, 2);
        FourSoldiersRoutine02Outcome::Fired { sprites: 0x96, times_fired, animation_delay, routine_update }
    };

    FourSoldiersRoutine02Result { sprite, velocity, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 4;
        rom[record_off..record_off + 4].copy_from_slice(&[0x80, 0x00, 0x01, 0x00]);
        rom
    }

    #[test]
    fn get_delay_offset_matches_the_real_row_times_4_plus_column_formula() {
        assert_eq!(four_soldiers_get_delay_offset(0, 0), 0);
        assert_eq!(four_soldiers_get_delay_offset(3, 0), 3);
        assert_eq!(four_soldiers_get_delay_offset(0, 1), 4);
        assert_eq!(four_soldiers_get_delay_offset(2, 1), 6);
    }

    #[test]
    fn set_firing_delay_reads_the_right_table_cell() {
        assert_eq!(four_soldiers_set_firing_delay(0, 0), 0x01);
        assert_eq!(four_soldiers_set_firing_delay(3, 0), 0x13);
        assert_eq!(four_soldiers_set_firing_delay(1, 1), 0x18);
    }

    #[test]
    fn routine_00_composes_init_index_2_firing_delay_and_advance() {
        let r = four_soldiers_routine_00(0x00, 2, 5);
        assert_eq!(r.init, init_indoor_enemy_pos_and_vel(2, 0x00));
        assert_eq!(r.animation_delay, four_soldiers_set_firing_delay(2, 0));
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn routine_01_waits_while_delay_is_nonzero_and_not_the_fire_frame() {
        let rom = synthetic_prg_rom();
        let r = four_soldiers_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x0A, 0, 0, 0, 0x01, 0x50, 0x6D, 5);
        assert_eq!(r, FourSoldiersRoutine01Outcome::Waiting { animation_delay: 0x09 });
    }

    #[test]
    fn routine_01_fires_on_the_decremented_0x04_frame() {
        let rom = synthetic_prg_rom();
        // x_pos must be inside create_indoor_bullet's own $60..$a0 range gate.
        let r = four_soldiers_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x05, 0, 0, 0, 0x01, 0x80, 0x6D, 5);
        match r {
            FourSoldiersRoutine01Outcome::Fired { animation_delay, bullet } => {
                assert_eq!(animation_delay, 0x04);
                assert!(bullet.is_some());
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_soldiers_0_and_1_never_reverse_even_after_firing_once() {
        let rom = synthetic_prg_rom();
        for soldier_index in [0u8, 1] {
            let r = four_soldiers_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x01, soldier_index, 1, 0, 0x01, 0x50, 0x6D, 5);
            match r {
                FourSoldiersRoutine01Outcome::Advanced { x_velocity, .. } => assert_eq!(x_velocity, None, "soldier {soldier_index}"),
                other => panic!("expected Advanced, got {other:?}"),
            }
        }
    }

    #[test]
    fn routine_01_soldiers_2_and_3_reverse_only_after_firing_once() {
        let rom = synthetic_prg_rom();
        for soldier_index in [2u8, 3] {
            let not_yet_fired = four_soldiers_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x01, soldier_index, 0, 0, 0x01, 0x50, 0x6D, 5);
            assert!(matches!(not_yet_fired, FourSoldiersRoutine01Outcome::Advanced { x_velocity: None, .. }));

            let fired_once = four_soldiers_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x01, soldier_index, 1, 0, 0x01, 0x50, 0x6D, 5);
            match fired_once {
                FourSoldiersRoutine01Outcome::Advanced { x_velocity: Some(v), .. } => {
                    assert_eq!(v, reverse_enemy_x_direction(0, 0x01));
                }
                other => panic!("expected Advanced with a reversal, got {other:?}"),
            }
        }
    }

    #[test]
    fn routine_01_advanced_picks_the_running_delay_from_the_offset_table() {
        let rom = synthetic_prg_rom();
        let r = four_soldiers_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x01, 1, 0, 0, 0x01, 0x50, 0x6D, 5);
        match r {
            FourSoldiersRoutine01Outcome::Advanced { delayed_routine, .. } => {
                assert_eq!(delayed_routine.animation_delay, FOUR_SOLDIERS_DELAY_RUNNING_TBL[four_soldiers_get_delay_offset(1, 0) as usize]);
                assert_eq!(delayed_routine.routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Advanced, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_still_moving_while_delay_is_nonzero() {
        let r = four_soldiers_routine_02(0, 0, 0x00, 0, 0, 0x01, 0x50, 0x0A, 0, 0, 5);
        assert_eq!(r.outcome, FourSoldiersRoutine02Outcome::StillMoving { animation_delay: 0x09 });
    }

    #[test]
    fn routine_02_fires_and_jumps_back_to_routine_01_once_delay_elapses() {
        let r = four_soldiers_routine_02(0, 0, 0x00, 0, 0, 0x01, 0x50, 0x01, 2, 0, 5);
        match r.outcome {
            FourSoldiersRoutine02Outcome::Fired { sprites, times_fired, animation_delay, routine_update } => {
                assert_eq!(sprites, 0x96);
                assert_eq!(times_fired, 1);
                assert_eq!(animation_delay, four_soldiers_set_firing_delay(2, 1));
                assert_eq!(routine_update, set_enemy_routine_to_a(5, 2));
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }
}
