//! Randomness.
//!
//! Contra's NES code has no LFSR or noise-channel RNG. Instead, between the
//! end of one frame's NMI handler and the start of the next, the CPU spins
//! in an idle loop (`forever_loop` in `bank7.asm`) that does nothing but:
//!
//! ```text
//! forever_loop:
//!     lda FRAME_COUNTER
//!     adc RANDOM_NUM
//!     sta RANDOM_NUM
//!     jmp forever_loop
//! ```
//!
//! `RANDOM_NUM` keeps accumulating `FRAME_COUNTER` for as many iterations as
//! fit in whatever CPU time is left over that frame. Because the amount of
//! leftover time depends on exactly which branches the game logic took that
//! frame (how many enemies were on screen, which menu was open, etc.), the
//! final value is effectively unpredictable from the player's perspective -
//! but it is *not* a value you can reproduce by ticking a formula once per
//! frame. Bit-exact "Original NES" RNG requires knowing the idle-loop
//! iteration count, which in turn requires either (a) cycle-accurate
//! emulation of every routine that ran that frame, or (b) a static
//! recompilation of the original 6502 code that preserves its timing.
//!
//! [`NesAccumulatorRng`] below implements the *accumulator itself* correctly
//! (same `wrapping_add` behavior `RANDOM_NUM` has), and it is the seam
//! where a future cycle-accurate front-end plugs in idle-loop tick counts
//! (tracked in ROADMAP.md under Phase 1 - "Original NES" bit-exact RNG).
//! Until that lands, "Original NES" mode drives it with a tick count derived
//! from a fixed per-frame budget, which matches the original's *behavior*
//! (same update rule, same 8-bit wraparound) but not yet its exact sequence.
//!
//! [`ModernRng`] is an explicit, seedable RNG for every non-"Original" mode
//! (Randomizer, Daily Challenge, Roguelike drops, etc.) where a shareable,
//! documented seed matters more than matching 1988 hardware quirks.

/// Reproduces `RANDOM_NUM`'s update rule: `value = value.wrapping_add(x)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NesAccumulatorRng {
    value: u8,
}

impl NesAccumulatorRng {
    pub const fn new() -> Self {
        // RANDOM_NUM is explicitly zeroed once at boot (bank7.asm ~line 1125).
        Self { value: 0 }
    }

    /// One accumulation step, `value += x` with 8-bit wraparound.
    pub fn tick(&mut self, x: u8) {
        self.value = self.value.wrapping_add(x);
    }

    /// Runs `tick(frame_counter)` `iterations` times, i.e. simulates
    /// `iterations` passes through `forever_loop` during one frame's idle
    /// time.
    pub fn tick_idle_frame(&mut self, frame_counter: u8, iterations: u32) {
        for _ in 0..iterations {
            self.tick(frame_counter);
        }
    }

    pub fn value(&self) -> u8 {
        self.value
    }
}

impl Default for NesAccumulatorRng {
    fn default() -> Self {
        Self::new()
    }
}

/// A small, fast, seedable RNG (xorshift32) for non-"Original" gameplay
/// modes. Deterministic given a seed, so Daily Challenge / Randomizer seeds
/// (e.g. `KONAMI-1988-696969`) reproduce identically for every player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModernRng {
    state: u32,
}

impl ModernRng {
    pub fn new(seed: u32) -> Self {
        Self {
            state: if seed == 0 { 0x9E3779B9 } else { seed },
        }
    }

    /// Derives a 32-bit seed from an arbitrary human-typed string (used for
    /// shareable Daily Challenge / Randomizer / Custom Difficulty seeds).
    pub fn seed_from_str(s: &str) -> u32 {
        // FNV-1a, good enough for turning a seed *string* into a seed *u32*.
        let mut hash: u32 = 0x811c9dc5;
        for byte in s.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x01000193);
        }
        hash
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform value in `[0, bound)`.
    pub fn next_below(&mut self, bound: u32) -> u32 {
        assert!(bound > 0);
        self.next_u32() % bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulator_wraps_like_a_u8_register() {
        let mut rng = NesAccumulatorRng::new();
        rng.tick_idle_frame(200, 3); // 200*3 = 600 -> 600 % 256 = 88
        assert_eq!(rng.value(), 88);
    }

    #[test]
    fn modern_rng_is_deterministic_per_seed() {
        let seed = ModernRng::seed_from_str("KONAMI-1988-696969");
        let mut a = ModernRng::new(seed);
        let mut b = ModernRng::new(seed);
        let seq_a: Vec<u32> = (0..16).map(|_| a.next_u32()).collect();
        let seq_b: Vec<u32> = (0..16).map(|_| b.next_u32()).collect();
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn modern_rng_differs_across_seeds() {
        let mut a = ModernRng::new(1);
        let mut b = ModernRng::new(2);
        assert_ne!(a.next_u32(), b.next_u32());
    }
}
