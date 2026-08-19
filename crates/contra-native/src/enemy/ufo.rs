//! Native port of the mini UFO ("flying saucer", level 5 - alien
//! carrier boss) and its dropped bomb (`src/bank0.asm`, `$a8fa`-`$a97e`):
//! `mini_ufo` flies horizontally, cycling through its own sprite
//! animation, until it nears either screen edge, then descends,
//! reverses direction at the bottom of its arc, and repeats;
//! `boss_ufo_bomb` is the projectile it (or the carrier boss itself)
//! drops - falls under gravity until it reaches a fixed explosion
//! height.

use crate::enemy::enemy_position_utils::add_a_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::update_enemy_pos::{
    remove_enemy, set_enemy_y_velocity_to_0, update_enemy_pos, update_enemy_x_pos_rem_off_screen, update_enemy_y_pos_with_scroll, AxisUpdate, RemovedEnemy, UpdatedEnemyPos, ZeroedVelocity,
};

/// Native port of `set_mini_ufo_sprite` (`$a955`) - cycles the sprite
/// once every 4 frames (`ENEMY_ANIMATION_DELAY & 3 == 0`), wrapping back
/// to `sprite_7c` once it runs past the last mini-UFO sprite (`>= $7f`).
/// `None` means the real ASM's own early `bne @exit` - sprite untouched
/// this call.
fn set_mini_ufo_sprite(animation_delay: u8, sprite: u8) -> Option<u8> {
    if animation_delay & 0x03 != 0 {
        return None;
    }
    let advanced = sprite.wrapping_add(1);
    Some(if advanced >= 0x7F { advanced.wrapping_sub(3) } else { advanced })
}

/// Native port of `dec_mini_ufo_anim_delay_set_sprite` (`$a952`) - real
/// ASM's own `dec ENEMY_ANIMATION_DELAY,x` falling straight into `set_
/// mini_ufo_sprite`.
fn dec_mini_ufo_anim_delay_set_sprite(animation_delay: u8, sprite: u8) -> (u8, Option<u8>) {
    let delay = animation_delay.wrapping_sub(1);
    (delay, set_mini_ufo_sprite(delay, sprite))
}

/// One [`mini_ufo_routine_00`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniUfoRoutine00Outcome {
    Advanced(EnemyRoutineUpdate),
    Animating { sprite: Option<u8> },
}

/// The full result of one [`mini_ufo_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniUfoRoutine00Result {
    pub animation_delay: u8,
    pub outcome: MiniUfoRoutine00Outcome,
}

/// Native port of `mini_ufo_routine_00` (`$a8fa`).
pub fn mini_ufo_routine_00(animation_delay: u8, sprite: u8, current_routine: u8) -> MiniUfoRoutine00Result {
    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay == 0 {
        MiniUfoRoutine00Outcome::Advanced(advance_enemy_routine(current_routine))
    } else {
        MiniUfoRoutine00Outcome::Animating { sprite: set_mini_ufo_sprite(delay, sprite) }
    };
    MiniUfoRoutine00Result { animation_delay: delay, outcome }
}

