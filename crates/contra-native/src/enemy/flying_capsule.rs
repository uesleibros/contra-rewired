//! Native port of the flying weapon capsule ("weapon zeppelin")'s own
//! routine table (`src/bank0.asm`, `$830b`-`$8376`): `flying_capsule_
//! routine_00`/`_01`/`_02`. Flies a slow, oscillating spring-like path
//! (bobbing vertically on horizontal levels, side-to-side on the level 3
//! waterfall's vertical scroll) until destroyed, at which point it
//! explodes and drops a weapon item (`_02` is a one-line `jmp play_
//! explosion_sound`, already ported).
//!
//! ## `set_flying_capsule_path`'s spring term
//! [`set_flying_capsule_y_vel`]/[`set_flying_capsule_x_vel`] both fall
//! into one shared private core, `set_flying_capsule_path`: computes
//! `2 * (position - reference)` as
//! a signed 16-bit value (`reference` is the capsule's own starting
//! point, captured once in `ENEMY_VAR_1`/`ENEMY_VAR_2` by `_00`), then
//! subtracts that from the *base* velocity - a linear restoring force
//! that grows the further the capsule drifts from where it started,
//! producing the oscillation. Real ASM's own shift-count parameter (`y`
//! register) supports an arbitrary left or right shift via two loops,
//! but both real callers here always pass `y = 1` (a single left shift,
//! i.e. exactly the "times 2" described above) - the right-shift path
//! (negative `y`) is real, valid control flow this port still models,
//! but it's **not exercised by any real caller in this family** and so
//! isn't independently verified the way the `y = 1` path is.

use crate::enemy::enemy_explosion::{play_explosion_sound, PlayExplosionSoundResult};
use crate::enemy::enemy_position_utils::{add_a_to_enemy_x_pos, add_a_to_enemy_y_pos};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};

/// `flying_capsule_vel_tbl` (`$8355`, 2 rows of `(y_fract, y_fast,
/// x_fract, x_fast)`) - row 0 for horizontal/indoor levels, row 1 for a
/// vertical level's own initial velocity.
const FLYING_CAPSULE_VEL_TBL: [(u8, u8, u8, u8); 2] = [(0x00, 0x00, 0x80, 0x01), (0x80, 0xFE, 0x00, 0x00)];

/// Native port of `set_flying_capsule_path` (`$ec93`) - see this
/// module's doc comment for the spring-term math and the unverified
/// right-shift path.
fn set_flying_capsule_path(y_shift: i8, position: u8, reference: u8, base_vel_fract: u8, base_vel_fast: u8) -> (u8, u8) {
    let diff = position.wrapping_sub(reference) as i8 as i16;
    let shifted: i16 = if y_shift == 0 {
        diff
    } else if y_shift > 0 {
        diff.wrapping_shl(y_shift as u32)
    } else {
        diff.wrapping_shr((-y_shift) as u32)
    };
    let shifted_lo = shifted as u16 as u8;
    let shifted_hi = (shifted as u16 >> 8) as u8;

    let (new_fract, borrow) = base_vel_fract.overflowing_sub(shifted_lo);
    let new_fast = base_vel_fast.wrapping_sub(shifted_hi).wrapping_sub(borrow as u8);
    (new_fast, new_fract)
}

/// Native port of `set_flying_capsule_y_vel` (`$ec4b`) - real callers
/// always pass a shift of `1`. Returns `(y_vel_fast, y_vel_fract)`.
pub fn set_flying_capsule_y_vel(enemy_y_pos: u8, enemy_var_1: u8, y_vel_fast: u8, y_vel_fract: u8) -> (u8, u8) {
    set_flying_capsule_path(1, enemy_y_pos, enemy_var_1, y_vel_fract, y_vel_fast)
}

/// Native port of `set_flying_capsule_x_vel` (`$ec6f`) - real callers
/// always pass a shift of `1`. Returns `(x_vel_fast, x_vel_fract)`.
pub fn set_flying_capsule_x_vel(enemy_x_pos: u8, enemy_var_2: u8, x_vel_fast: u8, x_vel_fract: u8) -> (u8, u8) {
    set_flying_capsule_path(1, enemy_x_pos, enemy_var_2, x_vel_fract, x_vel_fast)
}

