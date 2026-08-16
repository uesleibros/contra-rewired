//! Native Rust port of Contra's graphics RLE decompressor
//! (`write_graphic_data_to_ppu`, bank 7 `$1c9a1`/mem `$c9a1`), the routine
//! the real game uses to unpack "graphic data" blobs from PRG-ROM into PPU
//! memory (almost always CHR pattern-table tiles, occasionally nametable
//! or attribute data - see `docs/Graphics Documentation.md` in
//! `vermiceli/nes-contra-us` for the full format writeup this is ported
//! from).
//!
//! This is the first piece of the "asset extraction" workstream described
//! in `docs/NATIVE_PORT.md`: unlike `collision`/`player_physics` (which
//! replace 6502 code that runs every frame), this module's job is to run
//! **once**, offline, against the player's own ROM, to produce plain
//! image files - not to be hooked into live emulation at all.
//!
//! ## Format
//!
//! A graphic-data blob is a sequence of one or more segments. Each segment
//! is a 2-byte big-endian-on-the-wire... no - little-endian PPU address
//! (low byte first, matching a real `PPUADDR` write pair as the game
//! issues them) followed by a stream of command bytes:
//!
//! - `0xFF` - end of the whole blob.
//! - `0x7F` - end of this segment; the next 2 bytes are a new PPU address.
//! - `0x00..=0x7E` (bit 7 clear) - RLE run: the *next* byte is written to
//!   PPU data that many times (the count itself is never written).
//! - `0x80..=0xFE` (bit 7 set, not `0xFF`/`0x7F`) - literal run: bits 0-6
//!   give a count, and that many following bytes are written verbatim.
//!
//! Note the source documentation's own pseudocode has a transcription bug
//! (it writes the RLE *count* byte itself instead of reading a separate
//! payload byte to repeat) - this implementation instead follows the
//! prose description and its worked example
//! (`06 00 85 0e 1f 07 04 c0 ff` decompressing to
//! `00 00 00 00 00 00 0e 1f 07 04 c0`), which are unambiguous and which
//! this module's test reproduces exactly.

/// One contiguous run of bytes destined for a specific PPU address, in the
/// order the real hardware would have received them (PPU auto-increments
/// by 1 per `PPUDATA` write, so `bytes[i]` lands at `ppu_addr + i`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PpuWrite {
    pub ppu_addr: u16,
    pub bytes: Vec<u8>,
}

/// Reverses the bit order of a byte (`horizontal_flip_graphic_byte`,
/// bank 7 `$c9a1`-area: "swap bit 0 with 7, bit 1 with 6, bit 2 with 5,
/// and bit 3 with 4") - since each byte of a pattern-table bitplane
/// encodes 8 horizontal pixels MSB-first, reversing bit order mirrors
/// that row of pixels left-right.
fn reverse_bits(b: u8) -> u8 {
    let b = (b & 0xF0) >> 4 | (b & 0x0F) << 4;
    let b = (b & 0xCC) >> 2 | (b & 0x33) << 2;
    (b & 0xAA) >> 1 | (b & 0x55) << 1
}

