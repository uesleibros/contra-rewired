//! A real 2A03 APU: pulse 1/2, triangle, and noise channels, the frame
//! sequencer, and the standard non-linear NES mixing formula. This is what
//! turns a silent emulator into one that actually sounds like Contra.
//!
//! DMC (the sample-playback channel, mostly used by NES games for a
//! handful of percussion/voice samples) is **not implemented** - its
//! registers are accepted so games never stall writing to them, but it
//! never produces sound. Everything else (music, most sound effects) goes
//! through pulse/triangle/noise and should be audible. See ROADMAP.md.
//!
//! Reference: the public NESdev wiki's "APU" pages document every register
//! layout, table, and the mixing formula used here - none of this is
//! Contra-specific data.

use serde::{Deserialize, Serialize};

const NTSC_CPU_HZ: f64 = 1_789_773.0;

const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16,
    28, 32, 30,
];

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

const NOISE_PERIOD_TABLE: [u16; 16] =
    [4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068];

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Envelope {
    start: bool,
    decay: u8,
    divider: u8,
    loop_flag: bool,
    constant: bool,
    volume: u8,
}

impl Envelope {
    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.volume;
        } else if self.divider == 0 {
            self.divider = self.volume;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.loop_flag {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    fn output(&self) -> u8 {
        if self.constant {
            self.volume
        } else {
            self.decay
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Sweep {
    enabled: bool,
    period: u8,
    negate: bool,
    shift: u8,
    divider: u8,
    reload: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Pulse {
    ones_complement: bool,
    enabled: bool,
    duty: u8,
    duty_step: u8,
    timer_period: u16,
    timer: u16,
    length: u8,
    length_halt: bool,
    envelope: Envelope,
    sweep: Sweep,
}

impl Pulse {
    fn new(ones_complement: bool) -> Self {
        Self { ones_complement, ..Default::default() }
    }

    fn write_ctrl(&mut self, v: u8) {
        self.duty = (v >> 6) & 0x03;
        self.length_halt = v & 0x20 != 0;
        self.envelope.loop_flag = self.length_halt;
        self.envelope.constant = v & 0x10 != 0;
        self.envelope.volume = v & 0x0F;
    }

    fn write_sweep(&mut self, v: u8) {
        self.sweep.enabled = v & 0x80 != 0;
        self.sweep.period = (v >> 4) & 0x07;
        self.sweep.negate = v & 0x08 != 0;
        self.sweep.shift = v & 0x07;
        self.sweep.reload = true;
    }

    fn write_timer_low(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0x0700) | v as u16;
    }

    fn write_timer_high_and_length(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (((v & 0x07) as u16) << 8);
        if self.enabled {
            self.length = LENGTH_TABLE[(v >> 3) as usize];
        }
        self.duty_step = 0;
        self.envelope.start = true;
    }

    fn sweep_target(&self) -> u16 {
        let change = self.timer_period >> self.sweep.shift;
        if self.sweep.negate {
            if self.ones_complement {
                self.timer_period.wrapping_sub(change).wrapping_sub(1)
            } else {
                self.timer_period.wrapping_sub(change)
            }
        } else {
            self.timer_period.wrapping_add(change)
        }
    }

    fn sweep_muting(&self) -> bool {
        self.timer_period < 8 || self.sweep_target() > 0x7FF
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.duty_step = (self.duty_step + 1) & 7;
        } else {
            self.timer -= 1;
        }
    }

    fn clock_sweep(&mut self) {
        if self.sweep.divider == 0 && self.sweep.enabled && self.sweep.shift > 0 && !self.sweep_muting() {
            self.timer_period = self.sweep_target();
        }
        if self.sweep.divider == 0 || self.sweep.reload {
            self.sweep.divider = self.sweep.period;
            self.sweep.reload = false;
        } else {
            self.sweep.divider -= 1;
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length == 0 || self.sweep_muting() || DUTY_TABLE[self.duty as usize][self.duty_step as usize] == 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Triangle {
    enabled: bool,
    timer_period: u16,
    timer: u16,
    length: u8,
    length_halt: bool,
    linear_reload_value: u8,
    linear_counter: u8,
    linear_reload_flag: bool,
    step: u8,
}

impl Triangle {
    fn write_ctrl(&mut self, v: u8) {
        self.length_halt = v & 0x80 != 0;
        self.linear_reload_value = v & 0x7F;
    }

    fn write_timer_low(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0x0700) | v as u16;
    }

    fn write_timer_high_and_length(&mut self, v: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (((v & 0x07) as u16) << 8);
        if self.enabled {
            self.length = LENGTH_TABLE[(v >> 3) as usize];
        }
        self.linear_reload_flag = true;
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            if self.length > 0 && self.linear_counter > 0 {
                self.step = (self.step + 1) & 31;
            }
        } else {
            self.timer -= 1;
        }
    }

    fn clock_linear(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }
        if !self.length_halt {
            self.linear_reload_flag = false;
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length == 0 {
            0
        } else {
            TRIANGLE_SEQUENCE[self.step as usize]
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Noise {
    enabled: bool,
    length: u8,
    length_halt: bool,
    envelope: Envelope,
    mode: bool,
    period_index: u8,
    timer: u16,
    shift: u16,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            enabled: false,
            length: 0,
            length_halt: false,
            envelope: Envelope::default(),
            mode: false,
            period_index: 0,
            timer: 0,
            shift: 1,
        }
    }
}

impl Noise {
    fn write_ctrl(&mut self, v: u8) {
        self.length_halt = v & 0x20 != 0;
        self.envelope.loop_flag = self.length_halt;
        self.envelope.constant = v & 0x10 != 0;
        self.envelope.volume = v & 0x0F;
    }

    fn write_period(&mut self, v: u8) {
        self.mode = v & 0x80 != 0;
        self.period_index = v & 0x0F;
    }

    fn write_length(&mut self, v: u8) {
        if self.enabled {
            self.length = LENGTH_TABLE[(v >> 3) as usize];
        }
        self.envelope.start = true;
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = NOISE_PERIOD_TABLE[self.period_index as usize];
            let feedback_bit = if self.mode { 6 } else { 1 };
            let feedback = (self.shift & 1) ^ ((self.shift >> feedback_bit) & 1);
            self.shift >>= 1;
            self.shift |= feedback << 14;
        } else {
            self.timer -= 1;
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length > 0 {
            self.length -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length == 0 || self.shift & 1 != 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Apu {
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    five_step: bool,
    irq_inhibit: bool,
    frame_irq: bool,
    frame_cycle: u32,
    half_cycle_toggle: bool,
    sample_rate: f64,
    sample_acc_timer: f64,
    #[serde(skip)]
    pub sample_buffer: Vec<f32>,
    /// Raw byte last written to each APU register (`$4000-$401F`,
    /// indexed by `addr - 0x4000`), kept alongside the decoded internal
    /// state above - for debug tooling (verifying a native sound_code
    /// engine's computed writes against what the real 6502 code actually
    /// wrote) that wants ground-truth register bytes rather than
    /// re-deriving them from `Pulse`/`Noise`'s decoded fields.
    #[serde(skip)]
    pub last_write: [u8; 0x20],
}

impl Apu {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            pulse1: Pulse::new(true),
            pulse2: Pulse::new(false),
            triangle: Triangle::default(),
            noise: Noise::default(),
            five_step: false,
            irq_inhibit: false,
            frame_irq: false,
            frame_cycle: 0,
            half_cycle_toggle: false,
            sample_rate,
            sample_acc_timer: 0.0,
            sample_buffer: Vec::new(),
            last_write: [0; 0x20],
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                let v = (self.frame_irq as u8) << 6
                    | ((self.noise.length > 0) as u8) << 3
                    | ((self.triangle.length > 0) as u8) << 2
                    | ((self.pulse2.length > 0) as u8) << 1
                    | (self.pulse1.length > 0) as u8;
                self.frame_irq = false;
                v
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        if (0x4000..0x4020).contains(&addr) {
            self.last_write[(addr - 0x4000) as usize] = value;
        }
        match addr {
            0x4000 => self.pulse1.write_ctrl(value),
            0x4001 => self.pulse1.write_sweep(value),
            0x4002 => self.pulse1.write_timer_low(value),
            0x4003 => self.pulse1.write_timer_high_and_length(value),
            0x4004 => self.pulse2.write_ctrl(value),
            0x4005 => self.pulse2.write_sweep(value),
            0x4006 => self.pulse2.write_timer_low(value),
            0x4007 => self.pulse2.write_timer_high_and_length(value),
            0x4008 => self.triangle.write_ctrl(value),
            0x400A => self.triangle.write_timer_low(value),
            0x400B => self.triangle.write_timer_high_and_length(value),
            0x400C => self.noise.write_ctrl(value),
            0x400E => self.noise.write_period(value),
            0x400F => self.noise.write_length(value),
            0x4015 => {
                self.pulse1.enabled = value & 0x01 != 0;
                self.pulse2.enabled = value & 0x02 != 0;
                self.triangle.enabled = value & 0x04 != 0;
                self.noise.enabled = value & 0x08 != 0;
                if !self.pulse1.enabled {
                    self.pulse1.length = 0;
                }
                if !self.pulse2.enabled {
                    self.pulse2.length = 0;
                }
                if !self.triangle.enabled {
                    self.triangle.length = 0;
                }
                if !self.noise.enabled {
                    self.noise.length = 0;
                }
            }
            0x4017 => {
                self.five_step = value & 0x80 != 0;
                self.irq_inhibit = value & 0x40 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                }
                self.frame_cycle = 0;
                if self.five_step {
                    self.quarter_frame_clock();
                    self.half_frame_clock();
                }
            }
            _ => {}
        }
    }

    fn quarter_frame_clock(&mut self) {
        self.pulse1.envelope.clock();
        self.pulse2.envelope.clock();
        self.noise.envelope.clock();
        self.triangle.clock_linear();
    }

    fn half_frame_clock(&mut self) {
        self.pulse1.clock_length();
        self.pulse2.clock_length();
        self.triangle.clock_length();
        self.noise.clock_length();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    /// Advances the APU by exactly one CPU cycle. Call this once per CPU
    /// cycle actually elapsed (including OAM DMA stall cycles - the APU
    /// keeps running while DMA halts the CPU on real hardware).
    pub fn step(&mut self) {
        self.triangle.clock_timer();

        self.half_cycle_toggle = !self.half_cycle_toggle;
        if self.half_cycle_toggle {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
        }

        self.frame_cycle += 1;
        match (self.five_step, self.frame_cycle) {
            (false, 7457) => self.quarter_frame_clock(),
            (false, 14913) => {
                self.quarter_frame_clock();
                self.half_frame_clock();
            }
            (false, 22371) => self.quarter_frame_clock(),
            (false, 29829) => {
                self.quarter_frame_clock();
                self.half_frame_clock();
                if !self.irq_inhibit {
                    self.frame_irq = true;
                }
                self.frame_cycle = 0;
            }
            (true, 7457) => self.quarter_frame_clock(),
            (true, 14913) => {
                self.quarter_frame_clock();
                self.half_frame_clock();
            }
            (true, 22371) => self.quarter_frame_clock(),
            (true, 37281) => {
                self.quarter_frame_clock();
                self.half_frame_clock();
                self.frame_cycle = 0;
            }
            _ => {}
        }

        self.sample_acc_timer += 1.0;
        let cycles_per_sample = NTSC_CPU_HZ / self.sample_rate;
        if self.sample_acc_timer >= cycles_per_sample {
            self.sample_acc_timer -= cycles_per_sample;
            self.sample_buffer.push(self.mix());
        }
    }

    fn mix(&self) -> f32 {
        let p1 = self.pulse1.output() as f32;
        let p2 = self.pulse2.output() as f32;
        let t = self.triangle.output() as f32;
        let n = self.noise.output() as f32;

        let pulse_out = if p1 + p2 > 0.0 { 95.88 / (8128.0 / (p1 + p2) + 100.0) } else { 0.0 };
        let tnd_denom = t / 8227.0 + n / 12241.0;
        let tnd_out = if tnd_denom > 0.0 { 159.79 / (1.0 / tnd_denom + 100.0) } else { 0.0 };

        pulse_out + tnd_out
    }

    /// Drains and returns every sample generated since the last call, for
    /// the front-end to hand to its audio output.
    pub fn take_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.sample_buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabling_a_pulse_channel_and_setting_length_makes_it_audible() {
        let mut apu = Apu::new(44100.0);
        apu.write(0x4015, 0x01); // enable pulse 1
        apu.write(0x4000, 0x3F); // constant volume, max volume, duty 0
        apu.write(0x4002, 0x00); // timer low
        apu.write(0x4003, 0x08); // timer high=0, length index selects a nonzero length
        assert!(apu.pulse1.length > 0);
        assert_eq!(apu.pulse1.envelope.output(), 0x0F);
    }

    #[test]
    fn disabling_a_channel_clears_its_length_counter() {
        let mut apu = Apu::new(44100.0);
        apu.write(0x4015, 0x01);
        apu.write(0x4003, 0x08);
        assert!(apu.pulse1.length > 0);
        apu.write(0x4015, 0x00);
        assert_eq!(apu.pulse1.length, 0);
    }

    #[test]
    fn status_read_reports_active_length_counters_and_clears_frame_irq() {
        let mut apu = Apu::new(44100.0);
        apu.write(0x4015, 0x01);
        apu.write(0x4003, 0x08);
        apu.frame_irq = true;
        let status = apu.read(0x4015);
        assert_eq!(status & 0x01, 0x01, "pulse1 length active bit should be set");
        assert_eq!(status & 0x40, 0x40, "frame irq bit should have been set");
        assert!(!apu.frame_irq, "reading $4015 must clear the frame IRQ flag");
    }

    #[test]
    fn stepping_generates_samples_at_the_configured_rate() {
        let mut apu = Apu::new(44100.0);
        apu.write(0x4015, 0x01);
        apu.write(0x4000, 0x3F);
        apu.write(0x4002, 0x10);
        apu.write(0x4003, 0x08);
        for _ in 0..(NTSC_CPU_HZ as u32) {
            apu.step();
        }
        let samples = apu.take_samples();
        // ~1 second of CPU cycles should produce ~1 second of samples at
        // the configured sample rate, within rounding.
        assert!((samples.len() as i64 - 44100).abs() < 100, "got {} samples", samples.len());
    }

    #[test]
    fn noise_lfsr_eventually_produces_zero_and_nonzero_output_bits() {
        let mut apu = Apu::new(44100.0);
        apu.write(0x4015, 0x08); // enable noise
        apu.write(0x400C, 0x1F); // constant volume flag (bit4) + max volume (bits0-3)
        apu.write(0x400E, 0x00); // shortest period
        apu.write(0x400F, 0x08); // set length
        let mut saw_zero = false;
        let mut saw_nonzero = false;
        for _ in 0..10_000 {
            apu.noise.clock_timer();
            if apu.noise.output() == 0 {
                saw_zero = true;
            } else {
                saw_nonzero = true;
            }
        }
        assert!(saw_zero && saw_nonzero, "noise channel should toggle audibly, not get stuck");
    }
}
