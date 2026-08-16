//! Wires `contra_native::audio` into a real extraction command: decodes
//! Contra's 2 DPCM samples straight from PRG-ROM and writes them as plain
//! 8-bit PCM WAV files - no emulation, no external audio crate, just the
//! DMC delta-decode algorithm and a minimal WAV header.

const SAMPLE_ENTRY_NAMES: [(usize, &str); 2] = [(0, "dpcm_sample_00"), (1, "dpcm_sample_01")];

/// Decodes both distinct DPCM samples (`dpcm_table_entry` indexes 0 and 1
/// - index 2 reuses index 0's sample data at a different volume, so
/// there's nothing new to extract there) to `<out_dir>/{name}.wav`.
/// Returns how many were written.
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<usize> {
    std::fs::create_dir_all(out_dir)?;
    for (index, name) in SAMPLE_ENTRY_NAMES {
        let entry = contra_native::audio::dpcm_table_entry(prg_rom, index);
        let raw = prg_rom.get(entry.prg_offset..entry.prg_offset + entry.length).ok_or_else(|| {
            anyhow::anyhow!("{name}: PRG range {:#06x}..{:#06x} is past the end of this ROM's PRG-ROM ({} bytes)", entry.prg_offset, entry.prg_offset + entry.length, prg_rom.len())
        })?;
        let samples = contra_native::audio::decode_dpcm(raw, entry.initial_level);
        // The DMC's real DAC output is a 7-bit level (0-127); standard
        // 8-bit PCM WAV is unsigned with silence at the *center* (128).
        // Scale by 2 (0-127 -> 0-254) so it fills the WAV format's range
        // and centers correctly, rather than only ever using the bottom
        // half of it - a presentation detail of this WAV export, not a
        // change to the decoded DAC levels themselves.
        let wav_samples: Vec<u8> = samples.iter().map(|&level| level * 2).collect();
        let path = out_dir.join(format!("{name}.wav"));
        write_wav_u8(&path, &wav_samples, contra_native::audio::DPCM_SAMPLE_RATE_HZ)?;
        log::info!("{name}: {} raw bytes -> {} PCM samples, initial level {:#04x}", raw.len(), samples.len(), entry.initial_level);
    }
    Ok(SAMPLE_ENTRY_NAMES.len())
}

/// Writes `samples` (already-decoded, unsigned 8-bit DAC levels 0-127 -
/// left as-is rather than rescaled, since that's the real output range
/// the 2A03's 7-bit DAC produces) as a minimal mono 8-bit PCM WAV file.
fn write_wav_u8(path: &std::path::Path, samples: &[u8], sample_rate_hz: u32) -> anyhow::Result<()> {
    let mut file = std::fs::File::create(path)?;
    use std::io::Write;

    let data_len = samples.len() as u32;
    let byte_rate = sample_rate_hz; // mono, 8-bit: byte_rate == sample_rate
    let block_align: u16 = 1;
    let bits_per_sample: u16 = 8;

    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_len).to_le_bytes())?;
    file.write_all(b"WAVE")?;

    file.write_all(b"fmt ")?;
    file.write_all(&16u32.to_le_bytes())?; // fmt chunk size
    file.write_all(&1u16.to_le_bytes())?; // PCM
    file.write_all(&1u16.to_le_bytes())?; // mono
    file.write_all(&sample_rate_hz.to_le_bytes())?;
    file.write_all(&byte_rate.to_le_bytes())?;
    file.write_all(&block_align.to_le_bytes())?;
    file.write_all(&bits_per_sample.to_le_bytes())?;

    file.write_all(b"data")?;
    file.write_all(&data_len.to_le_bytes())?;
    file.write_all(samples)?;

    Ok(())
}
