//! Native port of the level 5 (alien lair) alien fetus enemy,
//! `src/bank0.asm` (`alien_fetus_routine_ptr_tbl`, `$b6ec`-`$b736`):
//! `alien_fetus_routine_00` (spawn init - random initial aim direction
//! and re-aim timer) and `alien_fetus_routine_01` (mouth-flap animation,
//! periodic re-aiming toward a target player via `crate::enemy::
//! quadrant_aim_dir::aim_var_1_for_quadrant_aim_dir_00`, velocity
//! application). Previously assumed blocked behind the rotation/aiming
//! subsystem the same way `sniper_02`-`_05` were - unblocked in the same
//! pass. `alien_fetus_routine_ptr_tbl` entries `2`-`4` (explosion/
//! removal) are the same real shared `bank7.asm` routines most enemy
//! families use and aren't ported here.
//!
//! ## A real global (not per-enemy) read cursor
//!
//! `alien_fetus_get_aim_timer` (`$b7d2`) reads through `alien_fetus_aim_
//! timer_tbl` via `ALIEN_FETUS_AIM_TIMER_INDEX` - a single **global**
//! byte shared by every alien fetus enemy on screen, not a per-enemy
//! `ENEMY_VAR_x` field (contrast `red_blue_soldier_gen_routine_01`'s own
//! read cursor, `ENEMY_VAR_1`, which *is* per-enemy). Modeled as an
//! explicit `aim_timer_index: u8` in/out parameter, same shape as any
//! other piece of state this crate threads through pure functions.
//!
//! ## Two real quirks ported faithfully rather than "corrected"
//!
//! - `alien_fetus_routine_00`'s own random re-aim-target roll
//!   (`ENEMY_VAR_4`) is skipped entirely when `P2_GAME_OVER_STATUS` is
//!   set - real ASM branches straight past the whole random-number
//!   sequence, leaving `ENEMY_VAR_4` at whatever `initialize_enemy`
//!   already zeroed it to (this port's `var_4: u8` output is simply the
//!   caller's own input, unchanged, on that path).
//! - `alien_fetus_set_velocity`'s own `ENEMY_VAR_4 -= 3` (not `-2`) is a
//!   real, deliberate 6502 idiom the disassembly's own comment calls out
//!   ("clear carry so that #$03 is subtracted and not #$02") - `sbc`
//!   after `clc` (instead of the usual `sec`) subtracts one extra via the
//!   inverted borrow bit. Ported as a literal `wrapping_sub(3)`.

use crate::enemy::add_with_enemy_pos::set_08_09_to_enemy_pos;
use crate::enemy::quadrant_aim_dir::aim_var_1_for_quadrant_aim_dir_00;
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};

/// `alien_fetus_aim_timer_tbl` (`$b7e8`, 14 bytes, `0xff`-terminated) -
/// delay before re-aiming, read through sequentially and wrapping back to
/// the start once the terminator is hit.
const ALIEN_FETUS_AIM_TIMER_TBL: [u8; 14] = [0x16, 0x0F, 0x08, 0x13, 0x3A, 0x06, 0x21, 0x3A, 0x1D, 0x14, 0x12, 0x28, 0x48, 0xFF];

/// Native port of `alien_fetus_get_aim_timer` (`$b7d2`) - see this
/// module's doc comment for why `index` is a global, not per-enemy,
/// cursor. Returns `(timer, new_index)`.
pub fn alien_fetus_get_aim_timer(index: u8) -> (u8, u8) {
    let value = ALIEN_FETUS_AIM_TIMER_TBL[index as usize];
    if value == 0xFF {
        (ALIEN_FETUS_AIM_TIMER_TBL[0], 0x01)
    } else {
        (value, index.wrapping_add(1))
    }
}

