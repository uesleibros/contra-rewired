//! Native port of the plain soldier enemy's first two AI states
//! (`src/bank0.asm`): `soldier_routine_00` (CPU `$861e`-`$8633`), run once
//! right after `initialize_enemy` spawns it - nudges its position slightly
//! down so it visually stands on the ground, and sets a per-attribute
//! initial animation delay before advancing to `soldier_routine_01`
//! (`$8665`), which waits out that delay, checks for ground beneath the
//! soldier, and either removes it (no valid footing - e.g. a destroyed
//! bridge) or plants it and sets it walking. This crate's first
//! **composed enemy AI states** - every step is a call into an already
//! independently-verified building block
//! ([`crate::add_scroll_to_enemy_pos::add_scroll_to_enemy_pos`],
//! [`crate::update_enemy_pos::remove_enemy`],
//! [`crate::enemy_position_utils::add_4_to_enemy_y_pos`],
//! [`crate::enemy_routine_transition::set_enemy_delay_adv_routine`],
//! [`crate::collision::add_y_to_y_pos_get_bg_collision`],
//! [`crate::enemy_collision_flags::enable_enemy_collision`]) -
//! demonstrating the same real composition the ROM itself uses, no new
//! arithmetic beyond small bit tests, table lookups, and one real quirk
//! reproduced exactly: see [`SoldierRoutine01Outcome::DelayNotYetZero`]
//! for `soldier_routine_01`'s one path that decrements
//! `ENEMY_ANIMATION_DELAY` twice in a single call.

use crate::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::collision::{add_y_to_y_pos_get_bg_collision, CollisionCode, BG_COLLISION_DATA_LEN};
use crate::enemy_collision_flags::enable_enemy_collision;
use crate::enemy_position_utils::add_4_to_enemy_y_pos;
use crate::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::update_enemy_pos::{remove_enemy, set_enemy_y_velocity_to_0, RemovedEnemy, ZeroedVelocity};

/// `soldier_initial_anim_delay_tbl` (`$8634`, 4 bytes) - indexed by the
/// soldier type's `ENEMY_ATTRIBUTES` high nibble (bits 4-5).
const SOLDIER_INITIAL_ANIM_DELAY_TBL: [u8; 4] = [0x01, 0x10, 0x20, 0x30];

/// The full real result of one `soldier_routine_00` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine00Result {
    pub scroll: ScrolledEnemyPos,
    pub y_pos_after_offset: u8,
    /// `Some` if `add_scroll_to_enemy_pos` decided the soldier scrolled
    /// off-screen - real ASM runs the *rest* of the routine regardless
    /// (position offset, animation delay, and the guarded routine
    /// advance all still execute; the guard on the last step just
    /// naturally rejects since `remove_enemy` already zeroed
    /// `ENEMY_ROUTINE`).
    pub removed: Option<RemovedEnemy>,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `soldier_routine_00` (`$861e`).
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_00(
    level_scrolling_type: u8,
    frame_scroll: u8,
    vertical_scroll: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_attributes: u8,
    current_routine: u8,
) -> SoldierRoutine00Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
    let removed = if scroll.should_remove { Some(remove_enemy()) } else { None };
    let routine_for_advance = if scroll.should_remove { 0 } else { current_routine };

    let y_pos_after_offset = add_4_to_enemy_y_pos(vertical_scroll, scroll.y_pos);

    let anim_index = ((enemy_attributes >> 4) & 0x03) as usize;
    let delay = SOLDIER_INITIAL_ANIM_DELAY_TBL[anim_index];
    let delayed_routine = set_enemy_delay_adv_routine(delay, routine_for_advance);

    SoldierRoutine00Result { scroll, y_pos_after_offset, removed, delayed_routine }
}

/// `soldier_x_vel_tbl` (`$865d`, 8 bytes) - pairs of (X fractional
/// velocity, X fast velocity), indexed by running direction (`ENEMY_VAR_2`:
/// `0` = left, `1` = right) and `LEVEL_SCROLLING_TYPE` (horizontal levels
/// use the first pair, vertical levels the second). Real ASM computes the
/// byte offset as `(ENEMY_VAR_2 << 1) + (LEVEL_SCROLLING_TYPE != 0 ? 4 :
/// 0)` with no explicit mask on `ENEMY_VAR_2` before the shift - every real
/// writer of `ENEMY_VAR_2` masks it to `0`/`1` first (`and #$01`), so this
/// port masks defensively too rather than replicating an out-of-range
/// table read no real caller can ever trigger.
const SOLDIER_X_VEL_TBL: [(u8, u8); 4] = [
    (0x00, 0xFF), // horizontal, running left  (-1.00)
    (0x40, 0x01), // horizontal, running right ( 1.25)
    (0x00, 0xFF), // vertical,   running left  (-1.00)
    (0x00, 0x01), // vertical,   running right ( 1.00)
];

