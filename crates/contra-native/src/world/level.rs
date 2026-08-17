//! Native port of Contra's level header format (`level_headers`, bank 2,
//! CPU `$b319` - see `docs/Level Headers.md` in `vermiceli/nes-contra-us`
//! for the field-by-field writeup this is ported from). Each of the 8
//! levels gets a fixed 32-byte header; this module reads the fields
//! needed to generalize `supertile`/`graphics`/`palette` extraction
//! beyond level 1's originally hand-verified offsets to any level.
//!
//! Field byte offsets below were confirmed two independent ways: counting
//! literal fields in `src/bank2.asm`'s `level_1_header`, and (for fields
//! with a documented RAM address) subtracting that address from
//! `LEVEL_LOCATION_TYPE`'s ($40, offset 0) - both agree for every field
//! used here.

use crate::world::palette::{LEVEL_HEADERS_PRG_OFFSET, LEVEL_HEADER_LEN};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollingType {
    Horizontal,
    Vertical,
}

/// `LEVEL_LOCATION_TYPE` (header offset 0). The static level header table
/// only ever stores `Outdoor`/`Indoor` - `$80`/`$ff` ("indoor boss") is a
/// runtime-only value the game writes into RAM when a boss screen starts,
/// never present in the ROM's own header data (`docs/Level Headers.md`:
/// "the value is never #$80 in the level headers table").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocationType {
    Outdoor,
    Indoor,
}

/// The subset of a level header this crate's extraction pipeline needs -
/// not a transcription of every documented field (see `docs/Level
/// Headers.md` for the rest, e.g. collision-code tile boundaries, already
/// covered by `contra_native::collision`'s own hand-verified constants).
#[derive(Debug, Clone, Copy)]
pub struct LevelHeader {
    pub location_type: LocationType,
    pub scrolling_type: ScrollingType,
    /// PRG-ROM offset of this level's `level_X_supertiles_screen_ptr_table`
    /// (always bank 2).
    pub screen_ptr_table_prg_offset: usize,
    /// PRG-ROM offset of this level's `level_X_supertile_data` (always
    /// bank 3) - one 16-byte entry per super-tile ID.
    pub supertile_data_prg_offset: usize,
    /// PRG-ROM offset of this level's `level_X_palette_data` (always bank
    /// 3) - one attribute byte per super-tile ID.
    pub palette_data_prg_offset: usize,
    /// Number of screens in this level (`LEVEL_STOP_SCROLL + 2` - see
    /// `docs/Level Headers.md`'s own "(+2)" note on that field; confirmed
    /// for level 1 by reading `level_1_supertiles_screen_ptr_table`'s raw
    /// pointer bytes and finding exactly this many resolve to real,
    /// distinct, rom-symbols.txt-labeled screens).
    pub screen_count: usize,
}

fn mem_addr_at(prg_rom: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([prg_rom[offset], prg_rom[offset + 1]])
}

/// Reads level `level_index`'s (0-based, level 1 = 0) header directly from
/// PRG-ROM.
pub fn level_header(prg_rom: &[u8], level_index: usize) -> LevelHeader {
    let base = LEVEL_HEADERS_PRG_OFFSET + level_index * LEVEL_HEADER_LEN;

    let location_type = if prg_rom[base] == 0 { LocationType::Outdoor } else { LocationType::Indoor };
    let scrolling_type = if prg_rom[base + 1] == 0 { ScrollingType::Horizontal } else { ScrollingType::Vertical };

    let screen_ptr_mem = mem_addr_at(prg_rom, base + 2);
    let supertile_data_mem = mem_addr_at(prg_rom, base + 4);
    let palette_data_mem = mem_addr_at(prg_rom, base + 6);
    let level_stop_scroll = prg_rom[base + 24];

    LevelHeader {
        location_type,
        scrolling_type,
        screen_ptr_table_prg_offset: 2 * 0x4000 + (screen_ptr_mem as usize & 0x3FFF),
        supertile_data_prg_offset: 3 * 0x4000 + (supertile_data_mem as usize & 0x3FFF),
        palette_data_prg_offset: 3 * 0x4000 + (palette_data_mem as usize & 0x3FFF),
        screen_count: level_stop_scroll as usize + 2,
    }
}

/// Reads screen `index`'s 2-byte pointer (a bank-2-relative mem address)
/// out of a level's screen pointer table, resolved to a PRG-ROM offset.
pub fn screen_prg_offset(prg_rom: &[u8], header: &LevelHeader, index: usize) -> usize {
    let entry_offset = header.screen_ptr_table_prg_offset + index * 2;
    let mem_addr = mem_addr_at(prg_rom, entry_offset);
    2 * 0x4000 + (mem_addr as usize & 0x3FFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_1_header_matches_the_known_real_values() {
        // Synthetic ROM laid out with level 1's real, already-confirmed
        // header field values (src/bank2.asm's level_1_header, and the
        // screen count independently confirmed via
        // level_1_supertiles_screen_ptr_table's raw pointer bytes).
        let mut rom = vec![0u8; 0x20000];
        let base = LEVEL_HEADERS_PRG_OFFSET;
        rom[base] = 0x00; // location type: outdoor
        rom[base + 1] = 0x00; // scrolling type: horizontal
        rom[base + 2..base + 4].copy_from_slice(&0x8001u16.to_le_bytes()); // level_1_supertiles_screen_ptr_table
        rom[base + 4..base + 6].copy_from_slice(&0x8001u16.to_le_bytes()); // level_1_supertile_data
        rom[base + 6..base + 8].copy_from_slice(&0x8671u16.to_le_bytes()); // level_1_palette_data
        rom[base + 24] = 0x0b; // LEVEL_STOP_SCROLL

        let header = level_header(&rom, 0);
        assert_eq!(header.location_type, LocationType::Outdoor);
        assert_eq!(header.scrolling_type, ScrollingType::Horizontal);
        assert_eq!(header.screen_ptr_table_prg_offset, 0x8001);
        assert_eq!(header.supertile_data_prg_offset, 0xC001);
        assert_eq!(header.palette_data_prg_offset, 0xC671);
        assert_eq!(header.screen_count, 13);
    }

    #[test]
    fn screen_prg_offset_resolves_a_pointer_table_entry() {
        let mut rom = vec![0u8; 0x20000];
        let header = LevelHeader { location_type: LocationType::Outdoor, scrolling_type: ScrollingType::Horizontal, screen_ptr_table_prg_offset: 0x8001, supertile_data_prg_offset: 0, palette_data_prg_offset: 0, screen_count: 13 };
        rom[0x8001..0x8003].copy_from_slice(&0x801Du16.to_le_bytes());
        assert_eq!(screen_prg_offset(&rom, &header, 0), 0x801D);
    }
}
