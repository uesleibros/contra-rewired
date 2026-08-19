//! Native port of the level 3 boss eye's sphere projectile,
//! `src/bank0.asm` (`eye_projectile_routine_ptr_tbl`, `$8f3f`-`$8f58`):
//! `eye_projectile_routine_00` (launch, aims at the closer player via the
//! same [`crate::enemy::quadrant_aim_dir`] subsystem [`crate::enemy::
//! sniper`] uses) and `eye_projectile_routine_01` (fly toward the target,
//! growing from a small dot into a big hittable sphere partway down the
//! screen). Previously assumed blocked behind the rotation/aiming
//! subsystem the same way `sniper_02`-`_05` were - unblocked in the same
//! pass that ported those. `eye_projectile_routine_ptr_tbl` entries `2`-
//! `4` (explosion/removal) are the same real shared `bank7.asm` routines
//! most enemy families use and aren't ported here.

use crate::enemy::add_with_enemy_pos::set_08_09_to_enemy_pos;
use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::quadrant_aim_dir::{get_quadrant_aim_dir_for_player, QUADRANT_AIM_DIR_01};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};
use crate::physics::bullet_physics::calc_bullet_velocities;

/// The full result of one [`eye_projectile_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EyeProjectileRoutine00Result {
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `eye_projectile_routine_00` (`$8f3f`) - aims at the
/// closer player (fixed speed code `6`, `quadrant_aim_dir_01`) via
/// [`get_quadrant_aim_dir_for_player`], sets its own velocity from that
/// direction, and advances. `set_08_09_to_enemy_pos` is called for
/// faithfulness (a real `jsr` in the ASM) even though it's mathematically
/// an identity here - the projectile aims from its own position.
pub fn eye_projectile_routine_00(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_routine: u8,
) -> EyeProjectileRoutine00Result {
    let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
    let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
    let aimed = get_quadrant_aim_dir_for_player(
        source_y,
        source_x,
        closest.player_index,
        player_state,
        sprite_y_pos,
        sprite_x_pos,
        level_location_type,
        &QUADRANT_AIM_DIR_01,
    );
    let v = calc_bullet_velocities(aimed.aim_dir & 0x1F, 0x06, aimed.quadrant);
    EyeProjectileRoutine00Result {
        y_velocity: (v.frac_y, v.fast_y),
        x_velocity: (v.frac_x, v.fast_x),
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// `eye_projectile_sprite_attr_tbl` (`$8f82`, 4 bytes) - mirroring bits
/// cycled every 4 frames (`(FRAME_COUNTER >> 2) & 3`), giving the sphere
/// a lazy spin as it flies.
const EYE_PROJECTILE_SPRITE_ATTR_TBL: [u8; 4] = [0x00, 0x40, 0xC0, 0x80];

/// The full result of one [`eye_projectile_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EyeProjectileRoutine01Result {
    /// `ENEMY_SPRITES` - `$63` (small, not yet hittable) or `$64` (big,
    /// hittable) once `ENEMY_Y_POS >= $48`.
    pub sprite: u8,
    /// `Some(new_state_width)` only the call the sphere first grows big
    /// enough to enable player collision.
    pub collision_enabled: Option<u8>,
    pub sprite_attr: u8,
    pub position: UpdatedEnemyPos,
}

/// Native port of `eye_projectile_routine_01` (`$8f58`) - "fly toward the
/// target, growing into a big hittable sphere partway down the screen":
/// picks the sprite/collision state from `ENEMY_Y_POS`, cycles the
/// mirroring bits from the frame counter, then applies velocity/scroll.
#[allow(clippy::too_many_arguments)]
pub fn eye_projectile_routine_01(
    enemy_y_pos: u8,
    enemy_state_width: u8,
    frame_counter: u8,
    enemy_sprite_attr: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> EyeProjectileRoutine01Result {
    let (sprite, collision_enabled) =
        if enemy_y_pos >= 0x48 { (0x64, Some(enable_enemy_collision(enemy_state_width))) } else { (0x63, None) };

    let bucket = ((frame_counter >> 2) & 0x03) as usize;
    let sprite_attr = (enemy_sprite_attr & 0x3F) | EYE_PROJECTILE_SPRITE_ATTR_TBL[bucket];

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

    EyeProjectileRoutine01Result { sprite, collision_enabled, sprite_attr, position }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enemy::enemy_routine_transition::advance_enemy_routine as adv;

    #[test]
    fn routine_00_matches_a_direct_composition_of_the_aiming_and_velocity_primitives() {
        let closest = player_enemy_x_dist([0x90, 0x00], 0x50, [1, 0]);
        let aimed = get_quadrant_aim_dir_for_player(0x60, 0x50, closest.player_index, [1, 0], [0x60, 0x00], [0x90, 0x00], 0, &QUADRANT_AIM_DIR_01);
        let expected_v = calc_bullet_velocities(aimed.aim_dir & 0x1F, 0x06, aimed.quadrant);

        let r = eye_projectile_routine_00(0x50, 0x60, [1, 0], [0x60, 0x00], [0x90, 0x00], 0, 5);
        assert_eq!(r.y_velocity, (expected_v.frac_y, expected_v.fast_y));
        assert_eq!(r.x_velocity, (expected_v.frac_x, expected_v.fast_x));
        assert_eq!(r.routine_update, adv(5));
    }

    #[test]
    fn routine_00_advances_the_routine_guarded_the_usual_way() {
        let r = eye_projectile_routine_00(0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0);
        assert_eq!(r.routine_update.routine, 0);
    }

    #[test]
    fn routine_01_stays_small_and_uncollidable_below_the_growth_threshold() {
        let r = eye_projectile_routine_01(0x47, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.sprite, 0x63);
        assert_eq!(r.collision_enabled, None);
    }

    #[test]
    fn routine_01_grows_and_enables_collision_at_the_threshold() {
        let r = eye_projectile_routine_01(0x48, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.sprite, 0x64);
        assert_eq!(r.collision_enabled, Some(enable_enemy_collision(0x00)));
    }

    #[test]
    fn routine_01_cycles_the_mirroring_bits_from_the_frame_counter() {
        for (frame_counter, expected_bucket) in [(0x00u8, 0), (0x04, 1), (0x08, 2), (0x0C, 3), (0x10, 0)] {
            let r = eye_projectile_routine_01(0x40, 0x00, frame_counter, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
            assert_eq!(r.sprite_attr, EYE_PROJECTILE_SPRITE_ATTR_TBL[expected_bucket], "frame_counter={frame_counter:02X}");
        }
    }

    #[test]
    fn routine_01_sprite_attr_preserves_only_the_low_6_bits_of_the_input() {
        let r = eye_projectile_routine_01(0x40, 0x00, 0x00, 0b1111_1111, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.sprite_attr, (0b1111_1111 & 0x3F) | EYE_PROJECTILE_SPRITE_ATTR_TBL[0]);
    }

    #[test]
    fn routine_01_position_matches_update_enemy_pos_directly() {
        let r = eye_projectile_routine_01(0x40, 0x00, 0x00, 0x00, 0, 0x02, 0x50, 0, 0x10, 0x00, 0, 0x08, 0x00);
        let expected = update_enemy_pos(0, 0x02, 0x50, 0, 0x10, 0x00, 0x40, 0, 0x08, 0x00);
        assert_eq!(r.position, expected);
    }
}
