//! Native port of `get_quadrant_aim_dir` (`src/bank7.asm`, CPU
//! `$f55e`-`$f5ab`) - the core "which way should this enemy aim to hit a
//! target" routine: computes the absolute Y/X distance between an enemy
//! and its target, buckets each into a coarse row/column, and looks up
//! a nibble-packed angle byte from one of 3 real tables
//! (`quadrant_aim_dir_00`/`_01`/`_02`, `$f5b2`-`$f611`).
//!
//! `get_quadrant_aim_dir_for_player` (`$f52c`-`$f55d`, real caller one
//! level up - resolves *which* player position to target, including a
//! real fallback to a fixed off-screen position when neither player is
//! in a normal state) is **not ported yet** - this module covers only
//! the shared core both `get_quadrant_aim_dir_for_player` and
//! `aim_and_create_enemy_bullet` (the dragon-boss-arm-orb path, which
//! calls this routine directly with an already-known target) funnel
//! into.

/// One of the 3 real 32-byte (8 rows x 4 cols, 2 nibble-packed angle
/// values per byte) lookup tables `get_quadrant_aim_dir` selects between
/// via its `table_index` parameter (the real `$0f`/`quadrant_aim_dir_
/// lookup_ptr_tbl` offset).
pub type QuadrantAimDirTable = [u8; 32];

/// `quadrant_aim_dir_00` (`$f5b2`) - used by soldiers, weapon boxes, red
/// turrets, wall core, and dragon arm orb projectiles.
pub const QUADRANT_AIM_DIR_00: QuadrantAimDirTable = [
    0x00, 0x00, 0x00, 0x00, //
    0x32, 0x11, 0x00, 0x00, //
    0x32, 0x11, 0x11, 0x11, //
    0x32, 0x22, 0x11, 0x11, //
    0x33, 0x22, 0x11, 0x11, //
    0x33, 0x22, 0x22, 0x11, //
    0x33, 0x22, 0x22, 0x11, //
    0x33, 0x22, 0x22, 0x22, //
];

/// `quadrant_aim_dir_01` (`$f5d2`) - used by rotating gun, wall turrets,
/// sniper, eye projectile, spinning bubbles, jumping soldier, white
/// blob, and tank (also the only table indoor levels ever use).
pub const QUADRANT_AIM_DIR_01: QuadrantAimDirTable = [
    0x00, 0x00, 0x00, 0x00, //
    0x63, 0x21, 0x11, 0x11, //
    0x64, 0x32, 0x21, 0x11, //
    0x65, 0x43, 0x22, 0x22, //
    0x65, 0x44, 0x33, 0x22, //
    0x65, 0x54, 0x33, 0x32, //
    0x65, 0x54, 0x43, 0x33, //
    0x65, 0x54, 0x44, 0x33, //
];

/// `quadrant_aim_dir_02` (`$f5f2`) - used by the level 3 dragon boss's
/// seeking arm (not its straight projectile firing, which uses `_00`).
pub const QUADRANT_AIM_DIR_02: QuadrantAimDirTable = [
    0x80, 0x00, 0x00, 0x00, //
    0xf8, 0x53, 0x32, 0x21, //
    0xfb, 0x86, 0x54, 0x33, //
    0xfd, 0xa8, 0x75, 0x54, //
    0xfe, 0xb9, 0x87, 0x65, //
    0xfe, 0xcb, 0x98, 0x76, //
    0xfe, 0xdb, 0xa9, 0x87, //
    0xff, 0xdc, 0xba, 0x98, //
];

/// `sec / sbc / bcs` (unsigned subtract with overflow-flip) - the real
/// 6502 idiom `get_quadrant_aim_dir` uses twice to get an absolute
/// distance and remember which direction it was measured: returns
/// `(abs(target - source), target < source)`.
fn abs_diff_with_borrow_flag(target: u8, source: u8) -> (u8, bool) {
    if target >= source { (target - source, false) } else { (source - target, true) }
}

/// The real routine's two outputs: `aim_dir` (bits an angle-table lookup
/// like [`crate::bullet_physics::calc_bullet_velocities`] can consume)
/// and `quadrant` (the same 0-3 quadrant code
/// [`crate::create_enemy_bullet::create_enemy_bullet`] takes directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuadrantAimDir {
    pub aim_dir: u8,
    pub quadrant: u8,
}

