//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::sound_code`: walks every entry in
//! `sound_table_00` (bank 1, `$88e8`) straight from PRG-ROM, classifies
//! each as low/high format by its data's own first byte (per
//! `docs/Sound Documentation.md`), and for every entry, runs the
//! matching ported walker and prints its computed length. Cross-check
//! the output against `nes-contra-us/src/assets/audio_data/*.bin` file
//! sizes by name (via `docs/rom-symbols.txt`'s address->name mapping) to
//! confirm the walker is correct for every sound, not just the few
//! hand-verified in this module's own unit tests.
//!
//! ```text
//! cargo run -p contra-nes --release --example extract_sounds -- <rom>
//! ```

use contra_native::sound_code::Slot;

const SOUND_TABLE_00_PRG_OFFSET: usize = 0x48E8;
const SOUND_TABLE_00_ENTRIES: usize = 0x5e;

fn slot_for(byte0: u8) -> Slot {
    match byte0 & 0x07 {
        0 => Slot::Pulse1,
        1 => Slot::Pulse2,
        2 => Slot::Triangle,
        3 => Slot::Noise,
        4 => Slot::Pulse1,
        5 => Slot::Noise,
        other => panic!("sound_table_00 entry byte0 has an unexpected slot value {other:#03x}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: extract_sounds <rom>");
    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    let mut low_count = 0usize;
    let mut high_count = 0usize;
    for entry in 0..SOUND_TABLE_00_ENTRIES {
        let base = SOUND_TABLE_00_PRG_OFFSET + entry * 3;
        let byte0 = rom.prg_rom[base];
        let mem_addr = u16::from_le_bytes([rom.prg_rom[base + 1], rom.prg_rom[base + 2]]);
        let prg_offset = 0x4000 + (mem_addr as usize & 0x3FFF);
        let first_byte = rom.prg_rom[prg_offset];

        if first_byte < 0x30 {
            low_count += 1;
            let all = contra_native::sound_code::walk_low_recursive(&rom.prg_rom, prg_offset);
            let (_, top_extent) = all.iter().find(|(off, _)| *off == prg_offset).unwrap();
            println!(
                "entry {entry:#04x}: LOW  mem={mem_addr:#06x} prg={prg_offset:#06x} top_len={} children={} total_blobs={}",
                top_extent.length,
                top_extent.child_prg_offsets.len(),
                all.len()
            );
        } else {
            high_count += 1;
            let slot = slot_for(byte0);
            let all = contra_native::sound_code::walk_high_recursive(&rom.prg_rom, prg_offset, slot);
            let (_, top_extent) = all.iter().find(|(off, _)| *off == prg_offset).unwrap();
            println!(
                "entry {entry:#04x}: HIGH mem={mem_addr:#06x} prg={prg_offset:#06x} slot={slot:?} top_len={} children={} total_blobs={}",
                top_extent.length,
                top_extent.child_prg_offsets.len(),
                all.len()
            );
            if std::env::var("DEBUG_BLOBS").is_ok() {
                for (off, extent) in &all {
                    let mem = if *off >= 0x4000 { (*off - 0x4000) as u32 + 0x8000 } else { *off as u32 };
                    println!("    blob prg={off:#06x} mem={mem:#06x} len={}", extent.length);
                }
            }
        }
    }
    println!("\n{low_count} low-format, {high_count} high-format entries out of {SOUND_TABLE_00_ENTRIES} total");
}
