//! Ties [`Cpu`], [`Ppu`], and [`NesBus`] together and drives one frame at a
//! time at scanline granularity (see `ppu.rs` module docs for what that
//! does and doesn't reproduce faithfully).

use serde::{Deserialize, Serialize};

use crate::bus::NesBus;
use crate::cpu::Cpu;
use crate::mapper::Mapper2;
use crate::ppu::{Mirroring, Ppu, SCREEN_H};

const DOTS_PER_SCANLINE: f64 = 341.0;
const CPU_DOTS_PER_CYCLE: f64 = 3.0;
const SCANLINES_AFTER_VBLANK_START: u32 = 20; // 262 total = 1 pre-render + 240 visible + 1 post-render + 1 vblank-start + 19 more
const DEFAULT_AUDIO_SAMPLE_RATE: f64 = 44_100.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nes {
    pub cpu: Cpu,
    pub bus: NesBus,
}

impl Nes {
    /// `prg_rom` and `mirroring` come from the user's own dumped ROM (see
    /// `contra-assets`); this crate never bundles or reads a ROM itself.
    /// Uses a default 44.1kHz audio sample rate; see
    /// [`Self::new_with_audio`] to match your actual output device.
    pub fn new(prg_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::new_with_audio(prg_rom, mirroring, DEFAULT_AUDIO_SAMPLE_RATE)
    }

    pub fn new_with_audio(prg_rom: Vec<u8>, mirroring: Mirroring, audio_sample_rate: f64) -> Self {
        let mapper = Mapper2::new(prg_rom);
        let ppu = Ppu::new(mirroring);
        let mut bus = NesBus::new(mapper, ppu, audio_sample_rate);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        Self { cpu, bus }
    }

    pub fn set_controller(&mut self, player: usize, buttons: u8) {
        if let Some(c) = self.bus.controllers.get_mut(player) {
            c.state = buttons;
        }
    }

    pub fn framebuffer(&self) -> &[u32] {
        &self.bus.ppu.framebuffer
    }

    /// The "Extended" widescreen framebuffer, [`Self::wide_width`] pixels
    /// wide; only populated while widescreen is enabled (width >
    /// `contra_nes::SCREEN_W`).
    pub fn wide_framebuffer(&self) -> &[u32] {
        &self.bus.ppu.wide_framebuffer
    }

    /// The width [`Self::wide_framebuffer`] is currently rendered at.
    pub fn wide_width(&self) -> usize {
        self.bus.ppu.wide_width
    }

    /// Sets the "Extended" widescreen presentation width - `SCREEN_W` (256)
    /// disables it; anything greater (clamped to `EXTENDED_WIDTH`, the
    /// hardware-imposed safe cap) renders that many pixels wide instead,
    /// letting a front-end track a live, resizable window's aspect ratio
    /// frame by frame. Purely a rendering choice - it never touches RAM,
    /// collision, or any other game state, so it's safe to change at any
    /// time, mid-gameplay included.
    pub fn set_wide_width(&mut self, width: usize) {
        self.bus.ppu.wide_width = width.clamp(crate::ppu::SCREEN_W, crate::ppu::EXTENDED_WIDTH);
    }

    /// Lifts the real NES's 8-sprites-per-scanline rendering limit (the
    /// cause of "sprite flicker" whenever a scene has more sprites on one
    /// line than that). Purely a rendering choice, off by default so
    /// `Original` mode stays hardware-accurate.
    pub fn set_unlimited_sprites(&mut self, enabled: bool) {
        self.bus.ppu.unlimited_sprites = enabled;
    }

    /// Direct external write into PPU address space - see
    /// [`crate::ppu::Ppu::poke`]. This is the seam mods/trainers use (e.g.
    /// writing sprite palette entries each frame for a color-cycling
    /// effect); it never touches CPU/RAM state, only what the PPU renders.
    pub fn poke_ppu(&mut self, addr: u16, value: u8) {
        self.bus.ppu.poke(addr, value);
    }

