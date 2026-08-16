//! Native port of Contra's sound-code bytecode format - the custom
//! command language driving music and sound effects (`docs/Sound
//! Documentation.md` in `vermiceli/nes-contra-us`, "sound_code Parsing"
//! section). Unlike `graphics`/`palette`/`supertile`/`audio`, decoding
//! this isn't a one-time "produce a final asset" job - a sound_code
//! isn't a picture or a waveform, it's a small *program* the real
//! `handle_sound_code`/`read_low_sound_cmd`/`read_high_sound_cmd`/
//! `parse_percussion_cmd` routines (`src/bank1.asm`) interpret one frame
//! at a time, writing APU registers directly as it goes. This module's
//! job is narrower and comes first: **know exactly how many bytes each
//! sound_code occupies**, so every single byte of sound data in the ROM
//! can be located and extracted, before a playback engine (much larger,
//! separate work - see this module's "What's not here yet" note) exists
//! to actually make sound from it.
//!
//! ## Status: low-format (sound effects) only
//!
//! A sound_code's first byte decides its format: `< 0x30` is "low"
//! (`read_low_sound_cmd` - used by sound slots #$01/#$04/#$05, i.e. pulse
//! 2 and noise-channel sound *effects*), `>= 0x30` is "high" (used by
//! slots #$00-#$03, Contra's actual music). **Only the low format is
//! ported here.** The high format (`read_high_sound_cmd`) and percussion
//! sub-format (`parse_percussion_cmd`, used by slot #$03) have several
//! commands whose byte length depends on runtime state in ways the low
//! format's don't (e.g. `sound_cmd_routine_02`'s vibrato command reads a
//! variable number of trailing bytes depending on whether the first one
//! read is `$ff`) - porting those correctly needs more careful, separate
//! verification work than this first pass covers, and shipping a wrong
//! byte-length walker for them would silently corrupt extracted data
//! rather than just being incomplete. Tracked in docs/NATIVE_PORT.md.
//!
//! ## The low-format grammar, and how it was verified
//!
//! Ported from `interpret_sound_byte`/`read_low_sound_cmd` (`src/bank1.asm`).
//! Each "unit" is one command; a sound_code is a sequence of units followed
//! by a terminator:
//!
//! - `0xFD` - "child" reference: 3 bytes total (`0xFD`, addr lo, addr hi).
//!   Execution jumps to the 2-byte address, plays that as its own
//!   sub-sequence, then returns here - so *this* blob's own bytes are
//!   just these 3, but the referenced address is a separate blob to walk
//!   too (see [`SoundCodeExtent::child_prg_offsets`]).
//! - `0xFE` - repeat: 4 bytes total (`0xFE`, count, addr lo, addr hi) -
//!   same idea as `0xFD` but the child plays `count` times.
//! - `0xFF` - end.
//! - `0x20-0x2E` - set length multiplier + config high nibble: 2 bytes.
//! - `0x2F` - same, but an explicit length byte follows: 3 bytes.
//! - `0x10` exactly - sweep: 2 bytes (`0x10`, sweep value).
//! - `0x11-0x1F` - flatten-note flag (undocumented as ever actually used
//!   in Contra, per the source's own comment - still valid grammar): 1 byte.
//! - anything else (`0x00-0x0F`, `0x30-0xFC`) - a note: the byte's own
//!   high nibble is volume and low nibble is the note period's high bits
//!   (genuinely reused three ways - see the worked traces below), plus
//!   one more byte for the period's low bits: 2 bytes: **except** when
//!   the byte is exactly `0xF8`, an "escape" that decouples volume from
//!   period by reading one extra byte: 3 bytes.
//!
//! Verified by hand against two real sound effects' raw ROM bytes before
//! being coded, each cross-checked against its exact real length: `sound_03`
//! (17 bytes: `21 30 40 f0 00 00 00 00 21 f0 f8 20 0a f8 10 0b ff`) and
//! `sound_05` (55 bytes, starts `21 70 10 83 f3 8c f2 f6 ...`, ends `...
//! 32 ee ff`) - both documented lengths in `dpcm_sample_data_tbl`'s
//! neighbor, `sound_table_00`'s own entries are unrelated to these; these
//! two lengths come from `nes-contra-us/src/assets/audio_data/sound_{03,
//! 05}.bin`'s file sizes, the disassembly's own already-split copies of
//! this same data (the same kind of independent cross-check the DPCM
//! samples got). [`tests`] encodes both as regression cases.

