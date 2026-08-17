//! Native port of the "indoor soldier" enemy family's shared position/
//! velocity/sprite helpers, plus the indoor soldier's own init state.
//! `indoor_soldier_routine_ptr_tbl` (`src/bank0.asm`, `$92c8`-onward) is
//! actually shared by **4 real enemy types** ($15 indoor soldier, $16
//! jumping soldier, $17 grenade launcher, $18 group of four soldiers) -
//! this module only carries the pieces reachable from `indoor_soldier_
//! routine_00` so far ([`init_indoor_enemy_pos_and_vel`], [`apply_enemy_
//! velocity_set_bg_priority`], [`init_sprite_from_frame`]); `indoor_
//! soldier_routine_01`'s own firing logic (bullets/rollers/grenades, 3
//! more sub-systems) and the other 3 enemy types' own `_00`/`_01`
//! entries are not yet ported.

use crate::enemy::enemy_position_utils::reverse_enemy_x_direction;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::update_enemy_pos::{remove_enemy, RemovedEnemy};

/// `indoor_soldier_x_velocity_tbl` (`$96b9`, 8 bytes) - `(x_vel_fract,
/// x_vel_fast)` per real enemy type sharing this init helper, indexed by
/// the `y` value each type's own `routine_00` passes in (indoor soldier
/// itself always uses index `0`).
const INDOOR_SOLDIER_X_VELOCITY_TBL: [(u8, u8); 4] = [
    (0x20, 0xFF), // indoor soldier (-.875)
    (0x40, 0xFF), // jumping soldier (-.75)
    (0x40, 0xFF), // group of 4 (-.75)
    (0x40, 0xFF), // grenade launcher (-.75)
];

/// The full result of one [`init_indoor_enemy_pos_and_vel`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitIndoorEnemyPosAndVelResult {
    pub x_velocity: (u8, u8),
    pub x_pos: u8,
    /// Always `$6d` - indoor levels use a fixed enemy Y position.
    pub y_pos: u8,
}

/// Native port of `init_indoor_enemy_pos_and_vel` (`$9697`) - places the
/// enemy at one of 2 fixed X positions (screen edges) based on `ENEMY_
/// ATTRIBUTES` bit 0, with an initial velocity from the shared table
/// above, reversed to run the opposite direction when spawning from the
/// left instead of the right.
pub fn init_indoor_enemy_pos_and_vel(y_index: u8, enemy_attributes: u8) -> InitIndoorEnemyPosAndVelResult {
    let (fract, fast) = INDOOR_SOLDIER_X_VELOCITY_TBL[y_index as usize];
    let (x_velocity, x_pos) =
        if enemy_attributes & 0x01 == 0 { ((fract, fast), 0xA8) } else { (reverse_enemy_x_direction(fract, fast), 0x58) };
    InitIndoorEnemyPosAndVelResult { x_velocity, x_pos, y_pos: 0x6D }
}

/// The real, branchy result of one [`apply_enemy_velocity_set_bg_priority`]
/// call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyEnemyVelocityOutcome {
    /// Past the real disappearance limit for the enemy's travel
    /// direction (`>= $b0` moving right, `< $50` moving left).
    Removed(RemovedEnemy),
    /// Still on screen: `ENEMY_SPRITE_ATTR` after the background-
    /// priority bit is set or cleared depending on X position (real ASM
    /// draws the enemy *behind* the background near either screen edge,
    /// same "behind pillar" shape as `red_blue_soldier_set_bg_priority`).
    BgPriority(u8),
}

/// The full result of one [`apply_enemy_velocity_set_bg_priority`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyEnemyVelocityResult {
    pub x_vel_accum: u8,
    pub x_pos: u8,
    pub outcome: ApplyEnemyVelocityOutcome,
}

/// Native port of `apply_enemy_velocity_set_bg_priority` (`$96c1`) - the
/// indoor soldier family's X-only velocity integrator (indoor enemies
/// never move vertically), shared by every routine in this family that
/// needs to move and stay on screen.
pub fn apply_enemy_velocity_set_bg_priority(x_vel_accum: u8, x_vel_fract: u8, x_vel_fast: u8, x_pos: u8, sprite_attr: u8) -> ApplyEnemyVelocityResult {
    let (new_accum, carry) = x_vel_accum.overflowing_add(x_vel_fract);
    let new_x_pos = x_pos.wrapping_add(x_vel_fast).wrapping_add(carry as u8);

    let moving_left = (x_vel_fast as i8) < 0;
    let removed = if moving_left { new_x_pos < 0x50 } else { new_x_pos >= 0xB0 };

    let outcome = if removed {
        ApplyEnemyVelocityOutcome::Removed(remove_enemy())
    } else {
        let behind_bg = new_x_pos >= 0xA0 || new_x_pos < 0x60;
        let attr = if behind_bg { sprite_attr | 0x20 } else { sprite_attr & 0xDF };
        ApplyEnemyVelocityOutcome::BgPriority(attr)
    };

    ApplyEnemyVelocityResult { x_vel_accum: new_accum, x_pos: new_x_pos, outcome }
}

/// The full result of one [`init_sprite_from_frame`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitSpriteFromFrameResult {
    pub enemy_frame: u8,
    pub sprites: u8,
    pub sprite_attr: u8,
}