    /// The 2KB CPU-visible work RAM (`$0000-$07FF`), unmirrored. This is
    /// exactly what a real "Game Genie"-style trainer or a debug menu
    /// pokes: player lives, weapon ID, continues, and every other piece of
    /// gameplay state a running Contra ROM keeps here. Reading/writing it
    /// directly (vs. going through the CPU) can't desync instruction
    /// timing, but *does* change real game state - unlike
    /// [`Self::poke_ppu`], this is not a purely cosmetic knob.
    pub fn ram(&self) -> &[u8; 0x800] {
        &self.bus.ram
    }

    pub fn peek_ram(&self, addr: u16) -> u8 {
        self.bus.ram[(addr & 0x07FF) as usize]
    }

    pub fn poke_ram(&mut self, addr: u16, value: u8) {
        self.bus.ram[(addr & 0x07FF) as usize] = value;
    }

    /// Drains every audio sample generated since the last call (mono,
    /// `f32` in roughly `[0, 1)`), for the front-end to feed to its audio
    /// output device.
    pub fn take_audio_samples(&mut self) -> Vec<f32> {
        self.bus.apu.take_samples()
    }

    /// Runs exactly one NTSC frame (262 scanlines): pre-render, 240 visible
    /// (rendered into the framebuffer one line at a time), post-render,
    /// then vblank - firing NMI if the game has enabled it.
    pub fn run_frame(&mut self) {
        let mut budget = self.cpu.cycles as f64;

        self.bus.ppu.start_prerender();
        self.advance_cpu(&mut budget);

        for y in 0..SCREEN_H {
            self.bus.ppu.render_scanline(y);
            self.advance_cpu(&mut budget);
        }

        self.advance_cpu(&mut budget); // post-render line (240): no PPU memory access

        let want_nmi = self.bus.ppu.start_vblank();
        self.advance_cpu(&mut budget); // scanline 241: vblank flag becomes visible to the CPU here
        if want_nmi {
            self.cpu.nmi(&mut self.bus);
        }

        for _ in 0..SCANLINES_AFTER_VBLANK_START {
            self.advance_cpu(&mut budget);
        }
    }

    fn advance_cpu(&mut self, budget: &mut f64) {
        *budget += DOTS_PER_SCANLINE / CPU_DOTS_PER_CYCLE;
        while (self.cpu.cycles as f64) < *budget {
            let cycles = self.cpu.step(&mut self.bus);
            for _ in 0..cycles {
                self.bus.apu.step();
            }
            if self.bus.dma_stall > 0 {
                // OAM DMA halts the CPU but not the APU on real hardware.
                for _ in 0..self.bus.dma_stall {
                    self.bus.apu.step();
                }
                self.cpu.cycles += self.bus.dma_stall as u64;
                self.bus.dma_stall = 0;
            }
        }
    }

    /// Captures everything that changes at runtime - CPU, RAM, PPU/APU
    /// state, controller shift registers, and the mapper's current bank
    /// select - but deliberately **not** the (static, up to 128 KiB)
    /// PRG-ROM. Suitable for save states and a per-frame rewind buffer
    /// without copying the whole cartridge every time.
    pub fn snapshot(&self) -> NesSnapshot {
        NesSnapshot {
            cpu: self.cpu.clone(),
            ram: self.bus.ram,
            prg_ram: self.bus.prg_ram,
            ppu: self.bus.ppu.clone(),
            apu: self.bus.apu.clone(),
            controllers: self.bus.controllers,
            mapper_bank_select: self.bus.mapper.bank_select(),
        }
    }