/// Decompresses one graphic-data blob, starting at `data[0]`, per the
/// format above. Returns every `(ppu_addr, byte)` write the real routine
/// would have issued, as a list of contiguous runs.
///
/// `flip`: some blobs (`graphic_data_10`, a horizontally-mirrored reuse of
/// `graphic_data_0a`'s art, is the concrete example this was ported
/// against) don't store their own pixel data at all - they store a *new*
/// target PPU address, reuse another blob's exact compressed bytes
/// verbatim, and have every data byte bit-reversed as it's written
/// (`write_graphic_data_to_ppu`, bank 7 `$c9a1`, confirmed against the
/// real `horizontal_flip_graphic_byte` routine it calls). When `flip` is
/// set, this skips 2 extra bytes right after the (still normally-read)
/// target address - the reused blob's own embedded PPU address header,
/// irrelevant here since the real target was already read - and
/// bit-reverses every subsequent data byte before returning it. Per the
/// source's own caveat, a flipped blob is always single-segment (no
/// `0x7F` mid-stream), so the skip only ever needs to happen once, right
/// after the first header.
///
/// Panics if `data` runs out before a terminating `0xFF` is reached - a
/// malformed or mis-sliced input is a bug in the caller (wrong start
/// offset), not a recoverable runtime condition here.
pub fn decompress(data: &[u8], flip: bool) -> Vec<PpuWrite> {
    let mut pos = 0usize;
    let read_byte = |data: &[u8], pos: &mut usize| -> u8 {
        let b = data[*pos];
        *pos += 1;
        b
    };
    let read_data_byte = |data: &[u8], pos: &mut usize| -> u8 {
        let b = read_byte(data, pos);
        if flip {
            reverse_bits(b)
        } else {
            b
        }
    };

    let mut segments = Vec::new();
    let mut first_segment = true;
    'segments: loop {
        let lo = read_byte(data, &mut pos);
        let hi = read_byte(data, &mut pos);
        let ppu_addr = u16::from_le_bytes([lo, hi]);
        if flip && first_segment {
            pos += 2; // skip the reused blob's own (now-irrelevant) PPU address header
        }
        first_segment = false;
        let mut bytes = Vec::new();

        loop {
            let b = read_byte(data, &mut pos);
            if b == 0xff {
                if !bytes.is_empty() {
                    segments.push(PpuWrite { ppu_addr, bytes });
                }
                break 'segments;
            }
            if b == 0x7f {
                break;
            }
            if b < 0x7f {
                let count = b;
                let value = read_data_byte(data, &mut pos);
                for _ in 0..count {
                    bytes.push(value);
                }
            } else {
                let count = b & 0x7f;
                for _ in 0..count {
                    bytes.push(read_data_byte(data, &mut pos));
                }
            }
        }

        if !bytes.is_empty() {
            segments.push(PpuWrite { ppu_addr, bytes });
        }
    }

    segments
}

/// `level_graphic_data_tbl` (bank 7, fixed bank, CPU `$c8e3`): 13 2-byte
/// pointers (levels 1-8, level-2-boss, level-4-boss, 2 intro contexts,
/// ending), each pointing to that context's own 0xFF-terminated list of
/// `graphic_data_ptr_tbl` indexes. `level_index` is 0-based (level 1 = 0),
/// matching the convention `palette::level_palette_group_indexes` already
/// uses. PRG-ROM offset = `7*0x4000 + (0xc8e3-0xc000)`.
pub const LEVEL_GRAPHIC_DATA_TBL_PRG_OFFSET: usize = 0x1C8E3;

/// `graphic_data_ptr_tbl` (bank 7, fixed bank, CPU `$c950`): one 3-byte
/// entry per `graphic_data_XX` index - 2-byte mem address, then 1-byte
/// bank number (unlike `level_graphic_data_tbl`'s own list, which always
/// lives in the fixed bank, individual blobs are scattered across banks
/// 2/4/5/6/7, so each entry must say which). PRG-ROM offset =
/// `7*0x4000 + (0xc950-0xc000)`.
pub const GRAPHIC_DATA_PTR_TBL_PRG_OFFSET: usize = 0x1C950;

/// Reads level `level_index`'s (0-based) own list of `graphic_data_ptr_tbl`
/// indexes directly from PRG-ROM - the exact list `load_level_graphic_data`
/// walks at real level-load time, terminated by `0xFF`.
pub fn level_graphic_data_indexes(prg_rom: &[u8], level_index: usize) -> Vec<u8> {
    let ptr_offset = LEVEL_GRAPHIC_DATA_TBL_PRG_OFFSET + level_index * 2;
    let mem_addr = u16::from_le_bytes([prg_rom[ptr_offset], prg_rom[ptr_offset + 1]]);
    // This list itself always lives in the fixed bank (7), same as the
    // table pointing to it.
    let mut pos = 7 * 0x4000 + (mem_addr as usize & 0x3FFF);
    let mut indexes = Vec::new();
    loop {
        let b = prg_rom[pos];
        pos += 1;
        if b == 0xff {
            break;
        }
        indexes.push(b);
    }
    indexes
}

/// One resolved `graphic_data_ptr_tbl` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphicDataEntry {
    pub prg_offset: usize,
    /// Whether this blob's data must be read with [`decompress`]'s
    /// `flip: true` (bit 7 of the entry's third byte) - see
    /// `graphic_data_prg_offset`'s doc comment.
    pub flip: bool,
}

