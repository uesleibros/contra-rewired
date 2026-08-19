//! Native port of Contra's live nametable-write-queue subsystem
//! (`CPU_GRAPHICS_BUFFER`, `src/bank7.asm`) - the mechanism `draw_enemy_
//! supertile_a`/`update_enemy_nametable_tiles`/`load_bank_3_update_
//! nametable_supertile` and their many real callers (bridges, weapon
//! boxes, wall cores/turrets, the tank's tires, spiked walls, fire
//! beams, ...) all funnel through to redraw part of the nametable at
//! runtime, queueing writes for the NMI handler to actually flush to the
//! PPU rather than writing `PPUDATA` directly mid-frame. This is a
//! foundational piece of a still-unfinished subsystem - started here,
//! not complete: see [`set_ppu_addresses_in_mem`]'s own doc comment for
//! what's covered and `docs/NATIVE_PORT.md` for what's still blocked on
//! it.
//!
//! ## Why this was worth the extra care
//!
//! Unlike almost everything else ported so far, getting this wrong
//! wouldn't just break one enemy's behavior - every family this
//! subsystem eventually unblocks depends on its address math being
//! exactly right. Rather than trust a single literal transcription of
//! the (genuinely dense) 6502 bit-shuffling, this port cross-derived
//! the algorithm against the well-known, standard NES nametable/
//! attribute-table addressing formulas (`addr = $2000 + nt*$400 +
//! tile_y*32 + tile_x`, `attr_addr = $23C0 + nt*$400 + (tile_y/4)*8 +
//! (tile_x/4)`) by hand for concrete worked examples before writing any
//! Rust, and this module's own tests re-run that same cross-check in
//! code (see `matches_the_standard_nes_addressing_formula`) rather than
//! only asserting against values re-derived from the same ASM reading.

/// `nametable_base_high_byte` (`$b132`, 2 bytes) - PPU nametable high
/// byte per nametable half.
const NAMETABLE_BASE_HIGH_BYTE: [u8; 2] = [0x20, 0x24];
/// `attribute_base_high_byte` (`$b12e`, 2 bytes) - PPU attribute-table
/// high byte per nametable half (already `| 0x03`'d by the real ASM,
/// baked into this port's own use of it the same way).
const ATTRIBUTE_BASE_HIGH_BYTE: [u8; 2] = [0x23, 0x27];
/// `level_screen_mem_offset_tbl_00` (`$b136`, 2 bytes) - base offset
/// into `LEVEL_SCREEN_SUPERTILES` per nametable half (`$0600`/`$0640` -
/// the same `[0x00, 0x40]` shape `crate::physics::collision::bg_
/// collision`'s own `level_screen_mem_offset_tbl_01` uses for `BG_
/// COLLISION_DATA`, a structurally analogous but distinct table).
const LEVEL_SCREEN_MEM_OFFSET_TBL_00: [u8; 2] = [0x00, 0x40];

/// The real ASM's own vertical-scroll rounding idiom
/// (`clc;adc VERTICAL_SCROLL;bcs @overflow;cmp #$f0;bcc @continue;
/// @overflow: adc #$0f`) - identical to `crate::physics::collision::
/// bg_collision`'s own `vy` computation (confirmed by reading both real
/// routines side by side), reused here as the same function rather than
/// re-derived, since it's provably the same real instruction sequence.
fn round_vertical(y: u8, vertical_scroll: u8) -> u8 {
    let (raw, overflowed) = y.overflowing_add(vertical_scroll);
    if overflowed || raw >= 0xF0 { raw.wrapping_add(0x10) } else { raw }
}

/// The full set of addresses/offsets [`set_ppu_addresses_in_mem`]
/// computes for one supertile draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpuAddresses {
    /// Real ASM's `$0c`/`$0d` (PPU write address) and `$12`/`$13`
    /// ("collision" address) - always the same value in both places, so
    /// this port keeps just the one field.
    pub nametable_addr: u16,
    pub attribute_addr: u16,
    /// `$02` - offset into `LEVEL_SCREEN_SUPERTILES` for the supertile
    /// at this position (attribute-cell/supertile granularity: 4x4
    /// tiles, matching `crate::world::supertile`'s own definition of a
    /// supertile).
    pub level_screen_supertile_offset: u8,
    /// `$00` - which of the attribute byte's 4 packed 2-bit palette
    /// slots (quadrants) this tile's own 2x2-tile group falls in
    /// (`0..=3`).
    pub quadrant_bits: u8,
    /// From `$0f` (real ASM: `#$80` = don't touch the palette, any other
    /// value = do) - `true` unless the input's bit 7 was set.
    pub should_update_palette: bool,
    /// `$10` after the real ASM's own `and #$7f` - the clean supertile/
    /// palette-data index, with the "don't update palette" flag bit
    /// stripped.
    pub tile_index: u8,
}

