//! Native port of `add_scroll_to_enemy_pos` (`src/bank7.asm`, CPU
//! `$e8a7`-`$e8c6`, sharing the "remove" path with `remove_enemy`/
//! `set_sprite_0`, `$e809`-`$e813`) - the single most-reused routine
//! found in this codebase so far: **82 real call sites** (75 in
//! `bank0.asm`, 7 in `bank7.asm` itself), applied to essentially every
//! enemy/bullet object every frame to keep its on-screen position in
//! sync with camera scroll, and to flag it for removal once it scrolls
//! far enough off-screen.
//!
//! Real ASM branches on `LEVEL_SCROLLING_TYPE` (0 = horizontal, the
//! outdoor/typical case; nonzero = vertical) and only ever touches *one*
//! axis - the other stays whatever it already was. This port's
//! `should_remove` flag mirrors the real decision (`ENEMY_X_POS < $08`
//! horizontally, `ENEMY_Y_POS >= $e8` vertically) but **does not itself
//! perform the removal** - a caller integrating this port live should
//! call [`crate::enemy::update_enemy_pos::remove_enemy`] when `should_remove`
//! is `true`, exactly like the real ASM's `remove_enemy_far: jmp
//! remove_enemy` does (that function now lives in the sibling
//! `update_enemy_pos` module, which shares the same real `remove_enemy`/
//! `set_sprite_0` routine this one jumps into).

/// The result of one frame's scroll update: the enemy's new position
/// (only one axis actually changes, depending on `LEVEL_SCROLLING_TYPE`)
/// and whether the real ASM would have removed it for scrolling too far
/// off-screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrolledEnemyPos {
    pub x_pos: u8,
    pub y_pos: u8,
    pub should_remove: bool,
}

/// Native port of `add_scroll_to_enemy_pos` (`$e8a7`) /
/// `add_horizontal_scroll` (`$e8b9`).
pub fn add_scroll_to_enemy_pos(level_scrolling_type: u8, frame_scroll: u8, enemy_x_pos: u8, enemy_y_pos: u8) -> ScrolledEnemyPos {
    if level_scrolling_type == 0 {
        // Horizontal (outdoor) level: subtract scroll from X, remove if
        // it's scrolled off the left edge (`cmp #$08 / bcc`).
        let x_pos = enemy_x_pos.wrapping_sub(frame_scroll);
        ScrolledEnemyPos { x_pos, y_pos: enemy_y_pos, should_remove: x_pos < 0x08 }
    } else {
        // Vertical level: add scroll to Y, remove if it's scrolled off
        // the bottom edge (`cmp #$e8 / bcs`).
        let y_pos = enemy_y_pos.wrapping_add(frame_scroll);
        ScrolledEnemyPos { x_pos: enemy_x_pos, y_pos, should_remove: y_pos >= 0xE8 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_scroll_moves_x_left_and_leaves_y_untouched() {
        let r = add_scroll_to_enemy_pos(0, 0x05, 0x80, 0x40);
        assert_eq!(r, ScrolledEnemyPos { x_pos: 0x7B, y_pos: 0x40, should_remove: false });
    }

    #[test]
    fn horizontal_scroll_flags_removal_below_8() {
        let r = add_scroll_to_enemy_pos(0, 0x05, 0x0A, 0x40);
        assert_eq!(r.x_pos, 0x05);
        assert!(r.should_remove);
        // right at the boundary: exactly $08 is NOT removed (only < $08 is)
        let boundary = add_scroll_to_enemy_pos(0, 0x02, 0x0A, 0x40);
        assert_eq!(boundary.x_pos, 0x08);
        assert!(!boundary.should_remove);
    }

    #[test]
    fn horizontal_scroll_wraps_on_underflow() {
        // Real 6502 SBC wraps mod 256 - enemies far enough left with a
        // big scroll delta wrap around, but still end up < $08 so still
        // get flagged for removal (the wrapped value can land >= $08
        // too depending on magnitude - this case: 0x03 - 0x05 wraps to
        // 0xFE, which is NOT < 0x08, so NOT removed by this check alone,
        // real behavior ported faithfully rather than "corrected").
        let r = add_scroll_to_enemy_pos(0, 0x05, 0x03, 0x40);
        assert_eq!(r.x_pos, 0xFE);
        assert!(!r.should_remove);
    }

    #[test]
    fn vertical_scroll_moves_y_down_and_leaves_x_untouched() {
        let r = add_scroll_to_enemy_pos(1, 0x05, 0x80, 0x40);
        assert_eq!(r, ScrolledEnemyPos { x_pos: 0x80, y_pos: 0x45, should_remove: false });
    }

    #[test]
    fn vertical_scroll_flags_removal_at_or_past_0xe8() {
        let r = add_scroll_to_enemy_pos(1, 0x10, 0x80, 0xDF);
        assert_eq!(r.y_pos, 0xEF);
        assert!(r.should_remove);
        // exactly $e8 also removed (>=, not >)
        let boundary = add_scroll_to_enemy_pos(1, 0x08, 0x80, 0xE0);
        assert_eq!(boundary.y_pos, 0xE8);
        assert!(boundary.should_remove);
        // one less than the boundary is not removed
        let just_under = add_scroll_to_enemy_pos(1, 0x07, 0x80, 0xE0);
        assert_eq!(just_under.y_pos, 0xE7);
        assert!(!just_under.should_remove);
    }
}
