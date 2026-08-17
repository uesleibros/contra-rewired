//! Native port of the "grenade launcher"/"seeking guy" enemy type's
//! ($17) own `_00`/`_01` table entries (`src/bank0.asm`, `$9468`/
//! `$9479`), plus [`set_enemy_var_2_to_closest_x_player`] (`$9516`), a
//! small shared helper both real entries use. The other 3 entries of
//! `grenade_launcher_routine_ptr_tbl` are the same shared routines every
//! indoor-family type reuses, already ported. `grenade_launcher_
//! routine_06` ("remove enemy, clear `GRENADE_LAUNCHER_FLAG`") is a
//! trivial one-line composition of [`crate::enemy::update_enemy_pos::
//! enemy_routine_remove_enemy`] - **not ported here** since it needs no
//! new logic of its own, just a caller wiring `GRENADE_LAUNCHER_FLAG`
//! back to `0` alongside that existing call.
//!
//! ## Two distinct "which segment is the player in" checks
//!
//! [`grenade_launcher_routine_01`] has two separate real branches that
//! each compare the grenade launcher's own horizontal segment (via
//! [`crate::enemy::find_far_segment::find_far_segment_for_a`]) against a
//! player's (via [`crate::enemy::find_far_segment::find_close_segment`]):
//! the `ENEMY_VAR_3 != 0` "cooldown" branch re-resolves the closest
//! player fresh every call (via [`set_enemy_var_2_to_closest_x_player`])
//! purely to decide whether to reverse direction; [`grenade_launcher_
//! apply_vel_aim`]'s own segment check instead reuses whatever `ENEMY_
//! VAR_2` already holds (last set by `grenade_launcher_routine_00` or a
//! prior cooldown-branch call) to decide the pause length and whether to
//! arm grenades - a real, deliberate difference in the ROM, not
//! something this port reconciles into one shared computation.
//!
//! ## The `ENEMY_VAR_1` grenade-count assignment quirk
//!
//! `grenade_launcher_apply_vel_aim`'s real tail computes `(ENEMY_
//! ATTRIBUTES >> 1) & 3` (the configured grenade count) into `a`, then
//! immediately `plp`s the *earlier* same-segment comparison's saved flags
//! back - so the `beq` that follows branches on the same-segment test,
//! not on whether the just-computed count is zero (the `and`'s own flags
//! are silently discarded, `plp` only restores status flags, never `a`).
//! Net effect: `ENEMY_VAR_1` (the number of grenades armed) becomes the
//! configured count *only* when the player is in the same segment as the
//! launcher, and `0` otherwise - ported directly as that real behavior,
//! not the more "obvious" reading of the instruction sequence in
//! isolation.

use crate::enemy::enemy_position_utils::reverse_enemy_x_direction;
use crate::enemy::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::find_far_segment::{find_close_segment, find_far_segment_for_a};
use crate::enemy::indoor_soldier::{
    apply_enemy_velocity_set_bg_priority, enemy_launch_grenade, init_indoor_enemy_pos_and_vel, init_sprite_from_frame,
    ApplyEnemyVelocityResult, CreatedIndoorEnemy, InitIndoorEnemyPosAndVelResult, InitSpriteFromFrameResult,
};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::quadrant_aim_dir::PLAYER_STATE_NORMAL;

/// Native port of `set_enemy_var_2_to_closest_x_player` (`$9516`) -
/// resolves the closer player via [`player_enemy_x_dist`], then swaps to
/// the *other* player if that one isn't in [`PLAYER_STATE_NORMAL`] (real
/// ASM: unconditional `eor #$01`, doesn't check the other player's state
/// either - if both are abnormal this still returns the swapped index).
pub fn set_enemy_var_2_to_closest_x_player(sprite_x_pos: [u8; 2], enemy_x_pos: u8, player_state: [u8; 2]) -> u8 {
    let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
    if player_state[closest.player_index as usize] == PLAYER_STATE_NORMAL {
        closest.player_index
    } else {
        closest.player_index ^ 0x01
    }
}

