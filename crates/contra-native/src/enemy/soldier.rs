//! Native port of the plain soldier enemy's AI states (`src/bank0.asm`):
//! `soldier_routine_00` (CPU `$861e`-`$8633`), run once right after
//! `initialize_enemy` spawns it - nudges its position slightly down so it
//! visually stands on the ground, and sets a per-attribute initial
//! animation delay before advancing to `soldier_routine_01` (`$8665`),
//! which waits out that delay, checks for ground beneath the soldier, and
//! either removes it (no valid footing - e.g. a destroyed bridge) or
//! plants it and sets it walking (`soldier_routine_02`, jumping sub-path
//! only so far - see [`soldier_routine_02_jumping`]); `soldier_routine_03`
//! (`$8803`) is the "try and fire a bullet" state reached from the
//! walking sub-path once it's not yet ported. This crate's first
//! **composed enemy AI states** - every step is a call into an already
//! independently-verified building block
//! ([`crate::enemy::add_scroll_to_enemy_pos::add_scroll_to_enemy_pos`],
//! [`crate::enemy::update_enemy_pos::remove_enemy`],
//! [`crate::enemy::enemy_position_utils::add_4_to_enemy_y_pos`],
//! [`crate::enemy::enemy_routine_transition::set_enemy_delay_adv_routine`],
//! [`crate::physics::collision::add_y_to_y_pos_get_bg_collision`],
//! [`crate::enemy::enemy_collision_flags::enable_enemy_collision`]) -
//! demonstrating the same real composition the ROM itself uses, no new
//! arithmetic beyond small bit tests, table lookups, and one real quirk
//! reproduced exactly: see [`SoldierRoutine01Outcome::DelayNotYetZero`]
//! for `soldier_routine_01`'s one path that decrements
//! `ENEMY_ANIMATION_DELAY` twice in a single call.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::physics::collision::{add_y_to_y_pos_get_bg_collision, check_enemy_collision_solid_bg, get_bg_collision_far, CollisionCode, BG_COLLISION_DATA_LEN};
use crate::enemy::create_enemy_bullet::{create_enemy_bullet_angle_a, CreatedBullet};
use crate::enemy::enemy_collision_flags::{disable_enemy_collision, enable_enemy_collision};
use crate::enemy::enemy_position_utils::{
    add_10_to_enemy_y_fract_vel, add_4_to_enemy_y_pos, add_a_to_enemy_y_fract_vel, add_a_to_enemy_y_pos, reverse_enemy_x_direction,
};
use crate::enemy::enemy_routine_transition::{
    advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate,
};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::update_enemy_pos::{
    remove_enemy, set_enemy_x_velocity_to_0, set_enemy_y_velocity_to_0, update_enemy_pos, RemovedEnemy, UpdatedEnemyPos, ZeroedVelocity,
};

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
/// [`crate::enemy::update_enemy_pos::set_enemy_y_velocity_to_0`].
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
/// Live-verified indirectly via [`soldier_routine_02_jumping`]: 96 real
/// calls, zero mismatches.
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

/// `soldier_bullet_y_offset` (`$8882`, 4 bytes) - Y offset from the
/// soldier's own position for a spawned bullet, indexed the same way as
/// `soldier_bullet_x_offset`: `[standing-left, standing-right, crouch-
/// left, crouch-right]`.
const SOLDIER_BULLET_Y_OFFSET: [u8; 4] = [0xF7, 0xF7, 0x0A, 0x0A];
/// `soldier_bullet_x_offset` (`$8886`, 4 bytes) - same indexing as
/// [`SOLDIER_BULLET_Y_OFFSET`].
const SOLDIER_BULLET_X_OFFSET: [u8; 4] = [0xF0, 0x10, 0xF0, 0x10];
/// `soldier_bullet_type_tbl` (`$888a`, 2 bytes) - indexed by `ENEMY_VAR_2`
/// (running direction); fed through [`bullet_generation`]'s `asl` before
/// reaching [`create_enemy_bullet_angle_a`].
const SOLDIER_BULLET_TYPE_TBL: [u8; 2] = [0x06, 0x00];

/// Native port of `bullet_generation` (`$f2be`) - real ASM is a single
/// `asl` immediately falling through into `create_enemy_bullet_angle_a`;
/// this crate already has that routine ported
/// ([`crate::enemy::create_enemy_bullet::create_enemy_bullet_angle_a`]), so this
/// is just the one-instruction caller-side transform feeding into it.
pub fn bullet_generation(bullet_type_and_angle_pre_shift: u8) -> u8 {
    bullet_type_and_angle_pre_shift << 1
}

/// The result of one [`set_soldier_sprite_add_scroll_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierSpriteScrollResult {
    pub sprite: SoldierSpriteResult,
    pub scroll: ScrolledEnemyPos,
}

