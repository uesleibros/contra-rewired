//! Native port of the level-6 fire beam family's idle/ignition-wait
//! states (`src/bank0.asm`, `$a997`-`$aa9a`) - `fire_beam_down`/`_left`/
//! `_right` differ only in their own initial setup and ignition trigger
//! (proximity, synchronized timing, or plain re-delay), all funneling
//! into the same real `begin_fire_beam_attack`/`fire_beam_add_pos_set_
//! delay` shared tails. `_02`/`_03` (drawing/extending the beam itself)
//! are **not ported** - `draw_fire_beam_if_anim_elapsed` depends on the
//! unported PPU graphics-buffer subsystem, and `fire_beam_disable_
//! collision_routine_01` (only reachable from the unported `_03`) is
//! skipped along with it.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_collision_flags::enable_enemy_player_collision_check;
use crate::enemy::enemy_position_utils::add_a_to_enemy_y_pos;
use crate::enemy::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;

/// `fire_beam_anim_delay_tbl` (`$a9c8`, 4 bytes) - re-ignition delay,
/// indexed by `ENEMY_ATTRIBUTES` bits 2-3 (the flip bits `fire_beam_
/// add_pos_set_delay` just merged in change this index too - a real,
/// if easy-to-miss, coupling between the flip attributes and the timing
/// table).
const FIRE_BEAM_ANIM_DELAY_TBL: [u8; 4] = [0x00, 0x20, 0x40, 0x60];

/// The full result of one `fire_beam_add_pos_set_delay` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireBeamAddPosSetDelayResult {
    pub attributes: u8,
    pub y_pos: u8,
    pub var_a: u8,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `fire_beam_add_pos_set_delay` (`$a9b5`) - shared by
/// all 3 fire beam orientations' own `_00` entry.
fn fire_beam_add_pos_set_delay(merge_bits: u8, attributes: u8, y_pos: u8, current_routine: u8) -> FireBeamAddPosSetDelayResult {
    let attributes = merge_bits | attributes;
    let y_pos = add_a_to_enemy_y_pos(0x08, y_pos);
    let var_a = FIRE_BEAM_ANIM_DELAY_TBL[((attributes >> 2) & 0x03) as usize];
    FireBeamAddPosSetDelayResult { attributes, y_pos, var_a, delayed_routine: set_enemy_delay_adv_routine(var_a, current_routine) }
}

/// The full result of one [`fire_beam_down_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireBeamDownRoutine00Result {
    pub frame: u8,
    pub tail: FireBeamAddPosSetDelayResult,
}

/// Native port of `fire_beam_down_routine_00` (`$a997`).
pub fn fire_beam_down_routine_00(attributes: u8, y_pos: u8, current_routine: u8) -> FireBeamDownRoutine00Result {
    FireBeamDownRoutine00Result { frame: 0x04, tail: fire_beam_add_pos_set_delay(0x80, attributes, y_pos, current_routine) }
}

/// Native port of `fire_beam_left_routine_00` (`$aa55`).
pub fn fire_beam_left_routine_00(attributes: u8, y_pos: u8, current_routine: u8) -> FireBeamAddPosSetDelayResult {
    fire_beam_add_pos_set_delay(0x40, attributes, y_pos, current_routine)
}

/// The full result of one [`fire_beam_right_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireBeamRightRoutine00Result {
    pub sprite_attr: u8,
    pub tail: FireBeamAddPosSetDelayResult,
}

/// Native port of `fire_beam_right_routine_00` (`$aae0`).
pub fn fire_beam_right_routine_00(attributes: u8, y_pos: u8, current_routine: u8) -> FireBeamRightRoutine00Result {
    FireBeamRightRoutine00Result { sprite_attr: 0x40, tail: fire_beam_add_pos_set_delay(0x00, attributes, y_pos, current_routine) }
}

