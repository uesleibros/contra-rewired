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
//! [`red_blue_soldier_set_bg_priority`]) - the red soldier's own routines,
//! [`red_soldier_routine_01`]/[`_02`], reuse these same two helpers too.

use crate::enemy::create_enemy_bullet::{aim_and_create_enemy_bullet, CreatedBullet};
use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_position_utils::add_10_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{
    advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate,
};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
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

/// The real, branchy result of one [`red_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedSoldierRoutine01Outcome {
    /// `ENEMY_VAR_2` already nonzero - already fired once, just keeps
    /// running off screen, no further checks this call.
    AlreadyFired,
    /// Outside the trigger X range, or too far from every player once
    /// inside it.
    StillRunning,
    /// Close enough to a player: commits to firing, advances to `red_
    /// soldier_routine_02`.
    Attack { var_1: u8, attack_delay: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`red_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedSoldierRoutine01Result {
    pub enemy_frame: u8,
    /// `ENEMY_SPRITES` - `$8f` (facing the player) on the `Attack`
    /// outcome, overriding the run-animation sprite that was already
    /// computed and stored earlier in the same call; the plain run
    /// sprite (`enemy_frame + $8c`) on every other outcome.
    pub sprites: u8,
    pub sprite_attr: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: RedSoldierRoutine01Outcome,
}

/// Native port of `red_soldier_routine_01` (`$a266`) - "run across
/// screen, once past trigger point, see if close to player, if so
/// advance routine to fire at player; if already fired from `red_
/// soldier_routine_02`, just continue running off screen". The real
/// minimum attack distance is itself picked from `ENEMY_ATTRIBUTES` bit
/// 1 (`$10` or `$30`), not a single fixed value.
#[allow(clippy::too_many_arguments)]
pub fn red_soldier_routine_01(
    enemy_frame: u8,
    frame_counter: u8,
    enemy_attributes: u8,
    enemy_var_2: u8,
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
) -> RedSoldierRoutine01Result {
    let new_frame = red_blue_soldier_set_run_frame(frame_counter, enemy_frame);
    let run_sprite = new_frame.wrapping_add(0x8C);
    let sprite_attr_base = if enemy_attributes & 0x01 == 0 { 0x46 } else { 0x06 };
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

    let outcome = if enemy_var_2 != 0 {
        RedSoldierRoutine01Outcome::AlreadyFired
    } else {
        let updated_x = position.x.pos;
        if updated_x >= 0xD8 || updated_x < 0x28 {
            RedSoldierRoutine01Outcome::StillRunning
        } else {
            let min_dist = if enemy_attributes & 0x02 == 0 { 0x10 } else { 0x30 };
            let closest = player_enemy_x_dist(sprite_x_pos, updated_x, player_state);
            if closest.distance >= min_dist {
                RedSoldierRoutine01Outcome::StillRunning
            } else {
                let routine_update = advance_enemy_routine(current_routine);
                RedSoldierRoutine01Outcome::Attack { var_1: 0x03, attack_delay: 0x10, routine_update }
            }
        }
    };

    let sprites = match outcome {
        RedSoldierRoutine01Outcome::Attack { .. } => 0x8F,
        _ => run_sprite,
    };

    RedSoldierRoutine01Result { enemy_frame: new_frame, sprites, sprite_attr, position, outcome }
}

/// The result of one [`red_soldier_routine_02`] call's `Fired` outcome -
/// a bullet spawn attempt via the already-verified [`aim_and_create_
/// enemy_bullet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedSoldierRoutine02FiredResult {
    pub sprites: u8,
    pub var_1: u8,
    pub attack_delay: u8,
    pub sprite_attr: u8,
    pub bullet: Option<CreatedBullet>,
}

/// The real, branchy result of one [`red_soldier_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedSoldierRoutine02Outcome {
    /// Attack delay hadn't elapsed. `sprite_attr` is `Some` only on the
    /// real, specific case the delay lands on exactly `$2c` (a fixed
    /// point partway through the recoil animation) - strips the recoil
    /// sprite-attribute bit.
    Waiting { attack_delay: u8, sprite_attr: Option<u8> },
    /// Delay elapsed and bullets remain: fires one.
    Fired(RedSoldierRoutine02FiredResult),
    /// Delay elapsed and `ENEMY_VAR_1` just went negative (all bullets
    /// fired): marks the soldier as having fired and returns to `red_
    /// soldier_routine_01` to keep running off screen.
    AllFired { var_2: u8, routine_update: EnemyRoutineUpdate },
}

