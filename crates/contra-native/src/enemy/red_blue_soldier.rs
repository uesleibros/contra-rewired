//! Native port of the red/blue soldier enemy family's shared spawn-init
//! state, `red_blue_soldier_routine_00` (`src/bank0.asm`, CPU `$a157`-
//! `$a17d`) - entry 0 of both `blue_soldier_routine_ptr_tbl` and `red_
//! soldier_routine_ptr_tbl` (a distinct enemy type from the plain
//! soldier in [`crate::enemy::soldier`], though both families share the
//! generic explosion/removal routines, [`crate::enemy::enemy_explosion`]).
//! Places the enemy at one of 4 fixed screen corners and gives it an
//! initial horizontal running velocity, both picked from `ENEMY_
//! ATTRIBUTES`, then advances to the next routine.
//!
//! Also carries the blue soldier's own 3 routines beyond that shared
//! init state ([`blue_soldier_routine_01`]/[`_02`]/[`_03`]: run across
//! the screen, jump-attack windup, then fall) and the 2 small helpers
//! both blue *and* red soldiers share ([`red_blue_soldier_set_run_frame`]/
//! [`red_blue_soldier_set_bg_priority`]) - the red soldier's own routines
//! (`red_soldier_routine_01`/`02`, not yet ported) reuse these same two
//! helpers too.

use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_position_utils::add_10_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};

/// `red_blue_soldier_init_pos_tbl` (`$a17e`, 8 bytes) - `(y_pos, x_pos)`
/// per spawn corner, indexed by `ENEMY_ATTRIBUTES` (real ASM doesn't mask
/// this before indexing - every real spawn placement for this enemy type
/// uses attributes `0`-`3` exactly, so this port masks defensively
/// rather than replicating an out-of-range table read no real caller can
/// trigger, the same reasoning `soldier::soldier_set_x_velocity` already
/// documents for an analogous case).
const RED_BLUE_SOLDIER_INIT_POS_TBL: [(u8, u8); 4] = [
    (0x9C, 0xF0), // lower right - negative (leftward) X velocity
    (0x9C, 0x10), // lower left - positive (rightward) X velocity
    (0x61, 0xF0), // upper right
    (0x61, 0x10), // upper left
];

/// `red_blue_soldier_init_vel_tbl` (`$a186`, 4 bytes) - `(x_vel_fract,
/// x_vel_fast)`, indexed by `ENEMY_ATTRIBUTES & 1` (real ASM does mask
/// this one explicitly).
const RED_BLUE_SOLDIER_INIT_VEL_TBL: [(u8, u8); 2] = [
    (0x00, 0xFF), // running from the left side of the screen
    (0x00, 0x01), // running from the right side
];

/// The full result of one [`red_blue_soldier_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedBlueSoldierRoutine00Result {
    pub y_pos: u8,
    pub x_pos: u8,
    pub x_velocity: (u8, u8),
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `red_blue_soldier_routine_00` (`$a157`).
pub fn red_blue_soldier_routine_00(enemy_attributes: u8, current_routine: u8) -> RedBlueSoldierRoutine00Result {
    let (y_pos, x_pos) = RED_BLUE_SOLDIER_INIT_POS_TBL[(enemy_attributes & 0x03) as usize];
    let x_velocity = RED_BLUE_SOLDIER_INIT_VEL_TBL[(enemy_attributes & 0x01) as usize];
    let routine_update = advance_enemy_routine(current_routine);
    RedBlueSoldierRoutine00Result { y_pos, x_pos, x_velocity, routine_update }
}

/// Native port of `red_blue_soldier_set_run_frame` (`$a1c5`) - cycles
/// `ENEMY_FRAME` through `0..3` once every 4th frame (`FRAME_COUNTER &
/// 3 == 0`), used by both blue and red soldiers' own "running" states
/// for their run-cycle animation.
pub fn red_blue_soldier_set_run_frame(frame_counter: u8, enemy_frame: u8) -> u8 {
    if frame_counter & 0x03 != 0 {
        return enemy_frame;
    }
    let new_frame = enemy_frame.wrapping_add(1);
    if new_frame < 0x03 {
        new_frame
    } else {
        0x00
    }
}

