//! Native port of `find_far_segment_for_x_pos`/`find_far_segment_for_a`
//! (`src/bank7.asm`, CPU `$ed4c`-`$ed66`) - buckets an X position into a
//! 0-6 "horizontal segment" code (6 = farthest left, 0 = farthest right)
//! by comparing against 7 ascending thresholds, tightest (leftmost)
//! first. Real comment: "usually used together to compare player and
//! enemy X positions on indoor levels" - both real callers found
//! (`create_roller`, an indoor roller enemy; `grenade_launcher_
//! routine_01`, an indoor grenade launcher) are indoor/base-level-only
//! enemies, so this routine is **not reachable by this project's current
//! scripted playthrough** (a level-1/outdoor walk-and-shoot) - unlike
//! every other port so far, this one has no live-verification result
//! yet. Confidence instead rests on this being a small, pure table
//! lookup, exhaustively unit-tested across the routine's full 0-255
//! input domain against a direct re-implementation of the real 6502
//! loop's own logic (not just a handful of hand-picked cases).
//!
//! Also carries [`find_close_segment`] (`$967c`, `bank0.asm`) - a real,
//! separate routine reusing the exact same descending-threshold-scan
//! shape (own 7-byte table, own real address) to bucket a *player's* X
//! position instead of an enemy's; the scan itself is factored into one
//! shared private helper here rather than duplicated, even though the
//! real ROM does have two separate copies of the loop.

/// `far_segment_code_tbl` (`$ed5f`, 7 bytes) - ascending thresholds,
/// index 6 (leftmost bucket) has the smallest threshold.
const FAR_SEGMENT_CODE_TBL: [u8; 7] = [0xFF, 0x94, 0x8C, 0x84, 0x7C, 0x74, 0x6C];

/// `indoor_close_segment_tbl` (`$9690`, 7 bytes) - same shape as
/// [`FAR_SEGMENT_CODE_TBL`], different thresholds.
const INDOOR_CLOSE_SEGMENT_TBL: [u8; 7] = [0xFF, 0xBC, 0xA4, 0x8C, 0x74, 0x5C, 0x44];

/// The shared scan both `find_far_segment_for_a` and `find_close_segment`
/// use: highest `y` (6 down to 0) where `x < table[y]`, else `0` (both
/// real ASM's own documented "shouldn't happen" safety fallback - in
/// practice only reached when `x == 0xff`, since `table[0] == 0xff` in
/// both real tables and nothing is `< 0xff` once it already equals
/// `0xff`).
fn descending_segment_scan(x: u8, table: &[u8; 7]) -> u8 {
    for y in (0..=6u8).rev() {
        if x < table[y as usize] {
            return y;
        }
    }
    0
}

/// Native port of `find_far_segment_for_a` (`$ed4e`) - the shared core;
/// `find_far_segment_for_x_pos` (`$ed4c`) is just this applied to a
/// given X position (real ASM: `lda $09` then falls straight through).
pub fn find_far_segment_for_a(x_pos: u8) -> u8 {
    descending_segment_scan(x_pos, &FAR_SEGMENT_CODE_TBL)
}

/// Native port of `find_far_segment_for_x_pos` (`$ed4c`).
pub fn find_far_segment_for_x_pos(enemy_x_pos: u8) -> u8 {
    find_far_segment_for_a(enemy_x_pos)
}

