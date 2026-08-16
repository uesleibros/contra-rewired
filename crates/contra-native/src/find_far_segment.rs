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

/// `far_segment_code_tbl` (`$ed5f`, 7 bytes) - ascending thresholds,
/// index 6 (leftmost bucket) has the smallest threshold.
const FAR_SEGMENT_CODE_TBL: [u8; 7] = [0xFF, 0x94, 0x8C, 0x84, 0x7C, 0x74, 0x6C];

/// Native port of `find_far_segment_for_a` (`$ed4e`) - the shared core;
/// `find_far_segment_for_x_pos` (`$ed4c`) is just this applied to a
/// given X position (real ASM: `lda $09` then falls straight through).
/// Scans `y` from 6 down to 0, returning the first (highest) `y` where
/// `x_pos < FAR_SEGMENT_CODE_TBL[y]` - `0` if none match (real ASM's own
/// documented "shouldn't happen" safety fallback, `@use_code_0`; in
/// practice this happens only for `x_pos == 0xff`, since
/// `FAR_SEGMENT_CODE_TBL[0] == 0xff` and nothing is `< 0xff` when it
/// already equals `0xff`).
pub fn find_far_segment_for_a(x_pos: u8) -> u8 {
    for y in (0..=6u8).rev() {
        if x_pos < FAR_SEGMENT_CODE_TBL[y as usize] {
            return y;
        }
    }
    0
}

/// Native port of `find_far_segment_for_x_pos` (`$ed4c`).
pub fn find_far_segment_for_x_pos(enemy_x_pos: u8) -> u8 {
    find_far_segment_for_a(enemy_x_pos)
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
}