/// The real branch [`flying_capsule_routine_00`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlyingCapsuleRoutine00Outcome {
    /// `LEVEL_SCROLLING_TYPE == 0` - horizontal/indoor level.
    Horizontal { y_pos: u8, x_pos: u8 },
    /// `LEVEL_SCROLLING_TYPE != 0` - vertical level (level 3's
    /// waterfall).
    Vertical { x_pos: u8, y_pos: u8 },
}

/// The full result of one [`flying_capsule_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlyingCapsuleRoutine00Result {
    /// Always `$03` - the zeppelin's own sprite palette.
    pub sprite_attr: u8,
    /// `ENEMY_VAR_1` - the capsule's own starting Y position, captured
    /// *before* [`FlyingCapsuleRoutine00Outcome`]'s own position write -
    /// [`set_flying_capsule_y_vel`]'s later "reference point".
    pub var_1: u8,
    /// `ENEMY_VAR_2` - same, for X position / [`set_flying_capsule_x_vel`].
    pub var_2: u8,
    pub outcome: FlyingCapsuleRoutine00Outcome,
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `flying_capsule_routine_00` (`$830b`).
pub fn flying_capsule_routine_00(level_scrolling_type: u8, enemy_y_pos: u8, enemy_x_pos: u8, current_routine: u8) -> FlyingCapsuleRoutine00Result {
    let var_1 = enemy_y_pos;
    let var_2 = enemy_x_pos;

    let (outcome, row) = if level_scrolling_type == 0 {
        (FlyingCapsuleRoutine00Outcome::Horizontal { y_pos: add_a_to_enemy_y_pos(0x20, enemy_y_pos), x_pos: 0x10 }, 0)
    } else {
        (FlyingCapsuleRoutine00Outcome::Vertical { x_pos: add_a_to_enemy_x_pos(0x20, enemy_x_pos), y_pos: 0xE0 }, 1)
    };

    let (y_fract, y_fast, x_fract, x_fast) = FLYING_CAPSULE_VEL_TBL[row];

    FlyingCapsuleRoutine00Result {
        sprite_attr: 0x03,
        var_1,
        var_2,
        outcome,
        y_velocity: (y_fract, y_fast),
        x_velocity: (x_fract, x_fast),
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// The full result of one [`flying_capsule_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlyingCapsuleRoutine01Result {
    /// Always `$4d`.
    pub sprites: u8,
    pub position: UpdatedEnemyPos,
}

/// Native port of `flying_capsule_routine_01` (`$835d`) - horizontal
/// levels oscillate the capsule's Y velocity (bobbing while flying
/// across), vertical levels oscillate its X velocity (swaying while
/// flying up the waterfall) - see this module's doc comment.
#[allow(clippy::too_many_arguments)]
pub fn flying_capsule_routine_01(
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    enemy_var_2: u8,
    y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    enemy_var_1: u8,
) -> FlyingCapsuleRoutine01Result {
    let (final_x_fract, final_x_fast, final_y_fract, final_y_fast) = if level_scrolling_type == 0 {
        let (new_y_fast, new_y_fract) = set_flying_capsule_y_vel(y_pos, enemy_var_1, y_vel_fast, y_vel_fract);
        (x_vel_fract, x_vel_fast, new_y_fract, new_y_fast)
    } else {
        let (new_x_fast, new_x_fract) = set_flying_capsule_x_vel(x_pos, enemy_var_2, x_vel_fast, x_vel_fract);
        (new_x_fract, new_x_fast, y_vel_fract, y_vel_fast)
    };

    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, final_x_fract, final_x_fast, y_pos, y_vel_accum, final_y_fract, final_y_fast);

    FlyingCapsuleRoutine01Result { sprites: 0x4D, position }
}

