//! Native port of Contra's DPCM (delta pulse-code modulation) sample
//! playback data - the 2A03 APU's 5th audio channel, used by 3 of
//! Contra's sound effects (hi-hat/snare percussion; `docs/Sound
//! Documentation.md` in `vermiceli/nes-contra-us`, "Delta Pulse Coded
//! Modulation (DPCM) Samples" section).
//!
//! Unlike `graphics`/`palette`/`supertile`, this isn't Contra-specific
//! logic being reverse-engineered - the DMC sample-address/length
//! register encoding and the delta-decode algorithm are standard 2A03
//! hardware behavior (documented on the NESdev wiki, not derived from
//! this game). What *is* Contra-specific, and what this module reads, is
//! `dpcm_sample_data_tbl` (bank 1, CPU `$88db`) - the small table that
//! says which 2 of the ROM's PRG bytes are actually DPCM sample data, and
//! at what playback volume each of the 3 sounds that use them starts.
//!
//! ## What this doesn't cover
//!
//! Contra's "music" (the background scores, the intro/ending themes) and
//! most of its non-percussion sound effects are a completely different,
//! much larger thing: a custom bytecode sequencer driving the pulse/
//! triangle/noise channels in real time (see `docs/Sound Documentation.md`'s
//! "sound_code Parsing" section - `low`/`high` sound commands, percussion
//! commands, a `sound_cmd_ptr_tbl` dispatch table, note-period tables,
//! and so on). That isn't a one-time-decode "asset" the way DPCM samples,
//! CHR tiles, or level layouts are - reproducing it means porting a real
//! playback *engine*, closer in kind to `collision`/`player_physics` than
//! to this module. Not started; tracked in docs/NATIVE_PORT.md.
//!
//! ## Verification
//!
//! `contra-nes`'s APU doesn't emulate the DMC channel yet (see
//! `crates/contra-nes/src/apu.rs`'s doc comment), so there's no live
//! playback to diff against the way graphics/palette/level extraction
//! were verified. Instead: the DMC address/length formulas were checked
//! against `docs/Sound Documentation.md`'s own worked examples (both
//! samples' documented addresses/lengths reproduce exactly), and the
//! resulting byte ranges were diffed (`cmp`) against
//! `nes-contra-us/src/assets/audio_data/dpcm_sample_{00,01}.bin` - the
//! disassembly's own separately-shipped copies of this same data,
//! independently confirming the offset math - both came back identical.

/// `dpcm_sample_data_tbl` (bank 1, CPU `$88db`): 3 4-byte entries (DMC
/// config byte, initial output level, DMC sample-address byte, DMC
/// sample-length byte) - one per sound that plays a DPCM sample
/// (`sound_5a`/`_5b`/`_5c`). Entries 0 and 2 both reference the same
/// underlying sample (`dpcm_sample_00`) at different playback volumes;
/// entry 1 references `dpcm_sample_01`. PRG-ROM offset =
/// `1*0x4000 + (0x88db-0x8000)`.
pub const DPCM_SAMPLE_DATA_TBL_PRG_OFFSET: usize = 0x48DB;
const DPCM_ENTRY_LEN: usize = 4;

/// One resolved `dpcm_sample_data_tbl` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DpcmSampleRef {
    pub prg_offset: usize,
    pub length: usize,
    /// The DMC channel's starting 7-bit output level (`$4011`) when this
    /// sound begins playing - not part of the sample data itself, but
    /// needed to reproduce the same audio (`decode_dpcm`'s second
    /// argument).
    pub initial_level: u8,
}

/// Reads and resolves one `dpcm_sample_data_tbl` entry (index 0-2).
/// Address/length decoding matches real 2A03 DMC registers exactly
/// (`$4012`/`$4013`, standard NES hardware, not Contra-specific): sample
/// address = `$C000 + addr_byte*64`; sample length = `len_byte*16 + 1`.
pub fn dpcm_table_entry(prg_rom: &[u8], entry_index: usize) -> DpcmSampleRef {
    let base = DPCM_SAMPLE_DATA_TBL_PRG_OFFSET + entry_index * DPCM_ENTRY_LEN;
    let initial_level = prg_rom[base + 1];
    let addr_byte = prg_rom[base + 2] as u32;
    let len_byte = prg_rom[base + 3] as usize;
    let mem_addr = 0xC000u32 + addr_byte * 64;
    DpcmSampleRef {
        prg_offset: 7 * 0x4000 + (mem_addr as usize & 0x3FFF),
        length: len_byte * 16 + 1,
        initial_level,
    }
}