/// Native port of `get_quadrant_aim_dir` (`$f55e`): given an enemy's
/// position (`source_y`/`source_x`) and its target's
/// (`target_y`/`target_x`), picks a row/column into `table` from the
/// coarse Y/X distance and extracts the correct nibble.
pub fn get_quadrant_aim_dir(source_y: u8, source_x: u8, target_y: u8, target_x: u8, table: &QuadrantAimDirTable) -> QuadrantAimDir {
    let mut quadrant: u8 = 0;

    let (abs_dy, y_borrowed) = abs_diff_with_borrow_flag(target_y, source_y);
    if y_borrowed {
        quadrant += 1; // enemy below target
    }
    let row = (abs_dy >> 5) as usize;

    let (abs_dx, x_borrowed) = abs_diff_with_borrow_flag(target_x, source_x);
    if x_borrowed {
        quadrant += 2; // target to the left of enemy
    }
    let col = (abs_dx >> 6) as usize;
    let use_low_nibble = (abs_dx >> 5) & 1 != 0;

    let byte = table[row * 4 + col];
    let aim_dir = if use_low_nibble { byte & 0x0f } else { (byte >> 4) & 0x0f };

    QuadrantAimDir { aim_dir, quadrant }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_position_gives_quadrant_0_row_0_col_0() {
        // target == source: no borrow on either axis -> quadrant 0,
        // abs_dy=abs_dx=0 -> row=0, col=0, table[0] used, high nibble
        // (bit5 of abs_dx=0 is clear).
        let r = get_quadrant_aim_dir(0x80, 0x80, 0x80, 0x80, &QUADRANT_AIM_DIR_00);
        assert_eq!(r, QuadrantAimDir { aim_dir: 0, quadrant: 0 });
    }

    #[test]
    fn target_above_and_left_sets_both_quadrant_bits() {
        // target_y < source_y (enemy below target -> bit 0), target_x <
        // source_x (target left of enemy -> bit 1) -> quadrant 3.
        let r = get_quadrant_aim_dir(0x80, 0x80, 0x40, 0x40, &QUADRANT_AIM_DIR_00);
        assert_eq!(r.quadrant, 3);
    }

    #[test]
    fn target_below_and_right_gives_quadrant_0() {
        let r = get_quadrant_aim_dir(0x40, 0x40, 0x80, 0x80, &QUADRANT_AIM_DIR_00);
        assert_eq!(r.quadrant, 0);
    }

    #[test]
    fn row_and_column_bucket_by_shifted_distance() {
        // abs_dy = 0x21 (33) -> row = 33>>5 = 1. abs_dx = 0x41 (65) ->
        // col = 65>>6 = 1. bit5 of abs_dx (65 = 0b0100_0001, bit5=0) is
        // clear -> use high nibble. QUADRANT_AIM_DIR_00 row 1 = [0x32,
        // 0x11, 0x00, 0x00], entry at col 1 = 0x11 -> high nibble = 1.
        let r = get_quadrant_aim_dir(0x00, 0x00, 0x21, 0x41, &QUADRANT_AIM_DIR_00);
        assert_eq!(r.aim_dir, 1);
    }

    #[test]
    fn bit5_of_abs_dx_selects_low_vs_high_nibble() {
        // abs_dx = 0x60 (96 = 0b0110_0000): bit5 set -> low nibble.
        // col = 96>>6 = 1, row=0 -> table[1] = 0x00 either nibble is 0,
        // not a useful differentiator - use quadrant_aim_dir_01 row 1
        // instead: abs_dy=0x21->row=1; table row1=[0x63,0x21,0x11,0x11].
        // abs_dx=0x60 -> col=1 -> byte=0x21. bit5 set -> low nibble = 1.
        let with_bit5 = get_quadrant_aim_dir(0x00, 0x00, 0x21, 0x60, &QUADRANT_AIM_DIR_01);
        assert_eq!(with_bit5.aim_dir, 1); // low nibble of 0x21
        // Same row/col (abs_dx=0x40, bit5 clear) -> high nibble of 0x21 = 2.
        let without_bit5 = get_quadrant_aim_dir(0x00, 0x00, 0x21, 0x40, &QUADRANT_AIM_DIR_01);
        assert_eq!(without_bit5.aim_dir, 2); // high nibble of 0x21
    }
}
