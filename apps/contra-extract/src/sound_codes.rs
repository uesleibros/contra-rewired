//! Wires `contra_native::sound_code` into a real extraction command:
//! walks `sound_table_00` (bank 1, `$88e8`) straight from PRG-ROM,
//! extracts every low-format sound_code's raw bytecode (see
//! `crates/contra-native/src/sound_code.rs`'s doc comment for exactly
//! what "low-format" covers and why high-format/music isn't here yet),
//! and writes one file per *distinct* blob (shared/repeated blobs get
//! written once, not duplicated per sound that references them), plus a
//! plain-text index tying sound codes back to their files. No emulation
//! involved.

const SOUND_TABLE_00_PRG_OFFSET: usize = 0x48E8;
const SOUND_TABLE_00_ENTRIES: usize = 0x5e;

/// Extracts every low-format sound_code's raw bytes to `<out_dir>/blob_
/// XXXXXX.bin` (one per distinct PRG-ROM offset) plus `index.txt`.
/// Returns `(low_format_sounds, distinct_blobs_written, high_format_sounds_skipped)`.
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<(usize, usize, usize)> {
    std::fs::create_dir_all(out_dir)?;

    let mut low_count = 0usize;
    let mut high_count = 0usize;
    let mut blob_lengths: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    let mut index = String::new();

    for entry in 0..SOUND_TABLE_00_ENTRIES {
        let base = SOUND_TABLE_00_PRG_OFFSET + entry * 3;
        let mem_addr = u16::from_le_bytes([prg_rom[base + 1], prg_rom[base + 2]]);
        let prg_offset = 0x4000 + (mem_addr as usize & 0x3FFF);
        let first_byte = prg_rom[prg_offset];

        if first_byte >= 0x30 {
            high_count += 1;
            index.push_str(&format!("sound_{entry:02x}: HIGH format (music/BGM) - not decoded yet\n"));
            continue;
        }

        low_count += 1;
        let all = contra_native::sound_code::walk_low_recursive(prg_rom, prg_offset);
        for (offset, extent) in &all {
            blob_lengths.insert(*offset, extent.length);
        }
        let child_offsets: Vec<usize> = all.iter().filter(|(off, _)| *off != prg_offset).map(|(off, _)| *off).collect();
        index.push_str(&format!("sound_{entry:02x}: LOW format, top-level blob_{prg_offset:06x}.bin"));
        if child_offsets.is_empty() {
            index.push('\n');
        } else {
            let child_names: Vec<String> = child_offsets.iter().map(|off| format!("blob_{off:06x}.bin")).collect();
            index.push_str(&format!(", references {}\n", child_names.join(", ")));
        }
    }

    for (&offset, &length) in &blob_lengths {
        let bytes = &prg_rom[offset..offset + length];
        let path = out_dir.join(format!("blob_{offset:06x}.bin"));
        std::fs::write(&path, bytes)?;
    }
    std::fs::write(out_dir.join("index.txt"), &index)?;

    Ok((low_count, blob_lengths.len(), high_count))
}