/// Native port of `set_ppu_addresses_in_mem` (`$e999`) - given a pixel
/// position and which supertile/palette entry to draw there (`tile_idx_
/// with_flag`, bit 7 = "skip the palette update"), computes every PPU
/// address and table offset the rest of the nametable-update subsystem
/// needs. `x_pixel`/`y_pixel` are the *unscrolled* pixel coordinates the
/// real ASM takes in `a`/`y` (this function applies scroll itself, the
/// same way the real code does).
#[allow(clippy::too_many_arguments)]
pub fn set_ppu_addresses_in_mem(
    x_pixel: u8,
    y_pixel: u8,
    tile_idx_with_flag: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
) -> PpuAddresses {
    let should_update_palette = tile_idx_with_flag & 0x80 == 0;
    let tile_index = tile_idx_with_flag & 0x7F;

    let vy = round_vertical(y_pixel, vertical_scroll);
    let vy_masked = vy & 0xF8;

    // Two in-place `asl $12` on `vy_masked`, rotated into the
    // accumulator: extracts bits 6-7 of `vy` (the nametable-row
    // "which 256px vertical band" overflow bits).
    let nametable_y_bits = (vy >> 6) & 0x03;
    // The *value* left in `$12` by those same two shifts (mod 256) -
    // `tile_row`'s low 3 bits repositioned into the nametable low
    // byte's own bits 5-7.
    let vy_shifted_twice = vy_masked << 2;

    let vy2 = vy_masked >> 2;
    // Attribute-table row contribution: `(tile_row / 4) * 8`.
    let attr_row_contribution = vy2 & 0x38;
    // Bit 1 of `tile_row` - half of the attribute quadrant selector.
    let vy_bit_for_quadrant = (vy2 >> 1) & 0x02;

    let (hx, hx_overflow) = x_pixel.overflowing_add(horizontal_scroll);
    let base_nametable_bit = ppuctrl_settings & 0x01;
    let nametable_index = (if hx_overflow { base_nametable_bit ^ 0x01 } else { base_nametable_bit }) as usize;

    let attribute_high = ATTRIBUTE_BASE_HIGH_BYTE[nametable_index];
    let nametable_high = NAMETABLE_BASE_HIGH_BYTE[nametable_index] | nametable_y_bits;
    let level_screen_base = LEVEL_SCREEN_MEM_OFFSET_TBL_00[nametable_index];

    let hx_masked = hx & 0xF8;
    // `tile_col`, shifted into position for the nametable low byte.
    let x_reg2 = hx_masked >> 3;
    // `tile_col / 2` - shared by the quadrant bit and the attribute
    // column contribution below.
    let x_reg1 = hx_masked >> 4;

    // Bit 1 of `tile_col` (the other half of the quadrant selector).
    let quadrant_bits = vy_bit_for_quadrant | (x_reg1 & 0x01);

    // Attribute-table column contribution: `tile_col / 4`.
    let attr_col_contribution = x_reg1 >> 1;
    let attr_low = attr_col_contribution | attr_row_contribution;
    let attribute_low = attr_low.wrapping_add(0xC0);

    let nametable_low = x_reg2 | vy_shifted_twice;

    let level_screen_supertile_offset = level_screen_base | attr_low;

    PpuAddresses {
        nametable_addr: u16::from_be_bytes([nametable_high, nametable_low]),
        attribute_addr: u16::from_be_bytes([attribute_high, attribute_low]),
        level_screen_supertile_offset,
        quadrant_bits,
        should_update_palette,
        tile_index,
    }
}