/// One low-format sound_code's byte extent, starting from a given
/// PRG-ROM offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundCodeExtent {
    /// Total bytes in this blob's own linear stream, from its start
    /// through its terminating `0xFF` (inclusive) - or through an
    /// `0xFD`/`0xFE` command's own bytes, if this blob doesn't end in a
    /// real `0xFF` within the scanned region (shouldn't happen for a
    /// well-formed sound_code, but this walker doesn't assume it can't).
    pub length: usize,
    /// PRG-ROM offsets of every distinct child blob referenced by an
    /// `0xFD`/`0xFE` command in this blob, in the order encountered.
    /// Each is a separate sound_code of its own and needs its own
    /// [`walk_low`] call to find its extent.
    pub child_prg_offsets: Vec<usize>,
}

/// Converts a bank-1-relative CPU memory address (as read from an
/// `0xFD`/`0xFE` command's address bytes) to a PRG-ROM offset. All of
/// Contra's sound_code data lives in bank 1.
fn bank1_prg_offset(mem_addr: u16) -> usize {
    0x4000 + (mem_addr as usize & 0x3FFF)
}

/// Walks one low-format sound_code starting at `prg_rom[start_offset]`,
/// per this module's doc comment's grammar. Panics if the data runs out
/// before a terminator - same "malformed input is a caller bug" stance
/// as `graphics::decompress`.
pub fn walk_low(prg_rom: &[u8], start_offset: usize) -> SoundCodeExtent {
    let mut pos = start_offset;
    let mut children = Vec::new();

    loop {
        let b = prg_rom[pos];

        if b >= 0xfd {
            match b {
                0xff => {
                    pos += 1;
                    break;
                }
                0xfd => {
                    let mem_addr = u16::from_le_bytes([prg_rom[pos + 1], prg_rom[pos + 2]]);
                    children.push(bank1_prg_offset(mem_addr));
                    pos += 3;
                }
                0xfe => {
                    let mem_addr = u16::from_le_bytes([prg_rom[pos + 2], prg_rom[pos + 3]]);
                    children.push(bank1_prg_offset(mem_addr));
                    pos += 4;
                }
                _ => unreachable!("b >= 0xfd and matched none of 0xff/0xfd/0xfe"),
            }
            continue;
        }

        if b >= 0x20 && b <= 0x2f {
            // Case 1: length multiplier + config high.
            pos += if b == 0x2f { 3 } else { 2 };
        } else if b == 0x10 {
            // Case 2: sweep.
            pos += 2;
        } else if b >= 0x11 && b <= 0x1f {
            // Case 3: flatten-note flag.
            pos += 1;
        } else {
            // Case 4: note. The command byte itself doubles as
            // volume(high nibble)/period-high(low nibble), plus one more
            // byte for period-low - except the 0xF8 escape, which reads
            // a separate volume/period-high byte instead of reusing the
            // command byte, adding one extra byte.
            pos += if b == 0xf8 { 3 } else { 2 };
        }
    }

    SoundCodeExtent { length: pos - start_offset, child_prg_offsets: children }
}