/// `white_blob_alien_fetus_vel_tbl` (`$b9ef`, 30 `(fast, frac)` pairs,
/// raw ROM byte order) - the same "one overlapping sine table, no
/// separate cosine table" trick `crate::enemy::spinning_bubbles::
/// SPINNING_BULLET_VEL_TBL` uses, but sampled at *half* the resolution:
/// this enemy's 12-step wheel (`quadrant_aim_dir_00`) reads every other
/// entry of what is structurally the same 24-sample-per-circle table
/// shape (`Y = table[aim_dir*2]`, `X = table[aim_dir*2 + 6]` - a quarter
/// turn is 6 entries on a 24-sample circle, matching `spinning_bubbles`'
/// own `+6` even though the two *wheels* are different sizes). Verified
/// against real trigonometry, not just transcribed blindly: `aim_dir=0`
/// ("facing right") gives `Y=(0,0)` (no vertical motion) and `X=(0,
/// 0xff)` (peak rightward) as expected, and `aim_dir=1`'s `Y=0x7f`/
/// `X=0xdd` ratio (~0.498/~0.867) matches `sin(30°)`/`cos(30°)`
/// (~0.5/~0.866) to 3 decimal places - the real disassembly's own inline
/// comments ("aim rotation dir - #$00 - facing right" on entry `1`, not
/// `0`) are misplaced by one entry; this port follows the literal
/// `asl;asl` byte-offset arithmetic, not the comment text.
const WHITE_BLOB_ALIEN_FETUS_VEL_TBL: [(u8, u8); 30] = [
    (0x00, 0x00),
    (0x00, 0x42),
    (0x00, 0x7F),
    (0x00, 0xB2),
    (0x00, 0xDD),
    (0x00, 0xF7),
    (0x00, 0xFF),
    (0x00, 0xF7),
    (0x00, 0xDD),
    (0x00, 0xB2),
    (0x00, 0x7F),
    (0x00, 0x42),
    (0x00, 0x00),
    (0xFF, 0xBE),
    (0xFF, 0x81),
    (0xFF, 0x4E),
    (0xFF, 0x23),
    (0xFF, 0x09),
    (0xFF, 0x01),
    (0xFF, 0x09),
    (0xFF, 0x23),
    (0xFF, 0x4E),
    (0xFF, 0x81),
    (0xFF, 0xBE),
    (0x00, 0x00),
    (0x00, 0x42),
    (0x00, 0x7F),
    (0x00, 0xB2),
    (0x00, 0xDD),
    (0x00, 0xF7),
];

/// Native port of `set_white_blob_alien_fetus_vel` (`$b7b9`) - returns
/// `(y_velocity, x_velocity)` as `(frac, fast)` pairs, matching this
/// crate's usual convention (the table's own raw ROM layout is `(fast,
/// frac)` - see `WHITE_BLOB_ALIEN_FETUS_VEL_TBL`'s doc comment).
pub fn set_white_blob_alien_fetus_vel(aim_dir: u8) -> ((u8, u8), (u8, u8)) {
    let y_idx = aim_dir as usize * 2;
    let x_idx = y_idx + 6;
    let (y_fast, y_frac) = WHITE_BLOB_ALIEN_FETUS_VEL_TBL[y_idx];
    let (x_fast, x_frac) = WHITE_BLOB_ALIEN_FETUS_VEL_TBL[x_idx];
    ((y_frac, y_fast), (x_frac, x_fast))
}

/// Native port of `alien_fetus_set_velocity` (`$b7a6`) - the real shared
/// tail both `alien_fetus_routine_00` (`jmp`) and `alien_fetus_routine_
/// 01`'s own re-aim path (`jsr`, after rotating `ENEMY_VAR_1`) fall into:
/// sets velocity from `var_1` via [`set_white_blob_alien_fetus_vel`],
/// then reduces `var_4` by `3` (see this module's doc comment for the
/// real `clc`/`sbc` quirk this is). Returns `(y_velocity, x_velocity,
/// new_var_4)`.
fn alien_fetus_set_velocity(var_1: u8, var_4: u8) -> ((u8, u8), (u8, u8), u8) {
    let (y_velocity, x_velocity) = set_white_blob_alien_fetus_vel(var_1);
    (y_velocity, x_velocity, var_4.wrapping_sub(3))
}

