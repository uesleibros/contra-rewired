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
//! ## Status: low format and high format (including percussion) both
//! ported
//!
//! A sound_code's first byte decides its format: `< 0x30` is "low"
//! (`read_low_sound_cmd` - used by sound slots #$01/#$04/#$05, i.e. pulse
//! 2 and noise-channel sound *effects*), `>= 0x30` is "high" (used by
//! slots #$00-#$03, Contra's actual music) - and slot #$03 specifically
//! uses a third sub-grammar, percussion (`parse_percussion_cmd`), instead
//! of high format's regular note commands.
//!
//! An earlier pass here shipped only the low format, flagging high/
//! percussion as blocked on "commands whose byte length depends on
//! runtime state". Reading further showed that's only true in a
//! *behavioral* sense, not a *data-layout* sense: `sound_cmd_routine_01`'s
//! branch on `UNKNOWN_SOUND_01` (a RAM variable, confirmed via
//! `src/ram.asm`/`$013c` - genuinely written by unrelated game logic, not
//! ROM-constant) decides whether the interpreter *finishes reading* this
//! command's bytes or aborts early into re-initializing the channel - but
//! the bytes themselves are still compiled into the ROM either way, at a
//! fixed length, regardless of which playthrough-dependent path a given
//! run takes. For **extraction** (this module's job: how many bytes does
//! this blob occupy in ROM), the right length is always the "full
//! consumption" path's, since that's what the data was actually laid out
//! as - the early-abort path is an interpreter behavior, not a shorter
//! blob. The one place that distinction doesn't apply cleanly (a length
//! genuinely varying with the *data itself*, e.g. `sound_cmd_routine_02`'s
//! vibrato command reading one fewer trailing byte when the byte right
//! after it happens to be `$FF`) is handled by peeking that byte's own
//! value, the same pattern `walk_low`'s `0xF8` escape already used -
//! that's real, ROM-fixed variability, not runtime ambiguity, and is
//! fully resolvable from the ROM alone.
//!
//! One more real, data-determined (not runtime) fork:
//! `sound_cmd_routine_01` reads a different number of trailing bytes for
//! the triangle channel (slot #$02) than for pulse channels - which slot
//! a given sound_code plays on is fixed by its own `sound_table_00`
//! entry, so [`walk_high`] takes that as a parameter rather than guessing.
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
//!
//! Also caught a real bug before shipping: an early implementation
//! reused low format's `$FD`/`$FE`/`$FF` dispatch condition (byte
//! literally `>= 0xFD`) for high/percussion format too - wrong, since
//! those two dispatch to the same control-command handling for *any*
//! byte `>= 0xF0` instead (confirmed by re-reading `@regular_sound_cmd`'s
//! bits-4-5 table dispatch and `parse_percussion_cmd`'s own `and #$f0`
//! check). Left as-is, this would have silently misparsed any `0xF0`-
//! `0xFC` byte in high/percussion data as a 1-byte unit instead of a real
//! 3-byte child-jump, corrupting every length after it. See
//! [`control_command_body`]'s doc comment.
//!
//! ## The high/percussion grammar, and a real finding while verifying it
//!
//! Ported from `read_high_sound_cmd`/`parse_percussion_cmd`/
//! `sound_cmd_routine_00`-`03`. For non-percussion slots: `< 0xC0` is a
//! `simple_sound_cmd` (1 byte - high nibble = note pitch, low nibble =
//! length multiplier); `>= 0xC0` dispatches by bits 4-5 to
//! `sound_cmd_routine_00` (mute, 1 byte), `_01` (length + channel
//! config - 2 bytes on the triangle channel, 4 otherwise), or `_02`
//! (several sub-cases by low nibble: 1 byte for period-rotate/flip-
//! flatten/unknown, 2 for pitch-adjust, 2 or 3 for vibrato depending on
//! whether the byte right after it is `$FF`). For percussion (slot
//! #$03): `0xD0`-`0xDF` is a 1-byte delay command that loops back for
//! another; anything else is a 1-byte percussion trigger. Verified by
//! hand against 3 more real sounds before/while writing this - `sound_26`
//! (22 bytes, no children, unambiguous), `sound_29` (10 bytes, the
//! percussion sub-format specifically) - both exact - and `sound_2a`
//! (843 bytes), which surfaced something worth documenting rather than
//! just fixing: its one `$FE` repeat command targets an address 29 bytes
//! into its *own* already-scanned range (not the very start, unlike
//! `sound_08`'s low-format self-reference) - walking that "child" from
//! scratch retraces the parent's own tail and lands on the exact same
//! terminator (`29 + 814 == 843`), which is genuinely correct, self-
//! consistent behavior, not a bug: a repeat command replaying a middle
//! section of the same phrase back to the phrase's own end.
//! [`tests::high_format_child_can_overlap_the_end_of_its_own_parent`]
//! encodes a small synthetic version of this as a regression case (the
//! real `sound_2a` bytes are long enough that hand-verifying a
//! contrived, minimal example was more useful than pasting all 843
//! bytes into a test).

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