/// The full result of one [`grenade_launcher_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrenadeLauncherRoutine00Result {
    /// Always `1` - `GRENADE_LAUNCHER_FLAG` (prevents other enemies from
    /// being generated while a grenade launcher is on screen).
    pub grenade_launcher_flag: u8,
    pub var_2: u8,
    pub init: InitIndoorEnemyPosAndVelResult,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `grenade_launcher_routine_00` (`$9468`). Always
/// initializes with [`init_indoor_enemy_pos_and_vel`]'s logical index `3`
/// (real ASM: `ldy #$06`, a raw byte offset - `6/2=3`, the table's
/// "grenade launcher" row).
pub fn grenade_launcher_routine_00(
    enemy_attributes: u8,
    enemy_x_pos: u8,
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    current_routine: u8,
) -> GrenadeLauncherRoutine00Result {
    let var_2 = set_enemy_var_2_to_closest_x_player(sprite_x_pos, enemy_x_pos, player_state);
    let init = init_indoor_enemy_pos_and_vel(3, enemy_attributes);
    let delayed_routine = set_enemy_delay_adv_routine(0x20, current_routine);
    GrenadeLauncherRoutine00Result { grenade_launcher_flag: 1, var_2, init, delayed_routine }
}

/// [`grenade_launcher_apply_vel_aim`]'s real branch once it reaches the
/// segment/grenade-count check (`@cmp_player_enemy_segment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrenadeLauncherAimedResult {
    pub animation_delay: u8,
    /// Always `1` - real ASM's `inc ENEMY_VAR_3,x`, and this routine's
    /// only real caller only reaches it when `ENEMY_VAR_3` is already
    /// `0` (the `beq grenade_launcher_apply_vel_aim` branch condition in
    /// [`grenade_launcher_routine_01`]), so the increment's result is
    /// always exactly `1`, not a generic "+1".
    pub var_3: u8,
    /// Always `$04`.
    pub attack_delay: u8,
    /// See this module's doc comment for why this is `0` whenever the
    /// player isn't in the launcher's own segment, regardless of the
    /// configured grenade count.
    pub var_1: u8,
}

/// The real branch [`grenade_launcher_apply_vel_aim`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrenadeLauncherApplyVelAimOutcome {
    /// Velocity was applied but `ENEMY_ANIMATION_DELAY` (post-decrement)
    /// hasn't reached `0` yet - exits without touching the segment/
    /// grenade-count state at all.
    StillMoving { velocity: ApplyEnemyVelocityResult, animation_delay: u8 },
    /// Reached the segment/grenade-count check, either because the delay
    /// just hit `0` (`velocity` is `Some`) or the enemy was too far past
    /// either screen edge to move at all this call (`velocity` is
    /// `None` - real ASM never runs `apply_enemy_velocity_set_bg_
    /// priority` on that path).
    Aimed { velocity: Option<ApplyEnemyVelocityResult>, result: GrenadeLauncherAimedResult },
}

/// The full result of one [`grenade_launcher_apply_vel_aim`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrenadeLauncherApplyVelAimResult {
    pub sprite: InitSpriteFromFrameResult,
    pub outcome: GrenadeLauncherApplyVelAimOutcome,
}

/// Native port of `grenade_launcher_apply_vel_aim` (`$94c7`) - "apply
/// velocities, if animation timer elapsed, aim and set number of
/// grenades to fire". See this module's doc comment for the `ENEMY_
/// VAR_1` assignment quirk.
#[allow(clippy::too_many_arguments)]
pub fn grenade_launcher_apply_vel_aim(
    frame_counter: u8,
    enemy_frame: u8,
    enemy_sprite_attr: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    x_pos: u8,
    var_2: u8,
    enemy_animation_delay: u8,
    enemy_attributes: u8,
    sprite_x_pos: [u8; 2],
) -> GrenadeLauncherApplyVelAimResult {
    let sprite = init_sprite_from_frame(frame_counter, enemy_frame, enemy_sprite_attr, x_vel_fast);

    let moving_left = (x_vel_fast as i8) < 0;
    let off_edge = if moving_left { x_pos < 0x60 } else { x_pos >= 0xA0 };

    let outcome = if off_edge {
        aimed(None, var_2, enemy_attributes, x_pos, sprite_x_pos)
    } else {
        let velocity = apply_enemy_velocity_set_bg_priority(x_vel_accum, x_vel_fract, x_vel_fast, x_pos, sprite.sprite_attr);
        let animation_delay = enemy_animation_delay.wrapping_sub(1);
        if animation_delay != 0 {
            GrenadeLauncherApplyVelAimOutcome::StillMoving { velocity, animation_delay }
        } else {
            aimed(Some(velocity), var_2, enemy_attributes, x_pos, sprite_x_pos)
        }
    };

    GrenadeLauncherApplyVelAimResult { sprite, outcome }
}

