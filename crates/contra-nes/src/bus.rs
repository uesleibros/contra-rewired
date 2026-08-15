//! The CPU's view of the whole machine: 2KB internal RAM (mirrored through
//! `$1FFF`), PPU registers (`$2000-$3FFF`, mirrored every 8 bytes), APU/
//! controller registers (`$4000-$4017`), optional 8KB PRG-RAM
//! (`$6000-$7FFF`), and the cartridge mapper (`$8000-$FFFF`).

use serde::{Deserialize, Serialize};

use crate::apu::Apu;
use crate::controller::Controller;
use crate::cpu::Bus as CpuBus;
use crate::mapper::Mapper2;
use crate::ppu::Ppu;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NesBus {
    #[serde(with = "crate::serde_arrays::arr_0x800")]
    pub ram: [u8; 0x800],
    #[serde(with = "crate::serde_arrays::arr_0x2000")]
    pub prg_ram: [u8; 0x2000],
    pub ppu: Ppu,
    pub apu: Apu,
    pub mapper: Mapper2,
    pub controllers: [Controller; 2],
    /// CPU cycles still owed to an in-progress OAM DMA transfer, consumed
    /// by [`crate::nes::Nes::run_frame`] right after the triggering write.
    pub dma_stall: u32,
}

impl NesBus {
    pub fn new(mapper: Mapper2, ppu: Ppu, audio_sample_rate: f64) -> Self {
        Self {
            ram: [0; 0x800],
            prg_ram: [0; 0x2000],
            ppu,
            apu: Apu::new(audio_sample_rate),
            mapper,
            controllers: [Controller::default(), Controller::default()],
            dma_stall: 0,
        }
    }

    fn oam_dma(&mut self, page: u8) {
        let base = (page as u16) << 8;
        let start = self.ppu.oam_addr;
        for i in 0..256u16 {
            let byte = self.read(base + i);
            self.ppu.oam_dma_write(start.wrapping_add(i as u8), byte);
        }
        self.dma_stall += 513;
    }
}

impl CpuBus for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize],
            0x2000..=0x3FFF => self.ppu.read_register(addr),
            0x4015 => self.apu.read(addr),
            0x4016 => self.controllers[0].read(),
            0x4017 => self.controllers[1].read(),
            0x4000..=0x4014 | 0x4018..=0x401F => 0,
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => self.mapper.read(addr),
            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x07FF) as usize] = value,
            0x2000..=0x3FFF => self.ppu.write_register(addr, value),
            0x4014 => self.oam_dma(value),
            0x4016 => {
                self.controllers[0].write_strobe(value);
                self.controllers[1].write_strobe(value);
            }
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write(addr, value),
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            0x8000..=0xFFFF => self.mapper.write(addr, value),
            _ => {}
        }
    }
}