/// The full result of one [`alien_fetus_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienFetusRoutine00Result {
    /// `ENEMY_VAR_3` - the re-aim timer, doubled from the raw table
    /// value (`asl ENEMY_VAR_3,x`).
    pub var_3: u8,
    pub aim_timer_index: u8,
    /// `ENEMY_HP` - `GAME_COMPLETION_COUNT + 2`.
    pub hp: u8,
    /// Always `0xac`.
    pub sprite: u8,
    /// Always `0x06`.
    pub animation_delay: u8,
    pub var_4: u8,
    pub var_1: u8,
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    /// `ENEMY_ROUTINE` after this call - a real, literal `inc ENEMY_
    /// ROUTINE,x`, *not* a call through the usual guarded [`crate::enemy::
    /// enemy_routine_transition::advance_enemy_routine`] helper. This
    /// only differs from the guarded version if the enemy was already
    /// removed earlier in the same call - nothing in this routine's own
    /// body can trigger that, so the two are behaviorally identical here;
    /// ported as the literal instruction regardless.
    pub new_routine: u8,
}

/// Native port of `alien_fetus_routine_00` (`$b6ec`) - spawn init: rolls
/// a random re-aim timer and initial aim direction/velocity, sets HP from
/// the game-completion count, and picks a target-player-control value for
/// `_01`'s own re-aiming (skipped entirely if player 2 is in a game-over
/// state - see this module's doc comment).
#[allow(clippy::too_many_arguments)]
pub fn alien_fetus_routine_00(
    aim_timer_index: u8,
    game_completion_count: u8,
    p1_game_over_status: u8,
    p2_game_over_status: u8,
    random_num: u8,
    frame_counter: u8,
    enemy_attributes: u8,
    enemy_var_4: u8,
    current_routine: u8,
) -> AlienFetusRoutine00Result {
    let (timer, aim_timer_index) = alien_fetus_get_aim_timer(aim_timer_index);
    let var_3 = timer.wrapping_mul(2);

    let (hp, hp_carry) = game_completion_count.overflowing_add(2);

    let var_4 = if p2_game_over_status != 0 {
        enemy_var_4
    } else {
        let sum = random_num as u16 + frame_counter as u16 + hp_carry as u16;
        let rolled = ((sum as u8) & 0x1F).wrapping_add(0x0E);
        if p1_game_over_status != 0 { 0x01 } else { rolled }
    };

    let r = random_num & 0x03;
    let r = if r == 0 { 0x03 } else { r };
    let doubled = r.wrapping_mul(2);
    let var_1 = if enemy_attributes == 0 { doubled } else { 0x06 };

    let (y_velocity, x_velocity, var_4) = alien_fetus_set_velocity(var_1, var_4);
    let new_routine = current_routine.wrapping_add(1);

    AlienFetusRoutine00Result {
        var_3,
        aim_timer_index,
        hp,
        sprite: 0xAC,
        animation_delay: 0x06,
        var_4,
        var_1,
        y_velocity,
        x_velocity,
        new_routine,
    }
}

/// One [`alien_fetus_routine_01`] call's own mouth-flap sprite update -
/// only computed when the animation delay actually elapses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienFetusSpriteResult {
    pub var_2: u8,
    pub sprite_attr: u8,
    pub sprite: u8,
}

/// Native port of `alien_fetus_routine_01`'s own inline sprite-cycling
/// logic (`$b74a`-`$b78e`) - picks one of 3 sprite offsets from the
/// (already-clockwise-advanced) aim direction, toggles the mouth-open
/// flag, and mirrors the sprite for the "other half" of the direction
/// wheel. The real ASM derives the offset via a repeated-subtract-3 loop
/// bounded to a `0..11` input - exactly integer division by `3` over that
/// domain, so this port uses `/` directly rather than replicating the
/// loop.
fn alien_fetus_animate(enemy_var_1: u8, enemy_var_2: u8, enemy_sprite_attr: u8) -> AlienFetusSpriteResult {
    let advanced = enemy_var_1.wrapping_add(1);
    let dir_plus_1 = if advanced == 0x0C { 0x00 } else { advanced };
    let step = dir_plus_1 / 3;

    let var_2 = enemy_var_2 ^ 0x01;
    let mut sprite_attr = enemy_sprite_attr & 0x3F;
    let mut offset = step.wrapping_mul(2);
    if offset >= 0x04 {
        sprite_attr |= 0xC0;
        offset = offset.wrapping_sub(0x04);
    }
    let sprite = offset.wrapping_add(0xAC).wrapping_add(var_2);

    AlienFetusSpriteResult { var_2, sprite_attr, sprite }
}