/// Native port of `set_soldier_sprite_add_scroll_01` (`$8864`) - the tail
/// `soldier_routine_03`/`soldier_fired_all_bullets` share: updates the
/// sprite, then applies camera scroll to the position (unlike `soldier_
/// routine_01`/`02`'s own tail, `set_soldier_sprite_update_pos`, this one
/// does *not* apply velocity - firing doesn't move the soldier).
#[allow(clippy::too_many_arguments)]
pub fn set_soldier_sprite_add_scroll_01(
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
) -> SoldierSpriteScrollResult {
    let sprite = set_soldier_sprite(enemy_frame, enemy_var_2, enemy_var_1);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
    SoldierSpriteScrollResult { sprite, scroll }
}

/// [`soldier_routine_03`]'s result when `ENEMY_ATTACK_DELAY` hadn't
/// elapsed yet this call - nothing beyond the sprite/scroll tail runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine03Waiting {
    /// `Some($1b)` only if crouching to fire this call (real ASM sets
    /// this unconditionally whenever the crouch branch is taken, even
    /// though the delay hasn't elapsed - it's a per-attribute constant,
    /// not tied to when firing actually happens).
    pub score_collision: Option<u8>,
    pub enemy_frame: u8,
    pub attack_delay: u8,
    pub tail: SoldierSpriteScrollResult,
}

/// [`soldier_routine_03`]'s result when the delay elapsed and a bullet
/// was (or wasn't) fired this call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine03Fired {
    pub score_collision: Option<u8>,
    pub enemy_frame: u8,
    /// Always `$10` (real ASM resets the delay unconditionally on this
    /// path, whether or not the bullet itself actually spawns).
    pub attack_delay: u8,
    pub var_3: u8,
    /// `None` covers *both* "computed spawn position was off-screen" (no
    /// creation even attempted) and "on-screen, but `create_enemy_
    /// bullet_angle_a` itself declined" (attack flag off, or no free
    /// enemy slot) - both leave identical real RAM state beyond this
    /// composition's own earlier steps, so collapsing them loses no
    /// verifiable fidelity.
    pub bullet: Option<CreatedBullet>,
    /// `Some($06)` only if `bullet` is `Some` - the gun recoil timer real
    /// ASM sets right after a successful spawn.
    pub gun_recoil_timer: Option<u8>,
    pub tail: SoldierSpriteScrollResult,
}

/// [`soldier_routine_03`]'s result when the soldier just fired its last
/// bullet this call - real ASM resets crouch/frame/bullet-count and
/// advances back to `soldier_routine_02`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine03AllFired {
    pub routine_update: EnemyRoutineUpdate,
    pub tail: SoldierSpriteScrollResult,
}

/// The real, branchy result of one [`soldier_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoldierRoutine03Outcome {
    Waiting(SoldierRoutine03Waiting),
    Fired(SoldierRoutine03Fired),
    AllFired(SoldierRoutine03AllFired),
}

