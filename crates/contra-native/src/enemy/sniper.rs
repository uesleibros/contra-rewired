//! Native port of the sniper ("rifle man")'s own `_00` table entry
//! (`src/bank0.asm`, `$8958`). `sniper_routine_01`-`_05` (crouch-cycle
//! animation, then a real bullet-angle-quadrant aiming/firing subsystem
//! built around `get_rotate_01` and several new sprite/offset tables)
//! are **not yet ported** - substantially larger than `_00` alone and
//! deferred to a future pass rather than rushed.
//!
//! `ENEMY_ATTRIBUTES` selects one of 3 real sniper types: `0` standing
//! (always visible, fires from a fixed pose), `1` crouching/hiding
//! (only visible - and only takes the extra `+5` Y nudge this routine
//! applies - while popped up to fire), `2` boss-screen hiding (same
//! shape as type 0 for everything `_00` itself touches).

use crate::enemy::enemy_position_utils::{add_a_to_enemy_y_pos, add_a_with_vert_scroll_to_enemy_y_pos};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};

/// `sniper_animation_delay_tbl` (`$8979`, 3 bytes) - initial `ENEMY_
/// ANIMATION_DELAY`, indexed by sniper type.
const SNIPER_ANIMATION_DELAY_TBL: [u8; 3] = [0x01, 0x30, 0x80];
/// `sniper_frame_tbl` (`$897f`, 3 bytes) - initial `ENEMY_FRAME`
/// (`sniper_sprite_xx` offset), same indexing.
const SNIPER_FRAME_TBL: [u8; 3] = [0x03, 0x00, 0x00];

/// The full result of one [`sniper_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine00Result {
    pub animation_delay: u8,
    pub frame: u8,
    pub y_pos: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `sniper_routine_00` (`$8958`) - "load variables
/// (`ENEMY_ANIMATION_DELAY`, `ENEMY_FRAME`), adjust Y position for
/// crouching sniper". `sniper_type` is `ENEMY_ATTRIBUTES` (`0`/`1`/`2`).
pub fn sniper_routine_00(sniper_type: u8, enemy_y_pos: u8, vertical_scroll: u8, current_routine: u8) -> SniperRoutine00Result {
    let animation_delay = SNIPER_ANIMATION_DELAY_TBL[sniper_type as usize];
    let frame = SNIPER_FRAME_TBL[sniper_type as usize];

    let y_pos = add_a_with_vert_scroll_to_enemy_y_pos(0x04, vertical_scroll, enemy_y_pos);
    let y_pos = if sniper_type == 1 { add_a_to_enemy_y_pos(0x05, y_pos) } else { y_pos };

    SniperRoutine00Result { animation_delay, frame, y_pos, routine_update: advance_enemy_routine(current_routine) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_sniper_uses_type_0_row_and_no_extra_nudge() {
        let r = sniper_routine_00(0, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x01);
        assert_eq!(r.frame, 0x03);
        assert_eq!(r.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60));
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn crouching_sniper_gets_the_extra_5_pixel_nudge() {
        let r = sniper_routine_00(1, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x30);
        assert_eq!(r.frame, 0x00);
        let after_4 = add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60);
        assert_eq!(r.y_pos, add_a_to_enemy_y_pos(0x05, after_4));
    }

    #[test]
    fn boss_screen_sniper_uses_type_2_row_and_no_extra_nudge() {
        let r = sniper_routine_00(2, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x80);
        assert_eq!(r.frame, 0x00);
        assert_eq!(r.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60));
    }

    #[test]
    fn vertical_scroll_is_threaded_through_the_first_add() {
        let with_scroll = sniper_routine_00(0, 0x60, 0x08, 5);
        assert_eq!(with_scroll.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x08, 0x60));
    }
}