/// Native port of `red_soldier_routine_02` (`$a2bb`) - "fire `ENEMY_
/// VAR_1` times and then go back to `red_soldier_routine_01`".
#[allow(clippy::too_many_arguments)]
pub fn red_soldier_routine_02(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_attack_delay: u8,
    enemy_var_1: u8,
    enemy_var_2: u8,
    enemy_sprite_attr: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_routine: u8,
) -> RedSoldierRoutine02Outcome {
    let delay = enemy_attack_delay.wrapping_sub(1);
    if delay != 0 {
        let sprite_attr = if delay == 0x2C { Some(enemy_sprite_attr & 0xF7) } else { None };
        return RedSoldierRoutine02Outcome::Waiting { attack_delay: delay, sprite_attr };
    }

    let sprites = 0x90;
    let var_1 = enemy_var_1.wrapping_sub(1);
    if (var_1 as i8) < 0 {
        let var_2 = enemy_var_2.wrapping_add(1);
        let routine_update = set_enemy_routine_to_a(current_routine, 0x02);
        return RedSoldierRoutine02Outcome::AllFired { var_2, routine_update };
    }

    let attack_delay = 0x30;
    let sprite_attr = enemy_sprite_attr | 0x08;
    let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
    let bullet = aim_and_create_enemy_bullet(
        prg_rom,
        enemy_routine,
        current_level,
        enemy_attack_flag,
        0x00,
        0x04,
        enemy_y_pos,
        enemy_x_pos,
        closest.player_index,
        0,
        0,
        player_state,
        sprite_y_pos,
        sprite_x_pos,
        level_location_type,
    );
    RedSoldierRoutine02Outcome::Fired(RedSoldierRoutine02FiredResult { sprites, var_1, attack_delay, sprite_attr, bullet })
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
    fn routine_01_already_fired_skips_every_other_check() {
        let r = red_soldier_routine_01(0x00, 0x00, 0x00, 0x01, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0, 0], [0, 0], 5);
        assert_eq!(r.outcome, RedSoldierRoutine01Outcome::AlreadyFired);
    }

    #[test]
    fn routine_01_still_running_outside_trigger_range() {
        let r = red_soldier_routine_01(0x00, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0xFF, 0x60, 0, 0, 0, [0, 0], [0, 0], 5);
        assert_eq!(r.outcome, RedSoldierRoutine01Outcome::StillRunning);
    }

    #[test]
    fn routine_01_attacks_when_close_enough_and_overrides_the_sprite() {
        // attrs bit1=0 -> min_dist=0x10; player at 0x55, enemy ends up at 0x50 -> dist=5 < 0x10.
        let r = red_soldier_routine_01(0x00, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x55, 0x00], [1, 0], 5);
        assert_eq!(r.sprites, 0x8F);
        match r.outcome {
            RedSoldierRoutine01Outcome::Attack { var_1, attack_delay, routine_update } => {
                assert_eq!(var_1, 0x03);
                assert_eq!(attack_delay, 0x10);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Attack, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_min_dist_widens_when_attribute_bit_1_is_set() {
        // player at distance 0x20: within 0x30 (bit1 set) but not within 0x10 (bit1 clear).
        let narrow = red_soldier_routine_01(0x00, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x70, 0x00], [1, 0], 5);
        assert_eq!(narrow.outcome, RedSoldierRoutine01Outcome::StillRunning);
        let wide = red_soldier_routine_01(0x00, 0x00, 0x02, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x70, 0x00], [1, 0], 5);
        assert!(matches!(wide.outcome, RedSoldierRoutine01Outcome::Attack { .. }));
    }

    #[test]
    fn routine_02_waits_and_strips_recoil_exactly_at_0x2c() {
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x2D, 0x02, 0x00, 0b0000_1000, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r, RedSoldierRoutine02Outcome::Waiting { attack_delay: 0x2C, sprite_attr: Some(0x00) });
    }

    #[test]
    fn routine_02_waits_without_touching_sprite_attr_otherwise() {
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x05, 0x02, 0x00, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r, RedSoldierRoutine02Outcome::Waiting { attack_delay: 0x04, sprite_attr: None });
    }

    #[test]
    fn routine_02_fires_a_bullet_when_bullets_remain() {
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[5] = 0; // free slot
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &routine, 0, 1, 0x01, 0x02, 0x00, 0x00, 0x50, 0x60, [1, 0], [0, 0], [0x60, 0], 0, 5);
        match r {
            RedSoldierRoutine02Outcome::Fired(f) => {
                assert_eq!(f.sprites, 0x90);
                assert_eq!(f.var_1, 0x01);
                assert_eq!(f.attack_delay, 0x30);
                assert_eq!(f.sprite_attr, 0x08);
                assert!(f.bullet.is_some());
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_all_fired_marks_var_2_and_returns_to_routine_01() {
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x01, 0x00, 0x00, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r, RedSoldierRoutine02Outcome::AllFired { var_2: 0x01, routine_update: set_enemy_routine_to_a(5, 0x02) });
    }
}