/// `sound_cmd_routine_03`'s control-command body, shared identically by
/// all three sub-formats - **but each dispatches to it under a different
/// condition**, which is why this takes an already-isolated low nibble
/// rather than inspecting the byte itself:
///
/// - Low format (`read_low_sound_cmd`) only reaches here for a byte
///   literally `>= 0xFD` (i.e. `cmp #$fd; bcc interpret_sound_byte`) -
///   whose low nibbles happen to be `0xD`/`0xE`/`0xF` for `0xFD`/`0xFE`/
///   `0xFF` respectively, whereas a byte in `0xF0`-`0xFC` is a *regular
///   note* command in this format (see [`walk_low`]'s `0xF8` case), not
///   a control command at all.
/// - High/percussion format (`read_high_sound_cmd`'s `@regular_sound_cmd`
///   bits-4-5 dispatch, and `parse_percussion_cmd`'s own `and #$f0; cmp
///   #$f0` check) reach here for **any** byte `>= 0xF0` - a much wider
///   range than low format's - passing its low nibble directly. Getting
///   this distinction wrong (reusing low format's narrower `>= 0xFD`
///   trigger for high/percussion format too) was a real bug caught before
///   shipping: it would silently misparse any `0xF0`-`0xFC` byte in
///   high/percussion data as a 1-byte unit instead of the real 3-byte
///   child-jump command, corrupting every subsequent length in that blob.
///
/// Low nibble `0x0`-`0xD` is a child-jump (3 bytes: opcode + address);
/// `0xE` is a repeat (4 bytes: opcode + count + address); `0xF` is end
/// (1 byte).
fn control_command_body(prg_rom: &[u8], pos: usize, low_nibble: u8, children: &mut Vec<usize>) -> (usize, bool) {
    match low_nibble {
        0xf => (1, true),
        0xe => {
            let mem_addr = u16::from_le_bytes([prg_rom[pos + 2], prg_rom[pos + 3]]);
            children.push(bank1_prg_offset(mem_addr));
            (4, false)
        }
        _ => {
            let mem_addr = u16::from_le_bytes([prg_rom[pos + 1], prg_rom[pos + 2]]);
            children.push(bank1_prg_offset(mem_addr));
            (3, false)
        }
    }
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
            let (len, end) = control_command_body(prg_rom, pos, b & 0x0f, &mut children);
            pos += len;
            if end {
                break;
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

/// Which of the 4 "music" channel slots a high/percussion-format
/// sound_code plays on - fixed per sound_code by its own `sound_table_00`
/// entry (bits 0-2 of that entry's first byte), needed because
/// `sound_cmd_routine_01`'s byte length genuinely differs for the
/// triangle channel (see this module's doc comment).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Pulse1,
    Pulse2,
    Triangle,
    Noise,
}

/// Walks one high-format sound_code (Contra's music) or, when `slot ==
/// Slot::Noise` (sound slot #$03), its percussion sub-format instead -
/// ported from `read_high_sound_cmd`/`parse_percussion_cmd`/
/// `sound_cmd_routine_00`-`03` (`src/bank1.asm`). See this module's doc
/// comment for the reasoning behind each command's byte length.
pub fn walk_high(prg_rom: &[u8], start_offset: usize, slot: Slot) -> SoundCodeExtent {
    let mut pos = start_offset;
    let mut children = Vec::new();

    loop {
        let b = prg_rom[pos];

        // Both high-regular (`@regular_sound_cmd`'s bits-4-5 dispatch)
        // and percussion (`parse_percussion_cmd`'s own `and #$f0; cmp
        // #$f0` check) send *any* byte `>= 0xF0` to the shared control
        // body - a wider trigger than low format's `>= 0xFD` (see
        // `control_command_body`'s doc comment).
        if b >= 0xf0 {
            let (len, end) = control_command_body(prg_rom, pos, b & 0x0f, &mut children);
            pos += len;
            if end {
                break;
            }
            continue;
        }

        if slot == Slot::Noise {
            // Percussion sub-format (parse_percussion_cmd): every
            // remaining byte (high nibble != 0xf, already excluded above)
            // is exactly 1 byte, whether it's a 0xd0-0xdf delay command
            // (low nibble = length multiplier, loops back for another
            // command) or a percussion trigger (low nibble = delay
            // multiplier, high nibble = percussion_tbl index).
            pos += 1;
            continue;
        }

        if b < 0xc0 {
            // simple_sound_cmd: one byte, high nibble = note_period_tbl
            // offset, low nibble = length multiplier.
            pos += 1;
            continue;
        }

        // regular_sound_cmd, 0xc0-0xef (0xf0+ already handled above):
        // bits 4-5 select sound_cmd_routine_00-02.
        match b & 0x30 {
            0x00 => {
                // sound_cmd_routine_00: mute channel. One byte.
                pos += 1;
            }
            0x10 => {
                // sound_cmd_routine_01: length multiplier + channel
                // config. Triangle reads one config byte (2 bytes total);
                // pulse/noise read config-low/vol-env/unknown (4 bytes
                // total) - see this module's doc comment for why the
                // byte layout doesn't depend on UNKNOWN_SOUND_01 despite
                // the real routine branching on it.
                pos += if slot == Slot::Triangle { 2 } else { 4 };
            }
            0x20 => {
                // sound_cmd_routine_02: several sub-cases by low nibble.
                let low_nibble = b & 0x0f;
                pos += match low_nibble {
                    0x0..=0x4 | 0x8 => 1,
                    0xb => {
                        // Vibrato: delay byte, then (unless delay == 0xFF) an amount byte.
                        let delay = prg_rom[pos + 1];
                        if delay == 0xff {
                            2
                        } else {
                            3
                        }
                    }
                    0xc => 2, // pitch adjustment.
                    _ => 1,   // unknown/ignored low nibble.
                };
            }
            _ => unreachable!("b < 0xf0 here, so b & 0x30 can only be 0x00, 0x10, or 0x20"),
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

/// Walks `walk_high` recursively through every child blob too, same
/// dedup/queue behavior as [`walk_low_recursive`] - children of a
/// high/percussion-format blob are always the same slot's format, since
/// a shared phrase reused via `$FD`/`$FE` stays on the same channel.
pub fn walk_high_recursive(prg_rom: &[u8], start_offset: usize, slot: Slot) -> Vec<(usize, SoundCodeExtent)> {
    let mut visited = std::collections::BTreeSet::new();
    let mut queue = vec![start_offset];
    let mut result = Vec::new();

    while let Some(offset) = queue.pop() {
        if !visited.insert(offset) {
            continue;
        }
        let extent = walk_high(prg_rom, offset, slot);
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

    #[test]
    fn sound_29_percussion_matches_its_real_10_byte_length() {
        // sound_29's real ROM bytes (slot #$03/noise, so percussion
        // sub-format): delay(0xd9), 3 regular triggers, delay(0xda), 3
        // more triggers, then the real 0xff end.
        let data = [0xd9, 0x33, 0x03, 0x03, 0x33, 0xda, 0x55, 0x11, 0x17, 0xff];
        assert_eq!(data.len(), 10);
        let extent = walk_high(&data, 0, Slot::Noise);
        assert_eq!(extent.length, 10);
        assert!(extent.child_prg_offsets.is_empty());
    }

    #[test]
    fn sound_26_high_format_matches_its_real_22_byte_length() {
        // sound_26 (TITLE, pulse 1) - no $fd/$fe children, so its total
        // length is an unambiguous, direct real-ROM check (unlike
        // sound_2a's overlapping-child case, this one has nothing to
        // reconcile).
        #[rustfmt::skip]
        let data = [
            0xe8, 0xd9, 0xf8, 0x87, 0x0a, 0xe3, 0x03, 0xd9, 0xf7, 0x84, 0x06, 0xe2,
            0x93, 0x73, 0x23, 0xda, 0xf7, 0x84, 0x02, 0xe2, 0x4f, 0xff,
        ];
        assert_eq!(data.len(), 22);
        let extent = walk_high(&data, 0, Slot::Pulse1);
        assert_eq!(extent.length, 22);
    }

    #[test]
    fn high_format_child_can_overlap_the_end_of_its_own_parent() {
        // Models sound_2a's real structure at a tiny scale: a repeat
        // command partway through jumps *backward* into the parent's own
        // already-scanned range (not all the way to its start, unlike
        // sound_08's low-format self-reference) - walking that "child"
        // from scratch retraces the parent's own tail and lands on the
        // exact same terminator, which is the real, verified behavior,
        // not a bug (see this module's doc comment / NATIVE_PORT.md for
        // the full account of confirming this against real sound_2a data).
        let mut prg_rom = vec![0u8; 0x20000];
        let start = bank1_prg_offset(0x9000);
        let bytes = [
            0xe2, // routine02 low_nibble=2: period rotate, 1 byte
            0x40, // simple note (this is where the repeat will jump back to)
            0xfe, 0x01, 0x01, 0x90, // repeat 1x -> mem $9001 (the note above)
            0xff, // real end
        ];
        prg_rom[start..start + bytes.len()].copy_from_slice(&bytes);

        let all = walk_high_recursive(&prg_rom, start, Slot::Pulse1);
        let (_, top) = all.iter().find(|(off, _)| *off == start).unwrap();
        assert_eq!(top.length, 7);
        let child_offset = bank1_prg_offset(0x9001);
        assert_eq!(top.child_prg_offsets, vec![child_offset]);
        let (_, child) = all.iter().find(|(off, _)| *off == child_offset).unwrap();
        // The child, walked fresh from mem $9001 (relative offset 1),
        // retraces [simple note, $fe repeat, $ff] = exactly the parent's
        // own tail from that point: 6 bytes (1 + 4 + 1), landing on the
        // same terminator - `1 (own start offset) + 6 = 7` matches the
        // parent's own total length exactly, the same self-consistency
        // check that validated the real sound_2a case.
        assert_eq!(child.length, 6);
    }
}