/// One [`mini_ufo_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniUfoRoutine01Outcome {
    /// Mid-flight, not yet near either screen edge.
    Waiting,
    /// Reached a descent point (`< $20` moving left, `>= $e0` moving
    /// right) - sets a fixed downward velocity and advances.
    BeginDescent { y_vel_fract: u8, y_vel_fast: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`mini_ufo_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniUfoRoutine01Result {
    pub animation_delay: u8,
    pub sprite: Option<u8>,
    pub x: AxisUpdate,
    /// `Some` when the X update alone pushed it off the left edge. Real
    /// ASM never checks this before continuing - the `$20`/`$e0` check
    /// below runs against the (already scroll-removed) position either
    /// way, so `BeginDescent` can still be the real outcome even when
    /// removed (its own `advance_enemy_routine` call is simply guard-
    /// rejected in that case).
    pub removed: Option<RemovedEnemy>,
    pub outcome: MiniUfoRoutine01Outcome,
}

/// Native port of `mini_ufo_routine_01` (`$a905`) - real ASM's own two
/// `cmp`/`bcc` checks (`< $20` then `< $e0`) fall through into the same
/// `@begin_descent` code on *both* "moving left past $20" and "moving
/// right past $e0" (the second `bcc` only skips descent for the middle
/// band, `$20..$e0`), not two separate branches - ported as one `||`
/// condition rather than two lookalike match arms to keep that real
/// fall-through visible. `update_enemy_x_pos_rem_off_screen` reaches its
/// own removal via a real tail `jmp`, so - like `enemy_bullet_
/// routine_01` and every other same-shaped call in this crate - a
/// removal there still returns into this routine's own remaining code,
/// which must treat `current_routine` as already-zeroed for its own
/// `advance_enemy_routine` call in that case.
#[allow(clippy::too_many_arguments)]
pub fn mini_ufo_routine_01(animation_delay: u8, sprite: u8, x_pos: u8, x_vel_accum: u8, x_vel_fract: u8, x_vel_fast: u8, frame_scroll: u8, current_routine: u8) -> MiniUfoRoutine01Result {
    let (animation_delay, sprite) = dec_mini_ufo_anim_delay_set_sprite(animation_delay, sprite);
    let (x, removed) = update_enemy_x_pos_rem_off_screen(x_pos, x_vel_accum, x_vel_fract, x_vel_fast, frame_scroll);
    let effective_routine = if removed.is_some() { 0 } else { current_routine };

    let outcome = if x.pos < 0x20 || x.pos >= 0xE0 {
        MiniUfoRoutine01Outcome::BeginDescent { y_vel_fract: 0x80, y_vel_fast: 0x01, routine_update: advance_enemy_routine(effective_routine) }
    } else {
        MiniUfoRoutine01Outcome::Waiting
    };

    MiniUfoRoutine01Result { animation_delay, sprite, x, removed, outcome }
}

/// One [`mini_ufo_routine_02`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniUfoRoutine02Outcome {
    /// Still descending, hasn't reached the bottom limit (`< $a8`) yet.
    StillDescending,
    /// Reached the bottom - snaps to the fixed `$a9` Y position, picks a
    /// horizontal direction based on which side of the screen it's on,
    /// zeroes Y velocity, and advances.
    ReachedBottom { y_pos: u8, x_vel_fast: u8, x_vel_fract: u8, zeroed_y_vel: ZeroedVelocity, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`mini_ufo_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniUfoRoutine02Result {
    pub animation_delay: u8,
    pub sprite: Option<u8>,
    pub y: AxisUpdate,
    /// `Some` when the Y update alone pushed it off the bottom edge -
    /// same "still reaches `ReachedBottom`, but its own `advance_enemy_
    /// routine` call is guard-rejected" shape as [`MiniUfoRoutine01Result::removed`].
    pub removed: Option<RemovedEnemy>,
    pub outcome: MiniUfoRoutine02Outcome,
}

/// Native port of `mini_ufo_routine_02` (`$a922`).
#[allow(clippy::too_many_arguments)]
pub fn mini_ufo_routine_02(animation_delay: u8, sprite: u8, x_pos: u8, y_pos: u8, y_vel_accum: u8, y_vel_fract: u8, y_vel_fast: u8, frame_scroll: u8, current_routine: u8) -> MiniUfoRoutine02Result {
    let (animation_delay, sprite) = dec_mini_ufo_anim_delay_set_sprite(animation_delay, sprite);

    let y = update_enemy_y_pos_with_scroll(y_pos, y_vel_accum, y_vel_fract, y_vel_fast, frame_scroll);
    let removed = if y.pos >= 0xE8 { Some(remove_enemy()) } else { None };
    let effective_routine = if removed.is_some() { 0 } else { current_routine };

    let outcome = if y.pos < 0xA8 {
        MiniUfoRoutine02Outcome::StillDescending
    } else {
        let x_vel_fast = if (x_pos as i8) >= 0 { 0x01 } else { 0xFE };
        MiniUfoRoutine02Outcome::ReachedBottom {
            y_pos: 0xA9,
            x_vel_fast,
            x_vel_fract: 0x80,
            zeroed_y_vel: set_enemy_y_velocity_to_0(),
            routine_update: advance_enemy_routine(effective_routine),
        }
    };

    MiniUfoRoutine02Result { animation_delay, sprite, y, removed, outcome }
}

/// The full result of one [`mini_ufo_routine_03`] call. Real ASM's own
/// tail is a bare `jmp update_enemy_pos` after the sprite-cycle helper,
/// so its own real exits are that routine's own (success/off-screen
/// removal on either axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiniUfoRoutine03Result {
    pub animation_delay: u8,
    pub sprite: Option<u8>,
    pub position: UpdatedEnemyPos,
}

