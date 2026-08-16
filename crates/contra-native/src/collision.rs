//! Background collision - ported from `bank7.asm`'s `get_bg_collision`
//! (entry `$e0bb` in the real ROM's fixed bank) and the `read_bg_collision_
//! byte`/`bg_collision_logic` code it falls through into. This is the same
//! routine the community's own Mesen/FCEUX Lua debug scripts
//! (`nes-contra-us/docs/lua_scripts/*/Show Background Collisions.lua`)
//! reimplement for visualization - this port is a stricter, full-precision
//! translation of the real 6502 arithmetic (see [`bg_collision`]'s doc
//! comment for one case where that matters and the Lua scripts' simpler
//! version doesn't handle it), not a copy of that script.

/// What a background collision point resolves to - `BG_COLLISION_DATA`
/// stores 2 bits per point, and `collision_code_lookup_tbl` in the
/// disassembly maps all 4 possible 2-bit values to exactly these codes (no
/// 5th/reserved value exists in the lookup table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionCode {
    Empty,
    Floor,
    Water,
    Solid,
}

impl CollisionCode {
    /// From the raw 2-bit `BG_COLLISION_DATA` value via
    /// `collision_code_lookup_tbl: .byte $00,$01,$02,$80`.
    fn from_raw(code2: u8) -> Self {
        match code2 & 0x03 {
            0 => CollisionCode::Empty,
            1 => CollisionCode::Floor,
            2 => CollisionCode::Water,
            _ => CollisionCode::Solid,
        }
    }

    /// The exact byte `get_bg_collision` leaves in the accumulator
    /// (`collision_code_lookup_tbl`'s raw values, `$00`/`$01`/`$02`/`$80`) -
    /// for tests comparing directly against a captured real-hardware `a`
    /// register, and for any future caller that needs the original
    /// encoding rather than this enum.
    pub fn to_raw_byte(self) -> u8 {
        match self {
            CollisionCode::Empty => 0x00,
            CollisionCode::Floor => 0x01,
            CollisionCode::Water => 0x02,
            CollisionCode::Solid => 0x80,
        }
    }
}

/// `BG_COLLISION_DATA`'s real size: the offset computed below
/// (`nametable_bit(0x40) | vy_bits(0x00-0x3c) | hx_bits(0x00-0x03)`) can
/// reach at most `0x7f`, so the buffer is 128 bytes (`$0680-$06ff` in the
/// real ROM's RAM map, per `ram.asm`) - two nametables' worth, 64 bytes
/// each, matching `Ppu`'s own 2KB `vram` covering exactly 2 nametables.
pub const BG_COLLISION_DATA_LEN: usize = 128;