/// Native port of `init_sprite_from_frame` (`$9316`) - cycles `ENEMY_
/// FRAME` through `0..3` every 4th frame (same cadence as `red_blue_
/// soldier_set_run_frame`, a different real routine with the same
/// shape), then sets the sprite code and horizontal-flip bit from the
/// current travel direction.
pub fn init_sprite_from_frame(frame_counter: u8, enemy_frame: u8, enemy_sprite_attr: u8, x_vel_fast: u8) -> InitSpriteFromFrameResult {
    let new_frame = if frame_counter & 0x03 != 0 {
        enemy_frame
    } else {
        let incremented = enemy_frame.wrapping_add(1);
        if incremented >= 0x03 {
            0x00
        } else {
            incremented
        }
    };
    let sprites = new_frame.wrapping_add(0x93);
    let moving_left = (x_vel_fast as i8) < 0;
    let sprite_attr = if moving_left { enemy_sprite_attr | 0x40 } else { enemy_sprite_attr & 0xBF };
    InitSpriteFromFrameResult { enemy_frame: new_frame, sprites, sprite_attr }
}

/// The full result of one [`indoor_soldier_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndoorSoldierRoutine00Result {
    pub init: InitIndoorEnemyPosAndVelResult,
    /// Always `$08`.
    pub attack_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `indoor_soldier_routine_00` (`$92c8`) - "initializes
/// indoor soldier: sets position, velocity and attack delay".
pub fn indoor_soldier_routine_00(enemy_attributes: u8, current_routine: u8) -> IndoorSoldierRoutine00Result {
    let init = init_indoor_enemy_pos_and_vel(0, enemy_attributes);
    let routine_update = advance_enemy_routine(current_routine);
    IndoorSoldierRoutine00Result { init, attack_delay: 0x08, routine_update }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_pos_and_vel_from_the_right_uses_the_table_velocity_directly() {
        let r = init_indoor_enemy_pos_and_vel(0, 0x00);
        assert_eq!(r.x_velocity, (0x20, 0xFF));
        assert_eq!(r.x_pos, 0xA8);
        assert_eq!(r.y_pos, 0x6D);
    }

    #[test]
    fn init_pos_and_vel_from_the_left_reverses_the_table_velocity() {
        let r = init_indoor_enemy_pos_and_vel(0, 0x01);
        assert_eq!(r.x_velocity, reverse_enemy_x_direction(0x20, 0xFF));
        assert_eq!(r.x_pos, 0x58);
    }

    #[test]
    fn init_pos_and_vel_uses_the_right_table_row_per_enemy_type() {
        let r = init_indoor_enemy_pos_and_vel(1, 0x00); // jumping soldier
        assert_eq!(r.x_velocity, (0x40, 0xFF));
    }

    #[test]
    fn apply_velocity_removes_past_the_right_limit_moving_right() {
        let r = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0xAF, 0x00);
        assert_eq!(r.x_pos, 0xB0);
        assert_eq!(r.outcome, ApplyEnemyVelocityOutcome::Removed(remove_enemy()));
    }

    #[test]
    fn apply_velocity_removes_past_the_left_limit_moving_left() {
        let r = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0xFF, 0x50, 0x00);
        assert_eq!(r.x_pos, 0x4F);
        assert_eq!(r.outcome, ApplyEnemyVelocityOutcome::Removed(remove_enemy()));
    }

    #[test]
    fn apply_velocity_sets_bg_priority_near_either_edge_and_clears_it_mid_screen() {
        let left_edge = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0x5E, 0x00);
        assert_eq!(left_edge.outcome, ApplyEnemyVelocityOutcome::BgPriority(0x20));
        let right_edge = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0x9F, 0x00);
        assert_eq!(right_edge.outcome, ApplyEnemyVelocityOutcome::BgPriority(0x20));
        let middle = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0x7F, 0b0010_0000);
        assert_eq!(middle.outcome, ApplyEnemyVelocityOutcome::BgPriority(0x00));
    }

    #[test]
    fn init_sprite_from_frame_only_advances_on_the_4th_frame_and_wraps_at_3() {
        assert_eq!(init_sprite_from_frame(0x01, 0x01, 0x00, 0x00).enemy_frame, 0x01);
        assert_eq!(init_sprite_from_frame(0x04, 0x01, 0x00, 0x00).enemy_frame, 0x02);
        assert_eq!(init_sprite_from_frame(0x04, 0x02, 0x00, 0x00).enemy_frame, 0x00);
    }

    #[test]
    fn init_sprite_from_frame_sets_sprite_and_flip_bit_from_direction() {
        let right = init_sprite_from_frame(0x01, 0x01, 0b0100_0000, 0x01);
        assert_eq!(right.sprites, 0x94);
        assert_eq!(right.sprite_attr, 0x00);
        let left = init_sprite_from_frame(0x01, 0x01, 0x00, 0xFF);
        assert_eq!(left.sprite_attr, 0x40);
    }

    #[test]
    fn routine_00_composes_init_and_advances() {
        let r = indoor_soldier_routine_00(0x00, 5);
        assert_eq!(r.init, init_indoor_enemy_pos_and_vel(0, 0x00));
        assert_eq!(r.attack_delay, 0x08);
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }
}