/// Native port of `find_close_segment` (`$967c`) - real ASM reads
/// `SPRITE_X_POS,y` itself (`y` = a player index), so this port takes
/// the resolved `sprite_x_pos` array plus `player_index` rather than a
/// single already-looked-up X value, matching `player_enemy_x_dist`'s
/// own convention for the same array.
pub fn find_close_segment(sprite_x_pos: [u8; 2], player_index: u8) -> u8 {
    descending_segment_scan(sprite_x_pos[player_index as usize], &INDOOR_CLOSE_SEGMENT_TBL)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Direct re-implementation of the real 6502 loop (`cmp`/`bcc`/`dey`/
    /// `bmi`/`bcs`), independent of [`find_far_segment_for_a`]'s own
    /// (more idiomatic) implementation - used to exhaustively cross-check
    /// every possible input rather than trust a handful of hand-picked
    /// cases.
    fn reference_impl(a: u8) -> u8 {
        let mut y: i16 = 6;
        loop {
            if a < FAR_SEGMENT_CODE_TBL[y as usize] {
                return y as u8;
            }
            y -= 1;
            if y < 0 {
                return 0;
            }
        }
    }

    #[test]
    fn matches_the_reference_implementation_for_every_possible_input() {
        for x in 0..=255u8 {
            assert_eq!(find_far_segment_for_a(x), reference_impl(x), "x={x:#04x}");
        }
    }

    #[test]
    fn farthest_left_gives_segment_6() {
        assert_eq!(find_far_segment_for_a(0x00), 6);
    }

    #[test]
    fn boundary_values_match_hand_traced_buckets() {
        // table = [$ff,$94,$8c,$84,$7c,$74,$6c] at indices [0..6]
        assert_eq!(find_far_segment_for_a(0x6B), 6); // < table[6]=$6c
        assert_eq!(find_far_segment_for_a(0x6C), 5); // == table[6], not <, falls to index 5
        assert_eq!(find_far_segment_for_a(0x93), 1); // < table[1]=$94
        assert_eq!(find_far_segment_for_a(0x94), 0); // == table[1], falls to index 0
    }

    #[test]
    fn x_ff_hits_the_safety_fallback_returning_segment_0() {
        // The real ASM's own "shouldn't happen" `@use_code_0` path (loop
        // exhausted, y went negative) is only reached at x=$ff - every
        // other high x value (down to $94) already returns 0 via the
        // *normal* loop matching table[0]=$ff, so the return value alone
        // can't distinguish the two paths; this just confirms the
        // fallback's own output matches what the normal path already
        // produces for neighboring inputs, not that it's uniquely $ff.
        assert_eq!(find_far_segment_for_a(0xFF), 0);
        assert_eq!(find_far_segment_for_a(0x94), 0);
    }

    #[test]
    fn for_x_pos_is_the_same_function_as_for_a() {
        for x in 0..=255u8 {
            assert_eq!(find_far_segment_for_x_pos(x), find_far_segment_for_a(x));
        }
    }

    /// Same shape as [`reference_impl`] but against
    /// [`INDOOR_CLOSE_SEGMENT_TBL`], for [`find_close_segment`].
    fn close_segment_reference_impl(a: u8) -> u8 {
        let mut y: i16 = 6;
        loop {
            if a < INDOOR_CLOSE_SEGMENT_TBL[y as usize] {
                return y as u8;
            }
            y -= 1;
            if y < 0 {
                return 0;
            }
        }
    }

    #[test]
    fn find_close_segment_matches_the_reference_implementation_for_every_possible_input() {
        for x in 0..=255u8 {
            assert_eq!(descending_segment_scan(x, &INDOOR_CLOSE_SEGMENT_TBL), close_segment_reference_impl(x), "x={x:#04x}");
        }
    }

    #[test]
    fn find_close_segment_reads_the_indexed_player_from_the_sprite_x_pos_array() {
        let sprite_x_pos = [0x50, 0xE0];
        assert_eq!(find_close_segment(sprite_x_pos, 0), descending_segment_scan(0x50, &INDOOR_CLOSE_SEGMENT_TBL));
        assert_eq!(find_close_segment(sprite_x_pos, 1), descending_segment_scan(0xE0, &INDOOR_CLOSE_SEGMENT_TBL));
    }

    #[test]
    fn find_close_segment_uses_its_own_table_not_the_far_segment_one() {
        // Same input, different tables -> generally different results;
        // pick a value that lands in different buckets for each table.
        assert_ne!(descending_segment_scan(0x50, &FAR_SEGMENT_CODE_TBL), descending_segment_scan(0x50, &INDOOR_CLOSE_SEGMENT_TBL));
    }
}