/// Decodes raw DPCM-encoded bytes into a sequence of 7-bit (0-127) DAC
/// output levels, one per bit, starting from `initial_level`. Standard
/// 2A03 DMC hardware behavior: bits are consumed LSB-first from each
/// byte; a `1` bit increases the output level by 2 (clamped at 127), a
/// `0` bit decreases it by 2 (clamped at 0).
pub fn decode_dpcm(data: &[u8], initial_level: u8) -> Vec<u8> {
    let mut level = initial_level.min(127);
    let mut out = Vec::with_capacity(data.len() * 8);
    for &byte in data {
        for bit_index in 0..8 {
            let bit = (byte >> bit_index) & 1;
            level = if bit == 1 { level.saturating_add(2).min(127) } else { level.saturating_sub(2) };
            out.push(level);
        }
    }
    out
}

/// The DMC channel's fixed NTSC playback rate for the fastest (and only,
/// in Contra's case - `dpcm_sample_data_tbl`'s config byte is `$0f` for
/// all 3 entries) rate-table entry: ~33144 Hz, matching
/// `docs/Sound Documentation.md`'s "33.1 kHz" note.
pub const DPCM_SAMPLE_RATE_HZ: u32 = 33144;

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 0x20000];
        let base = DPCM_SAMPLE_DATA_TBL_PRG_OFFSET;
        // Real values from `src/bank1.asm`'s `dpcm_sample_data_tbl`.
        rom[base..base + 4].copy_from_slice(&[0x0f, 0x2f, 0xf0, 0x05]); // sound_5a
        rom[base + 4..base + 8].copy_from_slice(&[0x0f, 0x75, 0xf3, 0x25]); // sound_5b
        rom[base + 8..base + 12].copy_from_slice(&[0x0f, 0x00, 0xf0, 0x05]); // sound_5c
        rom
    }

    #[test]
    fn resolves_sound_5a_to_dpcm_sample_00_documented_values() {
        let rom = fake_rom();
        let entry = dpcm_table_entry(&rom, 0);
        assert_eq!(entry.initial_level, 0x2f);
        assert_eq!(entry.length, 81); // documented "#$51 bytes"
        assert_eq!(entry.prg_offset, 0x1FC00); // documented "$fc00"
    }

    #[test]
    fn resolves_sound_5b_to_dpcm_sample_01_documented_values() {
        let rom = fake_rom();
        let entry = dpcm_table_entry(&rom, 1);
        assert_eq!(entry.initial_level, 0x75);
        assert_eq!(entry.length, 593); // documented "#$251 bytes"
        assert_eq!(entry.prg_offset, 0x1FCC0); // documented "$fcc0"
    }

    #[test]
    fn sound_5c_reuses_sample_00_at_a_different_initial_level() {
        let rom = fake_rom();
        let entry_5a = dpcm_table_entry(&rom, 0);
        let entry_5c = dpcm_table_entry(&rom, 2);
        assert_eq!(entry_5a.prg_offset, entry_5c.prg_offset);
        assert_eq!(entry_5a.length, entry_5c.length);
        assert_eq!(entry_5c.initial_level, 0x00);
        assert_ne!(entry_5a.initial_level, entry_5c.initial_level);
    }

    #[test]
    fn decode_dpcm_reads_bits_lsb_first_and_clamps_at_both_ends() {
        // 0b0000_0001: bit0 (LSB, read first) = 1 -> level rises; bits
        // 1-7 = 0 -> level falls each step.
        let decoded = decode_dpcm(&[0b0000_0001], 10);
        assert_eq!(decoded[0], 12); // bit0=1: 10+2
        assert_eq!(decoded[1], 10); // bit1=0: 12-2
        assert_eq!(decoded.len(), 8);

        // Clamps at 127 (rising) and 0 (falling), never wrapping.
        let saturated_high = decode_dpcm(&[0xFF], 126);
        assert_eq!(saturated_high, vec![127; 8]);
        let saturated_low = decode_dpcm(&[0x00], 1);
        assert_eq!(saturated_low, vec![0; 8]);
    }
}
