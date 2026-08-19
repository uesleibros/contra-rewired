//! Native port of the level-7 mechanical claw's `_00`/`_01` routine
//! (`src/bank0.asm`, `$aec3`-`$af45`) - the "wait, then decide to
//! descend" half of the claw's state machine. `claw_routine_02`/`_03`
//! (the actual descend/ascend animation) are **not ported here** - both
//! depend on `load_bank_3_update_nametable_tiles`, the unported PPU
//! graphics-buffer subsystem.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;

/// `claw_frame_trigger_tbl` (`$aee7`, 4 bytes) - which `FRAME_COUNTER &
/// $7f` value (of 4 possible, picked from `ENEMY_ATTRIBUTES` bits 2-3)
/// triggers this claw's own descent, so claws sharing a delay group
/// descend in sync.
const CLAW_FRAME_TRIGGER_TBL: [u8; 4] = [0x00, 0x20, 0x40, 0x60];

/// The full result of one [`claw_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClawRoutine00Result {
    pub scroll: ScrolledEnemyPos,
    pub frame: u8,
    /// `ENEMY_ATTRIBUTES` after real ASM overwrites it with just the
    /// claw-length code (bits 0-1) - the descend-delay bits (2-3) are
    /// consumed once here (into `frame`) and never needed again.
    pub attributes: u8,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `claw_routine_00` (`$aec3`).
pub fn claw_routine_00(attributes: u8, level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> ClawRoutine00Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let frame = CLAW_FRAME_TRIGGER_TBL[((attributes >> 2) & 0x03) as usize];
    let attributes = attributes & 0x03;
    ClawRoutine00Result { scroll, frame, attributes, delayed_routine: set_enemy_delay_adv_routine(0x20, current_routine) }
}

/// `claw_length_tbl` (`$af0d`, 4 bytes) - real ASM comment: "length code
/// 3 makes the claw activate only when the player is near" (the
/// "seeking claw" variant).
const CLAW_LENGTH_TBL: [u8; 4] = [0x04, 0x03, 0x08, 0x03];

/// One [`claw_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClawRoutine01Outcome {
    /// Not descending this call - covers all 4 real "no, not yet" exits
    /// (non-seeking frame mismatch, too far left, seeking's random 25%
    /// skip window, seeking player too far) - none of them touch
    /// anything beyond the unconditional scroll.
    Waiting,
    /// Seeking claw (`ENEMY_ATTRIBUTES == 3`) with its own attack-delay
    /// timer still running - just counts it down.
    SeekingDelayCountdown { animation_delay: u8 },
    /// Descending - sets up the claw's own extension state and hands
    /// off to `claw_routine_02` (not ported).
    Descending { var_2: u8, var_3: u8, var_4: u8, delayed_routine: DelayedRoutineUpdate },
}

/// The full result of one [`claw_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClawRoutine01Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: ClawRoutine01Outcome,
}

/// Native port of `claw_routine_01` (`$aee5`) - both the non-seeking
/// "frame counter matches my trigger" path and the seeking "player got
/// close" path converge on the *same* `< $2c` (17% of screen) left-edge
/// check before actually descending - ported as one shared `should_
/// descend` gate rather than duplicating that check per path, to keep
/// the real fall-through visible.
#[allow(clippy::too_many_arguments)]
pub fn claw_routine_01(
    attributes: u8,
    animation_delay: u8,
    frame_counter: u8,
    enemy_frame: u8,
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> ClawRoutine01Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let should_descend = if attributes == 0x03 {
        if animation_delay != 0 {
            return ClawRoutine01Result { scroll, outcome: ClawRoutine01Outcome::SeekingDelayCountdown { animation_delay: animation_delay.wrapping_sub(1) } };
        }
        if frame_counter >= 0xC0 {
            false
        } else {
            player_enemy_x_dist(sprite_x_pos, scroll.x_pos, player_state).distance < 0x10
        }
    } else {
        (frame_counter & 0x7F) == enemy_frame
    };

    let outcome = if should_descend && scroll.x_pos >= 0x2C {
        ClawRoutine01Outcome::Descending {
            var_2: CLAW_LENGTH_TBL[attributes as usize],
            var_3: 0x00,
            var_4: attributes << 1,
            delayed_routine: set_enemy_delay_adv_routine(0x00, current_routine),
        }
    } else {
        ClawRoutine01Outcome::Waiting
    };

    ClawRoutine01Result { scroll, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_splits_attributes_into_trigger_frame_and_claw_length() {
        // attrs = 0b0000_1101: bits 2-3 = 0b11 (trigger idx 3 -> 0x60), bits 0-1 = 0b01 (length code 1)
        let r = claw_routine_00(0b0000_1101, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.frame, 0x60);
        assert_eq!(r.attributes, 0b01);
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x20, 3));
    }

    #[test]
    fn routine_01_non_seeking_waits_until_the_frame_counter_matches() {
        let r = claw_routine_01(0x01, 0x00, 0x10, 0x20, [0, 0], [1, 1], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, ClawRoutine01Outcome::Waiting);
    }

    #[test]
    fn routine_01_non_seeking_descends_on_a_matching_frame_counter() {
        let r = claw_routine_01(0x01, 0x00, 0x20, 0x20, [0, 0], [1, 1], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(
            r.outcome,
            ClawRoutine01Outcome::Descending { var_2: CLAW_LENGTH_TBL[1], var_3: 0, var_4: 0x02, delayed_routine: set_enemy_delay_adv_routine(0x00, 3) }
        );
    }

    #[test]
    fn routine_01_non_seeking_still_waits_too_far_left_even_on_a_matching_frame() {
        let r = claw_routine_01(0x01, 0x00, 0x20, 0x20, [0, 0], [1, 1], 0, 0x00, 0x10, 0x60, 3);
        assert_eq!(r.outcome, ClawRoutine01Outcome::Waiting);
    }

    #[test]
    fn routine_01_seeking_counts_down_its_own_delay() {
        let r = claw_routine_01(0x03, 0x05, 0x00, 0x00, [0, 0], [1, 1], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, ClawRoutine01Outcome::SeekingDelayCountdown { animation_delay: 0x04 });
    }

    #[test]
    fn routine_01_seeking_skips_attack_in_the_random_timing_window() {
        let r = claw_routine_01(0x03, 0x00, 0xC5, 0x00, [0x50, 0x50], [1, 1], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, ClawRoutine01Outcome::Waiting);
    }

    #[test]
    fn routine_01_seeking_descends_when_a_player_is_close() {
        let r = claw_routine_01(0x03, 0x00, 0x00, 0x00, [0x55, 0x00], [1, 0], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(
            r.outcome,
            ClawRoutine01Outcome::Descending { var_2: CLAW_LENGTH_TBL[3], var_3: 0, var_4: 0x06, delayed_routine: set_enemy_delay_adv_routine(0x00, 3) }
        );
    }

    #[test]
    fn routine_01_seeking_waits_when_no_player_is_close() {
        let r = claw_routine_01(0x03, 0x00, 0x00, 0x00, [0x00, 0x00], [1, 1], 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, ClawRoutine01Outcome::Waiting);
    }
}
