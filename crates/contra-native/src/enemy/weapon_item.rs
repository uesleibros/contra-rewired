//! Native port of the weapon item pickup's own `_00` table entry
//! (`src/bank0.asm`, `$8007`) - "weapon items are created after flying
//! capsule, pill box sensors, or red soldiers (in indoor/base levels)
//! are destroyed". `weapon_item_routine_01`/`_02` (falling/landing on
//! the ground, then watching for the ground to disappear) are **not yet
//! ported** - they pull in a much deeper dependency chain than `_00`
//! does (`set_outdoor_weapon_item_vel`, `set_weapon_item_y_vel_enemy_
//! frame`, `update_enemy_x_pos_rem_off_screen`, `set_enemy_falling_arc_
//! pos`, `weapon_item_check_bg_collision`, `check_weapon_item_
//! collision`, `set_weapon_item_sprite`, none of which exist in this
//! crate yet), deferred to a future pass rather than rushed.
//!
//! `ENEMY_VAR_B` (real ASM's own name for `$558+x` in this routine) is
//! the *same physical byte* `ENEMY_ATTACK_DELAY` uses elsewhere - a
//! real RAM-aliasing trick this ROM uses throughout (confirmed directly
//! from `docs/rom-symbols.txt` listing both names at the identical
//! address) - named `var_b` here to match what *this* routine actually
//! means by it, not borrowed "attack delay" terminology from an
//! unrelated enemy type.

use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::find_far_segment::find_far_segment_for_a;

/// `weapon_item_indoor_vel_tbl` (`$edbc`, 7 `(fract, fast)` entries) -
/// indexed by [`crate::enemy::find_far_segment::find_far_segment_for_a`]'s
/// 0-6 horizontal-segment code.
const WEAPON_ITEM_INDOOR_VEL_TBL: [(u8, u8); 7] =
    [(0xAA, 0x00), (0x71, 0x00), (0x38, 0x00), (0x00, 0x00), (0xC8, 0xFF), (0x8F, 0xFF), (0x56, 0xFF)];

/// `weapon_item_init_vel_tbl` (`$805c`, 3 `(y_fract, y_fast, x_fract,
/// x_fast)` rows) - row 0 for horizontal-scrolling/indoor levels, row 1
/// for a vertical level's left half, row 2 for a vertical level's right
/// half (`ENEMY_X_POS >= $80`).
const WEAPON_ITEM_INIT_VEL_TBL: [(u8, u8, u8, u8); 3] = [(0x00, 0xFD, 0x80, 0x00), (0x00, 0xFD, 0x40, 0x00), (0x00, 0xFD, 0xC0, 0xFF)];

/// The result of one [`set_weapon_item_indoor_velocity`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponItemIndoorVelocity {
    pub x_velocity: (u8, u8),
    /// Always `(0x00, 0x01)` - a fixed, slow constant fall speed.
    pub y_velocity: (u8, u8),
}

/// Native port of `set_weapon_item_indoor_velocity` (`$ed9d`) - picks an
/// X velocity from the enemy's own horizontal segment (same segment
/// code the indoor-family roller/grenade routines already use).
pub fn set_weapon_item_indoor_velocity(enemy_x_pos: u8) -> WeaponItemIndoorVelocity {
    let segment = find_far_segment_for_a(enemy_x_pos);
    WeaponItemIndoorVelocity { x_velocity: WEAPON_ITEM_INDOOR_VEL_TBL[segment as usize], y_velocity: (0x00, 0x01) }
}

/// The real branch [`weapon_item_routine_00`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponItemRoutine00Outcome {
    /// `LEVEL_LOCATION_TYPE != 0` - indoor/base level.
    Indoor {
        /// `ENEMY_VAR_1` - set to the item's own initial Y position,
        /// used later by the (not yet ported) falling-arc calculation.
        var_1: u8,
        velocity: WeaponItemIndoorVelocity,
        /// Always `$80` - `ENEMY_VAR_4`, the falling-arc accumulator's
        /// low byte seed.
        var_4: u8,
        /// Always `$fd` - `ENEMY_VAR_B` (real ASM's own name; see this
        /// module's doc comment for why it's not called "attack delay"
        /// here), the falling-arc accumulator's high byte seed.
        var_b: u8,
        routine_update: EnemyRoutineUpdate,
    },
    /// `LEVEL_LOCATION_TYPE == 0` - outdoor level.
    Outdoor {
        y_velocity: (u8, u8),
        x_velocity: (u8, u8),
        routine_update: EnemyRoutineUpdate,
    },
}

