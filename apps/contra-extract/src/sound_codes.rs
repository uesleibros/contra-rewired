//! Wires `contra_native::sound_code` into a real extraction command:
//! walks `sound_table_00` (bank 1, `$88e8`) straight from PRG-ROM,
//! extracts every sound_code's raw bytecode - low format (sound
//! effects), high format (music), and percussion, all three (see
//! `crates/contra-native/src/sound_code.rs`'s doc comment for the
//! grammar and how each was verified) - and writes one file per
//! *distinct* blob (shared/repeated blobs get written once, not
//! duplicated per sound that references them, though blobs that only
//! partially overlap - a repeat command replaying the tail of its own
//! parent phrase, see the module doc comment's `sound_2a` account - are
//! written as separate files even though their bytes overlap on disk).
//! Plus a plain-text index tying sound codes back to their files. No
//! emulation involved.

use contra_native::audio::sound_code::Slot;

const SOUND_TABLE_00_PRG_OFFSET: usize = 0x48E8;
const SOUND_TABLE_00_ENTRIES: usize = 0x5e;

fn slot_for(byte0: u8) -> Slot {
    match byte0 & 0x07 {
        0 | 4 => Slot::Pulse1,
        1 => Slot::Pulse2,
        2 => Slot::Triangle,
        _ => Slot::Noise,
    }
}

/// Extracts every sound_code's raw bytes to `<out_dir>/blob_XXXXXX.bin`
/// (one per distinct PRG-ROM offset) plus `index.txt`. Returns
/// `(low_format_sounds, high_format_sounds, distinct_blobs_written)`.
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<(usize, usize, usize)> {
    std::fs::create_dir_all(out_dir)?;

    let mut low_count = 0usize;
    let mut high_count = 0usize;
    let mut blob_lengths: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
    let mut index = String::new();

    for entry in 0..SOUND_TABLE_00_ENTRIES {
        let base = SOUND_TABLE_00_PRG_OFFSET + entry * 3;
        let byte0 = prg_rom[base];
        let mem_addr = u16::from_le_bytes([prg_rom[base + 1], prg_rom[base + 2]]);
        let prg_offset = 0x4000 + (mem_addr as usize & 0x3FFF);
        let first_byte = prg_rom[prg_offset];

        let (format, all) = if first_byte < 0x30 {
            low_count += 1;
            ("LOW", contra_native::audio::sound_code::walk_low_recursive(prg_rom, prg_offset))
        } else {
            high_count += 1;
            let slot = slot_for(byte0);
            (if slot == Slot::Noise { "PERCUSSION" } else { "HIGH" }, contra_native::audio::sound_code::walk_high_recursive(prg_rom, prg_offset, slot))
        };

        for (offset, extent) in &all {
            blob_lengths.insert(*offset, extent.length);
        }
        let child_offsets: Vec<usize> = all.iter().filter(|(off, _)| *off != prg_offset).map(|(off, _)| *off).collect();
        index.push_str(&format!("sound_{entry:02x}: {format} format, top-level blob_{prg_offset:06x}.bin"));
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

    Ok((low_count, high_count, blob_lengths.len()))
}
