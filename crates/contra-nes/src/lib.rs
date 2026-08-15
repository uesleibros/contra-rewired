//! `contra-nes`: a from-scratch NES emulation core (6502/2A03 CPU, 2C02
//! PPU, mapper 2/UxROM). This is the actual "play the real game" path -
//! see the workspace README for why: hand-porting Contra's game logic
//! routine-by-routine is a years-long undertaking with no way to verify it
//! against the real thing, whereas running the original code on an
//! accurate-enough general-purpose NES core gets bit-exact behavior (RNG,
//! hitboxes, quirks included) essentially for free, and is independently
//! testable without any copyrighted ROM (see `cpu.rs`'s and `nes.rs`'s
//! unit tests, which use small original hand-assembled programs).
//!
//! This crate contains **no game-specific code**. It doesn't know it's
//! running Contra; it just emulates the console. `contra-assets` loads and
//! validates the user's own ROM file; `apps/contra-pc` wires the two
//! together and presents the resulting framebuffer.
//!
//! Known scope limits, all tracked in ROADMAP.md / docs/FIDELITY.md:
//! - PPU rendering is scanline-granular, not per-dot (see `ppu.rs`).
//! - APU is a silent stub - no audio synthesis yet (see `apu.rs`).
//! - Only mapper 2 (UxROM) with CHR-RAM is implemented.
//! - Only official 6502 opcodes are implemented; undocumented opcodes are
//!   treated as a no-op and recorded rather than crashing (see `cpu.rs`).

pub mod apu;
pub mod bus;
pub mod controller;
pub mod cpu;
pub mod mapper;
pub mod nes;
pub mod ppu;
mod serde_arrays;

pub use nes::{Nes, NesSnapshot};
pub use ppu::{Mirroring, EXTENDED_WIDTH, SCREEN_H, SCREEN_W};