/// Native port of `red_blue_soldier_set_bg_priority` (`$a1db`) - forces
/// background priority (sprite drawn *behind* background tiles) whenever
/// the soldier is near either screen edge, where it'd otherwise draw
/// over the level's pillar/wall decorations it's meant to be behind;
/// clear in the middle of the screen. Real ASM's own branch comments are
/// misleading here (mislabeled "not behind pillar" on the branch that's
/// actually taken for the *behind-pillar* edges) - this port follows the
/// real control flow, not the comment text.
pub fn red_blue_soldier_set_bg_priority(enemy_x_pos: u8, enemy_sprite_attr: u8) -> u8 {
    let behind_pillar_flag = if enemy_x_pos >= 0xDC || enemy_x_pos < 0x24 { 0x20 } else { 0x00 };
    (enemy_sprite_attr & 0xDF) | behind_pillar_flag
}

/// The real, branchy result of one [`blue_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueSoldierRoutine01Outcome {
    /// Still outside the attack-trigger X range, or not yet close enough
    /// to a player within it.
    StillRunning,
    /// Close enough to a player: `ENEMY_FRAME` resets to `0`, advances to
    /// `blue_soldier_routine_02` after 1 frame.
    CloseToPlayer(DelayedRoutineUpdate),
}

/// The full result of one [`blue_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueSoldierRoutine01Result {
    /// Final `ENEMY_FRAME` - the freshly cycled run-frame normally, or
    /// `0` if [`BlueSoldierRoutine01Outcome::CloseToPlayer`] overrides it
    /// (a real, later `sta` that runs *after* `ENEMY_SPRITES` was already
    /// computed from the pre-override frame - see `sprites` below).
    pub enemy_frame: u8,
    /// `ENEMY_SPRITES` - always computed from the *pre-override* run
    /// frame (`+ $85`), even on the `CloseToPlayer` path.
    pub sprites: u8,
    pub sprite_attr: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: BlueSoldierRoutine01Outcome,
}

/// Native port of `blue_soldier_routine_01` (`$a18a`) - "run across
/// screen, once past trigger point, see if close to player, if so
/// advance routine to jump down": cycles the run animation, updates
/// position/scroll, and once inside the real `[$28, $d8)` X range,
/// checks real proximity to a player before deciding to start the
/// jump-attack.
#[allow(clippy::too_many_arguments)]
pub fn blue_soldier_routine_01(
    enemy_frame: u8,
    frame_counter: u8,
    enemy_attributes: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    enemy_y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    current_routine: u8,
) -> BlueSoldierRoutine01Result {
    let new_frame = red_blue_soldier_set_run_frame(frame_counter, enemy_frame);
    let sprites = new_frame.wrapping_add(0x85);
    let sprite_attr_base = if enemy_attributes & 0x01 == 0 { 0x47 } else { 0x07 };
    let sprite_attr = red_blue_soldier_set_bg_priority(enemy_x_pos, sprite_attr_base);

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

    let updated_x_pos = position.x.pos;
    let outcome = if updated_x_pos >= 0xD8 || updated_x_pos < 0x28 {
        BlueSoldierRoutine01Outcome::StillRunning
    } else {
        let closest = player_enemy_x_dist(sprite_x_pos, updated_x_pos, player_state);
        if closest.distance >= 0x10 {
            BlueSoldierRoutine01Outcome::StillRunning
        } else {
            BlueSoldierRoutine01Outcome::CloseToPlayer(set_enemy_delay_adv_routine(0x01, current_routine))
        }
    };

    let enemy_frame = match outcome {
        BlueSoldierRoutine01Outcome::CloseToPlayer(_) => 0x00,
        BlueSoldierRoutine01Outcome::StillRunning => new_frame,
    };

    BlueSoldierRoutine01Result { enemy_frame, sprites, sprite_attr, position, outcome }
}

/// `blue_soldier_jmp_x_vel_tbl` (`$a241`, 4 bytes) - `(x_vel_fract,
/// x_vel_fast)` per direction, indexed by `ENEMY_ATTRIBUTES & 1`.
const BLUE_SOLDIER_JMP_X_VEL_TBL: [(u8, u8); 2] = [
    (0xC0, 0xFF), // coming from the left
    (0x40, 0x00), // coming from the right
];

/// The real, branchy result of one [`blue_soldier_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueSoldierRoutine02Outcome {
    /// Attack windup delay hadn't elapsed.
    Waiting { animation_delay: u8 },
    /// Delay elapsed, windup animation isn't done: sprite advances,
    /// delay resets to `$08`.
    Animating { enemy_frame: u8, sprites: u8, animation_delay: u8 },
    /// Windup finished: forces foreground draw priority, enables
    /// collision, sets the jump velocity from `ENEMY_ATTRIBUTES`'
    /// direction bit, and advances to `blue_soldier_routine_03` after
    /// `$10` frames.
    JumpStart {
        sprites: u8,
        sprite_attr: u8,
        state_width: u8,
        x_velocity: (u8, u8),
        y_velocity: (u8, u8),
        delayed_routine: DelayedRoutineUpdate,
    },
}