/// Native port of `soldier_set_x_velocity` (`$863e`).
pub fn soldier_set_x_velocity(enemy_var_2: u8, level_scrolling_type: u8) -> (u8, u8) {
    let base = if level_scrolling_type == 0 { 0 } else { 2 };
    let dir = (enemy_var_2 & 0x01) as usize;
    SOLDIER_X_VEL_TBL[base + dir]
}

/// The combined result of `soldier_stop_y_set_x_velocity` (`$8638`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierStoppedYVelocity {
    pub x_velocity: (u8, u8),
    pub y_velocity: ZeroedVelocity,
}

/// Native port of `soldier_stop_y_set_x_velocity` (`$8638`) - sets X
/// velocity from [`soldier_set_x_velocity`], then zeroes Y velocity via
/// [`crate::update_enemy_pos::set_enemy_y_velocity_to_0`].
pub fn soldier_stop_y_set_x_velocity(enemy_var_2: u8, level_scrolling_type: u8) -> SoldierStoppedYVelocity {
    SoldierStoppedYVelocity {
        x_velocity: soldier_set_x_velocity(enemy_var_2, level_scrolling_type),
        y_velocity: set_enemy_y_velocity_to_0(),
    }
}

/// The tail of [`soldier_routine_01`] reached once `ENEMY_ANIMATION_DELAY`
/// hits zero (`@enable_set_vel` onward): checks for ground under the
/// soldier, and either removes it (no valid footing - e.g. a destroyed
/// bridge) or plants it and sets it moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine01Advance {
    /// `ENEMY_STATE_WIDTH` after [`enable_enemy_collision`] re-enables
    /// both player and bullet collision.
    pub state_width: u8,
    pub enemy_var_2: u8,
    /// Final `ENEMY_X_POS,x` - snapped to `$0a` when running right, left
    /// untouched otherwise.
    pub enemy_x_pos: u8,
    pub velocity: SoldierStoppedYVelocity,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// The real, branchy result of one [`soldier_routine_01`] call - see the
/// module-level doc comment for how the real ASM reaches each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoldierRoutine01Outcome {
    /// Horizontal level, running right, even frame: exits before any
    /// decrement (`bcc soldier_routine_exit`).
    NoDecrement,
    /// `ENEMY_ANIMATION_DELAY` decremented but didn't reach zero.
    /// `decremented_twice` is `true` only on the horizontal "running
    /// left" path's fallthrough (`@continue` into
    /// `@dec_delay_enable_set_vel`) - every other path decrements once.
    DelayNotYetZero { animation_delay: u8, decremented_twice: bool },
    /// The `@enable_set_vel` ground check found no floor/water/solid
    /// under the soldier: `remove_enemy` ran, nothing else did.
    Removed(RemovedEnemy),
    /// Delay reached zero and the ground check passed: full tail ran.
    Advanced(SoldierRoutine01Advance),
}

/// The full result of one [`soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine01Result {
    /// `Some` only on vertical levels, where `soldier_routine_01` itself
    /// (not the generic per-frame velocity integrator) scrolls the
    /// enemy's position before anything else runs.
    pub scrolled: Option<ScrolledEnemyPos>,
    pub outcome: SoldierRoutine01Outcome,
}

