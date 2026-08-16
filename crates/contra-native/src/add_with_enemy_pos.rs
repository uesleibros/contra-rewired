//! Native port of `add_with_enemy_pos`/`set_08_09_to_enemy_pos`
//! (`src/bank7.asm`, CPU `$eb2f`-`$eb3f`) - adds an offset to an enemy's
//! position without modifying the enemy itself, writing the result to
//! the `$08`/`$09` scratch pair most of this crate's other aiming/
//! bullet-creation ports (e.g. [`crate::create_enemy_bullet`],
//! [`crate::quadrant_aim_dir`]) take as plain `source_y`/`source_x`
//! parameters. `set_08_09_to_enemy_pos` is the zero-offset special case
//! (real ASM: `lda #$00; tay` before falling straight into
//! `add_with_enemy_pos`) - together the two are called **29 real
//! times** across the ROM (21 for the zero-offset form alone), among
//! the most-reused small utilities found in this crate so far.

/// Native port of `add_with_enemy_pos` (`$eb32`) - returns `(x, y)`
/// matching the real ASM's own `$09`/`$08` write order.
pub fn add_with_enemy_pos(x_offset: u8, y_offset: u8, enemy_x_pos: u8, enemy_y_pos: u8) -> (u8, u8) {
    (x_offset.wrapping_add(enemy_x_pos), y_offset.wrapping_add(enemy_y_pos))
}

/// Native port of `set_08_09_to_enemy_pos` (`$eb2f`) - the zero-offset
/// special case, equivalent to just reading the enemy's own position.
pub fn set_08_09_to_enemy_pos(enemy_x_pos: u8, enemy_y_pos: u8) -> (u8, u8) {
    add_with_enemy_pos(0, 0, enemy_x_pos, enemy_y_pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_offsets_to_enemy_position() {
        assert_eq!(add_with_enemy_pos(0x05, 0x0A, 0x80, 0x40), (0x85, 0x4A));
    }

    #[test]
    fn wraps_on_overflow() {
        assert_eq!(add_with_enemy_pos(0x10, 0x10, 0xFF, 0xF8), (0x0F, 0x08));
    }

    #[test]
    fn set_08_09_is_the_zero_offset_case() {
        assert_eq!(set_08_09_to_enemy_pos(0x80, 0x40), (0x80, 0x40));
        assert_eq!(set_08_09_to_enemy_pos(0x80, 0x40), add_with_enemy_pos(0, 0, 0x80, 0x40));
    }
}