/// Resolves one `graphic_data_XX` index to its exact PRG-ROM offset and
/// flip flag, by reading `graphic_data_ptr_tbl`'s 3-byte entry for it:
/// 2-byte mem address, then a byte packing bit 7 = flip-horizontally and
/// bits 0-2 = bank number (confirmed against the real consuming code,
/// `write_graphic_data_to_ppu`: `and #$80` for the flip check, `and #$07`
/// for the bank - *not* the full low 7 bits as a first attempt at this
/// assumed, which reads a real flipped entry's flip bit as part of a
/// bogus 132-or-so "bank number" and panics on the resulting
/// out-of-range PRG offset).
pub fn graphic_data_prg_offset(prg_rom: &[u8], index: u8) -> GraphicDataEntry {
    let entry_offset = GRAPHIC_DATA_PTR_TBL_PRG_OFFSET + index as usize * 3;
    let mem_addr = u16::from_le_bytes([prg_rom[entry_offset], prg_rom[entry_offset + 1]]);
    let flags = prg_rom[entry_offset + 2];
    let bank = (flags & 0x07) as usize;
    GraphicDataEntry { prg_offset: bank * 0x4000 + (mem_addr as usize & 0x3FFF), flip: flags & 0x80 != 0 }
}

/// The full, real, per-level graphics pipeline: level `level_index`'s own
/// list of graphic-data blobs, each resolved to a PRG-ROM offset and flip
/// flag - no hardcoded per-level tables, just the same two lookups
/// `load_level_graphic_data` performs at real load time.
pub fn level_graphic_data_entries(prg_rom: &[u8], level_index: usize) -> Vec<GraphicDataEntry> {
    level_graphic_data_indexes(prg_rom, level_index).into_iter().map(|index| graphic_data_prg_offset(prg_rom, index)).collect()
}

