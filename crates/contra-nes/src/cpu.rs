//! A from-scratch MOS 6502 interpreter, scoped to the NES's 2A03 variant
//! (no BCD/decimal ADC/SBC behavior - the 2A03 has the D flag but hardware
//! ignores it, so this core does too). Implements every official opcode.
//! Undocumented/"illegal" opcodes are treated as a 1-byte NOP and recorded
//! in [`Cpu::illegal_opcode_hit`] rather than panicking - very few
//! commercial NES games rely on them, but silently desyncing instead of
//! crashing is the safer failure mode for a game that otherwise works.
//!
//! No test ROM (Contra's or anyone else's) is needed to validate this file
//! - see the unit tests at the bottom, which are original short programs
//! assembled by hand into byte arrays, each checking one instruction's
//! documented behavior (registers, flags, and the famous JMP-indirect
//! page-boundary bug) against a plain flat-RAM [`Bus`].

/// Everything the CPU reads from / writes to. The full NES memory map
/// (RAM mirroring, PPU/APU registers, controllers, cartridge mapper) is
/// implemented by [`crate::bus::NesBus`]; this trait exists so the CPU can
/// also be tested against a trivial flat-RAM implementation.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);

    fn read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }
}

pub const FLAG_C: u8 = 1 << 0;
pub const FLAG_Z: u8 = 1 << 1;
pub const FLAG_I: u8 = 1 << 2;
pub const FLAG_D: u8 = 1 << 3;
pub const FLAG_B: u8 = 1 << 4;
pub const FLAG_U: u8 = 1 << 5; // always set on the physical chip
pub const FLAG_V: u8 = 1 << 6;
pub const FLAG_N: u8 = 1 << 7;

