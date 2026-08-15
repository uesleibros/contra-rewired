//! APU stub: accepts every register write so game code never stalls
//! waiting on audio hardware, but produces no sound yet. Real pulse/
//! triangle/noise/DMC synthesis is tracked in ROADMAP.md - this crate's
//! job so far was proving the CPU+PPU pipeline renders a real ROM;
//! audio is the next major subsystem, not yet started.
//!
//! `$4015` reads report every length counter as expired and no pending
//! frame/DMC IRQ, which is a safe default: it's what the register reads
//! as once a channel naturally finishes, so code that polls it once in a
//! while (rather than relying on exact timing) behaves reasonably.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Apu {
    /// Raw register shadow, kept only so a save state round-trips
    /// something plausible once real synthesis reads these back.
    pub registers: [u8; 0x18],
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => 0,
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        if let Some(slot) = (addr as usize).checked_sub(0x4000) {
            if slot < self.registers.len() {
                self.registers[slot] = value;
            }
        }
    }
}