/// Native port of `blue_soldier_routine_02` (`$a1f7`) - "go through jump
/// animation routine, then initialize jump velocities and advance
/// routine".
pub fn blue_soldier_routine_02(
    enemy_animation_delay: u8,
    enemy_frame: u8,
    enemy_attributes: u8,
    enemy_sprite_attr: u8,
    enemy_state_width: u8,
    current_routine: u8,
) -> BlueSoldierRoutine02Outcome {
    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return BlueSoldierRoutine02Outcome::Waiting { animation_delay: delay };
    }

    let sprites = enemy_frame.wrapping_add(0x88);
    let new_frame = enemy_frame.wrapping_add(1);
    if new_frame < 0x03 {
        return BlueSoldierRoutine02Outcome::Animating { enemy_frame: new_frame, sprites, animation_delay: 0x08 };
    }

    let sprite_attr = enemy_sprite_attr & 0xDF;
    let state_width = enable_enemy_collision(enemy_state_width);
    let x_velocity = BLUE_SOLDIER_JMP_X_VEL_TBL[(enemy_attributes & 0x01) as usize];
    let y_velocity = (0x00, 0xFF);
    let delayed_routine = set_enemy_delay_adv_routine(0x10, current_routine);
    BlueSoldierRoutine02Outcome::JumpStart { sprites, sprite_attr, state_width, x_velocity, y_velocity, delayed_routine }
}

/// The full result of one [`blue_soldier_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueSoldierRoutine03Result {
    pub sprites: u8,
    /// `0` if `ENEMY_ANIMATION_DELAY` was already `0` (real ASM doesn't
    /// decrement past zero here - it just keeps using `sprite_8b` and
    /// leaves the counter alone); otherwise decremented by one.
    pub animation_delay: u8,
    pub y_velocity: (u8, u8),
    pub position: UpdatedEnemyPos,
}