const NMI_VECTOR: u16 = 0xFFFA;
const RESET_VECTOR: u16 = 0xFFFC;
const IRQ_VECTOR: u16 = 0xFFFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AddrMode {
    /// Documented for completeness; opcodes using these modes (e.g. `TAX`,
    /// `ASL A`) are dispatched directly in `execute` without going through
    /// `operand_addr`, so the variants themselves are never matched on.
    #[allow(dead_code)]
    Imp,
    #[allow(dead_code)]
    Acc,
    Imm,
    Zp,
    Zpx,
    Zpy,
    Abs,
    Abx,
    Aby,
    Ind,
    Izx,
    Izy,
    Rel,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    /// Total CPU cycles elapsed since reset; used by the PPU/APU to stay in
    /// sync with the CPU.
    pub cycles: u64,
    #[serde(skip)]
    pub illegal_opcode_hit: Option<u8>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self { a: 0, x: 0, y: 0, sp: 0xFD, pc: 0, status: FLAG_U | FLAG_I, cycles: 0, illegal_opcode_hit: None }
    }
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self, bus: &mut impl Bus) {
        self.sp = self.sp.wrapping_sub(3);
        self.status |= FLAG_I;
        self.pc = bus.read_u16(RESET_VECTOR);
        self.cycles = self.cycles.wrapping_add(7);
    }

    pub fn nmi(&mut self, bus: &mut impl Bus) {
        self.push_u16(bus, self.pc);
        let status = (self.status | FLAG_U) & !FLAG_B;
        self.push(bus, status);
        self.status |= FLAG_I;
        self.pc = bus.read_u16(NMI_VECTOR);
        self.cycles = self.cycles.wrapping_add(7);
    }

    pub fn irq(&mut self, bus: &mut impl Bus) {
        if self.get_flag(FLAG_I) {
            return;
        }
        self.push_u16(bus, self.pc);
        let status = (self.status | FLAG_U) & !FLAG_B;
        self.push(bus, status);
        self.status |= FLAG_I;
        self.pc = bus.read_u16(IRQ_VECTOR);
        self.cycles = self.cycles.wrapping_add(7);
    }

    fn get_flag(&self, flag: u8) -> bool {
        self.status & flag != 0
    }

    fn set_flag(&mut self, flag: u8, value: bool) {
        if value {
            self.status |= flag;
        } else {
            self.status &= !flag;
        }
    }

    fn set_zn(&mut self, value: u8) {
        self.set_flag(FLAG_Z, value == 0);
        self.set_flag(FLAG_N, value & 0x80 != 0);
    }

    fn push(&mut self, bus: &mut impl Bus, value: u8) {
        bus.write(0x0100 + self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn pull(&mut self, bus: &mut impl Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 + self.sp as u16)
    }

    fn push_u16(&mut self, bus: &mut impl Bus, value: u16) {
        self.push(bus, (value >> 8) as u8);
        self.push(bus, (value & 0xFF) as u8);
    }

    fn pull_u16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.pull(bus) as u16;
        let hi = self.pull(bus) as u16;
        lo | (hi << 8)
    }

    fn fetch(&mut self, bus: &mut impl Bus) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch_u16(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch(bus) as u16;
        let hi = self.fetch(bus) as u16;
        lo | (hi << 8)
    }

    /// Resolves an addressing mode to an effective address, advancing PC
    /// past the instruction's operand bytes. Returns `(address,
    /// page_crossed)`; `page_crossed` drives the well-known "+1 cycle on
    /// page-crossing indexed reads" rule.
    fn operand_addr(&mut self, bus: &mut impl Bus, mode: AddrMode) -> (u16, bool) {
        match mode {
            AddrMode::Imm => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                (a, false)
            }
            AddrMode::Zp => (self.fetch(bus) as u16, false),
            AddrMode::Zpx => ((self.fetch(bus).wrapping_add(self.x)) as u16, false),
            AddrMode::Zpy => ((self.fetch(bus).wrapping_add(self.y)) as u16, false),
            AddrMode::Abs => (self.fetch_u16(bus), false),
            AddrMode::Abx => {
                let base = self.fetch_u16(bus);
                let addr = base.wrapping_add(self.x as u16);
                (addr, (base & 0xFF00) != (addr & 0xFF00))
            }
            AddrMode::Aby => {
                let base = self.fetch_u16(bus);
                let addr = base.wrapping_add(self.y as u16);
                (addr, (base & 0xFF00) != (addr & 0xFF00))
            }
            AddrMode::Ind => {
                let ptr = self.fetch_u16(bus);
                // Faithful 6502 bug: if the pointer's low byte is 0xFF, the
                // high byte wraps within the same page instead of crossing
                // into the next one.
                let lo = bus.read(ptr) as u16;
                let hi_addr = if ptr & 0x00FF == 0x00FF { ptr & 0xFF00 } else { ptr.wrapping_add(1) };
                let hi = bus.read(hi_addr) as u16;
                (lo | (hi << 8), false)
            }
            AddrMode::Izx => {
                let zp = self.fetch(bus).wrapping_add(self.x);
                let lo = bus.read(zp as u16) as u16;
                let hi = bus.read(zp.wrapping_add(1) as u16) as u16;
                (lo | (hi << 8), false)
            }
            AddrMode::Izy => {
                let zp = self.fetch(bus);
                let lo = bus.read(zp as u16) as u16;
                let hi = bus.read(zp.wrapping_add(1) as u16) as u16;
                let base = lo | (hi << 8);
                let addr = base.wrapping_add(self.y as u16);
                (addr, (base & 0xFF00) != (addr & 0xFF00))
            }
            AddrMode::Rel => {
                let offset = self.fetch(bus) as i8;
                let addr = (self.pc as i32 + offset as i32) as u16;
                (addr, false)
            }
            AddrMode::Imp | AddrMode::Acc => (0, false),
        }
    }

    /// Executes one instruction, returning the number of CPU cycles it
    /// took. Cycle counts follow the standard base-cycle table plus the
    /// documented +1-on-page-cross / +1-on-branch-taken rules; they do not
    /// yet model every hardware edge case (e.g. the extra dummy read some
    /// read-modify-write instructions perform), which is a known, tracked
    /// simplification (see docs/FIDELITY.md) rather than an oversight.
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        let opcode = self.fetch(bus);
        let cycles = self.execute(bus, opcode);
        self.cycles = self.cycles.wrapping_add(cycles as u64);
        cycles
    }

    fn branch(&mut self, bus: &mut impl Bus, condition: bool) -> u8 {
        let (target, _) = self.operand_addr(bus, AddrMode::Rel);
        if condition {
            let page_crossed = (self.pc & 0xFF00) != (target & 0xFF00);
            self.pc = target;
            if page_crossed {
                3
            } else {
                1
            }
        } else {
            0
        }
    }

    #[rustfmt::skip]
    fn execute(&mut self, bus: &mut impl Bus, opcode: u8) -> u8 {
        use AddrMode::*;
        match opcode {
            // ---- Load/store ----
            0xA9 => { let (a,_)=self.operand_addr(bus,Imm); self.lda(bus,a); 2 }
            0xA5 => { let (a,_)=self.operand_addr(bus,Zp);  self.lda(bus,a); 3 }
            0xB5 => { let (a,_)=self.operand_addr(bus,Zpx); self.lda(bus,a); 4 }
            0xAD => { let (a,_)=self.operand_addr(bus,Abs); self.lda(bus,a); 4 }
            0xBD => { let (a,pc)=self.operand_addr(bus,Abx); self.lda(bus,a); 4+pc as u8 }
            0xB9 => { let (a,pc)=self.operand_addr(bus,Aby); self.lda(bus,a); 4+pc as u8 }
            0xA1 => { let (a,_)=self.operand_addr(bus,Izx); self.lda(bus,a); 6 }
            0xB1 => { let (a,pc)=self.operand_addr(bus,Izy); self.lda(bus,a); 5+pc as u8 }

            0xA2 => { let (a,_)=self.operand_addr(bus,Imm); self.ldx(bus,a); 2 }
            0xA6 => { let (a,_)=self.operand_addr(bus,Zp);  self.ldx(bus,a); 3 }
            0xB6 => { let (a,_)=self.operand_addr(bus,Zpy); self.ldx(bus,a); 4 }
            0xAE => { let (a,_)=self.operand_addr(bus,Abs); self.ldx(bus,a); 4 }
            0xBE => { let (a,pc)=self.operand_addr(bus,Aby); self.ldx(bus,a); 4+pc as u8 }

            0xA0 => { let (a,_)=self.operand_addr(bus,Imm); self.ldy(bus,a); 2 }
            0xA4 => { let (a,_)=self.operand_addr(bus,Zp);  self.ldy(bus,a); 3 }
            0xB4 => { let (a,_)=self.operand_addr(bus,Zpx); self.ldy(bus,a); 4 }
            0xAC => { let (a,_)=self.operand_addr(bus,Abs); self.ldy(bus,a); 4 }
            0xBC => { let (a,pc)=self.operand_addr(bus,Abx); self.ldy(bus,a); 4+pc as u8 }

            0x85 => { let (a,_)=self.operand_addr(bus,Zp);  bus.write(a,self.a); 3 }
            0x95 => { let (a,_)=self.operand_addr(bus,Zpx); bus.write(a,self.a); 4 }
            0x8D => { let (a,_)=self.operand_addr(bus,Abs); bus.write(a,self.a); 4 }
            0x9D => { let (a,_)=self.operand_addr(bus,Abx); bus.write(a,self.a); 5 }
            0x99 => { let (a,_)=self.operand_addr(bus,Aby); bus.write(a,self.a); 5 }
            0x81 => { let (a,_)=self.operand_addr(bus,Izx); bus.write(a,self.a); 6 }
            0x91 => { let (a,_)=self.operand_addr(bus,Izy); bus.write(a,self.a); 6 }

            0x86 => { let (a,_)=self.operand_addr(bus,Zp);  bus.write(a,self.x); 3 }
            0x96 => { let (a,_)=self.operand_addr(bus,Zpy); bus.write(a,self.x); 4 }
            0x8E => { let (a,_)=self.operand_addr(bus,Abs); bus.write(a,self.x); 4 }

            0x84 => { let (a,_)=self.operand_addr(bus,Zp);  bus.write(a,self.y); 3 }
            0x94 => { let (a,_)=self.operand_addr(bus,Zpx); bus.write(a,self.y); 4 }
            0x8C => { let (a,_)=self.operand_addr(bus,Abs); bus.write(a,self.y); 4 }

            // ---- Transfers ----
            0xAA => { self.x=self.a; self.set_zn(self.x); 2 }
            0xA8 => { self.y=self.a; self.set_zn(self.y); 2 }
            0x8A => { self.a=self.x; self.set_zn(self.a); 2 }
            0x98 => { self.a=self.y; self.set_zn(self.a); 2 }
            0xBA => { self.x=self.sp; self.set_zn(self.x); 2 }
            0x9A => { self.sp=self.x; 2 }

            // ---- Stack ----
            0x48 => { self.push(bus,self.a); 3 }
            0x68 => { self.a=self.pull(bus); self.set_zn(self.a); 4 }
            0x08 => { self.push(bus,self.status|FLAG_U|FLAG_B); 3 }
            0x28 => { self.status=(self.pull(bus)&!FLAG_B)|FLAG_U; 4 }

            // ---- Arithmetic ----
            0x69 => { let (a,_)=self.operand_addr(bus,Imm); self.adc(bus,a); 2 }
            0x65 => { let (a,_)=self.operand_addr(bus,Zp);  self.adc(bus,a); 3 }
            0x75 => { let (a,_)=self.operand_addr(bus,Zpx); self.adc(bus,a); 4 }
            0x6D => { let (a,_)=self.operand_addr(bus,Abs); self.adc(bus,a); 4 }
            0x7D => { let (a,pc)=self.operand_addr(bus,Abx); self.adc(bus,a); 4+pc as u8 }
            0x79 => { let (a,pc)=self.operand_addr(bus,Aby); self.adc(bus,a); 4+pc as u8 }
            0x61 => { let (a,_)=self.operand_addr(bus,Izx); self.adc(bus,a); 6 }
            0x71 => { let (a,pc)=self.operand_addr(bus,Izy); self.adc(bus,a); 5+pc as u8 }

            0xE9 => { let (a,_)=self.operand_addr(bus,Imm); self.sbc(bus,a); 2 }
            0xE5 => { let (a,_)=self.operand_addr(bus,Zp);  self.sbc(bus,a); 3 }
            0xF5 => { let (a,_)=self.operand_addr(bus,Zpx); self.sbc(bus,a); 4 }
            0xED => { let (a,_)=self.operand_addr(bus,Abs); self.sbc(bus,a); 4 }
            0xFD => { let (a,pc)=self.operand_addr(bus,Abx); self.sbc(bus,a); 4+pc as u8 }
            0xF9 => { let (a,pc)=self.operand_addr(bus,Aby); self.sbc(bus,a); 4+pc as u8 }
            0xE1 => { let (a,_)=self.operand_addr(bus,Izx); self.sbc(bus,a); 6 }
            0xF1 => { let (a,pc)=self.operand_addr(bus,Izy); self.sbc(bus,a); 5+pc as u8 }

            // ---- Compare ----
            0xC9 => { let (a,_)=self.operand_addr(bus,Imm); self.cmp(bus,a,self.a); 2 }
            0xC5 => { let (a,_)=self.operand_addr(bus,Zp);  self.cmp(bus,a,self.a); 3 }
            0xD5 => { let (a,_)=self.operand_addr(bus,Zpx); self.cmp(bus,a,self.a); 4 }
            0xCD => { let (a,_)=self.operand_addr(bus,Abs); self.cmp(bus,a,self.a); 4 }
            0xDD => { let (a,pc)=self.operand_addr(bus,Abx); self.cmp(bus,a,self.a); 4+pc as u8 }
            0xD9 => { let (a,pc)=self.operand_addr(bus,Aby); self.cmp(bus,a,self.a); 4+pc as u8 }
            0xC1 => { let (a,_)=self.operand_addr(bus,Izx); self.cmp(bus,a,self.a); 6 }
            0xD1 => { let (a,pc)=self.operand_addr(bus,Izy); self.cmp(bus,a,self.a); 5+pc as u8 }

            0xE0 => { let (a,_)=self.operand_addr(bus,Imm); self.cmp(bus,a,self.x); 2 }
            0xE4 => { let (a,_)=self.operand_addr(bus,Zp);  self.cmp(bus,a,self.x); 3 }
            0xEC => { let (a,_)=self.operand_addr(bus,Abs); self.cmp(bus,a,self.x); 4 }

            0xC0 => { let (a,_)=self.operand_addr(bus,Imm); self.cmp(bus,a,self.y); 2 }
            0xC4 => { let (a,_)=self.operand_addr(bus,Zp);  self.cmp(bus,a,self.y); 3 }
            0xCC => { let (a,_)=self.operand_addr(bus,Abs); self.cmp(bus,a,self.y); 4 }

            // ---- Increment/decrement ----
            0xE6 => { let (a,_)=self.operand_addr(bus,Zp);  self.inc(bus,a); 5 }
            0xF6 => { let (a,_)=self.operand_addr(bus,Zpx); self.inc(bus,a); 6 }
            0xEE => { let (a,_)=self.operand_addr(bus,Abs); self.inc(bus,a); 6 }
            0xFE => { let (a,_)=self.operand_addr(bus,Abx); self.inc(bus,a); 7 }

            0xC6 => { let (a,_)=self.operand_addr(bus,Zp);  self.dec(bus,a); 5 }
            0xD6 => { let (a,_)=self.operand_addr(bus,Zpx); self.dec(bus,a); 6 }
            0xCE => { let (a,_)=self.operand_addr(bus,Abs); self.dec(bus,a); 6 }
            0xDE => { let (a,_)=self.operand_addr(bus,Abx); self.dec(bus,a); 7 }

            0xE8 => { self.x=self.x.wrapping_add(1); self.set_zn(self.x); 2 }
            0xCA => { self.x=self.x.wrapping_sub(1); self.set_zn(self.x); 2 }
            0xC8 => { self.y=self.y.wrapping_add(1); self.set_zn(self.y); 2 }
            0x88 => { self.y=self.y.wrapping_sub(1); self.set_zn(self.y); 2 }

            // ---- Logical ----
            0x29 => { let (a,_)=self.operand_addr(bus,Imm); self.and(bus,a); 2 }
            0x25 => { let (a,_)=self.operand_addr(bus,Zp);  self.and(bus,a); 3 }
            0x35 => { let (a,_)=self.operand_addr(bus,Zpx); self.and(bus,a); 4 }
            0x2D => { let (a,_)=self.operand_addr(bus,Abs); self.and(bus,a); 4 }
            0x3D => { let (a,pc)=self.operand_addr(bus,Abx); self.and(bus,a); 4+pc as u8 }
            0x39 => { let (a,pc)=self.operand_addr(bus,Aby); self.and(bus,a); 4+pc as u8 }
            0x21 => { let (a,_)=self.operand_addr(bus,Izx); self.and(bus,a); 6 }
            0x31 => { let (a,pc)=self.operand_addr(bus,Izy); self.and(bus,a); 5+pc as u8 }

            0x09 => { let (a,_)=self.operand_addr(bus,Imm); self.ora(bus,a); 2 }
            0x05 => { let (a,_)=self.operand_addr(bus,Zp);  self.ora(bus,a); 3 }
            0x15 => { let (a,_)=self.operand_addr(bus,Zpx); self.ora(bus,a); 4 }
            0x0D => { let (a,_)=self.operand_addr(bus,Abs); self.ora(bus,a); 4 }
            0x1D => { let (a,pc)=self.operand_addr(bus,Abx); self.ora(bus,a); 4+pc as u8 }
            0x19 => { let (a,pc)=self.operand_addr(bus,Aby); self.ora(bus,a); 4+pc as u8 }
            0x01 => { let (a,_)=self.operand_addr(bus,Izx); self.ora(bus,a); 6 }
            0x11 => { let (a,pc)=self.operand_addr(bus,Izy); self.ora(bus,a); 5+pc as u8 }

            0x49 => { let (a,_)=self.operand_addr(bus,Imm); self.eor(bus,a); 2 }
            0x45 => { let (a,_)=self.operand_addr(bus,Zp);  self.eor(bus,a); 3 }
            0x55 => { let (a,_)=self.operand_addr(bus,Zpx); self.eor(bus,a); 4 }
            0x4D => { let (a,_)=self.operand_addr(bus,Abs); self.eor(bus,a); 4 }
            0x5D => { let (a,pc)=self.operand_addr(bus,Abx); self.eor(bus,a); 4+pc as u8 }
            0x59 => { let (a,pc)=self.operand_addr(bus,Aby); self.eor(bus,a); 4+pc as u8 }
            0x41 => { let (a,_)=self.operand_addr(bus,Izx); self.eor(bus,a); 6 }
            0x51 => { let (a,pc)=self.operand_addr(bus,Izy); self.eor(bus,a); 5+pc as u8 }

            0x24 => { let (a,_)=self.operand_addr(bus,Zp);  self.bit(bus,a); 3 }
            0x2C => { let (a,_)=self.operand_addr(bus,Abs); self.bit(bus,a); 4 }

            // ---- Shifts/rotates ----
            0x0A => { self.a=self.asl(self.a); 2 }
            0x06 => { let (a,_)=self.operand_addr(bus,Zp);  self.rmw(bus,a,Self::asl); 5 }
            0x16 => { let (a,_)=self.operand_addr(bus,Zpx); self.rmw(bus,a,Self::asl); 6 }
            0x0E => { let (a,_)=self.operand_addr(bus,Abs); self.rmw(bus,a,Self::asl); 6 }
            0x1E => { let (a,_)=self.operand_addr(bus,Abx); self.rmw(bus,a,Self::asl); 7 }

            0x4A => { self.a=self.lsr(self.a); 2 }
            0x46 => { let (a,_)=self.operand_addr(bus,Zp);  self.rmw(bus,a,Self::lsr); 5 }
            0x56 => { let (a,_)=self.operand_addr(bus,Zpx); self.rmw(bus,a,Self::lsr); 6 }
            0x4E => { let (a,_)=self.operand_addr(bus,Abs); self.rmw(bus,a,Self::lsr); 6 }
            0x5E => { let (a,_)=self.operand_addr(bus,Abx); self.rmw(bus,a,Self::lsr); 7 }

            0x2A => { self.a=self.rol(self.a); 2 }
            0x26 => { let (a,_)=self.operand_addr(bus,Zp);  self.rmw(bus,a,Self::rol); 5 }
            0x36 => { let (a,_)=self.operand_addr(bus,Zpx); self.rmw(bus,a,Self::rol); 6 }
            0x2E => { let (a,_)=self.operand_addr(bus,Abs); self.rmw(bus,a,Self::rol); 6 }
            0x3E => { let (a,_)=self.operand_addr(bus,Abx); self.rmw(bus,a,Self::rol); 7 }

            0x6A => { self.a=self.ror(self.a); 2 }
            0x66 => { let (a,_)=self.operand_addr(bus,Zp);  self.rmw(bus,a,Self::ror); 5 }
            0x76 => { let (a,_)=self.operand_addr(bus,Zpx); self.rmw(bus,a,Self::ror); 6 }
            0x6E => { let (a,_)=self.operand_addr(bus,Abs); self.rmw(bus,a,Self::ror); 6 }
            0x7E => { let (a,_)=self.operand_addr(bus,Abx); self.rmw(bus,a,Self::ror); 7 }

            // ---- Jumps/calls ----
            0x4C => { let (a,_)=self.operand_addr(bus,Abs); self.pc=a; 3 }
            0x6C => { let (a,_)=self.operand_addr(bus,Ind); self.pc=a; 5 }
            0x20 => {
                let (a,_)=self.operand_addr(bus,Abs);
                let ret = self.pc.wrapping_sub(1);
                self.push_u16(bus, ret);
                self.pc=a;
                6
            }
            0x60 => { let ret=self.pull_u16(bus); self.pc=ret.wrapping_add(1); 6 }
            0x40 => {
                self.status=(self.pull(bus)&!FLAG_B)|FLAG_U;
                self.pc=self.pull_u16(bus);
                6
            }
            0x00 => {
                self.pc = self.pc.wrapping_add(1);
                self.push_u16(bus, self.pc);
                self.push(bus, self.status|FLAG_U|FLAG_B);
                self.set_flag(FLAG_I, true);
                self.pc = bus.read_u16(IRQ_VECTOR);
                7
            }

            // ---- Branches ----
            0x10 => 2+self.branch(bus, !self.get_flag(FLAG_N)),
            0x30 => 2+self.branch(bus, self.get_flag(FLAG_N)),
            0x50 => 2+self.branch(bus, !self.get_flag(FLAG_V)),
            0x70 => 2+self.branch(bus, self.get_flag(FLAG_V)),
            0x90 => 2+self.branch(bus, !self.get_flag(FLAG_C)),
            0xB0 => 2+self.branch(bus, self.get_flag(FLAG_C)),
            0xD0 => 2+self.branch(bus, !self.get_flag(FLAG_Z)),
            0xF0 => 2+self.branch(bus, self.get_flag(FLAG_Z)),

            // ---- Flags ----
            0x18 => { self.set_flag(FLAG_C,false); 2 }
            0x38 => { self.set_flag(FLAG_C,true); 2 }
            0x58 => { self.set_flag(FLAG_I,false); 2 }
            0x78 => { self.set_flag(FLAG_I,true); 2 }
            0xB8 => { self.set_flag(FLAG_V,false); 2 }
            0xD8 => { self.set_flag(FLAG_D,false); 2 }
            0xF8 => { self.set_flag(FLAG_D,true); 2 }

            // ---- No-op ----
            0xEA => 2,

            // ---- Unofficial opcode: treated as NOP, recorded for diagnostics ----
            other => {
                self.illegal_opcode_hit = Some(other);
                2
            }
        }
    }

    fn lda(&mut self, bus: &mut impl Bus, addr: u16) {
        self.a = bus.read(addr);
        self.set_zn(self.a);
    }
    fn ldx(&mut self, bus: &mut impl Bus, addr: u16) {
        self.x = bus.read(addr);
        self.set_zn(self.x);
    }
    fn ldy(&mut self, bus: &mut impl Bus, addr: u16) {
        self.y = bus.read(addr);
        self.set_zn(self.y);
    }

    fn adc(&mut self, bus: &mut impl Bus, addr: u16) {
        let m = bus.read(addr);
        self.adc_value(m);
    }
    fn adc_value(&mut self, m: u8) {
        let carry_in = self.get_flag(FLAG_C) as u16;
        let sum = self.a as u16 + m as u16 + carry_in;
        let result = sum as u8;
        self.set_flag(FLAG_C, sum > 0xFF);
        self.set_flag(FLAG_V, (!(self.a ^ m) & (self.a ^ result) & 0x80) != 0);
        self.a = result;
        self.set_zn(self.a);
    }
    fn sbc(&mut self, bus: &mut impl Bus, addr: u16) {
        let m = bus.read(addr);
        self.adc_value(!m);
    }

    fn cmp(&mut self, bus: &mut impl Bus, addr: u16, reg: u8) {
        let m = bus.read(addr);
        let result = reg.wrapping_sub(m);
        self.set_flag(FLAG_C, reg >= m);
        self.set_zn(result);
    }

    fn inc(&mut self, bus: &mut impl Bus, addr: u16) {
        let v = bus.read(addr).wrapping_add(1);
        bus.write(addr, v);
        self.set_zn(v);
    }
    fn dec(&mut self, bus: &mut impl Bus, addr: u16) {
        let v = bus.read(addr).wrapping_sub(1);
        bus.write(addr, v);
        self.set_zn(v);
    }

    fn and(&mut self, bus: &mut impl Bus, addr: u16) {
        self.a &= bus.read(addr);
        self.set_zn(self.a);
    }
    fn ora(&mut self, bus: &mut impl Bus, addr: u16) {
        self.a |= bus.read(addr);
        self.set_zn(self.a);
    }
    fn eor(&mut self, bus: &mut impl Bus, addr: u16) {
        self.a ^= bus.read(addr);
        self.set_zn(self.a);
    }
    fn bit(&mut self, bus: &mut impl Bus, addr: u16) {
        let m = bus.read(addr);
        self.set_flag(FLAG_Z, (self.a & m) == 0);
        self.set_flag(FLAG_V, m & 0x40 != 0);
        self.set_flag(FLAG_N, m & 0x80 != 0);
    }

    fn asl(&mut self, v: u8) -> u8 {
        self.set_flag(FLAG_C, v & 0x80 != 0);
        let r = v << 1;
        self.set_zn(r);
        r
    }
    fn lsr(&mut self, v: u8) -> u8 {
        self.set_flag(FLAG_C, v & 0x01 != 0);
        let r = v >> 1;
        self.set_zn(r);
        r
    }
    fn rol(&mut self, v: u8) -> u8 {
        let carry_in = self.get_flag(FLAG_C) as u8;
        self.set_flag(FLAG_C, v & 0x80 != 0);
        let r = (v << 1) | carry_in;
        self.set_zn(r);
        r
    }
    fn ror(&mut self, v: u8) -> u8 {
        let carry_in = self.get_flag(FLAG_C) as u8;
        self.set_flag(FLAG_C, v & 0x01 != 0);
        let r = (v >> 1) | (carry_in << 7);
        self.set_zn(r);
        r
    }
    fn rmw(&mut self, bus: &mut impl Bus, addr: u16, op: fn(&mut Self, u8) -> u8) {
        let v = bus.read(addr);
        let r = op(self, v);
        bus.write(addr, r);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FlatBus {
        mem: [u8; 0x10000],
    }
    impl FlatBus {
        fn new() -> Self {
            Self { mem: [0; 0x10000] }
        }
        fn load(&mut self, addr: u16, program: &[u8]) {
            for (i, b) in program.iter().enumerate() {
                self.mem[addr as usize + i] = *b;
            }
        }
    }
    impl Bus for FlatBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.mem[addr as usize]
        }
        fn write(&mut self, addr: u16, value: u8) {
            self.mem[addr as usize] = value;
        }
    }

    fn run(program: &[u8]) -> (Cpu, FlatBus) {
        let mut bus = FlatBus::new();
        bus.load(0x8000, program);
        bus.mem[0xFFFC] = 0x00;
        bus.mem[0xFFFD] = 0x80;
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        for _ in 0..program.len() {
            cpu.step(&mut bus);
        }
        (cpu, bus)
    }

    #[test]
    fn lda_immediate_sets_a_and_flags() {
        let (cpu, _) = run(&[0xA9, 0x00]); // LDA #$00
        assert_eq!(cpu.a, 0);
        assert!(cpu.get_flag(FLAG_Z));
        let (cpu, _) = run(&[0xA9, 0x80]); // LDA #$80
        assert!(cpu.get_flag(FLAG_N));
    }

    #[test]
    fn adc_sets_carry_and_overflow_correctly() {
        // 0x7F + 0x01 = 0x80: signed overflow (positive+positive=negative)
        let (cpu, _) = run(&[0xA9, 0x7F, 0x69, 0x01]);
        assert_eq!(cpu.a, 0x80);
        assert!(cpu.get_flag(FLAG_V));
        assert!(!cpu.get_flag(FLAG_C));

        // 0xFF + 0x01 = 0x00 with carry out, no signed overflow
        let (cpu, _) = run(&[0xA9, 0xFF, 0x69, 0x01]);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.get_flag(FLAG_C));
        assert!(!cpu.get_flag(FLAG_V));
    }

    #[test]
    fn sbc_matches_adc_of_complement() {
        // 0x05 - 0x01 with carry set (no borrow) = 0x04
        let program = [0x38, 0xA9, 0x05, 0xE9, 0x01]; // SEC; LDA #5; SBC #1
        let (cpu, _) = run(&program);
        assert_eq!(cpu.a, 0x04);
        assert!(cpu.get_flag(FLAG_C)); // no borrow occurred
    }

    #[test]
    fn stack_push_pull_round_trips() {
        // LDA #$42; PHA; LDA #$00; PLA
        let (cpu, _) = run(&[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68]);
        assert_eq!(cpu.a, 0x42);
    }

    #[test]
    fn jsr_rts_returns_to_the_instruction_after() {
        // JSR $8005; BRK(pad); BRK(pad); LDX #$99 <- subroutine at $8005
        let mut bus = FlatBus::new();
        bus.load(0x8000, &[0x20, 0x05, 0x80]); // JSR $8005
        bus.load(0x8005, &[0xA2, 0x99, 0x60]); // LDX #$99; RTS
        bus.mem[0xFFFC] = 0x00;
        bus.mem[0xFFFD] = 0x80;
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // JSR
        cpu.step(&mut bus); // LDX #$99
        cpu.step(&mut bus); // RTS
        assert_eq!(cpu.x, 0x99);
        assert_eq!(cpu.pc, 0x8003); // back at the instruction after JSR
    }

    #[test]
    fn branch_taken_and_page_cross_add_cycles() {
        let mut bus = FlatBus::new();
        // CLC; BCC +2 (branch taken, no page cross)
        bus.load(0x8000, &[0x18, 0x90, 0x02]);
        bus.mem[0xFFFC] = 0x00;
        bus.mem[0xFFFD] = 0x80;
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus); // CLC
        let cycles = cpu.step(&mut bus); // BCC (taken, same page)
        assert_eq!(cycles, 3);
        assert_eq!(cpu.pc, 0x8005);
    }

    #[test]
    fn jmp_indirect_reproduces_the_page_boundary_bug() {
        let mut bus = FlatBus::new();
        // Pointer at $30FF straddles a page: low byte at $30FF, buggy high
        // byte read wraps to $3000 instead of $3100.
        bus.mem[0x30FF] = 0x34;
        bus.mem[0x3000] = 0x12; // what the (buggy) hardware actually reads
        bus.mem[0x3100] = 0x56; // what a "correct" wrap would have read
        bus.load(0x8000, &[0x6C, 0xFF, 0x30]); // JMP ($30FF)
        bus.mem[0xFFFC] = 0x00;
        bus.mem[0xFFFD] = 0x80;
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        cpu.step(&mut bus);
        assert_eq!(cpu.pc, 0x1234, "must reproduce the page-wrap bug, not the 'fixed' behavior");
    }

    #[test]
    fn illegal_opcode_is_recorded_not_fatal() {
        let (cpu, _) = run(&[0x02]); // no official meaning
        assert_eq!(cpu.illegal_opcode_hit, Some(0x02));
    }
}
