//! Native port of Contra's bullet velocity math: [`adjust_bullet_velocity`]
//! (`adjust_bullet_velocity`/`bullet_velocity_adjust_00`-`_08`, `src/
//! bank7.asm`, CPU `$f3a5`-`$f419`) scales one axis of a bullet's
//! fixed-point velocity (a `(frac, fast)` pair, `value = fast +
//! frac/256`) by a weapon's speed code; [`calc_bullet_velocities`]
//! (`calc_bullet_velocities`, `$f334`-`$f37e`) is its real caller - looks
//! up a base X/Y velocity for an aim direction, scales both axes through
//! [`adjust_bullet_velocity`], then negates whichever axis should point
//! the other way.
//!
//! Ported using [`tools/nesrecomp-reference`](../../../tools/nesrecomp-reference)
//! (a static-recompilation reading aid - see `docs/NATIVE_PORT.md`'s
//! methodology section) to read the mechanically-translated C for
//! `bullet_velocity_adjust_00` alongside the raw 6502 as a faster first
//! pass; verified the usual way regardless - against the disassembly's
//! own 3 worked examples (this module's tests), not the generated C,
//! which is a reading aid and not a source of truth.
//!
//! ## A real, deliberate quirk: speed codes 3/5/6/7 branch on data
//!
//! This isn't a clean "multiply by a fraction" - it's the literal real
//! 6502 control flow, replicated exactly rather than simplified. Speed
//! codes 1 and 3 reuse `bullet_velocity_adjust_04`'s body (sometimes on
//! *already-halved* values, sometimes fresh - see [`adjust_01`] and
//! [`adjust_03`]'s doc comments), and 5/6/7 reuse a second shared tail
//! (`bullet_dir_half_a_add_to_vel`, see [`half_a_add_to_vel`]). Codes 3,
//! 5, and 6 each have a **data-dependent branch** (`bpl`, i.e. "is the
//! intermediate result's bit 7 clear") that picks between two different
//! computations - the disassembly's own comments note this doesn't
//! always produce a mathematically "clean" scaled result for every input
//! (e.g. code 6's comment: "doesn't work correctly when carry from
//! fractional velocity, e.g. 1.5 becomes 1.62 and not 2.62") - real,
//! verified hardware behavior, ported faithfully rather than corrected.
//!
//! Speed code 8 (`bullet_velocity_adjust_08`, 2x speed) is real ROM data
//! but is documented as unused - the only caller masks the speed code
//! with `& 0x07` before dispatching, so 8 is never actually reachable.
//! Not ported here (matching this crate's standing policy: don't ship a
//! path nothing can verify as reachable).

fn lsr(v: u8) -> (u8, bool) {
    (v >> 1, v & 1 != 0)
}

fn ror(v: u8, carry_in: bool) -> (u8, bool) {
    let out = (v >> 1) | if carry_in { 0x80 } else { 0 };
    (out, v & 1 != 0)
}

/// `bullet_velocity_adjust_add_a` - the shared tail every speed code
/// (except 0 and 2) ends with: `a` (an already-computed intermediate)
/// plus the *current* `frac`, carrying into `fast`.
fn add_a(a: u8, frac: u8, fast: u8) -> (u8, u8) {
    let (sum, carry) = a.overflowing_add(frac);
    (sum, fast.wrapping_add(carry as u8))
}

/// `bullet_velocity_adjust_04`'s own body, used standalone by speed code
/// 4 and reused (on possibly-different inputs) by codes 1 and 3: halves
/// the 16-bit `(fast:frac)` pair one more time, but only keeps the
/// *fractional* half of the result (the halved `fast` byte is computed
/// only for its carry-out, then discarded) - real, deliberate behavior,
/// not a bug in this port.
fn halve_frac_only(frac: u8, fast: u8) -> u8 {
    let (_, carry) = lsr(fast);
    let (a, _) = ror(frac, carry);
    a
}

/// `bullet_dir_half_a_add_to_vel` - the shared tail for speed codes 5
/// (when its own `bpl` is taken) and 6: halves `a` again, adds back the
/// pre-shift value (`mem00`, saved before that halving), then joins
/// [`add_a`].
fn half_a_add_to_vel(a: u8, mem00: u8, frac: u8, fast: u8) -> (u8, u8) {
    let (a, _) = lsr(a);
    let (a, _) = a.overflowing_add(mem00);
    add_a(a, frac, fast)
}

