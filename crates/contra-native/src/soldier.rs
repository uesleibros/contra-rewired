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
use crate::collision::{add_y_to_y_pos_get_bg_collision, check_enemy_collision_solid_bg, get_bg_collision_far, CollisionCode, BG_COLLISION_DATA_LEN};
use crate::enemy_collision_flags::enable_enemy_collision;
use crate::enemy_position_utils::{add_10_to_enemy_y_fract_vel, add_4_to_enemy_y_pos};
use crate::enemy_routine_transition::{set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::update_enemy_pos::{remove_enemy, set_enemy_y_velocity_to_0, update_enemy_pos, RemovedEnemy, UpdatedEnemyPos, ZeroedVelocity};

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

/// `soldier_sprite_codes` (`$8735`, 12 bytes) - raw sprite-tile codes for
/// each `ENEMY_FRAME` value the plain soldier's animation states use.
const SOLDIER_SPRITE_CODES: [u8; 12] = [0x3B, 0x3C, 0x3D, 0x3F, 0x3C, 0x3E, 0x40, 0x26, 0x73, 0x18, 0x28, 0x27];

/// The result of one [`set_soldier_sprite`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierSpriteResult {
    pub sprite: u8,
    pub sprite_attr: u8,
    /// `ENEMY_VAR_1` (gun recoil timer) after this call - decremented by
    /// one if it was nonzero, unchanged (already `0`) otherwise.
    pub var_1: u8,
}

/// Native port of `set_soldier_sprite` (`$891a`) - looks up the sprite
/// code for the current animation frame, sets the horizontal-flip bit
/// from the running direction, and (this real routine's one side effect
/// beyond a pure lookup) counts down a gun-recoil timer that, while
/// active, ORs an extra attribute bit into the sprite.
pub fn set_soldier_sprite(enemy_frame: u8, enemy_var_2: u8, enemy_var_1: u8) -> SoldierSpriteResult {
    let sprite = SOLDIER_SPRITE_CODES[enemy_frame as usize];
    let facing_attr = if enemy_var_2 == 0 { 0x40 } else { 0x00 };
    let (var_1, sprite_attr) =
        if enemy_var_1 != 0 { (enemy_var_1 - 1, facing_attr | 0x08) } else { (enemy_var_1, facing_attr) };
    SoldierSpriteResult { sprite, sprite_attr, var_1 }
}

/// The result of one [`soldier_change_direction`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierDirectionChange {
    /// `ENEMY_VAR_4` (turn counter) after this call, always `+1`.
    pub var_4: u8,
    /// `ENEMY_VAR_2` (running direction) after this call - the opposite
    /// of whatever it was.
    pub var_2: u8,
    pub x_velocity: (u8, u8),
}

/// Native port of `soldier_change_direction` (`$87cb`) - flips the
/// running direction, counts the turn, and re-derives X velocity for the
/// new direction via [`soldier_set_x_velocity`].
pub fn soldier_change_direction(enemy_var_2: u8, enemy_var_4: u8, level_scrolling_type: u8) -> SoldierDirectionChange {
    let var_4 = enemy_var_4.wrapping_add(1);
    let var_2 = enemy_var_2 ^ 0x01;
    let x_velocity = soldier_set_x_velocity(var_2, level_scrolling_type);
    SoldierDirectionChange { var_4, var_2, x_velocity }
}

/// The full result of one [`soldier_apply_vel_check_solid_collision`]
/// call that got past the solid-ahead check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierApplyVelResult {
    /// `Some` only if the soldier was about to walk into a solid object
    /// up to 8 pixels ahead and turned around.
    pub direction_change: Option<SoldierDirectionChange>,
    pub sprite: SoldierSpriteResult,
    pub position: UpdatedEnemyPos,
}

/// The real, branchy result of one [`soldier_apply_vel_check_solid_collision`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoldierApplyVelOutcome {
    /// Solid collision directly at the soldier's own (unmoved) position:
    /// switches to `soldier_routine_09` (`ENEMY_ROUTINE = $07`
    /// pre-guard), nothing else in this routine runs.
    SolidAtOwnPosition(EnemyRoutineUpdate),
    /// Not solid at the soldier's own position: ran the full ledge-turn-
    /// around-check, sprite, and position-update tail.
    Continued(SoldierApplyVelResult),
}