/// Native port of `soldier_routine_01` (`$8665`) - the soldier's "standing,
/// about to start moving" state: waits out `ENEMY_ANIMATION_DELAY` (with a
/// real, faithfully-reproduced quirk - see [`SoldierRoutine01Outcome::DelayNotYetZero`]),
/// then checks for ground beneath it and either removes itself or starts
/// walking.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_01(
    level_scrolling_type: u8,
    frame_scroll: u8,
    frame_counter: u8,
    enemy_attributes: u8,
    enemy_animation_delay: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_state_width: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    current_routine: u8,
) -> SoldierRoutine01Result {
    let (scrolled, pos_x, pos_y) = if level_scrolling_type != 0 {
        let s = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
        (Some(s), s.x_pos, s.y_pos)
    } else {
        (None, enemy_x_pos, enemy_y_pos)
    };

    let advance = |delay_after: u8, decremented_twice: bool| -> SoldierRoutine01Outcome {
        if delay_after != 0 {
            return SoldierRoutine01Outcome::DelayNotYetZero { animation_delay: delay_after, decremented_twice };
        }
        let collision = add_y_to_y_pos_get_bg_collision(
            0x10, pos_x, pos_y, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data,
        );
        if collision == CollisionCode::Empty {
            return SoldierRoutine01Outcome::Removed(remove_enemy());
        }
        let state_width = enable_enemy_collision(enemy_state_width);
        let enemy_var_2 = enemy_attributes & 0x01;
        let enemy_x_pos = if enemy_var_2 != 0 { 0x0A } else { pos_x };
        let velocity = soldier_stop_y_set_x_velocity(enemy_var_2, level_scrolling_type);
        let delayed_routine = set_enemy_delay_adv_routine(0x10, current_routine);
        SoldierRoutine01Outcome::Advanced(SoldierRoutine01Advance {
            state_width,
            enemy_var_2,
            enemy_x_pos,
            velocity,
            delayed_routine,
        })
    };

    // Real single-decrement path (`@dec_delay_enable_set_vel`, entered
    // directly): vertical levels, horizontal levels with no scroll this
    // frame, and horizontal "running right" on an odd frame.
    let single_decrement = |delay: u8| advance(delay.wrapping_sub(1), false);

    let outcome = if level_scrolling_type != 0 {
        single_decrement(enemy_animation_delay)
    } else if frame_scroll == 0 {
        single_decrement(enemy_animation_delay)
    } else if enemy_attributes & 0x01 == 0 {
        // Running left: `@continue` decrements once and only falls
        // through to a *second* decrement if the first didn't reach
        // zero - the one real path that can decrement twice in a call.
        let d1 = enemy_animation_delay.wrapping_sub(1);
        if d1 == 0 {
            advance(0, false)
        } else {
            advance(d1.wrapping_sub(1), true)
        }
    } else if frame_counter & 0x01 == 0 {
        // Running right, even frame: exit with no decrement at all.
        SoldierRoutine01Outcome::NoDecrement
    } else {
        // Running right, odd frame.
        single_decrement(enemy_animation_delay)
    };

    SoldierRoutine01Result { scrolled, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advances_normally_and_matches_each_composed_step() {
        let r = soldier_routine_00(0, 0x02, 0x00, 0x50, 0x63, 0x10, 3);
        let expected_scroll = add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x63);
        assert_eq!(r.scroll, expected_scroll);
        assert_eq!(r.y_pos_after_offset, add_4_to_enemy_y_pos(0x00, expected_scroll.y_pos));
        assert_eq!(r.removed, None);
        // ENEMY_ATTRIBUTES=0x10 -> high nibble 1 -> table[1]=0x10
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x10, 3));
    }

    #[test]
    fn anim_delay_index_uses_bits_4_and_5_of_attributes() {
        for (attr, expected_delay) in [(0x00u8, 0x01u8), (0x10, 0x10), (0x20, 0x20), (0x30, 0x30)] {
            let r = soldier_routine_00(0, 0x00, 0x00, 0x50, 0x60, attr, 3);
            assert_eq!(r.delayed_routine.animation_delay, expected_delay);
        }
    }

    #[test]
    fn scrolling_off_screen_still_runs_the_rest_but_advance_is_guard_rejected() {
        // x_pos=0x0a, frame_scroll=0x05 -> 0x0a-0x05=0x05, < $08,
        // triggering removal on a horizontal level.
        let r = soldier_routine_00(0, 0x05, 0x00, 0x0A, 0x60, 0x00, 3);
        assert!(r.scroll.should_remove);
        assert_eq!(r.removed, Some(remove_enemy()));
        // the guard sees routine=0 (just removed) and rejects the advance
        assert_eq!(r.delayed_routine.routine_update.routine, 0);
        assert_eq!(r.delayed_routine.routine_update.sprites, Some(0));
        // the animation delay store still happens unconditionally
        assert_eq!(r.delayed_routine.animation_delay, 0x01);
    }

    const NO_COLLISION_DATA: [u8; BG_COLLISION_DATA_LEN] = [0u8; BG_COLLISION_DATA_LEN];
    const SOLID_COLLISION_DATA: [u8; BG_COLLISION_DATA_LEN] = [0xFFu8; BG_COLLISION_DATA_LEN];

    #[test]
    fn soldier_set_x_velocity_matches_the_real_table() {
        assert_eq!(soldier_set_x_velocity(0, 0), (0x00, 0xFF)); // horizontal, left
        assert_eq!(soldier_set_x_velocity(1, 0), (0x40, 0x01)); // horizontal, right
        assert_eq!(soldier_set_x_velocity(0, 1), (0x00, 0xFF)); // vertical, left
        assert_eq!(soldier_set_x_velocity(1, 1), (0x00, 0x01)); // vertical, right
    }

    #[test]
    fn soldier_stop_y_set_x_velocity_zeroes_y_and_sets_x_from_the_table() {
        let r = soldier_stop_y_set_x_velocity(1, 0);
        assert_eq!(r.x_velocity, (0x40, 0x01));
        assert_eq!(r.y_velocity, ZeroedVelocity { vel_fract: 0, vel_fast: 0 });
    }

    #[test]
    fn routine_01_horizontal_no_scroll_takes_the_single_decrement_path_regardless_of_direction() {
        let r = soldier_routine_01(0, 0x00, 0xFF, 0x01, 0x05, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        assert_eq!(r.scrolled, None);
        assert_eq!(r.outcome, SoldierRoutine01Outcome::DelayNotYetZero { animation_delay: 0x04, decremented_twice: false });
    }

    #[test]
    fn routine_01_running_right_even_frame_exits_with_no_decrement() {
        let r = soldier_routine_01(0, 0x02, 0x10, 0x01, 0x05, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        assert_eq!(r.outcome, SoldierRoutine01Outcome::NoDecrement);
    }

    #[test]
    fn routine_01_running_right_odd_frame_takes_the_single_decrement_path() {
        let r = soldier_routine_01(0, 0x02, 0x11, 0x01, 0x05, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        assert_eq!(r.outcome, SoldierRoutine01Outcome::DelayNotYetZero { animation_delay: 0x04, decremented_twice: false });
    }

    #[test]
    fn routine_01_running_left_decrements_once_if_that_alone_reaches_zero() {
        let r = soldier_routine_01(0, 0x02, 0xFF, 0x00, 0x01, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        // delay=1 -> first decrement hits zero -> advances without a second
        // decrement (would have removed, since NO_COLLISION_DATA has no
        // floor under it).
        assert_eq!(r.outcome, SoldierRoutine01Outcome::Removed(remove_enemy()));
    }

    #[test]
    fn routine_01_running_left_decrements_twice_when_the_first_decrement_does_not_reach_zero() {
        let r = soldier_routine_01(0, 0x02, 0xFF, 0x00, 0x03, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        // delay=3 -> d1=2 (not zero) -> d2=1 (not zero): decremented twice,
        // still not zero.
        assert_eq!(r.outcome, SoldierRoutine01Outcome::DelayNotYetZero { animation_delay: 0x01, decremented_twice: true });
    }

    #[test]
    fn routine_01_running_left_second_decrement_can_also_reach_zero() {
        let r = soldier_routine_01(0, 0x02, 0xFF, 0x00, 0x02, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        // delay=2 -> d1=1 (not zero) -> d2=0: advances via the
        // twice-decremented path.
        assert_eq!(r.outcome, SoldierRoutine01Outcome::Removed(remove_enemy()));
    }

    #[test]
    fn routine_01_vertical_level_always_scrolls_and_single_decrements() {
        let r = soldier_routine_01(1, 0x03, 0x00, 0x01, 0x05, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        assert_eq!(r.scrolled, Some(add_scroll_to_enemy_pos(1, 0x03, 0x50, 0x60)));
        assert_eq!(r.outcome, SoldierRoutine01Outcome::DelayNotYetZero { animation_delay: 0x04, decremented_twice: false });
    }

    #[test]
    fn routine_01_no_floor_removes_the_enemy_instead_of_advancing() {
        let r = soldier_routine_01(0, 0x00, 0x00, 0x00, 0x01, 0x50, 0x60, 0x00, 0, 0, 0, &NO_COLLISION_DATA, 3);
        assert_eq!(r.outcome, SoldierRoutine01Outcome::Removed(remove_enemy()));
    }

    #[test]
    fn routine_01_floor_present_advances_running_left_leaves_x_pos_alone() {
        let r = soldier_routine_01(0, 0x00, 0x00, 0x00, 0x01, 0x50, 0x60, 0x00, 0, 0, 0, &SOLID_COLLISION_DATA, 3);
        match r.outcome {
            SoldierRoutine01Outcome::Advanced(a) => {
                assert_eq!(a.enemy_var_2, 0);
                assert_eq!(a.enemy_x_pos, 0x50); // untouched, running left
                assert_eq!(a.state_width, enable_enemy_collision(0x00));
                assert_eq!(a.velocity.x_velocity, soldier_set_x_velocity(0, 0));
                assert_eq!(a.velocity.y_velocity, ZeroedVelocity { vel_fract: 0, vel_fast: 0 });
                assert_eq!(a.delayed_routine, set_enemy_delay_adv_routine(0x10, 3));
            }
            other => panic!("expected Advanced, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_floor_present_advances_running_right_snaps_x_pos_to_0x0a() {
        let r = soldier_routine_01(0, 0x00, 0x00, 0x01, 0x01, 0x50, 0x60, 0x00, 0, 0, 0, &SOLID_COLLISION_DATA, 3);
        match r.outcome {
            SoldierRoutine01Outcome::Advanced(a) => {
                assert_eq!(a.enemy_var_2, 1);
                assert_eq!(a.enemy_x_pos, 0x0A);
                assert_eq!(a.velocity.x_velocity, soldier_set_x_velocity(1, 0));
            }
            other => panic!("expected Advanced, got {other:?}"),
        }
    }
}
