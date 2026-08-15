//! Mapper 2 (UxROM) - the mapper Contra (USA) uses: 16 KiB of PRG-ROM
//! switched into `$8000-$BFFF` by any write to `$8000-$FFFF` (the written
//! value's low bits select the bank), with the *last* 16 KiB bank fixed at
//! `$C000-$FFFF`. CHR is RAM (owned by [`crate::ppu::Ppu`], not this
//! module) rather than bank-switched CHR-ROM, which is UxROM's usual
//! configuration and why there's no CHR logic here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mapper2 {
    prg_rom: Vec<u8>,
    bank_count: usize,
    bank_select: u8,
}

impl Mapper2 {
    pub fn new(prg_rom: Vec<u8>) -> Self {
        let bank_count = (prg_rom.len() / 0x4000).max(1);
        Self { prg_rom, bank_count, bank_select: 0 }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.bank_select as usize % self.bank_count;
                self.prg_rom.get(bank * 0x4000 + (addr as usize - 0x8000)).copied().unwrap_or(0)
            }
            0xC000..=0xFFFF => {
                let bank = self.bank_count - 1;
                self.prg_rom.get(bank * 0x4000 + (addr as usize - 0xC000)).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, _addr: u16, value: u8) {
        // Any write in $8000-$FFFF selects the bank on real UxROM hardware;
        // there's no address decoding beyond "is this cartridge space".
        self.bank_select = value;
    }

    pub fn bank_select(&self) -> u8 {
        self.bank_select
    }

    pub fn set_bank_select(&mut self, value: u8) {
        self.bank_select = value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_banks(n: usize) -> Vec<u8> {
        let mut rom = vec![0u8; n * 0x4000];
        for (bank, chunk) in rom.chunks_mut(0x4000).enumerate() {
            chunk[0] = bank as u8;
        }
        rom
    }

    #[test]
    fn last_bank_is_fixed_at_c000() {
        let mapper = Mapper2::new(rom_with_banks(4));
        assert_eq!(mapper.read(0xC000), 3);
        mapper.read(0xC000); // still bank 3 regardless of bank_select
    }

    #[test]
    fn writes_switch_the_8000_window() {
        let mut mapper = Mapper2::new(rom_with_banks(4));
        mapper.write(0x8000, 2);
        assert_eq!(mapper.read(0x8000), 2);
        mapper.write(0x8000, 0);
        assert_eq!(mapper.read(0x8000), 0);
        // last bank never moves
        assert_eq!(mapper.read(0xC000), 3);
    }
}