/// Native port of `soldier_apply_vel_check_solid_collision` (`$8794`) -
/// the shared tail nearly every `soldier_routine_02`/`03`/`04`/`05` path
/// eventually reaches: bails out to `soldier_routine_09` if the soldier
/// is somehow embedded in solid ground, otherwise (up to twice a second,
/// gated by `ENEMY_VAR_4 < 2`) probes 8 pixels ahead in the direction
/// it's facing and turns around if that would walk it into a solid
/// object, then updates its sprite and applies velocity/scroll to its
/// position.
///
/// Live-verified indirectly via [`soldier_routine_02_jumping`]: 96 of 97
/// real calls matched exactly. The one open mismatch (see docs/
/// NATIVE_PORT.md) looks, from the real hardware's final RAM state, like
/// it involves *this* function's own `SolidAtOwnPosition` early exit
/// firing when this port's [`check_enemy_collision_solid_bg`] computed
/// `Floor` instead - root cause not yet identified despite re-deriving
/// the full formula line-by-line against the real ASM and finding no
/// discrepancy.
#[allow(clippy::too_many_arguments)]
pub fn soldier_apply_vel_check_solid_collision(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_4: u8,
    enemy_var_2: u8,
    enemy_frame: u8,
    enemy_var_1: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    current_routine: u8,
) -> SoldierApplyVelOutcome {
    let at_own_pos =
        check_enemy_collision_solid_bg(enemy_x_pos, enemy_y_pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
    if at_own_pos == CollisionCode::Solid {
        return SoldierApplyVelOutcome::SolidAtOwnPosition(set_enemy_routine_to_a(current_routine, 0x07));
    }

    let mut direction_change = None;
    let (final_x_fract, final_x_fast, final_var_2) = if enemy_var_4 >= 0x02 {
        (x_vel_fract, x_vel_fast, enemy_var_2)
    } else {
        let probe_x = enemy_x_pos.wrapping_add(if enemy_var_2 == 0 { 0xF8 } else { 0x08 });
        if !(0x10..0xF0).contains(&probe_x) {
            (x_vel_fract, x_vel_fast, enemy_var_2)
        } else {
            let ahead = get_bg_collision_far(probe_x, enemy_y_pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
            if ahead == CollisionCode::Solid {
                let change = soldier_change_direction(enemy_var_2, enemy_var_4, level_scrolling_type);
                let result = (change.x_velocity.0, change.x_velocity.1, change.var_2);
                direction_change = Some(change);
                result
            } else {
                (x_vel_fract, x_vel_fast, enemy_var_2)
            }
        }
    };

    let sprite = set_soldier_sprite(enemy_frame, final_var_2, enemy_var_1);
    let position = update_enemy_pos(
        level_scrolling_type,
        frame_scroll,
        enemy_x_pos,
        x_vel_accum,
        final_x_fract,
        final_x_fast,
        enemy_y_pos,
        y_vel_accum,
        y_vel_fract,
        y_vel_fast,
    );
    SoldierApplyVelOutcome::Continued(SoldierApplyVelResult { direction_change, sprite, position })
}

/// The two shapes [`soldier_routine_02_jumping`] can end in - see that
/// function's doc comment for why the real ASM only has these two
/// distinct outcomes despite reading as three branches (`bmi
/// @no_landing`, "checked and got empty/floor", and "checked and got
/// water" all converge on the exact same `@no_landing` code).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoldierRoutine02Landing {
    /// Landed on solid ground this call: jumping flag and frame cleared,
    /// position nudged down 4px, velocity zeroed and re-set from the
    /// walking table.
    Landed { y_pos: u8, velocity: SoldierStoppedYVelocity },
    /// Did not land this call (still rising, checked-and-not-solid, or
    /// checked-and-water) - Y fractional velocity bumped `+$10`.
    /// `water_routine_switch` is `Some` only for the water case, and (a
    /// real, faithfully-reproduced detail) is applied *before* the
    /// shared tail runs, so if that tail also decides to switch routines
    /// it does so guarded against this already-updated value, not the
    /// original.
    NotLanded { water_routine_switch: Option<EnemyRoutineUpdate>, y_velocity: (u8, u8) },
}

/// The full result of one [`soldier_routine_02_jumping`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine02JumpingResult {
    pub enemy_var_3: u8,
    pub enemy_frame: u8,
    pub landing: SoldierRoutine02Landing,
    pub tail: SoldierApplyVelOutcome,
}