/// The real, branchy result of [`alien_fetus_routine_01`]'s own periodic
/// re-aiming attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlienFetusVelocityOutcome {
    /// `ENEMY_VAR_3` (decremented) still nonzero - no readjustment
    /// attempt this call.
    NotDue { var_3: u8 },
    /// `ENEMY_VAR_3` reached `0`, but `ENEMY_VAR_4`'s readjustment-
    /// enabled bits (`& 0x3e`) were already clear - stops attempting
    /// permanently. `ENEMY_VAR_3` is *not* reset here (real ASM never
    /// calls `alien_fetus_get_aim_timer` on this path), so it keeps
    /// decrementing/wrapping every subsequent call, matching real
    /// behavior exactly rather than special-casing it away.
    ReadjustmentDisabled,
    /// Rotated one step toward the target player, re-derived velocity
    /// from the new aim direction, and reduced `ENEMY_VAR_4`'s own
    /// readjustment budget by `3`.
    Readjusted { var_3: u8, aim_timer_index: u8, aim_dir: u8, y_velocity: (u8, u8), x_velocity: (u8, u8), var_4: u8 },
}

/// The full result of one [`alien_fetus_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienFetusRoutine01Result {
    pub animation_delay: u8,
    /// `Some` only the call the animation delay actually elapses (real
    /// ASM skips the whole sprite-cycling block otherwise).
    pub sprite: Option<AlienFetusSpriteResult>,
    pub velocity_outcome: AlienFetusVelocityOutcome,
    pub position: UpdatedEnemyPos,
}