/// Walks `walk_low` recursively through every child blob too, returning
/// `(top_level_prg_offset, extent)` for the whole family - the top-level
/// sound_code plus every `0xFD`/`0xFE`-referenced child, transitively.
/// Blobs already visited (shared children referenced more than once)
/// aren't walked or returned twice.
pub fn walk_low_recursive(prg_rom: &[u8], start_offset: usize) -> Vec<(usize, SoundCodeExtent)> {
    let mut visited = std::collections::BTreeSet::new();
    let mut queue = vec![start_offset];
    let mut result = Vec::new();

    while let Some(offset) = queue.pop() {
        if !visited.insert(offset) {
            continue;
        }
        let extent = walk_low(prg_rom, offset);
        queue.extend(extent.child_prg_offsets.iter().copied());
        result.push((offset, extent));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_03_matches_its_real_17_byte_length() {
        let data = [0x21, 0x30, 0x40, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x21, 0xf0, 0xf8, 0x20, 0x0a, 0xf8, 0x10, 0x0b, 0xff];
        assert_eq!(data.len(), 17);
        let extent = walk_low(&data, 0);
        assert_eq!(extent.length, 17);
        assert!(extent.child_prg_offsets.is_empty());
    }

    #[test]
    fn sound_05_matches_its_real_55_byte_length() {
        #[rustfmt::skip]
        let data = [
            0x21, 0x70, 0x10, 0x83, 0xf3, 0x8c, 0xf2, 0xf6, 0xf3, 0x15, 0x73, 0x77, 0xd3, 0x8c, 0x00, 0x00,
            0xd3, 0x15, 0x00, 0x00, 0xd3, 0x8c, 0xb3, 0x45, 0x33, 0x11, 0xb3, 0x50, 0x73, 0x33, 0xb4, 0x00,
            0x00, 0x00, 0xb2, 0xdd, 0x00, 0x00, 0x00, 0x00, 0xa3, 0x50, 0x00, 0x00, 0x73, 0x22, 0x00, 0x00,
            0x53, 0x50, 0x00, 0x00, 0x32, 0xee, 0xff,
        ];
        assert_eq!(data.len(), 55);
        let extent = walk_low(&data, 0);
        assert_eq!(extent.length, 55);
    }

    #[test]
    fn fd_command_is_3_bytes_and_registers_a_child() {
        let data = [0xfd, 0x00, 0x90, 0xff]; // 0xfd -> mem $9000, then end
        let extent = walk_low(&data, 0);
        assert_eq!(extent.length, 4);
        assert_eq!(extent.child_prg_offsets, vec![bank1_prg_offset(0x9000)]);
    }

    #[test]
    fn fe_command_is_4_bytes_and_registers_a_child() {
        let data = [0xfe, 0x03, 0x00, 0x90, 0xff]; // repeat 3x at mem $9000, then end
        let extent = walk_low(&data, 0);
        assert_eq!(extent.length, 5);
        assert_eq!(extent.child_prg_offsets, vec![bank1_prg_offset(0x9000)]);
    }

    #[test]
    fn sound_08_self_referential_repeat_matches_its_real_35_byte_length() {
        // sound_08's real ROM bytes (mem $8a90): a 6-byte phrase, then
        // `$fe,$02,addr($8a90)` - a repeat-2x command whose target is
        // sound_08's *own* start address, looping the phrase once before
        // falling through past the $fe's own 4 bytes into a further
        // 25-byte tail, ending in the real `0xff`. This is exactly why
        // `nes-contra-us`'s own `sound_08.bin` (8 bytes) + `sound_08_
        // part_00.bin` (25 bytes) = 33, not 35: its source uses `.addr
        // sound_08` (symbolic self-reference) at that point rather than
        // including those 2 raw address bytes in either extracted file -
        // a difference in how the *disassembly's own build* happens to
        // chunk its files, not a discrepancy in the real ROM data. This
        // test encodes the full, real 35-byte sequence directly so the
        // self-reference (and the byte-accurate total) stays a locked-in
        // regression case.
        #[rustfmt::skip]
        let bytes = [
            0x21, 0x30, 0xc0, 0x0e, 0xc0, 0x0f, 0xfe, 0x02, 0x90, 0x8a, // phrase (6) + $fe repeat-2x -> mem $8a90 (itself)
            0x24, 0x30, 0xa0, 0x0f, 0x90, 0x0e, 0x80, 0x0f, 0x70, 0x0f, 0x60, 0x0f, 0x50, 0x0f, 0x40, 0x0f,
            0x30, 0x0f, 0xf8, 0x20, 0x0f, 0xf8, 0x10, 0x0f, 0xff,
        ];
        assert_eq!(bytes.len(), 35);
        // Placed at the real bank-1 offset its own $fe target resolves
        // to (mem $8a90), so the self-reference genuinely points back at
        // byte 0 of this same blob, exactly like the real ROM.
        let start = bank1_prg_offset(0x8a90);
        let mut data = vec![0u8; 0x20000];
        data[start..start + bytes.len()].copy_from_slice(&bytes);

        let all = walk_low_recursive(&data, start);
        // Self-referential: the only "child" is this same start offset,
        // already visited, so the walk finds exactly one blob.
        assert_eq!(all.len(), 1);
        assert_eq!(all[0], (start, SoundCodeExtent { length: 35, child_prg_offsets: vec![start] }));
    }

    #[test]
    fn recursive_walk_visits_children_without_duplicates() {
        // Top level: note (2 bytes) then $fd -> child at prg offset of mem $9000.
        // Child (at mem $9000 = prg 0x5000): single note then end.
        let mut prg_rom = vec![0u8; 0x20000];
        prg_rom[0..5].copy_from_slice(&[0x40, 0x00, 0xfd, 0x00, 0x90]);
        // no top-level 0xff - the $fd is the last thing, but walk_low
        // requires eventually hitting a real terminator, so give the
        // top level its own 0xff after the $fd would (in reality) return -
        // model it explicitly here instead.
        prg_rom[5] = 0xff;
        let child_offset = bank1_prg_offset(0x9000);
        prg_rom[child_offset..child_offset + 3].copy_from_slice(&[0x40, 0x00, 0xff]);

        let all = walk_low_recursive(&prg_rom, 0);
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|(off, _)| *off == 0));
        assert!(all.iter().any(|(off, _)| *off == child_offset));
    }
}