/// Ported from `get_bg_collision` (`bank7.asm`, CPU address `$e0bb` in the
/// real ROM - found by searching the ROM's raw bytes for this routine's
/// known opening instructions and converting the match to a CPU address,
/// the same technique the Base 1/Base 2 stage-select hang used). Answers
/// "what's at world position `(x, y)`, adjusted for the current camera
/// scroll" - the same question `soldier_generation_01`'s ground search and
/// the player's own falling/landing logic both ask before placing anything.
///
/// `x`/`y` are screen-relative sprite-style coordinates (not adjusted for
/// scroll yet - this function does that), matching what `get_bg_collision`
/// itself takes in the `a`/`y` registers. `vertical_scroll`/
/// `horizontal_scroll` and `ppuctrl_settings` are `VERTICAL_SCROLL`
/// (`$fc`), `HORIZONTAL_SCROLL` (`$fd`), and `PPUCTRL_SETTINGS` (`$ff`)
/// from `ram.asm` - the CPU-side shadow copies the game keeps of what it
/// last wrote to the PPU's actual scroll/control registers, since the PPU
/// registers themselves are write-only and can't be read back.
///
/// One place this diverges from the community's simpler Lua debug-overlay
/// version of this same routine: the real 6502 arithmetic for the
/// vertical-scroll adjustment can genuinely overflow a single byte (e.g.
/// `y=200, vertical_scroll=100`: `y+vertical_scroll=300`, which 8-bit
/// hardware truncates to `44` *before* deciding whether to add the `$10`
/// wraparound correction - and it always does add it here, since `BCS`
/// after the initial `ADC` catches this overflow case unconditionally,
/// regardless of what the truncated value ends up being). A version that
/// works entirely in un-truncated integers (as the Lua scripts do, treating
/// `y + VERTICAL_SCROLL` as a plain sum up to 510) happens to still get the
/// right *final* answer for this specific case through a different path,
/// but this port matches the hardware's actual 8-bit-at-each-step math
/// directly (`u8::overflowing_add`, mirroring the real `ADC`/`BCS`
/// sequence instruction for instruction) rather than relying on that
/// coincidence holding for every input.
pub fn bg_collision(
    x: u8,
    y: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
) -> CollisionCode {
    // bg_collision_logic: vertical adjustment.
    //   tya; sta $15 (y kept aside, unmodified, for the row-guard below)
    //   clc; adc VERTICAL_SCROLL; bcs @vert_overflow
    //   cmp #$f0; bcc @continue
    // @vert_overflow: adc #$0f   (carry is 1 on both paths into here, so
    //                             this is "+ 0x10" over the raw sum)
    // @continue: (raw unchanged)
    let (raw_vy, overflowed) = y.overflowing_add(vertical_scroll);
    let vy = if overflowed || raw_vy >= 0xF0 { raw_vy.wrapping_add(0x10) } else { raw_vy };

    // lda $13 (x); clc; adc HORIZONTAL_SCROLL; sta $12
    let (hx, hx_overflowed) = x.overflowing_add(horizontal_scroll);

    // lda PPUCTRL_SETTINGS; eor $10 (always 0 via this entry point - only
    // the hangar-mine-cart entry point, `get_cart_bg_collision`, leaves
    // $10 nonzero, and this function only ports the plain `get_bg_collision`
    // entry); and #$01; bcc @bg_collision_data; eor #$01
    // (the bcc tests the carry *from the ADC HORIZONTAL_SCROLL above* -
    // AND doesn't touch carry on 6502 - so this flips the nametable bit
    // exactly when x+horizontal_scroll overflowed a byte)
    let base_nametable_bit = ppuctrl_settings & 0x01;
    let nametable_number = if hx_overflowed { base_nametable_bit ^ 0x01 } else { base_nametable_bit };

    // lda $11 (vy); lsr; lsr; and #$3c
    let vy_bits = (vy >> 2) & 0x3C;
    // lda $12 (hx); lsr x4 -> $12; lsr x2 more (continues on the same
    // accumulator) -> hx >> 6; ora vy_bits; ora level_screen_mem_offset_tbl_01[nt]
    let hx_high_bits = hx >> 6;
    let nametable_offset_tbl = [0x00u8, 0x40u8]; // level_screen_mem_offset_tbl_01: .byte $00,$40
    let bg_collision_offset = (hx_high_bits | vy_bits | nametable_offset_tbl[nametable_number as usize]) as usize;
    // lda $12 (hx >> 4, from the earlier 4x lsr); and #$03
    let column = (hx >> 4) & 0x03;

    // read_bg_collision_byte: row-guard uses the ORIGINAL y (saved in $15
    // before any scroll adjustment), not vy - `cmp #$e0; bcs @set_code_exit`
    // (`lda #$00` already loaded before the branch, so the guard's result
    // is unconditionally "empty").
    if y >= 0xE0 {
        return CollisionCode::Empty;
    }

    let byte = bg_collision_data[bg_collision_offset];
    // column 0 -> bits 6-7 (shift right 6), 1 -> bits 4-5, 2 -> bits 2-3,
    // 3 -> bits 0-1 (no shift) - `read_bg_collision_byte`'s dey-chain.
    let shift = match column {
        0 => 6,
        1 => 4,
        2 => 2,
        _ => 0,
    };
    CollisionCode::from_raw((byte >> shift) & 0x03)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_with(offset: usize, byte: u8) -> [u8; BG_COLLISION_DATA_LEN] {
        let mut d = [0u8; BG_COLLISION_DATA_LEN];
        d[offset] = byte;
        d
    }

    #[test]
    fn empty_when_original_y_is_past_the_last_row() {
        let data = [0xFFu8; BG_COLLISION_DATA_LEN]; // would be "solid" everywhere if read
        assert_eq!(bg_collision(0, 0xE0, 0, 0, 0, &data), CollisionCode::Empty);
        assert_eq!(bg_collision(0, 0xFF, 0, 0, 0, &data), CollisionCode::Empty);
    }

    #[test]
    fn no_scroll_no_overflow_reads_the_expected_offset_and_column() {
        // x=0x10, y=0x10, no scroll: hx=0x10 -> column=(0x10>>4)&3=1,
        // hx_high_bits=0x10>>6=0. vy=0x10 -> vy_bits=(0x10>>2)&0x3c=0x04.
        // nametable bit from ppuctrl_settings=0 (bit0=0), no hx overflow ->
        // nametable_number=0 -> table[0]=0. offset = 0|0x04|0 = 0x04.
        // column=1 -> shift 4. Put code=2 (water) in bits 4-5 of that byte.
        let data = data_with(0x04, 0b0010_0000);
        assert_eq!(bg_collision(0x10, 0x10, 0, 0, 0, &data), CollisionCode::Water);
    }

    #[test]
    fn vertical_scroll_overflow_still_adds_the_wraparound_correction() {
        // y=200, vertical_scroll=100: raw sum=300, truncates to 44 (0x2c),
        // overflowed=true -> vy = 44+16 = 60 (0x3c). vy_bits=(60>>2)&0x3c=0x0c.
        // x=0, horizontal_scroll=0 -> hx=0 -> hx_high_bits=0, column=0 (shift 6).
        // offset = hx_high_bits(0) | vy_bits(0x0c) | table[0](0) = 0x0c.
        let data = data_with(0x0C, CollisionCode::Floor.to_raw_byte() << 6);
        let result = bg_collision(0x00, 200, 100, 0, 0, &data);
        assert_eq!(result, CollisionCode::Floor);
    }

    #[test]
    fn horizontal_overflow_flips_the_nametable_bit() {
        // x=0x80, horizontal_scroll=0x90: sum=0x110, overflows a u8.
        // base nametable bit (ppuctrl_settings & 1) = 1, flipped -> 0.
        // table[0]=0x00. hx (wrapped) = 0x10, hx_high_bits=0, column=(0x10>>4)&3=1.
        // y=0, vertical_scroll=0 -> vy=0, vy_bits=0. offset=0.
        let data = data_with(0x00, 0b0000_0011 << 4); // column 1 -> shift 4 -> code 3 (solid)
        let result = bg_collision(0x80, 0x00, 0x00, 0x90, 0x01, &data);
        assert_eq!(result, CollisionCode::Solid);
    }

    #[test]
    fn to_raw_byte_matches_the_documented_lookup_table_values() {
        assert_eq!(CollisionCode::Empty.to_raw_byte(), 0x00);
        assert_eq!(CollisionCode::Floor.to_raw_byte(), 0x01);
        assert_eq!(CollisionCode::Water.to_raw_byte(), 0x02);
        assert_eq!(CollisionCode::Solid.to_raw_byte(), 0x80);
    }
}