/// Native port of `set_graphics_buffer_header` (`$e9ea`) - the real
/// `CPU_GRAPHICS_BUFFER` command header for "write exactly one byte to
/// PPU address `addr`" (VRAM increment mode `1`, one group of one byte,
/// then the PPU address high/low): `[0x01, 0x01, 0x01, addr_hi,
/// addr_lo]`, in the exact byte order the real ASM writes them (this
/// port returns the bytes rather than taking a buffer + cursor to
/// mutate, since nothing else in this crate models `CPU_GRAPHICS_
/// BUFFER`/`GRAPHICS_BUFFER_OFFSET` as live state yet - see this
/// module's own doc comment).
pub fn set_graphics_buffer_header(addr: u16) -> [u8; 5] {
    let [hi, lo] = addr.to_be_bytes();
    [0x01, 0x01, 0x01, hi, lo]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphics_buffer_header_writes_a_single_byte_command() {
        assert_eq!(set_graphics_buffer_header(0x23D2), [0x01, 0x01, 0x01, 0x23, 0xD2]);
    }

    /// Standard NES addressing, computed independently of this port's
    /// own bit-shuffled derivation, for cross-checking.
    fn standard_addresses(tile_col: u16, tile_row: u16, nametable_index: u16) -> (u16, u16) {
        let nt_addr = 0x2000 + nametable_index * 0x400 + tile_row * 32 + tile_col;
        let attr_addr = 0x23C0 + nametable_index * 0x400 + (tile_row / 4) * 8 + (tile_col / 4);
        (nt_addr, attr_addr)
    }

    #[test]
    fn zero_position_gives_the_base_addresses() {
        let r = set_ppu_addresses_in_mem(0, 0, 0x00, 0, 0, 0);
        assert_eq!(r.nametable_addr, 0x2000);
        assert_eq!(r.attribute_addr, 0x23C0);
        assert_eq!(r.level_screen_supertile_offset, 0x00);
        assert_eq!(r.quadrant_bits, 0x00);
    }

    #[test]
    fn matches_the_standard_nes_addressing_formula() {
        // tile_col=8, tile_row=8 (pixel 64,64), nametable 0.
        let r = set_ppu_addresses_in_mem(64, 64, 0x00, 0, 0, 0);
        let (expected_nt, expected_attr) = standard_addresses(8, 8, 0);
        assert_eq!(r.nametable_addr, expected_nt);
        assert_eq!(r.attribute_addr, expected_attr);
    }

    #[test]
    fn matches_the_standard_formula_at_a_second_position() {
        // tile_col=19, tile_row=5 (pixel 152,40), nametable 0.
        let r = set_ppu_addresses_in_mem(152, 40, 0x00, 0, 0, 0);
        let (expected_nt, expected_attr) = standard_addresses(19, 5, 0);
        assert_eq!(r.nametable_addr, expected_nt);
        assert_eq!(r.attribute_addr, expected_attr);
    }

    #[test]
    fn nametable_row_overflow_bits_carry_into_the_high_byte() {
        // tile_row=8 needs bit 3 of the 5-bit row value set - verify the
        // high byte picks up the (vy>>6)&3 contribution.
        let r = set_ppu_addresses_in_mem(0, 64, 0x00, 0, 0, 0);
        assert_eq!(r.nametable_addr >> 8, 0x21); // 0x20 | ((64>>6)&3)=1
    }

    #[test]
    fn horizontal_scroll_overflow_flips_the_nametable_selection() {
        let no_overflow = set_ppu_addresses_in_mem(0x10, 0x00, 0x00, 0, 0, 0x00);
        assert_eq!(no_overflow.nametable_addr >> 8, 0x20);
        let overflow = set_ppu_addresses_in_mem(0xF0, 0x00, 0x00, 0, 0x20, 0x00); // x+horizontal_scroll overflows a byte
        assert_eq!(overflow.nametable_addr >> 8, 0x24); // flipped to nametable 1
    }

    #[test]
    fn vertical_scroll_is_applied_before_computing_addresses() {
        let plain = set_ppu_addresses_in_mem(0, 8, 0x00, 0, 0, 0);
        let scrolled = set_ppu_addresses_in_mem(0, 0, 0x00, 8, 0, 0);
        assert_eq!(plain.nametable_addr, scrolled.nametable_addr);
    }

    #[test]
    fn tile_idx_flag_bit_controls_should_update_palette() {
        let with_update = set_ppu_addresses_in_mem(0, 0, 0x05, 0, 0, 0);
        assert!(with_update.should_update_palette);
        assert_eq!(with_update.tile_index, 0x05);
        let without_update = set_ppu_addresses_in_mem(0, 0, 0x85, 0, 0, 0);
        assert!(!without_update.should_update_palette);
        assert_eq!(without_update.tile_index, 0x05);
    }

    #[test]
    fn quadrant_bits_cycle_through_all_four_values() {
        // (tile_col, tile_row) parity determines the quadrant: even/even=0,
        // odd/even=1, even/odd=2, odd/odd=3 (bit0=tile_col bit1, bit1=tile_row bit1).
        let q = |x_tile: u8, y_tile: u8| set_ppu_addresses_in_mem(x_tile * 8, y_tile * 8, 0x00, 0, 0, 0).quadrant_bits;
        assert_eq!(q(0, 0), 0);
        assert_eq!(q(2, 0), 1);
        assert_eq!(q(0, 2), 2);
        assert_eq!(q(2, 2), 3);
    }

    #[test]
    fn level_screen_supertile_offset_uses_supertile_granularity() {
        // Two positions within the same 4x4-tile supertile must produce
        // the same LEVEL_SCREEN_SUPERTILES offset.
        let a = set_ppu_addresses_in_mem(64, 64, 0x00, 0, 0, 0).level_screen_supertile_offset;
        let b = set_ppu_addresses_in_mem(88, 88, 0x00, 0, 0, 0).level_screen_supertile_offset; // still tile (11,11), same supertile (2,2)
        assert_eq!(a, b);
        // A position in the next supertile over must differ.
        let c = set_ppu_addresses_in_mem(96, 64, 0x00, 0, 0, 0).level_screen_supertile_offset;
        assert_ne!(a, c);
    }
}
