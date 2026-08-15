//! Fixed-point math mirroring the NES game's native representation.
//!
//! The original 6502 code stores most velocities as two separate bytes: a
//! "fractional" byte (sub-pixel accumulator, wraps with carry) and a "fast"
//! byte (signed whole-pixel velocity). This is *not* a generic Q8.8 type -
//! it's exactly the two-register accumulate-with-carry pattern used by
//! routines like `apply_gravity` in `bank7.asm` of the community
//! disassembly (<https://github.com/vermiceli/nes-contra-us>), reproduced
//! here byte-for-byte so "Original NES" fidelity mode behaves identically.
//!
//! ```text
//! apply_gravity:
//!     clc
//!     lda PLAYER_Y_FRACT_VELOCITY,x
//!     adc #$23                      ; .1367 px/frame^2
//!     sta PLAYER_Y_FRACT_VELOCITY,x
//!     lda PLAYER_Y_FAST_VELOCITY,x
//!     adc #$00                      ; carry into the whole-pixel byte
//!     sta PLAYER_Y_FAST_VELOCITY,x
//! ```

/// Gravity added to `Velocity16::fract` every frame. 0x23 / 256 ≈ 0.1367 px/frame².
pub const GRAVITY_RAW: u8 = 0x23;

/// A two-byte fractional/fast velocity pair, exactly as stored in NES RAM.
///
/// `fract` is an unsigned sub-pixel accumulator; `fast` is a signed
/// whole-pixel-per-frame velocity. Adding `fract` overflow into `fast`
/// reproduces the 6502 `ADC`/carry semantics via `u8::overflowing_add`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Velocity16 {
    pub fract: u8,
    pub fast: i8,
}

impl Velocity16 {
    pub const fn zero() -> Self {
        Self { fract: 0, fast: 0 }
    }

    pub const fn from_fast(fast: i8) -> Self {
        Self { fract: 0, fast }
    }

    /// Adds a raw signed 8-bit amount to `fract`, carrying into `fast` on
    /// overflow - the exact operation `apply_gravity` performs.
    pub fn add_fract(&mut self, amount: u8) {
        let (result, carry) = self.fract.overflowing_add(amount);
        self.fract = result;
        self.fast = self.fast.wrapping_add(carry as i8);
    }

    /// Applies one frame of gravity (adds `GRAVITY_RAW` to `fract`).
    pub fn apply_gravity(&mut self) {
        self.add_fract(GRAVITY_RAW);
    }

    /// Signed 16-bit value (fast:fract), useful for save states / debugging.
    pub fn as_i16(self) -> i16 {
        ((self.fast as i16) << 8) | self.fract as i16
    }
}

/// A position accumulator using a one-byte "jump coefficient" as the
/// sub-pixel carry register, mirroring `player_jumping_set_y_pos`:
///
/// ```text
/// lda PLAYER_JUMP_COEFFICIENT,x
/// clc
/// adc PLAYER_Y_FRACT_VELOCITY,x
/// sta PLAYER_JUMP_COEFFICIENT,x   ; new sub-pixel accumulator
/// lda SPRITE_Y_POS,x
/// adc PLAYER_Y_FAST_VELOCITY,x    ; + carry from the line above
/// sta SPRITE_Y_POS,x
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct JumpAccumulator {
    pub coefficient: u8,
}

impl JumpAccumulator {
    /// Integrates one frame of vertical motion into `pos`, given the
    /// current [`Velocity16`]. Returns the carry-adjusted position.
    pub fn integrate(&mut self, vel: Velocity16, pos: u8) -> u8 {
        let (new_coeff, carry) = self.coefficient.overflowing_add(vel.fract);
        self.coefficient = new_coeff;
        let (new_pos, _) = pos.overflowing_add(vel.fast as u8);
        new_pos.wrapping_add(carry as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gravity_accumulates_and_carries() {
        let mut v = Velocity16::zero();
        // 0x23 * 12 = 0x1a4 -> fract wraps once, fast becomes 1
        for _ in 0..12 {
            v.apply_gravity();
        }
        assert_eq!(v.fract, 0x1a4u16 as u8);
        assert_eq!(v.fast, 1);
    }

    #[test]
    fn deterministic_repeated_falls_match() {
        // Same starting state + same number of frames must always produce
        // the same result: this is the whole point of "same physics" fidelity.
        let run = || {
            let mut v = Velocity16::zero();
            for _ in 0..90 {
                v.apply_gravity();
            }
            v
        };
        assert_eq!(run(), run());
    }
}