fn aimed(
    velocity: Option<ApplyEnemyVelocityResult>,
    var_2: u8,
    enemy_attributes: u8,
    x_pos: u8,
    sprite_x_pos: [u8; 2],
) -> GrenadeLauncherApplyVelAimOutcome {
    let enemy_segment = find_far_segment_for_a(x_pos);
    let player_segment = find_close_segment(sprite_x_pos, var_2);
    let same_segment = player_segment == enemy_segment;

    let animation_delay = if same_segment { 0x38 } else { 0x18 };
    let num_grenades = (enemy_attributes >> 1) & 0x03;
    let var_1 = if same_segment { num_grenades } else { 0 };

    GrenadeLauncherApplyVelAimOutcome::Aimed {
        velocity,
        result: GrenadeLauncherAimedResult { animation_delay, var_3: 1, attack_delay: 0x04, var_1 },
    }
}

/// The real branch [`launch_grenade_if_appropriate`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchGrenadeOutcome {
    /// `ENEMY_VAR_1` was `0` - not armed to fire.
    NotReady,
    /// Armed, but `ENEMY_ATTACK_DELAY` hasn't elapsed yet.
    Waiting { attack_delay: u8 },
    /// Delay elapsed - fires a grenade.
    Launched { attack_delay: u8, var_1: u8, grenade: Option<CreatedIndoorEnemy> },
}

/// Native port of `launch_grenade_if_appropriate` (`$94b1`).
#[allow(clippy::too_many_arguments)]
pub fn launch_grenade_if_appropriate(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_var_1: u8,
    enemy_attack_delay: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
) -> LaunchGrenadeOutcome {
    if enemy_var_1 == 0 {
        return LaunchGrenadeOutcome::NotReady;
    }
    let attack_delay = enemy_attack_delay.wrapping_sub(1);
    if attack_delay != 0 {
        return LaunchGrenadeOutcome::Waiting { attack_delay };
    }
    let var_1 = enemy_var_1.wrapping_sub(1);
    let grenade = enemy_launch_grenade(prg_rom, enemy_routine, current_level, enemy_attack_flag, enemy_x_pos, enemy_y_pos);
    LaunchGrenadeOutcome::Launched { attack_delay: 0x14, var_1, grenade }
}

/// [`grenade_launcher_routine_01`]'s `ENEMY_VAR_3 != 0` "cooldown"
/// branch outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrenadeLauncherCooldownOutcome {
    /// `ENEMY_ANIMATION_DELAY` (post-decrement) hasn't reached `0` yet -
    /// checks whether it's time to actually launch a grenade.
    LaunchCheck { animation_delay: u8, launch: LaunchGrenadeOutcome },
    /// Delay reached `0` - resets the pause, re-aims toward the closest
    /// player, and reverses direction if not already facing them.
    Redirected {
        animation_delay: u8,
        var_3: u8,
        var_2: u8,
        /// `Some(new_x_velocity)` only if a direction reversal was
        /// needed.
        x_velocity: Option<(u8, u8)>,
    },
}

/// The real branch [`grenade_launcher_routine_01`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrenadeLauncherRoutine01Outcome {
    /// `ENEMY_VAR_3 == 0` - delegates entirely to [`grenade_launcher_
    /// apply_vel_aim`].
    ApplyVelAim(GrenadeLauncherApplyVelAimResult),
    /// `ENEMY_VAR_3 != 0` - the "pausing between grenade volleys" state.
    Cooldown { sprites: u8, outcome: GrenadeLauncherCooldownOutcome },
}