/// Native port of `alien_fetus_routine_01` (`$b736`) - cycles the mouth-
/// flap animation, periodically re-aims toward a target player (up to
/// `ENEMY_VAR_4`'s own budget, decremented by `3` per attempt), and
/// applies velocity/scroll every call regardless.
#[allow(clippy::too_many_arguments)]
pub fn alien_fetus_routine_01(
    enemy_animation_delay: u8,
    enemy_var_1: u8,
    enemy_var_2: u8,
    enemy_sprite_attr: u8,
    enemy_var_3: u8,
    enemy_var_4: u8,
    aim_timer_index: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> AlienFetusRoutine01Result {
    let decremented_delay = enemy_animation_delay.wrapping_sub(1);
    let (animation_delay, sprite) = if decremented_delay != 0 {
        (decremented_delay, None)
    } else {
        (0x06, Some(alien_fetus_animate(enemy_var_1, enemy_var_2, enemy_sprite_attr)))
    };

    let var_3 = enemy_var_3.wrapping_sub(1);
    let velocity_outcome = if var_3 != 0 {
        AlienFetusVelocityOutcome::NotDue { var_3 }
    } else if enemy_var_4 & 0x3E == 0 {
        AlienFetusVelocityOutcome::ReadjustmentDisabled
    } else {
        let (timer, aim_timer_index) = alien_fetus_get_aim_timer(aim_timer_index);
        let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
        let player_index = enemy_var_4 & 0x01;
        let rotate = aim_var_1_for_quadrant_aim_dir_00(
            source_y,
            source_x,
            player_index,
            player_state,
            sprite_y_pos,
            sprite_x_pos,
            level_location_type,
            enemy_var_1,
        );
        let (y_velocity, x_velocity, var_4) = alien_fetus_set_velocity(rotate.new_aim_dir, enemy_var_4);
        AlienFetusVelocityOutcome::Readjusted { var_3: timer, aim_timer_index, aim_dir: rotate.new_aim_dir, y_velocity, x_velocity, var_4 }
    };

    let position = update_enemy_pos(
        level_scrolling_type,
        frame_scroll,
        enemy_x_pos,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        enemy_y_pos,
        y_vel_accum,
        y_vel_fract,
        y_vel_fast,
    );

    AlienFetusRoutine01Result { animation_delay, sprite, velocity_outcome, position }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_aim_timer_reads_sequentially_and_advances_the_index() {
        let (v0, i1) = alien_fetus_get_aim_timer(0x00);
        assert_eq!((v0, i1), (0x16, 0x01));
        let (v1, i2) = alien_fetus_get_aim_timer(i1);
        assert_eq!((v1, i2), (0x0F, 0x02));
    }

    #[test]
    fn get_aim_timer_wraps_at_the_terminator() {
        // index 13 holds the 0xff terminator - resets and re-reads entry 0.
        let (v, new_index) = alien_fetus_get_aim_timer(0x0D);
        assert_eq!((v, new_index), (0x16, 0x01));
    }

    #[test]
    fn set_white_blob_alien_fetus_vel_matches_expected_trig_ratios() {
        // aim_dir=0 ("facing right"): Y should be ~0, X should be peak.
        let (y0, x0) = set_white_blob_alien_fetus_vel(0x00);
        assert_eq!(y0, (0x00, 0x00));
        assert_eq!(x0, (0xFF, 0x00)); // (frac, fast) = (0xff, 0x00) -> ~0.996, "peak"
    }

    #[test]
    fn set_white_blob_alien_fetus_vel_swaps_the_raw_fast_frac_order() {
        // aim_dir=1 -> y_idx=2 -> raw entry 2 = (fast=0x00, frac=0x7f);
        // the function must return (frac, fast) = (0x7f, 0x00).
        let (y, _x) = set_white_blob_alien_fetus_vel(0x01);
        assert_eq!(WHITE_BLOB_ALIEN_FETUS_VEL_TBL[2], (0x00, 0x7F));
        assert_eq!(y, (0x7F, 0x00));
    }

    #[test]
    fn routine_00_p2_game_over_leaves_var_4_untouched_before_the_shared_tail_subtracts_3() {
        let r = alien_fetus_routine_00(0x00, 0x00, 0x00, 0x01, 0x10, 0x20, 0x00, 0x50, 5);
        // p2 game over -> var_4 input (0x50) passed straight to the shared
        // tail, which subtracts 3.
        assert_eq!(r.var_4, 0x50u8.wrapping_sub(3));
    }

    #[test]
    fn routine_00_p1_game_over_forces_var_4_to_1_then_the_shared_tail_subtracts_3() {
        let r = alien_fetus_routine_00(0x00, 0x00, 0x01, 0x00, 0x10, 0x20, 0x00, 0x00, 5);
        assert_eq!(r.var_4, 0x01u8.wrapping_sub(3));
    }

    #[test]
    fn routine_00_normal_case_rolls_var_4_from_rng_then_the_shared_tail_subtracts_3() {
        let r = alien_fetus_routine_00(0x00, 0x00, 0x00, 0x00, 0x10, 0x20, 0x00, 0x00, 5);
        let rolled = ((0x10u16 + 0x20u16) as u8 & 0x1F).wrapping_add(0x0E);
        assert_eq!(r.var_4, rolled.wrapping_sub(3));
    }

    #[test]
    fn routine_00_var_1_uses_the_doubled_random_pick_when_attributes_is_zero() {
        // random_num & 3 == 0 -> forced to 3 -> doubled to 6.
        let r = alien_fetus_routine_00(0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 5);
        assert_eq!(r.var_1, 0x06);
    }

    #[test]
    fn routine_00_var_1_is_fixed_when_attributes_is_nonzero() {
        let r = alien_fetus_routine_00(0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 5);
        assert_eq!(r.var_1, 0x06);
    }

    #[test]
    fn routine_00_hp_is_completion_count_plus_2() {
        let r = alien_fetus_routine_00(0x00, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 5);
        assert_eq!(r.hp, 0x07);
    }

    #[test]
    fn routine_00_doubles_the_aim_timer_and_advances_routine_by_one_raw() {
        let r = alien_fetus_routine_00(0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 5);
        assert_eq!(r.var_3, 0x16u8.wrapping_mul(2));
        assert_eq!(r.new_routine, 6);
    }

    #[test]
    fn animate_toggles_mouth_and_picks_the_unmirrored_offset_for_a_low_direction() {
        // enemy_var_1=0 -> advanced=1 -> step=1/3=0 -> offset=0 -> unmirrored.
        let r = alien_fetus_animate(0x00, 0x00, 0b0011_0000);
        assert_eq!(r.var_2, 0x01);
        assert_eq!(r.sprite_attr & 0xC0, 0x00);
        assert_eq!(r.sprite, 0xAC + 0x01);
    }

    #[test]
    fn animate_mirrors_and_offsets_for_a_high_direction() {
        // enemy_var_1=8 -> advanced=9 -> step=9/3=3 -> offset=6 -> mirrored, offset-4=2.
        let r = alien_fetus_animate(0x08, 0x00, 0b0011_0000);
        assert_eq!(r.sprite_attr & 0xC0, 0xC0);
        assert_eq!(r.sprite, 0xAC + 2 + 0x01);
    }

    #[test]
    fn animate_wraps_the_advanced_direction_at_12() {
        // enemy_var_1=11 -> advanced=12 -> wraps to 0 -> step=0.
        let r = alien_fetus_animate(0x0B, 0x00, 0x00);
        assert_eq!(r.sprite, 0xAC + 0x01);
    }

    #[test]
    fn routine_01_waits_when_the_animation_delay_has_not_elapsed() {
        let r = alien_fetus_routine_01(0x05, 0, 0, 0, 0x14, 0x00, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.animation_delay, 0x04);
        assert_eq!(r.sprite, None);
    }

    #[test]
    fn routine_01_animates_once_the_delay_elapses() {
        let r = alien_fetus_routine_01(0x01, 0x00, 0x00, 0x00, 0x14, 0x00, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.animation_delay, 0x06);
        assert!(r.sprite.is_some());
    }

    #[test]
    fn routine_01_not_due_when_var_3_has_not_reached_zero() {
        let r = alien_fetus_routine_01(0x05, 0, 0, 0, 0x05, 0x3E, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.velocity_outcome, AlienFetusVelocityOutcome::NotDue { var_3: 0x04 });
    }

    #[test]
    fn routine_01_readjustment_disabled_once_var_4s_relevant_bits_are_clear() {
        let r = alien_fetus_routine_01(0x05, 0, 0, 0, 0x01, 0x01, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.velocity_outcome, AlienFetusVelocityOutcome::ReadjustmentDisabled);
    }

    #[test]
    fn routine_01_readjusts_and_resets_the_timer_when_var_3_is_due_and_var_4_permits() {
        let r = alien_fetus_routine_01(0x05, 0x00, 0, 0, 0x01, 0x3E, 0x00, 0x50, 0x60, [1, 0], [0x30, 0], [0x90, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        match r.velocity_outcome {
            AlienFetusVelocityOutcome::Readjusted { var_3, var_4, .. } => {
                assert_eq!(var_3, ALIEN_FETUS_AIM_TIMER_TBL[0]);
                assert_eq!(var_4, 0x3Eu8.wrapping_sub(3));
            }
            other => panic!("expected Readjusted, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_position_matches_update_enemy_pos_directly() {
        let r = alien_fetus_routine_01(0x05, 0, 0, 0, 0x14, 0x00, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x02, 0, 0x10, 0x00, 0, 0x08, 0x00);
        let expected = update_enemy_pos(0, 0x02, 0x50, 0, 0x10, 0x00, 0x60, 0, 0x08, 0x00);
        assert_eq!(r.position, expected);
    }
}