    /// Restores everything [`Self::snapshot`] captured. The framebuffer is
    /// left as-is until the next `run_frame` repaints it.
    pub fn restore(&mut self, snap: &NesSnapshot) {
        self.cpu = snap.cpu.clone();
        self.bus.ram = snap.ram;
        self.bus.prg_ram = snap.prg_ram;
        let framebuffer = std::mem::take(&mut self.bus.ppu.framebuffer);
        self.bus.ppu = snap.ppu.clone();
        self.bus.ppu.framebuffer = framebuffer;
        self.bus.apu = snap.apu.clone();
        self.bus.controllers = snap.controllers;
        self.bus.mapper.set_bank_select(snap.mapper_bank_select);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NesSnapshot {
    cpu: Cpu,
    #[serde(with = "crate::serde_arrays::arr_0x800")]
    ram: [u8; 0x800],
    #[serde(with = "crate::serde_arrays::arr_0x2000")]
    prg_ram: [u8; 0x2000],
    ppu: Ppu,
    apu: crate::apu::Apu,
    controllers: [crate::controller::Controller; 2],
    mapper_bank_select: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::Bus as CpuBus;

    /// A tiny, original, hand-assembled "ROM": on NMI, increments a RAM
    /// counter; main loop spins forever. Proves the CPU/PPU/bus/NMI wiring
    /// runs a program across multiple frames without needing any real game
    /// ROM. Not derived from or resembling Contra in any way.
    fn counter_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x4000];
        // Reset vector routine at $8000: enable NMI (PPUCTRL bit7), then spin.
        rom[0x0000] = 0xA9; rom[0x0001] = 0x80; // LDA #$80
        rom[0x0002] = 0x8D; rom[0x0003] = 0x00; rom[0x0004] = 0x20; // STA $2000
        rom[0x0005] = 0x4C; rom[0x0006] = 0x05; rom[0x0007] = 0x80; // JMP $8005 (spin)

        // NMI handler at $8010: INC $0000; RTI
        rom[0x0010] = 0xE6; rom[0x0011] = 0x00; // INC $00
        rom[0x0012] = 0x40; // RTI

        // Vectors at the end of the fixed bank ($FFFA-$FFFF -> file offset
        // 0x3FFA..0x3FFF for a single 16KB bank mapped at $C000, but this
        // ROM is only one bank so it's mirrored to both $8000 and $C000).
        rom[0x3FFA] = 0x10; rom[0x3FFB] = 0x80; // NMI -> $8010
        rom[0x3FFC] = 0x00; rom[0x3FFD] = 0x80; // RESET -> $8000
        rom[0x3FFE] = 0x00; rom[0x3FFF] = 0x00; // IRQ -> $0000 (unused)
        rom
    }

    #[test]
    fn nmi_fires_once_per_frame_and_runs_the_handler() {
        let mut nes = Nes::new(counter_rom(), Mirroring::Horizontal);
        nes.run_frame();
        nes.run_frame();
        nes.run_frame();
        assert_eq!(nes.bus.ram[0x0000], 3, "handler should have run exactly once per frame");
    }

    #[test]
    fn framebuffer_is_the_right_size() {
        let nes = Nes::new(counter_rom(), Mirroring::Horizontal);
        assert_eq!(nes.framebuffer().len(), crate::ppu::SCREEN_W * crate::ppu::SCREEN_H);
    }

    #[test]
    fn snapshot_restore_round_trips_without_touching_prg_rom() {
        let mut nes = Nes::new(counter_rom(), Mirroring::Horizontal);
        nes.run_frame();
        nes.run_frame();
        assert_eq!(nes.bus.ram[0x0000], 2);

        let snap = nes.snapshot();
        nes.run_frame();
        nes.run_frame();
        assert_eq!(nes.bus.ram[0x0000], 4);

        nes.restore(&snap);
        assert_eq!(nes.bus.ram[0x0000], 2, "restore should roll RAM back to the snapshot point");
        assert_eq!(nes.cpu.pc, snap.cpu.pc);
        // PRG-ROM must still be intact/readable after restore (it was never
        // touched by snapshot/restore in the first place).
        assert_eq!(nes.bus.mapper.read(0xFFFC), 0x00);
        assert_eq!(nes.bus.mapper.read(0xFFFD), 0x80);
    }

    #[test]
    fn controller_state_is_readable_through_the_bus() {
        let mut nes = Nes::new(counter_rom(), Mirroring::Horizontal);
        nes.set_controller(0, crate::controller::BUTTON_A);
        nes.bus.write(0x4016, 1);
        nes.bus.write(0x4016, 0);
        assert_eq!(nes.bus.read(0x4016) & 1, 1);
    }
}
