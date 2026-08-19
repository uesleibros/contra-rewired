//! Native port of Contra's "aim at the player" logic, `src/bank7.asm`,
//! CPU `$f52c`-`$f5ab`: [`get_quadrant_aim_dir`] (`$f55e`) is the core
//! "which way should this enemy aim to hit a target" routine - computes
//! the absolute Y/X distance between an enemy and its target, buckets
//! each into a coarse row/column, and looks up a nibble-packed angle
//! byte from one of 3 real tables (`quadrant_aim_dir_00`/`_01`/`_02`,
//! `$f5b2`-`$f611`). [`get_quadrant_aim_dir_for_player`] (`$f52c`) is its
//! real caller one level up - resolves *which* player position to
//! target (falling back to the requested player's teammate if they're
//! not in a normal state, and to a fixed off-screen position if
//! *neither* player is), then delegates to [`get_quadrant_aim_dir`].
//! `aim_and_create_enemy_bullet` (the dragon-boss-arm-orb path, which
//! calls [`get_quadrant_aim_dir`] directly with an already-known target
//! instead of going through the player-resolution step) is not ported.
//!
//! [`get_rotate_dir`] (`$f466`) is the next layer up - "given an already-
//! computed aim direction and quadrant, and the enemy's own *current*
//! stored aim direction, which way (if any) should it rotate to face the
//! target": reflects the raw aim direction across whichever axes the
//! target quadrant calls for, then compares the result against the
//! current direction (handling the direction-index wraparound at the
//! table's own max value) to decide clockwise/counterclockwise/no
//! change. [`get_rotate_00`]/[`get_rotate_01`] (`$f44d`/`$f451`) are its
//! real callers' entry points, fixing the table selector to `0`/`1`
//! (`quadrant_aim_dir_00`/`_01`) before resolving the target via
//! [`get_quadrant_aim_dir_for_player`] - `get_rotate_02` (there's no
//! real label for it; nothing in the ROM ever calls `get_rotate_dir_for_
//! index` with table selector `2`) doesn't exist. The real ASM's own
//! third entry point, `get_rotate_dir_for_index`, has one more path this
//! port doesn't model: if the player-index input is negative (bit 7
//! set), it reads `$0c`'s *prior* value (from whatever the caller last
//! left there) as if it were a target Y position and aims at that
//! directly - the real disassembly's own comment on this branch is "not
//! sure when this happens"; every real caller of `get_rotate_00`/`_01`
//! passes a player index straight from `player_enemy_x_dist` (always
//! `0`/`1`), so this port only models the always-reachable "resolve via
//! player state" path.

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
/// like [`crate::physics::bullet_physics::calc_bullet_velocities`] can consume)
/// and `quadrant` (the same 0-3 quadrant code
/// [`crate::enemy::create_enemy_bullet::create_enemy_bullet`] takes directly).
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

/// Real `PLAYER_STATE` value meaning "normal" (able to be targeted) -
/// any other value means falling, dead, or otherwise unable to move.
pub const PLAYER_STATE_NORMAL: u8 = 1;

/// Native port of `get_quadrant_aim_dir_for_player` (`$f52c`): resolves
/// which player to actually target - `player_index` (masked to 0/1) if
/// they're in a normal state, else their teammate if *that* player is
/// normal, else a fixed off-screen fallback position (`Y=$ff, X=$80`,
/// real comment: "bottom of screen"/"center of screen") - then
/// delegates to [`get_quadrant_aim_dir`]. `player_state`/`sprite_y_pos`/
/// `sprite_x_pos` are indexed `[player 1, player 2]`, matching the real
/// `PLAYER_STATE`/`SPRITE_Y_POS`/`SPRITE_X_POS` arrays' own layout.
#[allow(clippy::too_many_arguments)]
pub fn get_quadrant_aim_dir_for_player(
    source_y: u8,
    source_x: u8,
    player_index: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    table: &QuadrantAimDirTable,
) -> QuadrantAimDir {
    let requested = (player_index & 0x01) as usize;
    let target_index = if player_state[requested] == PLAYER_STATE_NORMAL {
        Some(requested)
    } else {
        let other = requested ^ 1;
        if player_state[other] == PLAYER_STATE_NORMAL { Some(other) } else { None }
    };

    let (target_y, target_x) = match target_index {
        Some(idx) => {
            // Real comment: "player Y position is set at #$a8 on indoor
            // levels" (loaded here as #$b0 - see the real ASM; this
            // port follows the real load, not the comment's own typo'd
            // value) - "note this means enemies won't aim correctly at
            // a player who is jumping on an indoor level", a real,
            // documented imprecision ported faithfully.
            let y = if level_location_type & 1 != 0 { 0xB0 } else { sprite_y_pos[idx] };
            (y, sprite_x_pos[idx])
        }
        None => (0xFF, 0x80),
    };

    get_quadrant_aim_dir(source_y, source_x, target_y, target_x, table)
}