/// Decompresses `data` and applies every write that lands in the pattern
/// tables (`$0000-$1FFF`, i.e. CHR) onto `chr`, exactly like the real PPU
/// would as `write_graphic_data_to_ppu` streams `PPUDATA` writes with
/// auto-increment 1. Writes outside that range (nametable/attribute data,
/// used by a handful of blobs like `graphic_data_00`) are ignored - CHR
/// extraction isn't the right place for those.
pub fn apply_chr_writes(data: &[u8], chr: &mut [u8; 0x2000], flip: bool) {
    for write in decompress(data, flip) {
        for (i, byte) in write.bytes.iter().enumerate() {
            let addr = write.ppu_addr.wrapping_add(i as u16);
            if (addr as usize) < chr.len() {
                chr[addr as usize] = *byte;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_worked_example_round_trips_exactly() {
        // From `docs/Graphics Documentation.md`'s "Contra Compression"
        // section, prefixed here with a PPU address header since real
        // blobs always start with one.
        let compressed = [
            0x00, 0x00, // PPU addr $0000
            0x06, 0x00, // RLE: write 0x00 six times
            0x85, 0x0e, 0x1f, 0x07, 0x04, 0xc0, // literal run of 5 bytes
            0xff, // end
        ];
        let segments = decompress(&compressed, false);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].ppu_addr, 0x0000);
        assert_eq!(segments[0].bytes, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x1f, 0x07, 0x04, 0xc0]);
    }

    #[test]
    fn a_0x7f_command_switches_to_a_new_ppu_address_mid_blob() {
        let compressed = [
            0x00, 0x00, // PPU addr $0000
            0x02, 0xAA, // RLE: write 0xAA twice -> [0xAA, 0xAA] at $0000
            0x7f, // switch address
            0x00, 0x10, // PPU addr $1000
            0x81, 0xBB, // literal run of 1 byte -> [0xBB] at $1000
            0xff,
        ];
        let segments = decompress(&compressed, false);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], PpuWrite { ppu_addr: 0x0000, bytes: vec![0xAA, 0xAA] });
        assert_eq!(segments[1], PpuWrite { ppu_addr: 0x1000, bytes: vec![0xBB] });
    }

    #[test]
    fn apply_chr_writes_places_bytes_at_the_right_offsets_and_ignores_out_of_range() {
        let compressed = [
            0x1f, 0xfe, // PPU addr $fe1f (out of CHR range - should be ignored)
            0x81, 0x42, 0xff,
        ];
        let mut chr = [0u8; 0x2000];
        apply_chr_writes(&compressed, &mut chr, false);
        assert!(chr.iter().all(|&b| b == 0), "out-of-range write must not touch CHR buffer");

        let compressed_in_range = [0x80, 0x06, 0x02, 0x11, 0xff]; // PPU addr $0680, RLE 0x11 x2
        apply_chr_writes(&compressed_in_range, &mut chr, false);
        assert_eq!(chr[0x0680], 0x11);
        assert_eq!(chr[0x0681], 0x11);
        assert_eq!(chr[0x0682], 0x00);
    }

    #[test]
    fn flip_skips_the_reused_blobs_own_header_and_bit_reverses_data_bytes() {
        // A "flipped" blob per `write_graphic_data_to_ppu`: 2 bytes of
        // real target address, then 2 bytes of the *reused* blob's own
        // (irrelevant, skipped) address, then that reused blob's actual
        // command stream, bit-reversed as read.
        let compressed = [
            0x00, 0x16, // real target PPU addr $1600
            0x00, 0x11, // reused blob's own header ($1100) - skipped
            0x81, 0b0001_0011, // literal run of 1 byte, to be bit-reversed
            0xff,
        ];
        let segments = decompress(&compressed, true);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].ppu_addr, 0x1600);
        assert_eq!(segments[0].bytes, vec![0b1100_1000]);

        // Sanity-check reverse_bits itself against a known-unambiguous case.
        assert_eq!(reverse_bits(0b0000_0001), 0b1000_0000);
        assert_eq!(reverse_bits(0b1100_0000), 0b0000_0011);
        assert_eq!(reverse_bits(0b1010_1010), 0b0101_0101);
    }

    #[test]
    fn level_graphic_data_lookup_matches_the_real_level_1_list_and_offsets() {
        // Synthetic ROM laid out just enough to exercise the two-table
        // walk: level_graphic_data_tbl -> level 1's index list -> per-index
        // graphic_data_ptr_tbl (mem, bank) entries. Values mirror the real
        // ROM's own level_1_graphic_data (`03,13,19,1a,14,16,05,ff`) and a
        // couple of its real graphic_data_ptr_tbl entries (graphic_data_03
        // is bank 4 mem $8001; graphic_data_05 is bank 5 mem $8001) - the
        // same facts `extract_level.rs`'s hardcoded constants encode,
        // confirming this general lookup reproduces them from first
        // principles rather than by coincidence.
        let mut rom = vec![0u8; 0x20000];
        let list_mem_addr: u16 = 0xC900; // arbitrary, within fixed-bank range
        rom[LEVEL_GRAPHIC_DATA_TBL_PRG_OFFSET..LEVEL_GRAPHIC_DATA_TBL_PRG_OFFSET + 2].copy_from_slice(&list_mem_addr.to_le_bytes());
        let list_prg_offset = 7 * 0x4000 + (list_mem_addr as usize & 0x3FFF);
        rom[list_prg_offset..list_prg_offset + 8].copy_from_slice(&[0x03, 0x13, 0x19, 0x1a, 0x14, 0x16, 0x05, 0xff]);

        let set_ptr_entry = |rom: &mut [u8], index: u8, mem_addr: u16, bank: u8| {
            let offset = GRAPHIC_DATA_PTR_TBL_PRG_OFFSET + index as usize * 3;
            rom[offset..offset + 2].copy_from_slice(&mem_addr.to_le_bytes());
            rom[offset + 2] = bank;
        };
        set_ptr_entry(&mut rom, 0x03, 0x8001, 4);
        set_ptr_entry(&mut rom, 0x05, 0x8001, 5);

        assert_eq!(level_graphic_data_indexes(&rom, 0), vec![0x03, 0x13, 0x19, 0x1a, 0x14, 0x16, 0x05]);
        assert_eq!(graphic_data_prg_offset(&rom, 0x03), GraphicDataEntry { prg_offset: 0x10001, flip: false });
        assert_eq!(graphic_data_prg_offset(&rom, 0x05), GraphicDataEntry { prg_offset: 0x14001, flip: false });
    }

    #[test]
    fn graphic_data_ptr_tbl_bank_byte_is_only_the_low_3_bits_flip_is_bit_7() {
        // graphic_data_10's real entry: bank nibble $04 with the flip bit
        // set, stored as the single byte $84 - confirmed directly from
        // the real ROM (this is what a first attempt at this lookup,
        // using the byte's low 7 bits as "the bank", misread as bank 132
        // and panicked on).
        let mut rom = vec![0u8; 0x20000];
        let offset = GRAPHIC_DATA_PTR_TBL_PRG_OFFSET + 0x10 * 3;
        rom[offset..offset + 2].copy_from_slice(&0xA003u16.to_le_bytes());
        rom[offset + 2] = 0x84;
        let entry = graphic_data_prg_offset(&rom, 0x10);
        assert!(entry.flip);
        assert_eq!(entry.prg_offset, 4 * 0x4000 + 0x2003);
    }
}