/// Native port of `blue_soldier_routine_03` (`$a245`) - "animate jumping
/// down frames based on time since jump, apply velocity": falls under
/// gravity, showing one of two sprites depending on whether the windup
/// delay (reused here as a brief falling-animation timer) has run out.
#[allow(clippy::too_many_arguments)]
pub fn blue_soldier_routine_03(
    enemy_animation_delay: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    enemy_y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> BlueSoldierRoutine03Result {
    let (sprites, animation_delay) =
        if enemy_animation_delay == 0 { (0x8B, 0x00) } else { (0x8A, enemy_animation_delay.wrapping_sub(1)) };
    let y_velocity = add_10_to_enemy_y_fract_vel(y_vel_fract, y_vel_fast);
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
    BlueSoldierRoutine03Result { sprites, animation_delay, y_velocity, position }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_attribute_value_selects_the_right_corner_and_direction() {
        let cases = [
            (0x00u8, (0x9C, 0xF0), (0x00, 0xFF)),
            (0x01, (0x9C, 0x10), (0x00, 0x01)),
            (0x02, (0x61, 0xF0), (0x00, 0xFF)),
            (0x03, (0x61, 0x10), (0x00, 0x01)),
        ];
        for (attrs, (y, x), vel) in cases {
            let r = red_blue_soldier_routine_00(attrs, 5);
            assert_eq!((r.y_pos, r.x_pos), (y, x), "attrs={attrs:02X}");
            assert_eq!(r.x_velocity, vel, "attrs={attrs:02X}");
        }
    }

    #[test]
    fn advances_the_routine_guarded_the_usual_way() {
        let r = red_blue_soldier_routine_00(0x00, 5);
        assert_eq!(r.routine_update, advance_enemy_routine(5));

        let guarded = red_blue_soldier_routine_00(0x00, 0);
        assert_eq!(guarded.routine_update, advance_enemy_routine(0));
        assert_eq!(guarded.routine_update.routine, 0);
        assert_eq!(guarded.routine_update.sprites, Some(0));
    }

    #[test]
    fn set_run_frame_only_advances_on_the_4th_frame() {
        assert_eq!(red_blue_soldier_set_run_frame(0x01, 0x01), 0x01); // not 4th frame, unchanged
        assert_eq!(red_blue_soldier_set_run_frame(0x04, 0x01), 0x02); // 4th frame, increments
    }

    #[test]
    fn set_run_frame_wraps_at_3() {
        assert_eq!(red_blue_soldier_set_run_frame(0x00, 0x02), 0x00);
    }

    #[test]
    fn set_bg_priority_forces_background_near_either_edge() {
        assert_eq!(red_blue_soldier_set_bg_priority(0x10, 0x00) & 0x20, 0x20); // left edge
        assert_eq!(red_blue_soldier_set_bg_priority(0xE0, 0x00) & 0x20, 0x20); // right edge
        assert_eq!(red_blue_soldier_set_bg_priority(0x50, 0x00) & 0x20, 0x00); // middle
    }

    #[test]
    fn set_bg_priority_preserves_bits_outside_the_priority_flag() {
        let r = red_blue_soldier_set_bg_priority(0x50, 0b0011_0111);
        assert_eq!(r, 0b0001_0111); // bit 5 stripped, no priority flag added (middle of screen)
    }

    #[test]
    fn routine_01_still_running_when_outside_the_trigger_x_range() {
        // x=0x50, running left (fract=0,fast=0xff -> -1/frame): stays > 0x28.
        let r = blue_soldier_routine_01(0x00, 0x04, 0x00, 0, 0x00, 0x50, 0, 0, 0xFF, 0x60, 0, 0, 0, [0, 0], [0, 0], 5);
        assert_eq!(r.outcome, BlueSoldierRoutine01Outcome::StillRunning);
        assert_eq!(r.enemy_frame, red_blue_soldier_set_run_frame(0x04, 0x00));
    }

    #[test]
    fn routine_01_still_running_when_in_range_but_far_from_every_player() {
        let r = blue_soldier_routine_01(0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x00, 0x00], [1, 1], 5);
        assert_eq!(r.outcome, BlueSoldierRoutine01Outcome::StillRunning);
    }

    #[test]
    fn routine_01_close_to_player_resets_frame_and_advances() {
        let r = blue_soldier_routine_01(0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x55, 0x00], [1, 0], 5);
        match r.outcome {
            BlueSoldierRoutine01Outcome::CloseToPlayer(delayed) => {
                assert_eq!(delayed, set_enemy_delay_adv_routine(0x01, 5));
                assert_eq!(r.enemy_frame, 0x00);
            }
            other => panic!("expected CloseToPlayer, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_sprites_always_use_the_pre_override_frame() {
        let r = blue_soldier_routine_01(0x02, 0x04, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x55, 0x00], [1, 0], 5);
        let new_frame = red_blue_soldier_set_run_frame(0x04, 0x02);
        assert_eq!(r.sprites, new_frame.wrapping_add(0x85));
    }

    #[test]
    fn routine_02_waits_when_delay_has_not_elapsed() {
        let r = blue_soldier_routine_02(0x03, 0x00, 0x00, 0x00, 0x00, 5);
        assert_eq!(r, BlueSoldierRoutine02Outcome::Waiting { animation_delay: 0x02 });
    }

    #[test]
    fn routine_02_animates_when_windup_is_not_done() {
        let r = blue_soldier_routine_02(0x01, 0x00, 0x00, 0x00, 0x00, 5);
        assert_eq!(r, BlueSoldierRoutine02Outcome::Animating { enemy_frame: 0x01, sprites: 0x88, animation_delay: 0x08 });
    }

    #[test]
    fn routine_02_starts_the_jump_once_windup_finishes() {
        let r = blue_soldier_routine_02(0x01, 0x02, 0x00, 0b0010_0000, 0x00, 5);
        match r {
            BlueSoldierRoutine02Outcome::JumpStart { sprites, sprite_attr, state_width, x_velocity, y_velocity, delayed_routine } => {
                assert_eq!(sprites, 0x8A);
                assert_eq!(sprite_attr, 0x00); // bg priority bit stripped
                assert_eq!(state_width, enable_enemy_collision(0x00));
                assert_eq!(x_velocity, BLUE_SOLDIER_JMP_X_VEL_TBL[0]);
                assert_eq!(y_velocity, (0x00, 0xFF));
                assert_eq!(delayed_routine, set_enemy_delay_adv_routine(0x10, 5));
            }
            other => panic!("expected JumpStart, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_uses_8b_and_leaves_delay_alone_when_already_zero() {
        let r = blue_soldier_routine_03(0x00, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0x00, 0x10);
        assert_eq!(r.sprites, 0x8B);
        assert_eq!(r.animation_delay, 0x00);
        assert_eq!(r.y_velocity, add_10_to_enemy_y_fract_vel(0x00, 0x10));
    }

    #[test]
    fn routine_03_uses_8a_and_decrements_when_delay_is_nonzero() {
        let r = blue_soldier_routine_03(0x05, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0x00, 0x10);
        assert_eq!(r.sprites, 0x8A);
        assert_eq!(r.animation_delay, 0x04);
    }
}