/// `fire_beam_not_firing_sprite_tbl` (`$aa1e`, 8 bytes, 2 rows of 4 -
/// `ENEMY_FRAME` picks the row: `$00` for down, `$04` for left/right).
const FIRE_BEAM_NOT_FIRING_SPRITE_TBL: [u8; 8] = [0x01, 0xBF, 0xC0, 0xBF, 0x01, 0xC1, 0xC2, 0xC1];

/// The full result of one `animate_small_flame` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimateSmallFlameResult {
    pub attack_delay: u8,
    /// `None` when not yet the 8th frame (real ASM's own early exit).
    pub sprite: Option<u8>,
}

/// Native port of `animate_small_flame` (`$aa0f`) - cycles the "not
/// firing yet" flicker sprite once every 8 frames.
fn animate_small_flame(attack_delay: u8, enemy_frame: u8) -> AnimateSmallFlameResult {
    let attack_delay = attack_delay.wrapping_sub(1);
    let sprite = if attack_delay & 0x07 != 0 {
        None
    } else {
        let idx = ((attack_delay >> 3) & 0x03) | enemy_frame;
        Some(FIRE_BEAM_NOT_FIRING_SPRITE_TBL[idx as usize])
    };
    AnimateSmallFlameResult { attack_delay, sprite }
}

/// `fire_beam_length_tbl` (`$aa32`, 4 bytes) - indexed by `ENEMY_
/// ATTRIBUTES` bits 0-1.
const FIRE_BEAM_LENGTH_TBL: [u8; 4] = [0x05, 0x09, 0x0D, 0x0F];

/// One `begin_fire_beam_attack` call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginFireBeamAttackOutcome {
    /// `ENEMY_X_POS < $30` - too far left on screen to fire yet.
    TooFarLeft,
    /// Ignites: plays the burn sound, enables player collision, picks
    /// this beam's own length, hides the sprite, and hands off to
    /// `_02` (not ported) with a `0` delay.
    Ignited { sound: u8, state_width: u8, var_2: u8, sprite: u8, var_1: u8, var_3: u8, var_4: u8, delayed_routine: DelayedRoutineUpdate },
}

/// Native port of `begin_fire_beam_attack` (`$aa26`).
fn begin_fire_beam_attack(x_pos: u8, attributes: u8, state_width: u8, current_routine: u8) -> BeginFireBeamAttackOutcome {
    if x_pos < 0x30 {
        BeginFireBeamAttackOutcome::TooFarLeft
    } else {
        BeginFireBeamAttackOutcome::Ignited {
            sound: 0x09,
            state_width: enable_enemy_player_collision_check(state_width),
            var_2: FIRE_BEAM_LENGTH_TBL[(attributes & 0x03) as usize],
            sprite: 0x01,
            var_1: 0x00,
            var_3: 0x00,
            var_4: 0x00,
            delayed_routine: set_enemy_delay_adv_routine(0x00, current_routine),
        }
    }
}

/// One [`fire_beam_down_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireBeamDownRoutine01Outcome {
    Waiting { animation_delay: u8 },
    /// Delay elapsed, but no player within `$20` pixels yet.
    TooFar,
    Attack(BeginFireBeamAttackOutcome),
}

/// The full result of one [`fire_beam_down_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireBeamDownRoutine01Result {
    pub flame: AnimateSmallFlameResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: FireBeamDownRoutine01Outcome,
}