/// Speed code 0 (0.5x): halve the full 16-bit `(fast:frac)` pair.
/// Worked example (`src/bank7.asm`'s own comment): 3.5 (`$03 $80`) ->
/// 1.75 (`$01 $c0`).
fn adjust_00(frac: u8, fast: u8) -> (u8, u8) {
    let (fast, carry) = lsr(fast);
    let (frac, _) = ror(frac, carry);
    (frac, fast)
}

/// Speed code 1 (0.75x): halve the pair (write-back, like code 0), then
/// add a quarter computed via [`halve_frac_only`] from the *already-
/// halved* values - `v/2 + v/4`.
fn adjust_01(frac: u8, fast: u8) -> (u8, u8) {
    let (frac, fast) = adjust_00(frac, fast);
    let a = halve_frac_only(frac, fast);
    add_a(a, frac, fast)
}

/// Speed code 3 (1.25x): halve the pair *without writing back*, halve
/// the fractional result again, then branch on its sign (`bpl`) - if bit
/// 7 is clear, add that value directly; otherwise fall through to
/// [`adjust_04`]'s fresh computation from the (still-unmodified)
/// original `frac`/`fast` instead.
fn adjust_03(frac: u8, fast: u8) -> (u8, u8) {
    let (_, carry) = lsr(fast);
    let (a, _) = ror(frac, carry);
    let (a, _) = lsr(a);
    if a & 0x80 == 0 {
        add_a(a, frac, fast)
    } else {
        adjust_04(frac, fast)
    }
}

/// Speed code 4 (1.5x): `v/2 + v/4`, both computed fresh via
/// [`halve_frac_only`] from the original `frac`/`fast`.
fn adjust_04(frac: u8, fast: u8) -> (u8, u8) {
    let a = halve_frac_only(frac, fast);
    add_a(a, frac, fast)
}

/// Speed code 5 (1.62x): like [`adjust_04`]'s first halving, saved to a
/// scratch value, halved again and branched on sign (`bpl`) - taken
/// joins [`half_a_add_to_vel`] with the once-more-halved value; not
/// taken falls through to [`adjust_06`]'s fresh computation instead.
fn adjust_05(frac: u8, fast: u8) -> (u8, u8) {
    let (_, carry) = lsr(fast);
    let (a, _) = ror(frac, carry);
    let mem00 = a;
    let (a, _) = lsr(a);
    if a & 0x80 == 0 {
        half_a_add_to_vel(a, mem00, frac, fast)
    } else {
        adjust_06(frac, fast)
    }
}

/// Speed code 6 (1.75x): [`halve_frac_only`]'s value, then
/// [`half_a_add_to_vel`] on it directly (no branch). Disassembly's own
/// comment: "doesn't work correctly when carry from fractional
/// velocity, e.g. 1.5 becomes 1.62 and not 2.62" - real behavior, ported
/// as-is. Worked example: 1.1 (`$01 $1a`) -> 1.92 (`$01 $ed`).
fn adjust_06(frac: u8, fast: u8) -> (u8, u8) {
    let (_, carry) = lsr(fast);
    let (a, _) = ror(frac, carry);
    half_a_add_to_vel(a, a, frac, fast)
}

/// Speed code 7 (1.87x): the same first halving as code 6, then two more
/// halve-and-add steps chained together before [`add_a`]. Disassembly's
/// own comment: same carry caveat as code 6. Worked example: 1.05
/// (`$01 $0d`) -> 1.96 (`$01 $f7`).
fn adjust_07(frac: u8, fast: u8) -> (u8, u8) {
    let (_, carry) = lsr(fast);
    let (a, _) = ror(frac, carry);
    let mem00 = a;
    let (a, _) = lsr(a);
    let mem01 = a;
    let (a, _) = a.overflowing_add(mem00);
    let (mem01, _) = lsr(mem01);
    let (a, _) = a.overflowing_add(mem01);
    add_a(a, frac, fast)
}

/// Adjusts one axis of a bullet's fixed-point velocity (`frac`/`fast`,
/// `value = fast + frac/256`) by weapon speed code `speed_code` (`0`-`7`
/// after masking, matching the real caller's `and #$07` before
/// dispatch - see this module's doc comment for why `8`/2x speed isn't
/// ported). Returns the adjusted `(frac, fast)` pair.
pub fn adjust_bullet_velocity(frac: u8, fast: u8, speed_code: u8) -> (u8, u8) {
    match speed_code & 0x07 {
        0 => adjust_00(frac, fast),
        1 => adjust_01(frac, fast),
        2 => (frac, fast),
        3 => adjust_03(frac, fast),
        4 => adjust_04(frac, fast),
        5 => adjust_05(frac, fast),
        6 => adjust_06(frac, fast),
        7 => adjust_07(frac, fast),
        _ => unreachable!("masked with & 0x07 above"),
    }
}

