//! Native port of a handful of tiny, widely-reused enemy position/
//! velocity mutators (`src/bank7.asm`): `add_a_to_enemy_y_pos`/`add_a_to_
//! enemy_x_pos` (`$eb1f`-`$eb2e`, 17 real call sites combined),
//! `add_10_to_enemy_y_fract_vel`/`add_a_to_enemy_y_fract_vel` (`$eb40`-
//! `$eb51`, 10 real call sites combined), `reverse_enemy_x_direction`
//! (`$e91e`-`$e92f`, 8 real call sites), and `add_4_to_enemy_y_pos`/
//! `add_a_with_vert_scroll_to_enemy_y_pos` (`$eb88`-`$eb8f`) - a real,
//! *non-trivial* Y adder despite the family it sits in: it rounds
//! `ENEMY_Y_POS` down to the nearest 16-pixel boundary **relative to the
//! current `VERTICAL_SCROLL` phase** before adding, not a plain `pos +
//! a` - real ASM comment: "accounting for `VERTICAL_SCROLL` overflow on
//! vertical levels". Confirmed genuinely different from a plain add by
//! hand-tracing a `VERTICAL_SCROLL=0` example (still not a no-op: e.g.
//! `pos=$63, a=$04` rounds to `$60` first, giving `$64`, not the `$67` a
//! plain add would).

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

/// Native port of `add_a_with_vert_scroll_to_enemy_y_pos` (`$eb8a`) - see
/// this module's doc comment for why this isn't a plain add.
pub fn add_a_with_vert_scroll_to_enemy_y_pos(a: u8, vertical_scroll: u8, enemy_y_pos: u8) -> u8 {
    let scroll_lo = (vertical_scroll & 0x0f) | 0xf0;
    let step1 = scroll_lo.wrapping_add(enemy_y_pos) & 0xf0;
    let step2 = step1.wrapping_sub(scroll_lo);
    step2.wrapping_add(a)
}

/// Native port of `add_4_to_enemy_y_pos` (`$eb88`) - the real ASM's own
/// `lda #$04` immediately falling into `add_a_with_vert_scroll_to_enemy_
/// y_pos`.
pub fn add_4_to_enemy_y_pos(vertical_scroll: u8, enemy_y_pos: u8) -> u8 {
    add_a_with_vert_scroll_to_enemy_y_pos(0x04, vertical_scroll, enemy_y_pos)
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

    #[test]
    fn vert_scroll_add_rounds_down_to_16px_boundary_before_adding_even_with_zero_scroll() {
        // Hand-traced example from this module's own doc comment.
        assert_eq!(add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x63), 0x64);
        assert_eq!(add_4_to_enemy_y_pos(0x00, 0x63), 0x64);
    }

    #[test]
    fn vert_scroll_add_is_not_a_plain_add() {
        // A plain add would give 0x67, not 0x64 - confirms the rounding
        // step actually does something for this input.
        assert_ne!(add_4_to_enemy_y_pos(0x00, 0x63), 0x63u8.wrapping_add(0x04));
    }

    #[test]
    fn vert_scroll_add_already_on_boundary_is_unaffected_by_rounding() {
        // pos already a multiple of 16: rounding down is a no-op either way.
        assert_eq!(add_4_to_enemy_y_pos(0x00, 0x60), 0x64);
    }

    #[test]
    fn vert_scroll_add_nonzero_scroll_shifts_the_rounding_phase() {
        // Same position, different VERTICAL_SCROLL - real hardware
        // rounds relative to the *current scroll phase*, not always to
        // an absolute 0-based 16px grid, so the result can differ from
        // the VERTICAL_SCROLL=0 case for the same position.
        let with_zero_scroll = add_4_to_enemy_y_pos(0x00, 0x63);
        let with_scroll = add_4_to_enemy_y_pos(0x08, 0x63);
        assert_ne!(with_zero_scroll, with_scroll);
    }
}