/// Native port of `grenade_launcher_routine_01` (`$9479`).
#[allow(clippy::too_many_arguments)]
pub fn grenade_launcher_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_var_3: u8,
    enemy_animation_delay: u8,
    enemy_attack_delay: u8,
    enemy_var_1: u8,
    enemy_attributes: u8,
    frame_counter: u8,
    enemy_frame: u8,
    enemy_sprite_attr: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    x_pos: u8,
    y_pos: u8,
    var_2: u8,
    player_state: [u8; 2],
    sprite_x_pos: [u8; 2],
) -> GrenadeLauncherRoutine01Outcome {
    if enemy_var_3 == 0 {
        let result = grenade_launcher_apply_vel_aim(
            frame_counter,
            enemy_frame,
            enemy_sprite_attr,
            x_vel_accum,
            x_vel_fract,
            x_vel_fast,
            x_pos,
            var_2,
            enemy_animation_delay,
            enemy_attributes,
            sprite_x_pos,
        );
        return GrenadeLauncherRoutine01Outcome::ApplyVelAim(result);
    }

    let animation_delay = enemy_animation_delay.wrapping_sub(1);
    let outcome = if animation_delay != 0 {
        let launch = launch_grenade_if_appropriate(prg_rom, enemy_routine, current_level, enemy_attack_flag, enemy_var_1, enemy_attack_delay, x_pos, y_pos);
        GrenadeLauncherCooldownOutcome::LaunchCheck { animation_delay, launch }
    } else {
        let enemy_segment = find_far_segment_for_a(x_pos);
        let new_var_2 = set_enemy_var_2_to_closest_x_player(sprite_x_pos, x_pos, player_state);
        let player_segment = find_close_segment(sprite_x_pos, new_var_2);
        let player_to_the_left = player_segment >= enemy_segment;
        let moving_left = (x_vel_fast as i8) < 0;
        let x_velocity = if moving_left != player_to_the_left { Some(reverse_enemy_x_direction(x_vel_fract, x_vel_fast)) } else { None };
        GrenadeLauncherCooldownOutcome::Redirected { animation_delay: 0x08, var_3: 0, var_2: new_var_2, x_velocity }
    };

    GrenadeLauncherRoutine01Outcome::Cooldown { sprites: 0x96, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enemy::indoor_soldier::ENEMY_TYPE_GRENADE;

    /// Shared property table (bullets, `ENEMY_TYPE=1 < $10`) plus level
    /// 0's per-level table with a real-shaped record for grenades
    /// (`$12`, `>= $10`) - same shape as `red_blue_soldier`'s own
    /// synthetic ROM fixture.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);

        let level0_table_addr: u16 = 0xF000;
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level0_table_addr.to_le_bytes());
        let level0_off = 7 * 0x4000 + (level0_table_addr as usize - 0xC000);
        rom[level0_off + ENEMY_TYPE_GRENADE as usize * 4..level0_off + ENEMY_TYPE_GRENADE as usize * 4 + 4]
            .copy_from_slice(&[0x10, 0x20, 0x0A, 0x30]);

        rom
    }

    #[test]
    fn set_var_2_picks_the_closer_normal_player() {
        let v = set_enemy_var_2_to_closest_x_player([0x50, 0x90], 0x55, [1, 1]);
        assert_eq!(v, 0);
    }

    #[test]
    fn set_var_2_swaps_to_the_other_player_when_the_closer_one_is_not_normal() {
        // player 0 is closer (dist 5) but not normal -> swap to player 1.
        let v = set_enemy_var_2_to_closest_x_player([0x55, 0x90], 0x50, [0, 1]);
        assert_eq!(v, 1);
    }

    #[test]
    fn routine_00_composes_var_2_init_index_3_and_delayed_advance() {
        let r = grenade_launcher_routine_00(0x00, 0x50, [0x55, 0x90], [1, 1], 5);
        assert_eq!(r.grenade_launcher_flag, 1);
        assert_eq!(r.var_2, set_enemy_var_2_to_closest_x_player([0x55, 0x90], 0x50, [1, 1]));
        assert_eq!(r.init, init_indoor_enemy_pos_and_vel(3, 0x00));
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x20, 5));
    }

    #[test]
    fn apply_vel_aim_skips_velocity_when_too_far_off_either_edge() {
        // moving left, x_pos < 0x60 -> off edge, no velocity applied.
        let r = grenade_launcher_apply_vel_aim(0, 0, 0x00, 0, 0, 0xFF, 0x50, 0, 0x05, 0x00, [0x50, 0x90]);
        match r.outcome {
            GrenadeLauncherApplyVelAimOutcome::Aimed { velocity: None, .. } => {}
            other => panic!("expected Aimed with no velocity, got {other:?}"),
        }
        // moving right, x_pos >= 0xa0 -> off edge too.
        let r2 = grenade_launcher_apply_vel_aim(0, 0, 0x00, 0, 0, 0x01, 0xA0, 0, 0x05, 0x00, [0x50, 0x90]);
        assert!(matches!(r2.outcome, GrenadeLauncherApplyVelAimOutcome::Aimed { velocity: None, .. }));
    }

    #[test]
    fn apply_vel_aim_still_moving_when_delay_has_not_elapsed() {
        let r = grenade_launcher_apply_vel_aim(0, 0, 0x00, 0, 0, 0x01, 0x70, 0, 0x05, 0x00, [0x50, 0x90]);
        match r.outcome {
            GrenadeLauncherApplyVelAimOutcome::StillMoving { animation_delay, .. } => assert_eq!(animation_delay, 0x04),
            other => panic!("expected StillMoving, got {other:?}"),
        }
    }

    #[test]
    fn apply_vel_aim_var_1_is_the_configured_count_only_when_same_segment() {
        // Force the enemy and player into the same close segment by using
        // an x_pos/var_2 pair that resolves to the same segment: pick
        // x_pos and sprite_x_pos so find_far_segment_for_a(x_pos) ==
        // find_close_segment(sprite_x_pos, 0).
        let x_pos = 0x30; // far segment 6 (< 0x6c)
        let sprite_x_pos = [0x30, 0x00]; // close segment also 6 for player 0 (< 0x44)
        assert_eq!(find_far_segment_for_a(x_pos), find_close_segment(sprite_x_pos, 0));

        // delay=1 so it reaches Aimed immediately without needing velocity math.
        let same = grenade_launcher_apply_vel_aim(0, 0, 0x00, 0, 0, 0x01, x_pos, 0, 0x01, 0b0000_0110, sprite_x_pos);
        match same.outcome {
            GrenadeLauncherApplyVelAimOutcome::Aimed { result, .. } => {
                assert_eq!(result.animation_delay, 0x38);
                assert_eq!(result.var_1, 0x03); // (attrs>>1)&3 = 3
            }
            other => panic!("expected Aimed, got {other:?}"),
        }

        // Different segment (player far from the enemy) -> var_1 forced to 0
        // regardless of the configured count.
        let different = grenade_launcher_apply_vel_aim(0, 0, 0x00, 0, 0, 0x01, x_pos, 0, 0x01, 0b0000_0110, [0xF0, 0x00]);
        match different.outcome {
            GrenadeLauncherApplyVelAimOutcome::Aimed { result, .. } => {
                assert_eq!(result.animation_delay, 0x18);
                assert_eq!(result.var_1, 0);
            }
            other => panic!("expected Aimed, got {other:?}"),
        }
    }

    #[test]
    fn launch_grenade_not_ready_when_var_1_is_zero() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = launch_grenade_if_appropriate(&rom, &routine, 0, 1, 0x00, 0x05, 0x50, 0x6D);
        assert_eq!(r, LaunchGrenadeOutcome::NotReady);
    }

    #[test]
    fn launch_grenade_waits_for_the_attack_delay() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = launch_grenade_if_appropriate(&rom, &routine, 0, 1, 0x02, 0x05, 0x50, 0x6D);
        assert_eq!(r, LaunchGrenadeOutcome::Waiting { attack_delay: 0x04 });
    }

    #[test]
    fn launch_grenade_fires_once_the_delay_elapses() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = launch_grenade_if_appropriate(&rom, &routine, 0, 1, 0x02, 0x01, 0x50, 0x6D);
        match r {
            LaunchGrenadeOutcome::Launched { attack_delay, var_1, grenade } => {
                assert_eq!(attack_delay, 0x14);
                assert_eq!(var_1, 0x01);
                assert!(grenade.is_some());
            }
            other => panic!("expected Launched, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_var_3_zero_delegates_to_apply_vel_aim() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = grenade_launcher_routine_01(&rom, &routine, 0, 1, 0x00, 0x05, 0x05, 0x00, 0x00, 0, 0, 0x00, 0, 0, 0x01, 0x70, 0x6D, 0, [1, 1], [0x50, 0x90]);
        assert!(matches!(r, GrenadeLauncherRoutine01Outcome::ApplyVelAim(_)));
    }

    #[test]
    fn routine_01_var_3_nonzero_and_delay_pending_checks_the_launch() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = grenade_launcher_routine_01(&rom, &routine, 0, 1, 0x01, 0x05, 0x05, 0x00, 0x00, 0, 0, 0x00, 0, 0, 0x01, 0x70, 0x6D, 0, [1, 1], [0x50, 0x90]);
        match r {
            GrenadeLauncherRoutine01Outcome::Cooldown { sprites, outcome: GrenadeLauncherCooldownOutcome::LaunchCheck { animation_delay, .. } } => {
                assert_eq!(sprites, 0x96);
                assert_eq!(animation_delay, 0x04);
            }
            other => panic!("expected Cooldown/LaunchCheck, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_var_3_nonzero_delay_elapsed_redirects_and_resets() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = grenade_launcher_routine_01(&rom, &routine, 0, 1, 0x01, 0x01, 0x05, 0x00, 0x00, 0, 0, 0x00, 0, 0, 0x01, 0x70, 0x6D, 0, [1, 1], [0x50, 0x90]);
        match r {
            GrenadeLauncherRoutine01Outcome::Cooldown { outcome: GrenadeLauncherCooldownOutcome::Redirected { animation_delay, var_3, .. }, .. } => {
                assert_eq!(animation_delay, 0x08);
                assert_eq!(var_3, 0);
            }
            other => panic!("expected Cooldown/Redirected, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_redirected_reverses_direction_only_when_facing_away_from_the_player() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // Enemy at 0x70, moving right (x_vel_fast positive); player far to the
        // left (sprite_x_pos low) - already facing the wrong way relative to
        // "player to the left", so this must NOT reverse (moving_left=false,
        // and we need player_to_the_left computed to match branch logic -
        // just assert the two calls disagree on reversal for opposite velocities).
        let moving_right = grenade_launcher_routine_01(&rom, &routine, 0, 1, 0x01, 0x01, 0x05, 0x00, 0x00, 0, 0, 0x00, 0, 0, 0x01, 0x70, 0x6D, 0, [1, 1], [0x10, 0x90]);
        let moving_left = grenade_launcher_routine_01(&rom, &routine, 0, 1, 0x01, 0x01, 0x05, 0x00, 0x00, 0, 0, 0x00, 0, 0, 0xFF, 0x70, 0x6D, 0, [1, 1], [0x10, 0x90]);
        let x_vel_of = |outcome: &GrenadeLauncherRoutine01Outcome| match outcome {
            GrenadeLauncherRoutine01Outcome::Cooldown { outcome: GrenadeLauncherCooldownOutcome::Redirected { x_velocity, .. }, .. } => *x_velocity,
            _ => panic!("expected Redirected"),
        };
        // Exactly one of the two directions should need a reversal (they can't both agree with "player to the left").
        assert_ne!(x_vel_of(&moving_right).is_some(), x_vel_of(&moving_left).is_some());
    }
}