/// `get_rotate_dir`'s own reflected-direction midpoint/max, selected by
/// table parity (`$f466`'s own `lsr`/`bcc` on the table selector):
/// `quadrant_aim_dir_01` (odd selector) uses a 24-step direction wheel
/// (midway `$0c`, max `$18`); `_00`/`_02` (even selector) use a 12-step
/// wheel (midway `$06`, max `$0c`).
fn rotate_midway_max(table_index: u8) -> (u8, u8) {
    if table_index & 0x01 != 0 { (0x0C, 0x18) } else { (0x06, 0x0C) }
}

/// Which way (if any) an enemy should rotate to face its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationDirection {
    Clockwise,
    CounterClockwise,
    /// Already aiming at the target - real ASM's own negative-flag/`$80`
    /// sentinel.
    NoChangeNeeded,
}

/// The full result of one [`get_rotate_dir`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GetRotateDirResult {
    /// `$0c` - the new aim direction, reflected into the correct
    /// half/quadrant of the direction wheel. This is the only field
    /// [`crate::enemy::sniper`]'s own callers actually use - they never
    /// look at `direction` at all, matching the real ASM (`sniper_
    /// routine_02` reads `$0c` straight back out and never inspects the
    /// rotation-direction return value `get_rotate_dir` leaves in `a`).
    pub new_aim_dir: u8,
    pub direction: RotationDirection,
}

/// Native port of `get_rotate_dir` (`$f466`) - see this module's doc
/// comment for the overall shape. `aim_dir`/`quadrant` are [`get_
/// quadrant_aim_dir`]'s own two outputs for the resolved target;
/// `current_aim_dir` is the enemy's own persisted aim direction (`ENEMY_
/// VAR_1`) to rotate *from*.
pub fn get_rotate_dir(aim_dir: u8, quadrant: u8, table_index: u8, current_aim_dir: u8) -> GetRotateDirResult {
    let (midway, max) = rotate_midway_max(table_index);

    let mut new_dir = aim_dir;
    if quadrant & 0x02 != 0 {
        // Target to the left of the enemy - reflect across the vertical
        // axis.
        new_dir = midway.wrapping_sub(new_dir);
    }
    if quadrant & 0x01 != 0 {
        // Target above the enemy - reflect across the horizontal axis,
        // clamping the one case (`new_dir == 0`) the real `sbc`/`cmp`
        // pair forces back to `0` instead of leaving it at `max`.
        let reflected = max.wrapping_sub(new_dir);
        new_dir = if reflected < max { reflected } else { 0x00 };
    }

    let summed = current_aim_dir.wrapping_add(midway);
    let (reflected_current, wrapped) = if summed < max { (summed, false) } else { (summed.wrapping_sub(max), true) };

    let direction = if new_dir == current_aim_dir {
        RotationDirection::NoChangeNeeded
    } else if !wrapped {
        if new_dir < current_aim_dir {
            RotationDirection::CounterClockwise
        } else if new_dir >= reflected_current {
            RotationDirection::CounterClockwise
        } else {
            RotationDirection::Clockwise
        }
    } else if new_dir > current_aim_dir {
        RotationDirection::Clockwise
    } else if new_dir < reflected_current {
        RotationDirection::Clockwise
    } else {
        RotationDirection::CounterClockwise
    };

    GetRotateDirResult { new_aim_dir: new_dir, direction }
}

/// Native port of `get_rotate_dir_for_index` (`$f453`) - resolves the
/// target via [`get_quadrant_aim_dir_for_player`] (using
/// `quadrant_aim_dir_lookup_ptr_tbl[table_index]`), then [`get_rotate_
/// dir`]. See this module's doc comment for the real negative-`player_
/// index` branch this port doesn't model.
#[allow(clippy::too_many_arguments)]
pub fn get_rotate_dir_for_index(
    table_index: u8,
    source_y: u8,
    source_x: u8,
    player_index: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_aim_dir: u8,
) -> GetRotateDirResult {
    let table = match table_index {
        0 => &QUADRANT_AIM_DIR_00,
        1 => &QUADRANT_AIM_DIR_01,
        _ => &QUADRANT_AIM_DIR_02,
    };
    let aimed =
        get_quadrant_aim_dir_for_player(source_y, source_x, player_index, player_state, sprite_y_pos, sprite_x_pos, level_location_type, table);
    get_rotate_dir(aimed.aim_dir, aimed.quadrant, table_index, current_aim_dir)
}

