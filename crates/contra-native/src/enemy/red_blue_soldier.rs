//! Native port of the red/blue soldier enemy family's shared spawn-init
//! state, `red_blue_soldier_routine_00` (`src/bank0.asm`, CPU `$a157`-
//! `$a17d`) - entry 0 of both `blue_soldier_routine_ptr_tbl` and `red_
//! soldier_routine_ptr_tbl` (a distinct enemy type from the plain
//! soldier in [`crate::enemy::soldier`], though both families share the
//! generic explosion/removal routines, [`crate::enemy::enemy_explosion`]).
//! Places the enemy at one of 4 fixed screen corners and gives it an
//! initial horizontal running velocity, both picked from `ENEMY_
//! ATTRIBUTES`, then advances to the next routine.

use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};

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
}