/// Native port of `soldier_routine_03` (`$8803`) - the soldier's "try and
/// fire a bullet" state: crouches or stands depending on `ENEMY_
/// ATTRIBUTES` bit 3, waits out `ENEMY_ATTACK_DELAY`, then either fires
/// one of `ENEMY_VAR_3` remaining bullets (computing its spawn position
/// from a per-direction/per-stance offset table and bailing without even
/// attempting a spawn if that position is off-screen) or, once all
/// bullets are spent, resets state and returns to `soldier_routine_02`.
/// No `RANDOM_NUM`/inherited-carry dependency anywhere in this routine
/// (unlike `soldier_routine_02`'s still-unported walking sub-path) - every
/// branch here is a plain, deterministic bit test or unsigned comparison.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_03(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_attributes: u8,
    enemy_attack_delay: u8,
    enemy_var_3: u8,
    enemy_var_2: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_1: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
) -> SoldierRoutine03Outcome {
    // Real ASM: `and #$0c; cmp #$05` - masks to bits 2-3 (values
    // 0/4/8/12), then an unsigned compare against 5; only 8 and 12 (bit 3
    // set) satisfy `>= 5`, so this is exactly a bit-3 test, computed the
    // literal way the real comparison does it rather than simplified.
    let crouching = (enemy_attributes & 0x0C) >= 0x05;
    let (score_collision, enemy_frame) = if crouching { (Some(0x1B), 0x07) } else { (None, 0x06) };

    let attack_delay = enemy_attack_delay.wrapping_sub(1);
    if attack_delay != 0 {
        let tail =
            set_soldier_sprite_add_scroll_01(enemy_frame, enemy_var_2, enemy_var_1, level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
        return SoldierRoutine03Outcome::Waiting(SoldierRoutine03Waiting { score_collision, enemy_frame, attack_delay, tail });
    }

    let var_3 = enemy_var_3.wrapping_sub(1);
    if (var_3 as i8) < 0 {
        let tail = set_soldier_sprite_add_scroll_01(0x00, enemy_var_2, enemy_var_1, level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
        let routine_update = set_enemy_routine_to_a(current_routine, 0x03);
        return SoldierRoutine03Outcome::AllFired(SoldierRoutine03AllFired { routine_update, tail });
    }

    let attack_delay = 0x10;
    let offset_index = (if crouching { 2 } else { 0 }) + (if enemy_var_2 != 0 { 1 } else { 0 });
    let bullet_y_pos = enemy_y_pos.wrapping_add(SOLDIER_BULLET_Y_OFFSET[offset_index]);
    let x_offset = SOLDIER_BULLET_X_OFFSET[offset_index];

    let bullet_x_pos = if (x_offset as i8) < 0 {
        let (bx, carry) = x_offset.overflowing_add(enemy_x_pos);
        if !carry || bx < 0x08 {
            None
        } else {
            Some(bx)
        }
    } else {
        let (bx, carry) = x_offset.overflowing_add(enemy_x_pos);
        if carry {
            None
        } else {
            Some(bx)
        }
    };

    let (bullet, gun_recoil_timer) = if let Some(bullet_x_pos) = bullet_x_pos {
        let bullet_type_and_angle = bullet_generation(SOLDIER_BULLET_TYPE_TBL[enemy_var_2 as usize]);
        let created =
            create_enemy_bullet_angle_a(prg_rom, enemy_routine, current_level, enemy_attack_flag, bullet_type_and_angle, 0x06, bullet_y_pos, bullet_x_pos);
        let recoil = if created.is_some() { Some(0x06) } else { None };
        (created, recoil)
    } else {
        (None, None)
    };

    // The gun recoil timer, if just set, is stored *before* falling into
    // the shared tail - `set_soldier_sprite` itself reads (and
    // decrements) `ENEMY_VAR_1` as part of its own logic, so a bullet
    // fired this exact call already sees the fresh `$06`, not the
    // original input.
    let tail_var_1 = gun_recoil_timer.unwrap_or(enemy_var_1);
    let tail =
        set_soldier_sprite_add_scroll_01(enemy_frame, enemy_var_2, tail_var_1, level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
    SoldierRoutine03Outcome::Fired(SoldierRoutine03Fired { score_collision, enemy_frame, attack_delay, var_3, bullet, gun_recoil_timer, tail })
}

/// The full result of one [`init_soldier_hit_vel`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitSoldierHitVelResult {
    /// `ENEMY_STATE_WIDTH` after [`disable_enemy_collision`] runs.
    pub state_width: u8,
    /// Always `($80, $fc)` - the fixed "fly up when hit" initial Y
    /// velocity.
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    pub scroll: ScrolledEnemyPos,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `init_soldier_hit_vel` (`$88cb`) - the real shared tail
/// `soldier_routine_04` falls straight into after setting its own
/// destroyed-soldier sprite frame, also reused directly (with no
/// soldier-specific step of its own first) by `sniper_routine_04`
/// (`crates/contra-native/src/enemy/sniper.rs`, `$8af1`): sets a fixed
/// initial "fly up when hit" velocity - X velocity is zeroed instead if
/// the enemy is near either screen edge (real ASM checks *both* edges,
/// `< $10` or `>= $f0`, into the *same* zeroing step), then reversed if
/// it was facing right (the fixed X velocity is authored assuming a
/// left-facing enemy, the same convention `soldier_x_vel_tbl` uses).
#[allow(clippy::too_many_arguments)]
pub fn init_soldier_hit_vel(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_2: u8,
    enemy_state_width: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
) -> InitSoldierHitVelResult {
    let state_width = disable_enemy_collision(enemy_state_width);
    let y_velocity = (0x80, 0xFC);

    let mut x_velocity = (0x60u8, 0x00u8);
    if enemy_x_pos < 0x10 || enemy_x_pos >= 0xF0 {
        let z = set_enemy_x_velocity_to_0();
        x_velocity = (z.vel_fract, z.vel_fast);
    }
    if enemy_var_2 != 0 {
        x_velocity = reverse_enemy_x_direction(x_velocity.0, x_velocity.1);
    }

    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
    let delayed_routine = set_enemy_delay_adv_routine(0x10, current_routine);

    InitSoldierHitVelResult { state_width, y_velocity, x_velocity, scroll, delayed_routine }
}

/// The full result of one [`soldier_routine_04`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine04Result {
    pub sprite: SoldierSpriteResult,
    pub state_width: u8,
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    pub scroll: ScrolledEnemyPos,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `soldier_routine_04` (`$88c3`) - "soldier hit, begin
/// destroying soldier": sets the destroyed-soldier sprite frame, then
/// falls into the shared [`init_soldier_hit_vel`] tail.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_04(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    enemy_state_width: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
) -> SoldierRoutine04Result {
    let sprite = set_soldier_sprite(0x0B, enemy_var_2, enemy_var_1);
    let tail = init_soldier_hit_vel(enemy_x_pos, enemy_y_pos, enemy_var_2, enemy_state_width, level_scrolling_type, frame_scroll, current_routine);
    SoldierRoutine04Result {
        sprite,
        state_width: tail.state_width,
        y_velocity: tail.y_velocity,
        x_velocity: tail.x_velocity,
        scroll: tail.scroll,
        delayed_routine: tail.delayed_routine,
    }
}

/// The real, branchy result of one [`apply_gravity_to_destroyed_soldier`]
/// call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyGravityToDestroyedSoldierOutcome {
    /// The enemy's (unmodified-this-call) Y position was already above
    /// the top of the screen: advances immediately, `update_enemy_pos`
    /// never runs at all (position/velocity-accumulator fields untouched
    /// beyond the Y-velocity gravity add every path gets).
    OffTopAdvance(EnemyRoutineUpdate),
    /// Position updated; `ENEMY_ANIMATION_DELAY` hadn't reached zero yet.
    StillWaiting { position: UpdatedEnemyPos, animation_delay: u8 },
    /// Position updated and the delay reached zero: advances to the next
    /// routine.
    Advanced { position: UpdatedEnemyPos, animation_delay: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`apply_gravity_to_destroyed_soldier`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyGravityToDestroyedSoldierResult {
    /// Y velocity after this call's fixed gravity add (`+$30` fractional,
    /// every call, regardless of outcome).
    pub y_velocity: (u8, u8),
    pub outcome: ApplyGravityToDestroyedSoldierOutcome,
}

/// Native port of `apply_gravity_to_destroyed_soldier` (`$8903`) - the
/// real shared tail `soldier_routine_05` falls straight into after
/// setting its own sprite, also reused directly (with no soldier-
/// specific step of its own first) by `sniper_routine_05`
/// (`crates/contra-native/src/enemy/sniper.rs`, `$8afc`): the destroyed
/// enemy launched by [`init_soldier_hit_vel`] keeps flying up,
/// decelerating under a fixed gravity constant, until either it drifts
/// off the top of the screen or its animation delay elapses, either of
/// which advances to the next routine (real explosion/removal handling,
/// not yet ported).
#[allow(clippy::too_many_arguments)]
pub fn apply_gravity_to_destroyed_soldier(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_animation_delay: u8,
    current_routine: u8,
) -> ApplyGravityToDestroyedSoldierResult {
    let y_velocity = add_a_to_enemy_y_fract_vel(0x30, y_vel_fract, y_vel_fast);

    if enemy_y_pos < 0x08 {
        let routine_update = advance_enemy_routine(current_routine);
        return ApplyGravityToDestroyedSoldierResult {
            y_velocity,
            outcome: ApplyGravityToDestroyedSoldierOutcome::OffTopAdvance(routine_update),
        };
    }

    let position = update_enemy_pos(
        level_scrolling_type,
        frame_scroll,
        enemy_x_pos,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        enemy_y_pos,
        y_vel_accum,
        y_velocity.0,
        y_velocity.1,
    );
    let animation_delay = enemy_animation_delay.wrapping_sub(1);
    let outcome = if animation_delay == 0 {
        ApplyGravityToDestroyedSoldierOutcome::Advanced { position, animation_delay, routine_update: advance_enemy_routine(current_routine) }
    } else {
        ApplyGravityToDestroyedSoldierOutcome::StillWaiting { position, animation_delay }
    };
    ApplyGravityToDestroyedSoldierResult { y_velocity, outcome }
}

/// The real, branchy result of one [`soldier_routine_05`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoldierRoutine05Outcome {
    OffTopAdvance(EnemyRoutineUpdate),
    StillWaiting { position: UpdatedEnemyPos, animation_delay: u8 },
    Advanced { position: UpdatedEnemyPos, animation_delay: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`soldier_routine_05`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine05Result {
    pub sprite: SoldierSpriteResult,
    pub y_velocity: (u8, u8),
    pub outcome: SoldierRoutine05Outcome,
}

/// Native port of `soldier_routine_05` (`$8900`) - "soldier hit, apply
/// negative gravity": sets the sprite, then falls into the shared
/// [`apply_gravity_to_destroyed_soldier`] tail.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_05(
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_animation_delay: u8,
    current_routine: u8,
) -> SoldierRoutine05Result {
    let sprite = set_soldier_sprite(enemy_frame, enemy_var_2, enemy_var_1);
    let tail = apply_gravity_to_destroyed_soldier(
        enemy_x_pos,
        enemy_y_pos,
        y_vel_fract,
        y_vel_fast,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        y_vel_accum,
        level_scrolling_type,
        frame_scroll,
        enemy_animation_delay,
        current_routine,
    );
    let outcome = match tail.outcome {
        ApplyGravityToDestroyedSoldierOutcome::OffTopAdvance(r) => SoldierRoutine05Outcome::OffTopAdvance(r),
        ApplyGravityToDestroyedSoldierOutcome::StillWaiting { position, animation_delay } => {
            SoldierRoutine05Outcome::StillWaiting { position, animation_delay }
        }
        ApplyGravityToDestroyedSoldierOutcome::Advanced { position, animation_delay, routine_update } => {
            SoldierRoutine05Outcome::Advanced { position, animation_delay, routine_update }
        }
    };
    SoldierRoutine05Result { sprite, y_velocity: tail.y_velocity, outcome }
}

/// Native port of `soldier_set_y_pos_sprite_add_scroll` (`$88ba`) - adds
/// `a` to `ENEMY_Y_POS`, then falls into `set_soldier_sprite_add_scroll`
/// (`$88bd`) with the new position. That fallthrough target is its own
/// separate real routine (a physically distinct `jsr set_soldier_sprite;
/// jmp add_scroll_to_enemy_pos` at `$88bd`, not the same bytes as
/// `soldier_routine_01`/`02`/`03`'s own identical-shaped tail at `$8864`)
/// but is mathematically the exact same composition, so this port reuses
/// [`set_soldier_sprite_add_scroll_01`] rather than duplicating it -
/// same reasoning as [`check_enemy_collision_solid_bg`] reusing [`get_bg_
/// collision_far`].
#[allow(clippy::too_many_arguments)]
pub fn soldier_set_y_pos_sprite_add_scroll(
    a: u8,
    enemy_y_pos: u8,
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
) -> SoldierSpriteScrollResult {
    let y_pos = add_a_to_enemy_y_pos(a, enemy_y_pos);
    set_soldier_sprite_add_scroll_01(enemy_frame, enemy_var_2, enemy_var_1, level_scrolling_type, frame_scroll, enemy_x_pos, y_pos)
}

/// The full result of one [`soldier_routine_09`] call - see this
/// function's own doc comment for why there are *two* sprite/scroll
/// results rather than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoldierRoutine09Result {
    /// The result of the `jsr soldier_set_y_pos_sprite_add_scroll` call
    /// (Y position nudged `+$10` first).
    pub first: SoldierSpriteScrollResult,
    /// The result of the real ASM's *second*, separate `jsr set_soldier_
    /// sprite` / `jsr add_scroll_to_enemy_pos` pair, run immediately
    /// after `first` - reading whatever RAM state `first` already left
    /// behind (so `second.scroll` applies camera scroll a *second* time
    /// on top of `first.scroll`'s already-adjusted position, and `second.
    /// sprite` decrements `ENEMY_VAR_1`/the gun-recoil timer a second
    /// time too, if it was still nonzero after `first`'s own decrement).
    pub second: SoldierSpriteScrollResult,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `soldier_routine_09` (`$888c`) - "soldier landing in
/// water": sets the water-splash sprite frame, nudges the soldier down
/// `$10` pixels into the water, and advances to `soldier_routine_0a`
/// after `$08` frames.
///
/// **Real ASM genuinely calls `set_soldier_sprite`/`add_scroll_to_enemy_
/// pos` twice, not once**: `jsr soldier_set_y_pos_sprite_add_scroll`
/// itself already falls all the way through that pair (there's no `rts`
/// between `soldier_set_y_pos_sprite_add_scroll`'s `jsr add_a_to_enemy_
/// y_pos` and `set_soldier_sprite_add_scroll`'s own body - confirmed via
/// `docs/rom-symbols.txt`'s real addresses, not just the local disassembly
/// text's line ordering), and the routine's next two lines call `jsr
/// set_soldier_sprite`/`jsr add_scroll_to_enemy_pos` again, separately.
/// Ported literally rather than "corrected" - if this reading is wrong,
/// live verification against real hardware will show it as a mismatch;
/// see [`SoldierRoutine09Result::second`]'s own doc comment for exactly
/// what the second pass does differently from a naive re-run.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_09(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
) -> SoldierRoutine09Result {
    let first = soldier_set_y_pos_sprite_add_scroll(0x10, enemy_y_pos, 0x08, enemy_var_2, enemy_var_1, level_scrolling_type, frame_scroll, enemy_x_pos);
    let second_sprite = set_soldier_sprite(0x08, enemy_var_2, first.sprite.var_1);
    let second_scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, first.scroll.x_pos, first.scroll.y_pos);
    let second = SoldierSpriteScrollResult { sprite: second_sprite, scroll: second_scroll };
    let delayed_routine = set_enemy_delay_adv_routine(0x08, current_routine);
    SoldierRoutine09Result { first, second, delayed_routine }
}

/// The real, branchy result of one [`soldier_routine_0a`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoldierRoutine0aOutcome {
    /// Splash animation delay hadn't elapsed yet - just the sprite/scroll
    /// tail ran, position untouched.
    Waiting(SoldierSpriteScrollResult),
    /// Delay elapsed and `ENEMY_FRAME` (after incrementing) reached the
    /// "splash finished" sentinel (`$0a`): removed, nothing else runs.
    Removed(RemovedEnemy),
    /// Delay elapsed but the splash animation isn't done: delay reset to
    /// `$08`, `ENEMY_FRAME` incremented, position nudged down another
    /// `$08` pixels.
    StillSplashing { animation_delay: u8, enemy_frame: u8, tail: SoldierSpriteScrollResult },
}