/// Native port of `get_rotate_00` (`$f44d`) - [`get_rotate_dir_for_
/// index`] fixed to table selector `0` (`quadrant_aim_dir_00`).
#[allow(clippy::too_many_arguments)]
pub fn get_rotate_00(
    source_y: u8,
    source_x: u8,
    player_index: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_aim_dir: u8,
) -> GetRotateDirResult {
    get_rotate_dir_for_index(0, source_y, source_x, player_index, player_state, sprite_y_pos, sprite_x_pos, level_location_type, current_aim_dir)
}

/// Native port of `get_rotate_01` (`$f451`) - [`get_rotate_dir_for_
/// index`] fixed to table selector `1` (`quadrant_aim_dir_01`).
#[allow(clippy::too_many_arguments)]
pub fn get_rotate_01(
    source_y: u8,
    source_x: u8,
    player_index: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_aim_dir: u8,
) -> GetRotateDirResult {
    get_rotate_dir_for_index(1, source_y, source_x, player_index, player_state, sprite_y_pos, sprite_x_pos, level_location_type, current_aim_dir)
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

    #[test]
    fn for_player_targets_the_requested_player_when_normal() {
        let r = get_quadrant_aim_dir_for_player(
            0x00,
            0x00,
            0,                    // requested = player 1
            [1, 1],               // both normal
            [0x40, 0x99],         // player 1 Y = 0x40, player 2 Y = 0x99
            [0x50, 0x88],
            0,                    // outdoor
            &QUADRANT_AIM_DIR_00,
        );
        let direct = get_quadrant_aim_dir(0x00, 0x00, 0x40, 0x50, &QUADRANT_AIM_DIR_00);
        assert_eq!(r, direct);
    }

    #[test]
    fn for_player_falls_back_to_teammate_when_requested_is_not_normal() {
        let r = get_quadrant_aim_dir_for_player(
            0x00,
            0x00,
            0,          // requested = player 1, but not normal
            [0, 1],     // player 1 falling/dead, player 2 normal
            [0x40, 0x99],
            [0x50, 0x88],
            0,
            &QUADRANT_AIM_DIR_00,
        );
        let direct = get_quadrant_aim_dir(0x00, 0x00, 0x99, 0x88, &QUADRANT_AIM_DIR_00);
        assert_eq!(r, direct);
    }

    #[test]
    fn for_player_falls_back_to_offscreen_position_when_neither_player_is_normal() {
        let r = get_quadrant_aim_dir_for_player(0x00, 0x00, 0, [0, 0], [0x40, 0x99], [0x50, 0x88], 0, &QUADRANT_AIM_DIR_00);
        let direct = get_quadrant_aim_dir(0x00, 0x00, 0xFF, 0x80, &QUADRANT_AIM_DIR_00);
        assert_eq!(r, direct);
    }

    #[test]
    fn for_player_overrides_y_but_not_x_on_indoor_levels() {
        let r = get_quadrant_aim_dir_for_player(0x00, 0x00, 0, [1, 1], [0x40, 0x99], [0x50, 0x88], 1, &QUADRANT_AIM_DIR_00);
        let direct = get_quadrant_aim_dir(0x00, 0x00, 0xB0, 0x50, &QUADRANT_AIM_DIR_00);
        assert_eq!(r, direct);
    }

    #[test]
    fn rotate_dir_quadrant_0_passes_the_aim_dir_through_unreflected() {
        let r = get_rotate_dir(0x05, 0x00, 0x00, 0x05);
        assert_eq!(r.new_aim_dir, 0x05);
        assert_eq!(r.direction, RotationDirection::NoChangeNeeded);
    }

    #[test]
    fn rotate_dir_reflects_across_the_vertical_axis_when_target_is_left() {
        // table 0: midway=6. aim_dir=2, quadrant bit1 set -> 6-2=4.
        let r = get_rotate_dir(0x02, 0x02, 0x00, 0x00);
        assert_eq!(r.new_aim_dir, 0x04);
    }

    #[test]
    fn rotate_dir_reflects_across_the_horizontal_axis_when_target_is_above() {
        // table 0: max=12. aim_dir=3, quadrant bit0 set -> 12-3=9.
        let r = get_rotate_dir(0x03, 0x01, 0x00, 0x00);
        assert_eq!(r.new_aim_dir, 0x09);
    }

    #[test]
    fn rotate_dir_composes_both_reflections_in_order() {
        // table 0: aim_dir=2, quadrant=3 (both bits) -> left: 6-2=4, then top: 12-4=8.
        let r = get_rotate_dir(0x02, 0x03, 0x00, 0x00);
        assert_eq!(r.new_aim_dir, 0x08);
    }

    #[test]
    fn rotate_dir_horizontal_reflection_clamps_the_zero_case_to_zero_not_max() {
        // table 0: max=12. aim_dir=0, quadrant bit0 set -> 12-0=12, clamped to 0.
        let r = get_rotate_dir(0x00, 0x01, 0x00, 0x00);
        assert_eq!(r.new_aim_dir, 0x00);
    }

    #[test]
    fn rotate_dir_no_wrap_counterclockwise_when_new_is_smaller() {
        // table 0 (midway=6, max=12): current=3 -> summed=9<12, no wrap.
        let r = get_rotate_dir(0x01, 0x00, 0x00, 0x03);
        assert_eq!(r.direction, RotationDirection::CounterClockwise);
    }

    #[test]
    fn rotate_dir_no_wrap_clockwise_when_new_is_between_current_and_reflected() {
        // current=2 -> summed=8<12, reflected_current=8. new_dir=5: 2<5<8.
        let r = get_rotate_dir(0x05, 0x00, 0x00, 0x02);
        assert_eq!(r.direction, RotationDirection::Clockwise);
    }

    #[test]
    fn rotate_dir_no_wrap_counterclockwise_when_new_passes_the_reflected_current() {
        // current=1 -> summed=7<12, reflected_current=7. new_dir=9 >= 7.
        let r = get_rotate_dir(0x09, 0x00, 0x00, 0x01);
        assert_eq!(r.direction, RotationDirection::CounterClockwise);
    }

    #[test]
    fn rotate_dir_wrapped_clockwise_when_new_exceeds_current() {
        // current=8 -> summed=14>=12, wrapped, reflected_current=2. new_dir=10>8.
        let r = get_rotate_dir(0x0A, 0x00, 0x00, 0x08);
        assert_eq!(r.direction, RotationDirection::Clockwise);
    }

    #[test]
    fn rotate_dir_wrapped_clockwise_when_new_is_below_the_reflected_current() {
        // current=8 (wrapped, reflected_current=2). new_dir=1 < current, 1 < 2.
        let r = get_rotate_dir(0x01, 0x00, 0x00, 0x08);
        assert_eq!(r.direction, RotationDirection::Clockwise);
    }

    #[test]
    fn rotate_dir_wrapped_counterclockwise_when_new_reaches_the_reflected_current() {
        // current=8 (wrapped, reflected_current=2). new_dir=5 < current, 5 >= 2.
        let r = get_rotate_dir(0x05, 0x00, 0x00, 0x08);
        assert_eq!(r.direction, RotationDirection::CounterClockwise);
    }

    #[test]
    fn rotate_dir_for_index_selects_the_matching_table_and_delegates_to_rotate_dir() {
        let r = get_rotate_dir_for_index(0, 0x00, 0x00, 0, [1, 1], [0x40, 0x50], [0x40, 0x50], 0, 0x00);
        let aimed = get_quadrant_aim_dir_for_player(0x00, 0x00, 0, [1, 1], [0x40, 0x50], [0x40, 0x50], 0, &QUADRANT_AIM_DIR_00);
        let expected = get_rotate_dir(aimed.aim_dir, aimed.quadrant, 0, 0x00);
        assert_eq!(r, expected);
    }

    #[test]
    fn rotate_00_uses_table_0() {
        let r = get_rotate_00(0x50, 0x50, 0, [1, 0], [0x30, 0x00], [0x90, 0x00], 0, 0x00);
        let expected = get_rotate_dir_for_index(0, 0x50, 0x50, 0, [1, 0], [0x30, 0x00], [0x90, 0x00], 0, 0x00);
        assert_eq!(r, expected);
    }

    #[test]
    fn rotate_01_uses_table_1() {
        let r = get_rotate_01(0x50, 0x50, 0, [1, 0], [0x30, 0x00], [0x90, 0x00], 0, 0x00);
        let expected = get_rotate_dir_for_index(1, 0x50, 0x50, 0, [1, 0], [0x30, 0x00], [0x90, 0x00], 0, 0x00);
        assert_eq!(r, expected);
        // table 1 uses the wider (midway=12, max=24) wheel - a case that
        // wraps under table 0 (max=12) shouldn't necessarily wrap here.
        let (midway, max) = rotate_midway_max(1);
        assert_eq!((midway, max), (0x0C, 0x18));
    }
}