/// Native port of `fire_beam_down_routine_01` (`$a9c0`) - ignites once
/// the delay has elapsed *and* a player has scrolled within `$20`
/// pixels.
#[allow(clippy::too_many_arguments)]
pub fn fire_beam_down_routine_01(
    attack_delay: u8,
    enemy_frame: u8,
    animation_delay: u8,
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    attributes: u8,
    state_width: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> FireBeamDownRoutine01Result {
    let flame = animate_small_flame(attack_delay, enemy_frame);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let outcome = if animation_delay != 0 {
        FireBeamDownRoutine01Outcome::Waiting { animation_delay: animation_delay.wrapping_sub(1) }
    } else {
        let closest = player_enemy_x_dist(sprite_x_pos, scroll.x_pos, player_state);
        if closest.distance >= 0x20 {
            FireBeamDownRoutine01Outcome::TooFar
        } else {
            FireBeamDownRoutine01Outcome::Attack(begin_fire_beam_attack(scroll.x_pos, attributes, state_width, current_routine))
        }
    };

    FireBeamDownRoutine01Result { flame, scroll, outcome }
}

/// One [`fire_beam_left_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireBeamLeftRoutine01Outcome {
    /// `FRAME_COUNTER & $7f` hasn't reached this beam's own trigger
    /// value (`ENEMY_VAR_A`) yet - real ASM comment elsewhere in this
    /// family: synchronizing same-delay beams to ignite together.
    Waiting,
    Attack(BeginFireBeamAttackOutcome),
}

/// The full result of one [`fire_beam_left_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireBeamLeftRoutine01Result {
    pub flame: AnimateSmallFlameResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: FireBeamLeftRoutine01Outcome,
}

/// Native port of `fire_beam_left_routine_01` (`$aa5a`) - unlike `down`
/// (proximity-triggered) or `right` (plain re-delay), `left` ignites on
/// a synchronized frame-counter match, no player-distance check at all.
#[allow(clippy::too_many_arguments)]
pub fn fire_beam_left_routine_01(
    attack_delay: u8,
    enemy_frame: u8,
    frame_counter: u8,
    var_a: u8,
    attributes: u8,
    state_width: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> FireBeamLeftRoutine01Result {
    let flame = animate_small_flame(attack_delay, enemy_frame);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let outcome = if (frame_counter & 0x7F) != var_a {
        FireBeamLeftRoutine01Outcome::Waiting
    } else {
        FireBeamLeftRoutine01Outcome::Attack(begin_fire_beam_attack(scroll.x_pos, attributes, state_width, current_routine))
    };

    FireBeamLeftRoutine01Result { flame, scroll, outcome }
}

/// One [`fire_beam_right_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FireBeamRightRoutine01Outcome {
    Waiting { animation_delay: u8 },
    /// Delay elapsed - picks a new random re-ignition delay (`ENEMY_
    /// VAR_A`, `$00`-`$3f`) for *next* time, then attacks immediately
    /// (no player-distance or frame-sync gate at all).
    Attack { var_a: u8, outcome: BeginFireBeamAttackOutcome },
}

/// The full result of one [`fire_beam_right_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FireBeamRightRoutine01Result {
    pub flame: AnimateSmallFlameResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: FireBeamRightRoutine01Outcome,
}