/// Native port of `soldier_routine_0a` (`$88a1`) - "continue splash
/// animation and begin removing soldier": waits out `ENEMY_ANIMATION_
/// DELAY`, then advances the water-splash animation frame by frame,
/// removing the soldier once it's played through.
#[allow(clippy::too_many_arguments)]
pub fn soldier_routine_0a(
    enemy_animation_delay: u8,
    enemy_frame: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
) -> SoldierRoutine0aOutcome {
    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        let tail =
            set_soldier_sprite_add_scroll_01(enemy_frame, enemy_var_2, enemy_var_1, level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
        return SoldierRoutine0aOutcome::Waiting(tail);
    }

    let new_frame = enemy_frame.wrapping_add(1);
    if new_frame >= 0x0A {
        return SoldierRoutine0aOutcome::Removed(remove_enemy());
    }

    let animation_delay = 0x08;
    let tail = soldier_set_y_pos_sprite_add_scroll(0x08, enemy_y_pos, new_frame, enemy_var_2, enemy_var_1, level_scrolling_type, frame_scroll, enemy_x_pos);
    SoldierRoutine0aOutcome::StillSplashing { animation_delay, enemy_frame: new_frame, tail }
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
    /// `(x, y)` to `code`, using [`crate::physics::collision::bg_collision_scratch`]
    /// to find the right byte/column rather than hand-deriving the
    /// offset formula again.
    fn set_collision_at(data: &mut [u8; BG_COLLISION_DATA_LEN], x: u8, y: u8, code: CollisionCode) {
        let scratch = crate::physics::collision::bg_collision_scratch(x, y, 0, 0, 0);
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

    fn synthetic_prg_rom() -> Vec<u8> {
        // Same shape as `create_enemy_bullet`'s own synthetic-ROM test
        // fixture: a shared property-table pointer with a recognizable
        // record at enemy_type=1's (bullets') offset.
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 4;
        rom[record_off..record_off + 4].copy_from_slice(&[0x80, 0x00, 0x01, 0x00]);
        rom
    }

    #[test]
    fn bullet_generation_shifts_left_by_one() {
        assert_eq!(bullet_generation(0x06), 0x0C);
        assert_eq!(bullet_generation(0x00), 0x00);
    }

    #[test]
    fn set_soldier_sprite_add_scroll_01_composes_sprite_and_scroll() {
        let r = set_soldier_sprite_add_scroll_01(0x06, 1, 0, 0, 0x03, 0x50, 0x60);
        assert_eq!(r.sprite, set_soldier_sprite(0x06, 1, 0));
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x03, 0x50, 0x60));
    }

    #[test]
    fn routine_03_waits_when_attack_delay_has_not_elapsed() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x00, 0x05, 0x02, 0, 0x50, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::Waiting(w) => {
                assert_eq!(w.attack_delay, 0x04);
                assert_eq!(w.score_collision, None);
                assert_eq!(w.enemy_frame, 0x06);
            }
            other => panic!("expected Waiting, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_crouching_sets_score_collision_and_crouch_frame() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x08, 0x05, 0x02, 0, 0x50, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::Waiting(w) => {
                assert_eq!(w.score_collision, Some(0x1B));
                assert_eq!(w.enemy_frame, 0x07);
            }
            other => panic!("expected Waiting, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_all_bullets_fired_resets_and_advances_to_routine_02() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // var_3=0x00 -> wrapping_sub(1) = 0xFF, negative as i8 -> all fired.
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x00, 0x01, 0x00, 0, 0x50, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::AllFired(a) => {
                assert_eq!(a.routine_update, set_enemy_routine_to_a(3, 0x03));
                assert_eq!(a.tail.sprite, set_soldier_sprite(0x00, 0, 0));
            }
            other => panic!("expected AllFired, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_off_screen_left_aborts_without_attempting_a_bullet() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // running left (var_2=0): x_offset=0xF0 (-16). enemy_x_pos=0x05
        // -> 0xF0+0x05=0xF5, carry=false -> off-screen (no carry-out).
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x00, 0x01, 0x02, 0, 0x05, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::Fired(f) => {
                assert_eq!(f.bullet, None);
                assert_eq!(f.gun_recoil_timer, None);
                assert_eq!(f.attack_delay, 0x10);
                assert_eq!(f.var_3, 0x01);
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_off_screen_right_aborts_without_attempting_a_bullet() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // running right (var_2=1): x_offset=0x10. enemy_x_pos=0xF5 ->
        // 0x10+0xF5=0x05 with carry=true -> off-screen (overflow).
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x00, 0x01, 0x02, 1, 0xF5, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::Fired(f) => assert_eq!(f.bullet, None),
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_on_screen_left_creates_a_bullet_and_sets_recoil() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[5] = 0; // only slot 5 free
        // running left, well on-screen: 0xF0+0x50=0x40, carry=true, 0x40>=0x08 -> fires.
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x00, 0x01, 0x02, 0, 0x50, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::Fired(f) => {
                let bullet = f.bullet.expect("expected a bullet to be created");
                assert_eq!(bullet.slot, 5);
                assert_eq!(f.gun_recoil_timer, Some(0x06));
                // bullet Y = enemy_y_pos(0x60) + soldier_bullet_y_offset[0](0xF7) = 0x57
                assert_eq!(bullet.fields.y_pos, 0x60u8.wrapping_add(0xF7));
                assert_eq!(bullet.fields.x_pos, 0x40);
                // `set_soldier_sprite` runs *after* the recoil timer is
                // stored, so it sees (and decrements) the fresh $06, not
                // whatever `enemy_var_1` was on entry.
                assert_eq!(f.tail.sprite, set_soldier_sprite(0x06, 0, 0x06));
                assert_eq!(f.tail.sprite.var_1, 0x05);
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_declined_creation_when_no_free_slot_still_updates_state_but_no_recoil() {
        let rom = synthetic_prg_rom();
        let routine = [1u8; ENEMY_SLOT_COUNT]; // no free slots
        let r = soldier_routine_03(&rom, &routine, 0, 1, 0x00, 0x01, 0x02, 0, 0x50, 0x60, 0, 0, 0x00, 3);
        match r {
            SoldierRoutine03Outcome::Fired(f) => {
                assert_eq!(f.bullet, None);
                assert_eq!(f.gun_recoil_timer, None);
                assert_eq!(f.var_3, 0x01);
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_04_mid_screen_keeps_the_fixed_x_velocity_facing_left() {
        let r = soldier_routine_04(0x50, 0x60, 0, 0, 0x00, 0, 0x02, 3);
        assert_eq!(r.sprite, set_soldier_sprite(0x0B, 0, 0));
        assert_eq!(r.state_width, disable_enemy_collision(0x00));
        assert_eq!(r.y_velocity, (0x80, 0xFC));
        assert_eq!(r.x_velocity, (0x60, 0x00));
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x10, 3));
    }

    #[test]
    fn routine_04_running_right_reverses_the_fixed_x_velocity() {
        let r = soldier_routine_04(0x50, 0x60, 1, 0, 0x00, 0, 0x02, 3);
        assert_eq!(r.x_velocity, reverse_enemy_x_direction(0x60, 0x00));
    }

    #[test]
    fn routine_04_near_left_edge_zeroes_x_velocity() {
        let r = soldier_routine_04(0x0F, 0x60, 0, 0, 0x00, 0, 0x02, 3);
        assert_eq!(r.x_velocity, (0x00, 0x00));
    }

    #[test]
    fn routine_04_near_right_edge_zeroes_x_velocity_too() {
        let r = soldier_routine_04(0xF0, 0x60, 0, 0, 0x00, 0, 0x02, 3);
        assert_eq!(r.x_velocity, (0x00, 0x00));
    }

    #[test]
    fn routine_04_edge_zero_still_reverses_to_zero_when_running_right() {
        // Zeroing then reversing must stay zero (negating zero is zero).
        let r = soldier_routine_04(0x0F, 0x60, 1, 0, 0x00, 0, 0x02, 3);
        assert_eq!(r.x_velocity, (0x00, 0x00));
    }

    #[test]
    fn routine_05_off_top_of_screen_advances_immediately_without_updating_position() {
        let r = soldier_routine_05(0x0B, 0, 0, 0x50, 0x05, 0x00, 0x00, 0, 0, 0, 0, 0, 0x00, 0x10, 3);
        assert_eq!(r.sprite, set_soldier_sprite(0x0B, 0, 0));
        assert_eq!(r.y_velocity, add_a_to_enemy_y_fract_vel(0x30, 0x00, 0x00));
        assert_eq!(r.outcome, SoldierRoutine05Outcome::OffTopAdvance(advance_enemy_routine(3)));
    }

    #[test]
    fn routine_05_on_screen_updates_position_and_waits_when_delay_not_elapsed() {
        let r = soldier_routine_05(0x0B, 0, 0, 0x50, 0x60, 0x00, 0x00, 0, 0, 0, 0, 0, 0x00, 0x05, 3);
        match r.outcome {
            SoldierRoutine05Outcome::StillWaiting { animation_delay, .. } => assert_eq!(animation_delay, 0x04),
            other => panic!("expected StillWaiting, got {other:?}"),
        }
    }

    #[test]
    fn routine_05_delay_reaching_zero_advances_to_the_next_routine() {
        let r = soldier_routine_05(0x0B, 0, 0, 0x50, 0x60, 0x00, 0x00, 0, 0, 0, 0, 0, 0x00, 0x01, 3);
        match r.outcome {
            SoldierRoutine05Outcome::Advanced { animation_delay, routine_update, .. } => {
                assert_eq!(animation_delay, 0x00);
                assert_eq!(routine_update, advance_enemy_routine(3));
            }
            other => panic!("expected Advanced, got {other:?}"),
        }
    }

    #[test]
    fn soldier_set_y_pos_sprite_add_scroll_composes_y_offset_and_sprite_scroll() {
        let r = soldier_set_y_pos_sprite_add_scroll(0x10, 0x50, 0x08, 0, 0, 0, 0x02, 0x60);
        let expected_y = add_a_to_enemy_y_pos(0x10, 0x50);
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x60, expected_y));
        assert_eq!(r.sprite, set_soldier_sprite(0x08, 0, 0));
    }

    #[test]
    fn routine_09_sets_water_splash_frame_and_advances_after_8_frames() {
        let r = soldier_routine_09(0x60, 0x50, 0, 0, 0, 0x02, 3);
        let expected_y_1 = add_a_to_enemy_y_pos(0x10, 0x50);
        let expected_scroll_1 = add_scroll_to_enemy_pos(0, 0x02, 0x60, expected_y_1);
        assert_eq!(r.first.scroll, expected_scroll_1);
        assert_eq!(r.first.sprite, set_soldier_sprite(0x08, 0, 0));
        // the real ASM's second, separate call re-applies scroll on top
        // of `first`'s already-scrolled position.
        let expected_scroll_2 = add_scroll_to_enemy_pos(0, 0x02, expected_scroll_1.x_pos, expected_scroll_1.y_pos);
        assert_eq!(r.second.scroll, expected_scroll_2);
        assert_eq!(r.second.sprite, set_soldier_sprite(0x08, 0, r.first.sprite.var_1));
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x08, 3));
    }

    #[test]
    fn routine_09_second_pass_decrements_gun_recoil_a_second_time_if_still_nonzero() {
        // var_1=2 entering: first pass decrements to 1, second pass to 0.
        let r = soldier_routine_09(0x60, 0x50, 0, 2, 0, 0x02, 3);
        assert_eq!(r.first.sprite.var_1, 1);
        assert_eq!(r.second.sprite.var_1, 0);
    }

    #[test]
    fn routine_0a_waits_when_delay_has_not_elapsed() {
        let r = soldier_routine_0a(0x05, 0x08, 0x60, 0x50, 0, 0, 0, 0x02);
        match r {
            SoldierRoutine0aOutcome::Waiting(tail) => {
                assert_eq!(tail.sprite, set_soldier_sprite(0x08, 0, 0));
                assert_eq!(tail.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x60, 0x50));
            }
            other => panic!("expected Waiting, got {other:?}"),
        }
    }

    #[test]
    fn routine_0a_still_splashing_increments_frame_and_nudges_y_down() {
        // delay=1 -> elapses this call; frame 8 -> 9, still < 0x0a.
        let r = soldier_routine_0a(0x01, 0x08, 0x60, 0x50, 0, 0, 0, 0x02);
        match r {
            SoldierRoutine0aOutcome::StillSplashing { animation_delay, enemy_frame, tail } => {
                assert_eq!(animation_delay, 0x08);
                assert_eq!(enemy_frame, 0x09);
                let expected_y = add_a_to_enemy_y_pos(0x08, 0x50);
                assert_eq!(tail.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x60, expected_y));
                assert_eq!(tail.sprite, set_soldier_sprite(0x09, 0, 0));
            }
            other => panic!("expected StillSplashing, got {other:?}"),
        }
    }

    #[test]
    fn routine_0a_removes_the_enemy_once_the_splash_animation_finishes() {
        // delay=1 -> elapses; frame 9 -> 0x0a, >= 0x0a -> removed.
        let r = soldier_routine_0a(0x01, 0x09, 0x60, 0x50, 0, 0, 0, 0x02);
        assert_eq!(r, SoldierRoutine0aOutcome::Removed(remove_enemy()));
    }
}
