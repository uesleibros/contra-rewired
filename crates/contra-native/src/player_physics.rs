//! Player vertical physics (gravity, jump integration) - ported from
//! `bank7.asm`'s `apply_gravity` (`$d9ec`) and `player_jumping_set_y_pos`
//! (`$d9cb`, reached by falling through from `apply_gravity_set_y_pos`'s
//! entry at `$d9c8`). This is a *different* (and, since it's a verified
//! line-for-line port, more trustworthy) source than
//! `contra_core::physics::PlayerPhysics`, which is the project's earlier,
//! honestly-labeled hand-ported *placeholder* physics - approximated from
//! reading comments, never checked against real gameplay. This module is
//! the real thing, checked against real gameplay (see the `contra-nes`
//! `VERIFY_PLAYER_GRAVITY=1` capture in `dump_frames.rs`).

/// `PLAYER_Y_FRACT_VELOCITY`/`PLAYER_Y_FAST_VELOCITY` (`$c4`/`$c6` + player
/// index) - an 8.8 fixed-point vertical velocity, exactly the shape
/// `contra_core::fixed::Velocity16` already models, but this is the value
/// as the real game actually keeps it (raw two's-complement bytes, no
/// reinterpretation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YVelocity {
    pub fract: u8,
    pub fast: u8,
}

/// Adds `carry_in` (0 or 1) to `a + b`, matching a 6502 `ADC` exactly:
/// returns `(result, carry_out)`.
fn adc(a: u8, b: u8, carry_in: bool) -> (u8, bool) {
    let sum = a as u16 + b as u16 + carry_in as u16;
    (sum as u8, sum > 0xFF)
}

/// Ported from `apply_gravity` (`$d9ec`): increments the fractional
/// velocity by `$23` (~0.1367 px/frame², per the disassembly's own
/// comment) every frame, carrying into the fast (whole-pixel) component on
/// overflow - the entire gravity model is this one constant, no curve or
/// terminal-velocity clamp in the original code. Doesn't touch position -
/// see [`integrate_y_position`] for that half (`player_jumping_set_y_pos`).
pub fn apply_gravity(v: YVelocity) -> YVelocity {
    let (fract, carry) = adc(v.fract, 0x23, false);
    let (fast, _) = adc(v.fast, 0x00, carry);
    YVelocity { fract, fast }
}

/// A player slot's fields `player_jumping_set_y_pos` reads and writes,
/// beyond the velocity itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YPositionState {
    /// `SPRITE_Y_POS` (`$031a` + player index) - the player's actual
    /// on-screen Y position.
    pub y_pos: u8,
    /// `PLAYER_JUMP_COEFFICIENT` (`$94` + player index) - the sub-pixel
    /// accumulator absorbing `YVelocity::fract` each frame, carrying a
    /// whole pixel into `y_pos` when it overflows. Same role as
    /// `contra_core::fixed::JumpAccumulator`.
    pub jump_coefficient: u8,
    /// `PLAYER_HIDDEN` (`$ba` + player index) - `0` visible, any nonzero
    /// hidden. The disassembly's own comment on this field is "not sure
    /// how this is really supposed to be used" - it's incremented or
    /// decremented by this routine based on vertical velocity direction
    /// every single frame the player is airborne, which is real, verified
    /// behavior of the original game regardless of what its gameplay
    /// purpose actually is. A hand-written reimplementation that didn't
    /// start from the real disassembly would have had no way to know this
    /// field is touched here at all - a concrete example of why this
    /// project ports real code instead of re-deriving behavior from
    /// observation.
    pub hidden: u8,
}