/// Native port of `fire_beam_right_routine_01` (`$aae9`).
#[allow(clippy::too_many_arguments)]
pub fn fire_beam_right_routine_01(
    attack_delay: u8,
    enemy_frame: u8,
    animation_delay: u8,
    random_num: u8,
    attributes: u8,
    state_width: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> FireBeamRightRoutine01Result {
    let flame = animate_small_flame(attack_delay, enemy_frame);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        FireBeamRightRoutine01Outcome::Waiting { animation_delay: delay }
    } else {
        FireBeamRightRoutine01Outcome::Attack { var_a: random_num & 0x3F, outcome: begin_fire_beam_attack(scroll.x_pos, attributes, state_width, current_routine) }
    };

    FireBeamRightRoutine01Result { flame, scroll, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_routine_00_sets_frame_4_and_merges_the_down_flip_bit() {
        let r = fire_beam_down_routine_00(0b0000_0001, 0x50, 3);
        assert_eq!(r.frame, 0x04);
        assert_eq!(r.tail.attributes, 0b1000_0001);
        assert_eq!(r.tail.y_pos, 0x58);
    }

    #[test]
    fn right_routine_00_sets_sprite_attr_and_no_flip_bit() {
        let r = fire_beam_right_routine_00(0b0000_0010, 0x50, 3);
        assert_eq!(r.sprite_attr, 0x40);
        assert_eq!(r.tail.attributes, 0b0000_0010);
    }

    #[test]
    fn add_pos_set_delay_picks_timing_from_flip_bits() {
        // attrs bits 2-3 = 0b10 (idx 2) -> delay 0x40
        let r = fire_beam_add_pos_set_delay(0x00, 0b0000_1000, 0x50, 3);
        assert_eq!(r.var_a, 0x40);
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x40, 3));
    }

    #[test]
    fn animate_small_flame_only_updates_sprite_on_the_8th_frame() {
        let no_update = animate_small_flame(0x05, 0x00);
        assert_eq!(no_update.sprite, None);
        let update = animate_small_flame(0x01, 0x00);
        assert!(update.sprite.is_some());
    }

    #[test]
    fn begin_attack_rejects_too_far_left() {
        assert_eq!(begin_fire_beam_attack(0x20, 0x00, 0x00, 3), BeginFireBeamAttackOutcome::TooFarLeft);
    }

    #[test]
    fn begin_attack_ignites_and_picks_length_from_attributes() {
        let outcome = begin_fire_beam_attack(0x50, 0x02, 0x00, 3);
        match outcome {
            BeginFireBeamAttackOutcome::Ignited { var_2, sound, delayed_routine, .. } => {
                assert_eq!(var_2, FIRE_BEAM_LENGTH_TBL[2]);
                assert_eq!(sound, 0x09);
                assert_eq!(delayed_routine, set_enemy_delay_adv_routine(0x00, 3));
            }
            other => panic!("expected Ignited, got {other:?}"),
        }
    }

    #[test]
    fn down_routine_01_waits_then_checks_proximity_once_delay_elapses() {
        let waiting = fire_beam_down_routine_01(0x08, 0x00, 0x05, [0, 0], [1, 1], 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(waiting.outcome, FireBeamDownRoutine01Outcome::Waiting { animation_delay: 0x04 });

        let too_far = fire_beam_down_routine_01(0x08, 0x00, 0x00, [0x00, 0x00], [1, 1], 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(too_far.outcome, FireBeamDownRoutine01Outcome::TooFar);

        let close = fire_beam_down_routine_01(0x08, 0x00, 0x00, [0x55, 0x00], [1, 0], 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert!(matches!(close.outcome, FireBeamDownRoutine01Outcome::Attack(_)));
    }

    #[test]
    fn left_routine_01_ignores_player_distance_and_only_checks_frame_sync() {
        let waiting = fire_beam_left_routine_01(0x08, 0x00, 0x10, 0x20, 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(waiting.outcome, FireBeamLeftRoutine01Outcome::Waiting);

        let attack = fire_beam_left_routine_01(0x08, 0x00, 0x20, 0x20, 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert!(matches!(attack.outcome, FireBeamLeftRoutine01Outcome::Attack(_)));
    }

    #[test]
    fn right_routine_01_picks_a_new_random_delay_and_attacks_immediately() {
        let r = fire_beam_right_routine_01(0x08, 0x00, 0x01, 0xFF, 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        match r.outcome {
            FireBeamRightRoutine01Outcome::Attack { var_a, outcome } => {
                assert_eq!(var_a, 0xFF & 0x3F);
                assert!(matches!(outcome, BeginFireBeamAttackOutcome::Ignited { .. }));
            }
            other => panic!("expected Attack, got {other:?}"),
        }
    }

    #[test]
    fn right_routine_01_waits_while_delay_has_not_elapsed() {
        let r = fire_beam_right_routine_01(0x08, 0x00, 0x05, 0xFF, 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, FireBeamRightRoutine01Outcome::Waiting { animation_delay: 0x04 });
    }
}