/// `bullet_fract_vel_dir_lookup_tbl` (`bank7.asm`, `$f37f`-`$f396`, 24
/// bytes) - maps a "quadrant aim dir" (0-23, six sub-directions per
/// quadrant) to a byte *offset* into [`BULLET_FRACT_VEL_TBL`] (divide by 2
/// for the pair index - each entry there is 2 bytes, Y then X).
const BULLET_FRACT_VEL_DIR_LOOKUP_TBL: [u8; 24] = [
    0x00, 0x02, 0x04, 0x06, 0x08, 0x0a, // quadrant IV
    0x0c, 0x0a, 0x08, 0x06, 0x04, 0x02, // quadrant III
    0x00, 0x02, 0x04, 0x06, 0x08, 0x0a, // quadrant II
    0x0c, 0x0a, 0x08, 0x06, 0x04, 0x02, // quadrant I
];

/// `bullet_fract_vel_tbl` (`bank7.asm`, `$f397`-`$f3a3`, 14 bytes / 7
/// `(Y fractional velocity, X fractional velocity)` pairs), indexed by
/// [`BULLET_FRACT_VEL_DIR_LOOKUP_TBL`]'s output (halved to a pair index).
const BULLET_FRACT_VEL_TBL: [(u8, u8); 7] = [
    (0x00, 0xff),
    (0x42, 0xf7),
    (0x80, 0xdd),
    (0xb5, 0xb5),
    (0xdd, 0x80),
    (0xf7, 0x42),
    (0xff, 0x00), // shooting horizontally
];

/// Negates a `(frac, fast)` fixed-point velocity pair as one 16-bit
/// quantity (`frac` low byte, `fast` high byte) - the real routine's
/// `lda #$00 / sec / sbc frac / sta frac / lda #$00 / sbc fast / sta fast`
/// idiom, a standard chained-borrow 16-bit two's-complement negation.
/// `pub(crate)` since [`crate::enemy_position_utils::reverse_enemy_x_direction`]
/// uses the exact same idiom on a different velocity pair.
pub(crate) fn negate16(frac: u8, fast: u8) -> (u8, u8) {
    let v = ((fast as u16) << 8) | frac as u16;
    let neg = v.wrapping_neg();
    (neg as u8, (neg >> 8) as u8)
}

/// The X/Y velocity a bullet/projectile should be given, as computed by
/// [`calc_bullet_velocities`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletVelocity {
    pub frac_y: u8,
    pub fast_y: u8,
    pub frac_x: u8,
    pub fast_x: u8,
}