/// Native port of `flying_capsule_routine_02` (`$8376`) - "create
/// explosion sound and 2 sets of explosion type `$89` at location", a
/// one-line `jmp play_explosion_sound` (already ported).
pub fn flying_capsule_routine_02(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_attributes: u8,
) -> PlayExplosionSoundResult {
    play_explosion_sound(prg_rom, enemy_routine, current_level, enemy_x_pos, enemy_y_pos, enemy_attributes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_flying_capsule_path_subtracts_twice_the_position_reference_diff() {
        // position=0x50, reference=0x40 -> diff=0x10 (16), shifted (y=1) = 32 (0x20).
        // base_vel = (fract=0x30, fast=0x00) -> new = 0x30 - 0x20 = 0x10, no borrow -> fast unchanged.
        let (fast, fract) = set_flying_capsule_path(1, 0x50, 0x40, 0x30, 0x00);
        assert_eq!((fast, fract), (0x00, 0x10));
    }

    #[test]
    fn set_flying_capsule_path_handles_a_negative_diff() {
        // position=0x40, reference=0x50 -> diff=-16 (0xf0 wrapped), shifted = -32 (0xffe0).
        // base_vel fract=0x00, fast=0x00 -> new = 0x0000 - 0xffe0 = 0x0020.
        let (fast, fract) = set_flying_capsule_path(1, 0x40, 0x50, 0x00, 0x00);
        assert_eq!((fast, fract), (0x00, 0x20));
    }

    #[test]
    fn set_flying_capsule_path_zero_shift_uses_the_diff_directly() {
        // y_shift=0 -> shifted = diff = 0x10 (16), no doubling.
        let (fast, fract) = set_flying_capsule_path(0, 0x50, 0x40, 0x30, 0x00);
        assert_eq!((fast, fract), (0x00, 0x20));
    }

    #[test]
    fn set_flying_capsule_y_vel_matches_the_shared_core_with_shift_1() {
        let direct = set_flying_capsule_path(1, 0x60, 0x50, 0x10, 0x00);
        let via_wrapper = set_flying_capsule_y_vel(0x60, 0x50, 0x00, 0x10);
        assert_eq!(direct, via_wrapper);
    }

    #[test]
    fn routine_00_horizontal_sets_the_right_position_and_table_row() {
        let r = flying_capsule_routine_00(0, 0x60, 0x50, 5);
        assert_eq!(r.sprite_attr, 0x03);
        assert_eq!(r.var_1, 0x60);
        assert_eq!(r.var_2, 0x50);
        assert_eq!(r.outcome, FlyingCapsuleRoutine00Outcome::Horizontal { y_pos: add_a_to_enemy_y_pos(0x20, 0x60), x_pos: 0x10 });
        assert_eq!(r.y_velocity, (0x00, 0x00));
        assert_eq!(r.x_velocity, (0x80, 0x01));
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn routine_00_vertical_sets_the_right_position_and_table_row() {
        let r = flying_capsule_routine_00(1, 0x60, 0x50, 5);
        assert_eq!(r.outcome, FlyingCapsuleRoutine00Outcome::Vertical { x_pos: add_a_to_enemy_x_pos(0x20, 0x50), y_pos: 0xE0 });
        assert_eq!(r.y_velocity, (0x80, 0xFE));
        assert_eq!(r.x_velocity, (0x00, 0x00));
    }

    #[test]
    fn routine_01_horizontal_level_oscillates_y_velocity_only() {
        let r = flying_capsule_routine_01(0, 0x01, 0x50, 0, 0x80, 0x01, 0x50, 0x60, 0, 0x00, 0x00, 0x40);
        let expected_y = set_flying_capsule_y_vel(0x60, 0x40, 0x00, 0x00);
        let expected_position = update_enemy_pos(0, 0x01, 0x50, 0, 0x80, 0x01, 0x60, 0, expected_y.1, expected_y.0);
        assert_eq!(r.sprites, 0x4D);
        assert_eq!(r.position, expected_position);
    }

    #[test]
    fn routine_01_vertical_level_oscillates_x_velocity_only() {
        let r = flying_capsule_routine_01(1, 0x01, 0x50, 0, 0x00, 0x00, 0x40, 0x60, 0, 0x80, 0xFE, 0x50);
        let expected_x = set_flying_capsule_x_vel(0x50, 0x40, 0x00, 0x00);
        let expected_position = update_enemy_pos(1, 0x01, 0x50, 0, expected_x.1, expected_x.0, 0x60, 0, 0x80, 0xFE);
        assert_eq!(r.position, expected_position);
    }

    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 0x02 * 4;
        rom[record_off..record_off + 4].copy_from_slice(&[0x80, 0x00, 0x05, 0x00]);
        rom
    }

    #[test]
    fn routine_02_delegates_entirely_to_play_explosion_sound() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = flying_capsule_routine_02(&rom, &routine, 0, 0x50, 0x60, 0b0000_0101);
        let expected = play_explosion_sound(&rom, &routine, 0, 0x50, 0x60, 0b0000_0101);
        assert_eq!(r, expected);
    }
}
