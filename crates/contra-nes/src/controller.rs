//! Standard NES controller: an 8-bit shift register loaded from the live
//! button state on strobe, shifted out one bit per `$4016`/`$4017` read.

use serde::{Deserialize, Serialize};

pub const BUTTON_A: u8 = 1 << 0;
pub const BUTTON_B: u8 = 1 << 1;
pub const BUTTON_SELECT: u8 = 1 << 2;
pub const BUTTON_START: u8 = 1 << 3;
pub const BUTTON_UP: u8 = 1 << 4;
pub const BUTTON_DOWN: u8 = 1 << 5;
pub const BUTTON_LEFT: u8 = 1 << 6;
pub const BUTTON_RIGHT: u8 = 1 << 7;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Controller {
    pub state: u8,
    shift: u8,
    strobe: bool,
}

impl Controller {
    pub fn write_strobe(&mut self, value: u8) {
        self.strobe = value & 1 != 0;
        if self.strobe {
            self.shift = self.state;
        }
    }

    pub fn read(&mut self) -> u8 {
        if self.strobe {
            self.shift = self.state;
        }
        let bit = self.shift & 1;
        self.shift = (self.shift >> 1) | 0x80;
        // Real hardware open-bus behavior returns 1s for reads past the
        // 8th; bit position 0x40 is conventionally left clear here since
        // we don't emulate expansion-port devices.
        bit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifts_out_buttons_msb_last_lsb_first() {
        let mut c = Controller { state: BUTTON_A | BUTTON_RIGHT, shift: 0, strobe: false };
        c.write_strobe(1);
        c.write_strobe(0);
        assert_eq!(c.read(), 1); // A
        for _ in 0..6 {
            assert_eq!(c.read(), 0);
        }
        assert_eq!(c.read(), 1); // Right (bit 7)
    }

    #[test]
    fn strobe_held_high_keeps_returning_bit_zero_state() {
        let mut c = Controller { state: BUTTON_START, shift: 0, strobe: false };
        c.write_strobe(1);
        assert_eq!(c.read(), 0); // START is bit 3, not bit 0
        c.state = BUTTON_A;
        assert_eq!(c.read(), 1); // still strobing, re-reads live state's bit 0
    }
}