/// Native port of `calc_bullet_velocities` (`bank7.asm`, `$f334`-`$f37e`) -
/// the real routine's own comment: "used by bullets, eye projectile, and
/// spinning bubbles". Looks up a base X/Y fractional velocity for
/// `aim_dir` (a 0-23 index - see [`BULLET_FRACT_VEL_DIR_LOOKUP_TBL`]'s doc
/// comment for the quadrant layout), scales both axes by `speed_code` via
/// [`adjust_bullet_velocity`], then negates whichever axis `quadrant`'s
/// low two bits say should point the other way (bit 0: top half of the
/// plane -> negate Y; bit 1: left half -> negate X) - matching the real
/// routine exactly, including doing X's lookup+scale *before* Y's (the
/// two don't interact, so the order is only observable if this were
/// mid-refactored to share more state, which it isn't).
///
/// `aim_dir` is only ever passed already masked to 5 bits (`& $1f`, 0-31)
/// by every real caller (see `@create_enemy_bullet` in `bank7.asm`), but
/// the lookup table itself only has 24 entries (0-23) - the routine
/// itself performs no further masking, so which of those 8 extra values
/// (if any) are actually reachable in real gameplay is a live-gameplay
/// question, not a static one; see this module's live-verification hook.
pub fn calc_bullet_velocities(aim_dir: u8, speed_code: u8, quadrant: u8) -> BulletVelocity {
    let offset = BULLET_FRACT_VEL_DIR_LOOKUP_TBL[aim_dir as usize] as usize;
    let (frac_x_base, frac_y_base) = {
        let (y, x) = BULLET_FRACT_VEL_TBL[offset / 2];
        (x, y)
    };

    let (mut frac_x, mut fast_x) = adjust_bullet_velocity(frac_x_base, 0, speed_code);
    let (mut frac_y, mut fast_y) = adjust_bullet_velocity(frac_y_base, 0, speed_code);

    if quadrant & 0x01 != 0 {
        let (f, fa) = negate16(frac_y, fast_y);
        frac_y = f;
        fast_y = fa;
    }
    if quadrant & 0x02 != 0 {
        let (f, fa) = negate16(frac_x, fast_x);
        frac_x = f;
        fast_x = fa;
    }

    BulletVelocity { frac_y, fast_y, frac_x, fast_x }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_0_halves_the_worked_example_exactly() {
        // src/bank7.asm's own comment: 3.5 ($03 $80) -> 1.75 ($01 $c0).
        assert_eq!(adjust_bullet_velocity(0x80, 0x03, 0), (0xc0, 0x01));
    }

    #[test]
    fn speed_2_is_unchanged() {
        assert_eq!(adjust_bullet_velocity(0x37, 0x02, 2), (0x37, 0x02));
    }

    #[test]
    fn speed_6_matches_the_worked_example_exactly() {
        // 1.1 ($01 $1a) -> 1.92 ($01 $ed).
        assert_eq!(adjust_bullet_velocity(0x1a, 0x01, 6), (0xed, 0x01));
    }

    #[test]
    fn speed_7_matches_the_worked_example_exactly() {
        // 1.05 ($01 $0d) -> 1.96 ($01 $f7).
        assert_eq!(adjust_bullet_velocity(0x0d, 0x01, 7), (0xf7, 0x01));
    }

    #[test]
    fn speed_code_is_masked_to_3_bits_before_dispatch() {
        // Real caller does `and #$07` before jsr - 0x08 must behave
        // identically to 0x00 (both select case 0), not panic.
        assert_eq!(adjust_bullet_velocity(0x80, 0x03, 0x08), adjust_bullet_velocity(0x80, 0x03, 0x00));
    }

    #[test]
    fn negate16_round_trips_through_zero() {
        assert_eq!(negate16(0x00, 0x00), (0x00, 0x00));
        // 0xff/0x00 (~-1/256) negated -> 0x01/0xff, matching the real
        // chained-SBC borrow trace by hand.
        assert_eq!(negate16(0xff, 0x00), (0x01, 0xff));
        assert_eq!(negate16(0x01, 0xff), (0xff, 0x00));
    }

    #[test]
    fn calc_bullet_velocities_aim_dir_0_speed_2_quadrant_0_is_untouched_table_lookup() {
        // aim_dir=0 -> offset 0x00 -> pair 0 = (Y=$00, X=$ff). speed_code
        // 2 is `adjust_bullet_velocity`'s identity case (already verified
        // separately), and quadrant=0 negates neither axis - isolates the
        // table lookup/split itself from there being anything to trust
        // about the scaling or negation steps for this one case.
        let v = calc_bullet_velocities(0, 2, 0);
        assert_eq!(v, BulletVelocity { frac_y: 0x00, fast_y: 0x00, frac_x: 0xff, fast_x: 0x00 });
    }

    #[test]
    fn calc_bullet_velocities_negates_y_when_top_half_bit_set() {
        // Same lookup as above (aim_dir=0, speed=2), but quadrant bit 0
        // set (top half) - Y must flip sign, X must not.
        let v = calc_bullet_velocities(0, 2, 0x01);
        assert_eq!(v, BulletVelocity { frac_y: 0x00, fast_y: 0x00, frac_x: 0xff, fast_x: 0x00 });
        // frac_y/fast_y are both zero here so negation is a no-op - use
        // the last table entry (Y=$ff, X=$00) instead to actually see
        // the flip.
        let v2 = calc_bullet_velocities(6, 2, 0x01); // offset 0x0c -> pair 6 = (Y=$ff, X=$00)
        assert_eq!(v2.frac_x, 0x00);
        assert_eq!(v2.fast_x, 0x00);
        assert_eq!((v2.frac_y, v2.fast_y), negate16(0xff, 0x00));
    }

    #[test]
    fn calc_bullet_velocities_negates_x_when_left_half_bit_set() {
        // aim_dir=0 (offset 0 -> pair 0, X=$ff, fast=$00), quadrant bit 1
        // set (left half) - X must flip sign, Y must not.
        let v = calc_bullet_velocities(0, 2, 0x02);
        assert_eq!((v.frac_y, v.fast_y), (0x00, 0x00));
        assert_eq!((v.frac_x, v.fast_x), negate16(0xff, 0x00));
    }
}