/// The full result of one [`weapon_item_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WeaponItemRoutine00Result {
    /// Always `$80` - `ENEMY_STATE_WIDTH` (real comment: "mark weapon
    /// item so bullets travel through it").
    pub state_width: u8,
    /// Always `$22` - `ENEMY_SCORE_COLLISION` (score code 2, collision
    /// type 2).
    pub score_collision: u8,
    /// Always `$05` (US ROM - a Probotector build uses `$00` instead,
    /// per the real `.ifdef`; this project targets the US ROM only).
    pub sprite_attr: u8,
    pub outcome: WeaponItemRoutine00Outcome,
}

/// Native port of `weapon_item_routine_00` (`$8007`) - "sets collision
/// code, velocity".
pub fn weapon_item_routine_00(
    level_location_type: u8,
    enemy_y_pos: u8,
    enemy_x_pos: u8,
    level_scrolling_type: u8,
    current_routine: u8,
) -> WeaponItemRoutine00Result {
    let outcome = if level_location_type != 0 {
        WeaponItemRoutine00Outcome::Indoor {
            var_1: enemy_y_pos,
            velocity: set_weapon_item_indoor_velocity(enemy_x_pos),
            var_4: 0x80,
            var_b: 0xFD,
            routine_update: advance_enemy_routine(current_routine),
        }
    } else {
        let row = if level_scrolling_type == 0 {
            0
        } else if enemy_x_pos < 0x80 {
            1
        } else {
            2
        };
        let (y_fract, y_fast, x_fract, x_fast) = WEAPON_ITEM_INIT_VEL_TBL[row];
        WeaponItemRoutine00Outcome::Outdoor { y_velocity: (y_fract, y_fast), x_velocity: (x_fract, x_fast), routine_update: advance_enemy_routine(current_routine) }
    };

    WeaponItemRoutine00Result { state_width: 0x80, score_collision: 0x22, sprite_attr: 0x05, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indoor_uses_the_horizontal_segment_velocity_and_fixed_seeds() {
        let r = weapon_item_routine_00(1, 0x60, 0x50, 0, 5);
        match r.outcome {
            WeaponItemRoutine00Outcome::Indoor { var_1, velocity, var_4, var_b, routine_update } => {
                assert_eq!(var_1, 0x60);
                assert_eq!(velocity, set_weapon_item_indoor_velocity(0x50));
                assert_eq!(var_4, 0x80);
                assert_eq!(var_b, 0xFD);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Indoor, got {other:?}"),
        }
        assert_eq!(r.state_width, 0x80);
        assert_eq!(r.score_collision, 0x22);
        assert_eq!(r.sprite_attr, 0x05);
    }

    #[test]
    fn outdoor_horizontal_level_uses_row_0() {
        let r = weapon_item_routine_00(0, 0x60, 0x50, 0, 5);
        match r.outcome {
            WeaponItemRoutine00Outcome::Outdoor { y_velocity, x_velocity, .. } => {
                assert_eq!(y_velocity, (0x00, 0xFD));
                assert_eq!(x_velocity, (0x80, 0x00));
            }
            other => panic!("expected Outdoor, got {other:?}"),
        }
    }

    #[test]
    fn outdoor_vertical_level_left_half_uses_row_1() {
        let r = weapon_item_routine_00(0, 0x60, 0x50, 1, 5);
        match r.outcome {
            WeaponItemRoutine00Outcome::Outdoor { x_velocity, .. } => assert_eq!(x_velocity, (0x40, 0x00)),
            other => panic!("expected Outdoor, got {other:?}"),
        }
    }

    #[test]
    fn outdoor_vertical_level_right_half_uses_row_2() {
        let r = weapon_item_routine_00(0, 0x60, 0x80, 1, 5);
        match r.outcome {
            WeaponItemRoutine00Outcome::Outdoor { x_velocity, .. } => assert_eq!(x_velocity, (0xC0, 0xFF)),
            other => panic!("expected Outdoor, got {other:?}"),
        }
    }

    #[test]
    fn set_weapon_item_indoor_velocity_uses_a_fixed_slow_fall_speed() {
        let v = set_weapon_item_indoor_velocity(0x50);
        assert_eq!(v.y_velocity, (0x00, 0x01));
        assert_eq!(v.x_velocity, WEAPON_ITEM_INDOOR_VEL_TBL[find_far_segment_for_a(0x50) as usize]);
    }
}