/// Native port of `mini_ufo_routine_03` (`$a94c`).
#[allow(clippy::too_many_arguments)]
pub fn mini_ufo_routine_03(
    animation_delay: u8,
    sprite: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> MiniUfoRoutine03Result {
    let (animation_delay, sprite) = dec_mini_ufo_anim_delay_set_sprite(animation_delay, sprite);
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);
    MiniUfoRoutine03Result { animation_delay, sprite, position }
}

/// One [`boss_ufo_bomb_routine_00`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossUfoBombRoutine00Outcome {
    /// Below the explosion height (`< $b0`) - real ASM's own comment
    /// names this branch `set_mini_ufo_drop_bomb_pos`, but it's just a
    /// bare `jmp update_enemy_pos`.
    Falling(UpdatedEnemyPos),
    /// Reached (or passed) the explosion height - advances without
    /// updating position this call (real ASM never reaches `update_
    /// enemy_pos` on this path).
    Exploding(EnemyRoutineUpdate),
}

/// Native port of `boss_ufo_bomb_routine_00` (`$a974`).
#[allow(clippy::too_many_arguments)]
pub fn boss_ufo_bomb_routine_00(
    y_pos: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    current_routine: u8,
) -> (u8, u8, BossUfoBombRoutine00Outcome) {
    let (y_vel_fract, y_vel_fast) = add_a_to_enemy_y_fract_vel(0x28, y_vel_fract, y_vel_fast);

    let outcome = if y_pos < 0xB0 {
        BossUfoBombRoutine00Outcome::Falling(update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast))
    } else {
        BossUfoBombRoutine00Outcome::Exploding(advance_enemy_routine(current_routine))
    };

    (y_vel_fract, y_vel_fast, outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_animates_sprite_while_waiting() {
        let r = mini_ufo_routine_00(0x05, 0x60, 3);
        assert_eq!(r.animation_delay, 0x04);
        assert_eq!(r.outcome, MiniUfoRoutine00Outcome::Animating { sprite: Some(0x61) });
    }

    #[test]
    fn routine_00_advances_when_delay_reaches_zero() {
        let r = mini_ufo_routine_00(0x01, 0x60, 3);
        assert_eq!(r.outcome, MiniUfoRoutine00Outcome::Advanced(advance_enemy_routine(3)));
    }

    #[test]
    fn sprite_wraps_past_the_last_mini_ufo_frame() {
        let r = mini_ufo_routine_00(0x05, 0x7E, 3);
        assert_eq!(r.outcome, MiniUfoRoutine00Outcome::Animating { sprite: Some(0x7C) }); // 0x7f -> 0x7c
    }

    #[test]
    fn routine_01_waits_mid_screen() {
        let r = mini_ufo_routine_01(0x01, 0x60, 0x80, 0, 0, 0, 0x00, 3);
        assert_eq!(r.outcome, MiniUfoRoutine01Outcome::Waiting);
    }

    #[test]
    fn routine_01_begins_descent_past_the_left_point() {
        let r = mini_ufo_routine_01(0x01, 0x60, 0x1F, 0, 0, 0, 0x00, 3);
        assert_eq!(r.outcome, MiniUfoRoutine01Outcome::BeginDescent { y_vel_fract: 0x80, y_vel_fast: 0x01, routine_update: advance_enemy_routine(3) });
    }

    #[test]
    fn routine_01_begins_descent_past_the_right_point() {
        let r = mini_ufo_routine_01(0x01, 0x60, 0xE0, 0, 0, 0, 0x00, 3);
        assert_eq!(r.outcome, MiniUfoRoutine01Outcome::BeginDescent { y_vel_fract: 0x80, y_vel_fast: 0x01, routine_update: advance_enemy_routine(3) });
    }

    #[test]
    fn routine_01_removed_off_screen_left_still_reaches_begin_descent_guard_rejected() {
        // x lands at 0x05 (< 0x08), which the X-removal primitive treats
        // as removed - but real ASM never checks that before running its
        // own $20/$e0 comparison, so BeginDescent is still reported,
        // just with its own advance_enemy_routine guard-rejected against
        // the now-zeroed routine.
        let r = mini_ufo_routine_01(0x01, 0x60, 0x0A, 0, 0, 0, 0x05, 3);
        assert_eq!(r.removed, Some(remove_enemy()));
        assert_eq!(r.outcome, MiniUfoRoutine01Outcome::BeginDescent { y_vel_fract: 0x80, y_vel_fast: 0x01, routine_update: advance_enemy_routine(0) });
    }

    #[test]
    fn routine_02_still_descending_below_the_limit() {
        let r = mini_ufo_routine_02(0x01, 0x60, 0x50, 0x90, 0, 0, 0x01, 0x00, 3);
        assert_eq!(r.outcome, MiniUfoRoutine02Outcome::StillDescending);
    }

    #[test]
    fn routine_02_reaches_bottom_and_picks_right_direction_for_left_side_ufo() {
        let r = mini_ufo_routine_02(0x01, 0x60, 0x50, 0xA8, 0, 0, 0x01, 0x00, 3);
        match r.outcome {
            MiniUfoRoutine02Outcome::ReachedBottom { y_pos, x_vel_fast, x_vel_fract, routine_update, .. } => {
                assert_eq!(y_pos, 0xA9);
                assert_eq!(x_vel_fast, 0x01);
                assert_eq!(x_vel_fract, 0x80);
                assert_eq!(routine_update, advance_enemy_routine(3));
            }
            other => panic!("expected ReachedBottom, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_reaches_bottom_and_picks_left_direction_for_right_side_ufo() {
        let r = mini_ufo_routine_02(0x01, 0x60, 0xD0, 0xA8, 0, 0, 0x01, 0x00, 3);
        match r.outcome {
            MiniUfoRoutine02Outcome::ReachedBottom { x_vel_fast, .. } => assert_eq!(x_vel_fast, 0xFE),
            other => panic!("expected ReachedBottom, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_removed_off_screen_bottom_still_reaches_reached_bottom_guard_rejected() {
        // y lands at 0xf0 (>= 0xe8, removed) - but that's also >= 0xa8,
        // so ReachedBottom still fires, with advance_enemy_routine
        // guard-rejected against the now-zeroed routine.
        let r = mini_ufo_routine_02(0x01, 0x60, 0x50, 0xE0, 0, 0, 0x10, 0x00, 3);
        assert_eq!(r.removed, Some(remove_enemy()));
        match r.outcome {
            MiniUfoRoutine02Outcome::ReachedBottom { routine_update, .. } => assert_eq!(routine_update, advance_enemy_routine(0)),
            other => panic!("expected ReachedBottom, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_composes_sprite_cycle_and_position_update() {
        let r = mini_ufo_routine_03(0x01, 0x60, 0, 0x02, 0x50, 0, 0, 0x01, 0x50, 0, 0, 0);
        assert_eq!(r.animation_delay, 0x00);
        assert_eq!(r.position, update_enemy_pos(0, 0x02, 0x50, 0, 0, 0x01, 0x50, 0, 0, 0));
    }

    #[test]
    fn bomb_falls_below_explosion_height() {
        let (y_vel_fract, y_vel_fast, outcome) = boss_ufo_bomb_routine_00(0x50, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0, 3);
        assert_eq!((y_vel_fract, y_vel_fast), add_a_to_enemy_y_fract_vel(0x28, 0x00, 0x00));
        assert!(matches!(outcome, BossUfoBombRoutine00Outcome::Falling(_)));
    }

    #[test]
    fn bomb_explodes_at_or_past_the_explosion_height() {
        let (_, _, outcome) = boss_ufo_bomb_routine_00(0xB0, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0, 3);
        assert_eq!(outcome, BossUfoBombRoutine00Outcome::Exploding(advance_enemy_routine(3)));
    }
}
