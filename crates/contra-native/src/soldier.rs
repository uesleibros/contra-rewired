//! Native port of `soldier_routine_00` (`src/bank0.asm`, CPU `$861e`-
//! `$8633`) - the soldier enemy's first AI state, run once right after
//! `initialize_enemy` spawns it: nudges its position slightly down so it
//! visually stands on the ground, and sets a per-attribute initial
//! animation delay before advancing to `soldier_routine_01`. This
//! crate's first **composed enemy AI state** - every step is a call
//! into an already independently-verified building block
//! ([`crate::add_scroll_to_enemy_pos::add_scroll_to_enemy_pos`],
//! [`crate::update_enemy_pos::remove_enemy`],
//! [`crate::enemy_position_utils::add_4_to_enemy_y_pos`],
//! [`crate::enemy_routine_transition::set_enemy_delay_adv_routine`]) -
//! demonstrating the same real composition the ROM itself uses (four
//! real `jsr`/`jmp`s, no new arithmetic of its own beyond a 4-bit
//! attribute shift and a 4-entry table lookup).

use crate::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy_position_utils::add_4_to_enemy_y_pos;
use crate::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::update_enemy_pos::{remove_enemy, RemovedEnemy};

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
}
