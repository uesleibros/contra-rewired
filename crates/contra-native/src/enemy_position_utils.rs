//! Native port of a handful of tiny, widely-reused enemy position/
//! velocity mutators (`src/bank7.asm`): `add_a_to_enemy_y_pos`/`add_a_to_
//! enemy_x_pos` (`$eb1f`-`$eb2e`, 17 real call sites combined),
//! `add_10_to_enemy_y_fract_vel`/`add_a_to_enemy_y_fract_vel` (`$eb40`-
//! `$eb51`, 10 real call sites combined), and `reverse_enemy_x_direction`
//! (`$e91e`-`$e92f`, 8 real call sites).

use crate::bullet_physics::negate16;

/// Native port of `add_a_to_enemy_y_pos` (`$eb1f`).
pub fn add_a_to_enemy_y_pos(a: u8, enemy_y_pos: u8) -> u8 {
    a.wrapping_add(enemy_y_pos)
}

/// Native port of `add_a_to_enemy_x_pos` (`$eb27`).
pub fn add_a_to_enemy_x_pos(a: u8, enemy_x_pos: u8) -> u8 {
    a.wrapping_add(enemy_x_pos)
}

/// Native port of `add_a_to_enemy_y_fract_vel` (`$eb42`) - adds `a` to
/// the enemy's Y fractional velocity, carrying into the fast velocity
/// byte exactly like [`crate::update_enemy_pos`]'s own fixed-point
/// integrator.
pub fn add_a_to_enemy_y_fract_vel(a: u8, y_vel_fract: u8, y_vel_fast: u8) -> (u8, u8) {
    let (new_fract, carry) = y_vel_fract.overflowing_add(a);
    let new_fast = y_vel_fast.wrapping_add(carry as u8);
    (new_fract, new_fast)
}

/// Native port of `add_10_to_enemy_y_fract_vel` (`$eb40`) - the real
/// ASM's own `lda #$10` immediately falling into `add_a_to_enemy_y_
/// fract_vel`.
pub fn add_10_to_enemy_y_fract_vel(y_vel_fract: u8, y_vel_fast: u8) -> (u8, u8) {
    add_a_to_enemy_y_fract_vel(0x10, y_vel_fract, y_vel_fast)
}

/// Native port of `reverse_enemy_x_direction` (`$e91e`) - flips an
/// enemy's X velocity to the opposite direction (e.g. hitting a wall or
/// screen edge), the same 16-bit two's-complement negation
/// [`crate::bullet_physics`] uses for bullet direction flips.
pub fn reverse_enemy_x_direction(x_vel_fract: u8, x_vel_fast: u8) -> (u8, u8) {
    negate16(x_vel_fract, x_vel_fast)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_a_to_enemy_y_pos_wraps() {
        assert_eq!(add_a_to_enemy_y_pos(0x10, 0xF8), 0x08);
    }

    #[test]
    fn add_a_to_enemy_x_pos_wraps() {
        assert_eq!(add_a_to_enemy_x_pos(0x10, 0xF8), 0x08);
    }

    #[test]
    fn add_a_to_enemy_y_fract_vel_carries_into_fast() {
        assert_eq!(add_a_to_enemy_y_fract_vel(0x20, 0xF0, 0x01), (0x10, 0x02));
    }

    #[test]
    fn add_a_to_enemy_y_fract_vel_no_carry() {
        assert_eq!(add_a_to_enemy_y_fract_vel(0x20, 0x10, 0x01), (0x30, 0x01));
    }

    #[test]
    fn add_10_to_enemy_y_fract_vel_matches_add_a_with_0x10() {
        assert_eq!(add_10_to_enemy_y_fract_vel(0xF0, 0x01), add_a_to_enemy_y_fract_vel(0x10, 0xF0, 0x01));
    }

    #[test]
    fn reverse_enemy_x_direction_flips_sign() {
        assert_eq!(reverse_enemy_x_direction(0x00, 0x03), (0x00, 0xFD));
        // reversing twice returns to the original value
        let (f, s) = reverse_enemy_x_direction(0x40, 0x02);
        assert_eq!(reverse_enemy_x_direction(f, s), (0x40, 0x02));
    }
}