/// Native port of `soldier_routine_02`'s **jumping sub-path only**
/// (`$86af`-`$8709`, the `ENEMY_VAR_3 != 0` branch through `@no_landing`/
/// `@floor_solid_landing`) - the walking/firing-decision/ledge-detection
/// sub-path (`@continue` onward, `$870a`-`$8793`) is **not yet ported**:
/// it depends on a real, deliberate 6502 quirk (`get_soldier_num_bullets`'s
/// `adc $08` with no preceding `clc`, meaning its result depends on the
/// carry flag inherited from well outside this routine) that needs to be
/// captured empirically from real hardware rather than guessed - left for
/// a follow-up pass rather than risking a silently wrong port of the
/// RNG-driven bullet count or jump-off-ledge velocity selection.
///
/// Real control flow: `ENEMY_FRAME` is set to `$0a` (jumping animation)
/// unconditionally the instant this branch is entered, *before* checking
/// anything else - only the solid-landing case later overwrites it back
/// to `$00`. If `ENEMY_Y_VELOCITY_FAST` is still negative (rising), the
/// landing check itself is skipped entirely (`bmi @no_landing` jumps
/// straight past it) - which lands on the *exact same* `@no_landing`
/// code a checked-but-not-solid result falls through to, so "still
/// rising" and "checked, not solid" are indistinguishable in their
/// effect and are merged into one [`SoldierRoutine02Landing::NotLanded`]
/// variant here.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_02_jumping(
    enemy_var_3: u8,
    enemy_y_velocity_fast: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_4: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    current_routine: u8,
) -> SoldierRoutine02JumpingResult {
    debug_assert!(enemy_var_3 != 0, "soldier_routine_02_jumping is only reached when ENEMY_VAR_3 != 0");

    let still_rising = (enemy_y_velocity_fast as i8) < 0;
    let checked_code = if still_rising {
        None
    } else {
        Some(add_y_to_y_pos_get_bg_collision(
            0x10,
            enemy_x_pos,
            enemy_y_pos,
            vertical_scroll,
            horizontal_scroll,
            ppuctrl_settings,
            bg_collision_data,
        ))
    };

    if checked_code == Some(CollisionCode::Solid) {
        let y_pos = add_4_to_enemy_y_pos(vertical_scroll, enemy_y_pos);
        let velocity = soldier_stop_y_set_x_velocity(enemy_var_2, level_scrolling_type);
        let tail = soldier_apply_vel_check_solid_collision(
            enemy_x_pos,
            y_pos,
            enemy_var_4,
            enemy_var_2,
            0x00,
            enemy_var_1,
            vertical_scroll,
            horizontal_scroll,
            ppuctrl_settings,
            bg_collision_data,
            level_scrolling_type,
            frame_scroll,
            x_vel_accum,
            velocity.x_velocity.0,
            velocity.x_velocity.1,
            y_vel_accum,
            velocity.y_velocity.vel_fract,
            velocity.y_velocity.vel_fast,
            current_routine,
        );
        return SoldierRoutine02JumpingResult {
            enemy_var_3: 0,
            enemy_frame: 0x00,
            landing: SoldierRoutine02Landing::Landed { y_pos, velocity },
            tail,
        };
    }

    let water_routine_switch =
        if checked_code == Some(CollisionCode::Water) { Some(set_enemy_routine_to_a(current_routine, 0x0A)) } else { None };
    let effective_routine = water_routine_switch.map(|u| u.routine).unwrap_or(current_routine);
    let y_velocity = add_10_to_enemy_y_fract_vel(y_vel_fract, y_vel_fast);
    let tail = soldier_apply_vel_check_solid_collision(
        enemy_x_pos,
        enemy_y_pos,
        enemy_var_4,
        enemy_var_2,
        0x0A,
        enemy_var_1,
        vertical_scroll,
        horizontal_scroll,
        ppuctrl_settings,
        bg_collision_data,
        level_scrolling_type,
        frame_scroll,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        y_vel_accum,
        y_velocity.0,
        y_velocity.1,
        effective_routine,
    );
    SoldierRoutine02JumpingResult {
        enemy_var_3,
        enemy_frame: 0x0A,
        landing: SoldierRoutine02Landing::NotLanded { water_routine_switch, y_velocity },
        tail,
    }
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

    fn table_code(code: CollisionCode) -> u8 {
        match code {
            CollisionCode::Empty => 0,
            CollisionCode::Floor => 1,
            CollisionCode::Water => 2,
            CollisionCode::Solid => 3,
        }
    }

    /// Test-only helper: sets the `BG_COLLISION_DATA` bits governing
    /// `(x, y)` to `code`, using [`crate::collision::bg_collision_scratch`]
    /// to find the right byte/column rather than hand-deriving the
    /// offset formula again.
    fn set_collision_at(data: &mut [u8; BG_COLLISION_DATA_LEN], x: u8, y: u8, code: CollisionCode) {
        let scratch = crate::collision::bg_collision_scratch(x, y, 0, 0, 0);
        let shift = match scratch.s12 & 0x03 {
            0 => 6,
            1 => 4,
            2 => 2,
            _ => 0,
        };
        let mask = 0b11u8 << shift;
        data[scratch.s13 as usize] = (data[scratch.s13 as usize] & !mask) | (table_code(code) << shift);
    }

    #[test]
    fn set_soldier_sprite_looks_up_the_table_and_flips_for_direction() {
        let r = set_soldier_sprite(0x02, 0, 0); // running left
        assert_eq!(r.sprite, SOLDIER_SPRITE_CODES[2]);
        assert_eq!(r.sprite_attr, 0x40);
        assert_eq!(r.var_1, 0);

        let r = set_soldier_sprite(0x02, 1, 0); // running right
        assert_eq!(r.sprite_attr, 0x00);
    }

    #[test]
    fn set_soldier_sprite_counts_down_gun_recoil_and_sets_the_recoil_bit() {
        let r = set_soldier_sprite(0x06, 0, 0x03);
        assert_eq!(r.var_1, 0x02);
        assert_eq!(r.sprite_attr, 0x40 | 0x08);

        let r = set_soldier_sprite(0x06, 0, 0x00);
        assert_eq!(r.var_1, 0x00);
        assert_eq!(r.sprite_attr, 0x40);
    }

    #[test]
    fn soldier_change_direction_flips_var_2_counts_var_4_and_rederives_x_velocity() {
        let r = soldier_change_direction(0, 5, 0); // was left, horizontal level
        assert_eq!(r.var_2, 1);
        assert_eq!(r.var_4, 6);
        assert_eq!(r.x_velocity, soldier_set_x_velocity(1, 0));
    }

    #[test]
    fn apply_vel_solid_at_own_position_switches_to_soldier_routine_09() {
        let r = soldier_apply_vel_check_solid_collision(
            0x50, 0x60, 0, 0, 0x00, 0, 0, 0, 0, &SOLID_COLLISION_DATA, 0, 0x00, 0, 0, 0, 0, 0, 0, 3,
        );
        assert_eq!(r, SoldierApplyVelOutcome::SolidAtOwnPosition(set_enemy_routine_to_a(3, 0x07)));
    }

    #[test]
    fn apply_vel_var_4_at_or_above_2_skips_the_ledge_probe_entirely() {
        let mut data = NO_COLLISION_DATA;
        // solid directly ahead (different 16px collision column than the
        // enemy's own position, 0x4C - see `set_collision_at`'s helper
        // doc), but var_4=2 should mean it's never even checked.
        set_collision_at(&mut data, 0x54, 0x60, CollisionCode::Solid);
        let r = soldier_apply_vel_check_solid_collision(0x4C, 0x60, 2, 1, 0x00, 0, 0, 0, 0, &data, 0, 0x00, 0, 0, 0, 0, 0, 0, 3);
        match r {
            SoldierApplyVelOutcome::Continued(a) => assert_eq!(a.direction_change, None),
            other => panic!("expected Continued, got {other:?}"),
        }
    }

    #[test]
    fn apply_vel_off_screen_probe_position_skips_the_ledge_check() {
        // running left from x=0x05: probe = 0x05 - 8 (wrapping) = 0xFD,
        // which is neither < 0x10 nor... wait it *is* >= 0xF0, so this
        // is the off-screen-right guard path (wrapped a small X down
        // past zero) - confirms the raw wrapping-u8 probe matches the
        // real ASM's unsigned `adc`/`cmp` sequence, not a signed check.
        let r = soldier_apply_vel_check_solid_collision(0x05, 0x60, 0, 0, 0x00, 0, 0, 0, 0, &NO_COLLISION_DATA, 0, 0x00, 0, 0, 0, 0, 0, 0, 3);
        match r {
            SoldierApplyVelOutcome::Continued(a) => assert_eq!(a.direction_change, None),
            other => panic!("expected Continued, got {other:?}"),
        }
    }

    #[test]
    fn apply_vel_turns_around_when_solid_ahead_and_updates_sprite_and_velocity() {
        let mut data = NO_COLLISION_DATA;
        // 0x4C and 0x4C+8=0x54 fall in different 16px collision columns
        // ((x>>4)&3 differs: 4 vs 5), so this genuinely tests "solid one
        // column ahead, not at the enemy's own position" rather than
        // accidentally marking the enemy's own column solid too.
        set_collision_at(&mut data, 0x54, 0x60, CollisionCode::Solid); // 8px ahead while running right (0x4C+8)
        let r = soldier_apply_vel_check_solid_collision(0x4C, 0x60, 0, 1, 0x00, 0, 0, 0, 0, &data, 0, 0x00, 0, 0, 0, 0, 0, 0, 3);
        match r {
            SoldierApplyVelOutcome::Continued(a) => {
                let change = a.direction_change.expect("expected a direction change");
                assert_eq!(change, soldier_change_direction(1, 0, 0));
                assert_eq!(a.sprite, set_soldier_sprite(0x00, change.var_2, 0));
            }
            other => panic!("expected Continued, got {other:?}"),
        }
    }

    #[test]
    fn apply_vel_no_direction_change_when_nothing_solid_ahead() {
        let r = soldier_apply_vel_check_solid_collision(0x50, 0x60, 0, 1, 0x00, 0, 0, 0, 0, &NO_COLLISION_DATA, 0, 0x00, 0, 0, 0, 0, 0, 0, 3);
        match r {
            SoldierApplyVelOutcome::Continued(a) => assert_eq!(a.direction_change, None),
            other => panic!("expected Continued, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_jumping_still_rising_skips_the_landing_check_and_bumps_y_fract_vel() {
        let r = soldier_routine_02_jumping(
            1, 0xFF, // ENEMY_VAR_3 nonzero, Y_VELOCITY_FAST negative (rising)
            0x50, 0x60, 0, 0, 0, 0, 0, 0, &NO_COLLISION_DATA, 0, 0x00, 0, 0, 0, 0, 0x00, 0x10, 3,
        );
        assert_eq!(r.enemy_var_3, 1);
        assert_eq!(r.enemy_frame, 0x0A);
        match r.landing {
            SoldierRoutine02Landing::NotLanded { water_routine_switch, y_velocity } => {
                assert_eq!(water_routine_switch, None);
                assert_eq!(y_velocity, add_10_to_enemy_y_fract_vel(0x00, 0x10));
            }
            other => panic!("expected NotLanded, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_jumping_lands_on_solid_clears_var_3_and_frame() {
        let r = soldier_routine_02_jumping(
            1, 0x01, // falling
            0x50, 0x60, 0, 0, 0, 0, 0, 0, &SOLID_COLLISION_DATA, 0, 0x00, 0, 0, 0, 0, 0x00, 0x10, 3,
        );
        assert_eq!(r.enemy_var_3, 0);
        assert_eq!(r.enemy_frame, 0x00);
        match r.landing {
            SoldierRoutine02Landing::Landed { y_pos, velocity } => {
                assert_eq!(y_pos, add_4_to_enemy_y_pos(0, 0x60));
                assert_eq!(velocity, soldier_stop_y_set_x_velocity(0, 0));
            }
            other => panic!("expected Landed, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_jumping_water_landing_switches_routine_before_the_tail_runs() {
        let mut data = NO_COLLISION_DATA;
        // the check is 0x10 below the enemy's Y position; put Water
        // there specifically (not at the enemy's own Y).
        set_collision_at(&mut data, 0x50, 0x70, CollisionCode::Water);
        let r = soldier_routine_02_jumping(1, 0x01, 0x50, 0x60, 0, 0, 0, 0, 0, 0, &data, 0, 0x00, 0, 0, 0, 0, 0x00, 0x10, 3);
        assert_eq!(r.enemy_frame, 0x0A);
        match r.landing {
            SoldierRoutine02Landing::NotLanded { water_routine_switch, .. } => {
                assert_eq!(water_routine_switch, Some(set_enemy_routine_to_a(3, 0x0A)));
            }
            other => panic!("expected NotLanded, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_jumping_checked_not_solid_not_water_still_bumps_y_fract_vel() {
        let r = soldier_routine_02_jumping(1, 0x01, 0x50, 0x60, 0, 0, 0, 0, 0, 0, &NO_COLLISION_DATA, 0, 0x00, 0, 0, 0, 0, 0x00, 0x10, 3);
        match r.landing {
            SoldierRoutine02Landing::NotLanded { water_routine_switch, y_velocity } => {
                assert_eq!(water_routine_switch, None);
                assert_eq!(y_velocity, add_10_to_enemy_y_fract_vel(0x00, 0x10));
            }
            other => panic!("expected NotLanded, got {other:?}"),
        }
    }
}
