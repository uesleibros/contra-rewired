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

/// Decompresses one graphic-data blob, starting at `data[0]`, per the
/// format above. Returns every `(ppu_addr, byte)` write the real routine
/// would have issued, as a list of contiguous runs.
///
/// Panics if `data` runs out before a terminating `0xFF` is reached - a
/// malformed or mis-sliced input is a bug in the caller (wrong start
/// offset), not a recoverable runtime condition here.
pub fn decompress(data: &[u8]) -> Vec<PpuWrite> {
    let mut pos = 0usize;
    let read_byte = |data: &[u8], pos: &mut usize| -> u8 {
        let b = data[*pos];
        *pos += 1;
        b
    };

    let mut segments = Vec::new();
    'segments: loop {
        let lo = read_byte(data, &mut pos);
        let hi = read_byte(data, &mut pos);
        let ppu_addr = u16::from_le_bytes([lo, hi]);
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
                let value = read_byte(data, &mut pos);
                for _ in 0..count {
                    bytes.push(value);
                }
            } else {
                let count = b & 0x7f;
                for _ in 0..count {
                    bytes.push(read_byte(data, &mut pos));
                }
            }
        }

        if !bytes.is_empty() {
            segments.push(PpuWrite { ppu_addr, bytes });
        }
    }

    segments
}

/// Decompresses `data` and applies every write that lands in the pattern
/// tables (`$0000-$1FFF`, i.e. CHR) onto `chr`, exactly like the real PPU
/// would as `write_graphic_data_to_ppu` streams `PPUDATA` writes with
/// auto-increment 1. Writes outside that range (nametable/attribute data,
/// used by a handful of blobs like `graphic_data_00`) are ignored - CHR
/// extraction isn't the right place for those.
pub fn apply_chr_writes(data: &[u8], chr: &mut [u8; 0x2000]) {
    for write in decompress(data) {
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
        let segments = decompress(&compressed);
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
        let segments = decompress(&compressed);
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
        apply_chr_writes(&compressed, &mut chr);
        assert!(chr.iter().all(|&b| b == 0), "out-of-range write must not touch CHR buffer");

        let compressed_in_range = [0x80, 0x06, 0x02, 0x11, 0xff]; // PPU addr $0680, RLE 0x11 x2
        apply_chr_writes(&compressed_in_range, &mut chr);
        assert_eq!(chr[0x0680], 0x11);
        assert_eq!(chr[0x0681], 0x11);
        assert_eq!(chr[0x0682], 0x00);
    }
}