/// Ported from `player_jumping_set_y_pos` (`$d9cb`). Integrates one frame
/// of vertical motion: the fractional velocity accumulates into
/// `jump_coefficient`, and a whole pixel carries into `y_pos` together
/// with the fast velocity component - standard 8.8 fixed-point
/// integration - but *also* nudges `hidden` by `-1`/`0`/`+1` (chained,
/// uncleared-carry `ADC`s, exactly like the real 6502 code - see this
/// function's source for why that's not three independent additions)
/// depending on whether the player is moving up or down/still. Real
/// behavior, not approximated - see [`YPositionState::hidden`]'s doc
/// comment.
pub fn integrate_y_position(v: YVelocity, state: YPositionState) -> YPositionState {
    // lda PLAYER_Y_FAST_VELOCITY,x; asl; lda #$00; bcc @continue; lda #$ff
    // ASL shifts bit 7 (the sign bit) into carry - carry set means
    // fast velocity is negative (moving up).
    let marker: u8 = if v.fast & 0x80 != 0 { 0xFF } else { 0x00 };

    // clc; lda PLAYER_JUMP_COEFFICIENT,x; adc PLAYER_Y_FRACT_VELOCITY,x
    let (jump_coefficient, carry1) = adc(state.jump_coefficient, v.fract, false);
    // lda SPRITE_Y_POS,x; adc PLAYER_Y_FAST_VELOCITY,x - no CLC: carry-in
    // is carry1, straight from the previous ADC.
    let (y_pos, carry2) = adc(state.y_pos, v.fast, carry1);
    // lda PLAYER_HIDDEN,x; adc $08 - no CLC here either: carry-in is carry2.
    let (hidden, _) = adc(state.hidden, marker, carry2);

    YPositionState { y_pos, jump_coefficient, hidden }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_increments_fractional_velocity_by_0x23_each_frame() {
        let v = apply_gravity(YVelocity { fract: 0x00, fast: 0x00 });
        assert_eq!(v, YVelocity { fract: 0x23, fast: 0x00 });
    }

    #[test]
    fn gravity_carries_into_fast_velocity_on_fractional_overflow() {
        // 0xE0 + 0x23 = 0x103 -> wraps to 0x03, carries 1 into fast.
        let v = apply_gravity(YVelocity { fract: 0xE0, fast: 0xFE });
        assert_eq!(v, YVelocity { fract: 0x03, fast: 0xFF });
    }

    #[test]
    fn integration_moves_position_by_fast_velocity_plus_jump_coefficient_carry() {
        // fast = 0x02 (moving down 2px), fract = 0x80 (will overflow the
        // jump coefficient from 0x90 -> 0x10, carrying 1 extra pixel).
        let v = YVelocity { fract: 0x80, fast: 0x02 };
        let state = YPositionState { y_pos: 100, jump_coefficient: 0x90, hidden: 0 };
        let result = integrate_y_position(v, state);
        assert_eq!(result.jump_coefficient, 0x10); // 0x90 + 0x80 = 0x110 -> 0x10, carry 1
        assert_eq!(result.y_pos, 103); // 100 + 2 + 1(carry)
    }

    #[test]
    fn hidden_marker_is_negative_one_when_moving_up() {
        // fast velocity with bit 7 set (e.g. 0xFC = -4, moving up).
        let v = YVelocity { fract: 0x00, fast: 0xFC };
        let state = YPositionState { y_pos: 100, jump_coefficient: 0x00, hidden: 5 };
        let result = integrate_y_position(v, state);
        // marker=0xff. jump_coefficient add: 0+0+carry_in(0)=0, no carry
        // out. y_pos add: 100 + 0xfc(252) + carry_in(0) = 352, which
        // overflows a u8 (>255), so *this* add's carry-out is 1:
        // y_pos = 352 mod 256 = 96, carry2 = true.
        // hidden add: 5 + 0xff(255) + carry_in(1, from carry2) = 261,
        // mod 256 = 5 - net change is zero here, since the carry2 from the
        // position overflow exactly cancels the "-1" the marker alone
        // would have applied. This is why the real 6502's uncleared-carry
        // chaining matters: three independent (non-chained) adds would
        // have given hidden=4 instead.
        assert_eq!(result.y_pos, 96);
        assert_eq!(result.hidden, 5);
    }
}
