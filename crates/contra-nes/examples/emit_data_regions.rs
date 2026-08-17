//! Debug-only tool (not part of the library or any shipped binary):
//! emits `nesrecomp` `[[data_region]]` TOML entries for every known
//! Contra data structure this project has already extracted and
//! verified - sound_code (bank 1), graphics/level/enemy-spawn/palette
//! data (banks 0/2/3/4/5/6/7) - straight from `contra_native`'s
//! already-verified walkers, rather than hand-transcribing addresses.
//! This is exploratory tooling for evaluating github.com/mstan/nesrecomp
//! (a static 6502->C recompiler) as a possible complement to this
//! project's own hand-port effort - see docs/NATIVE_PORT.md for context.
//! Not part of the native port itself.
//!
//! ```text
//! cargo run -p contra-nes --release --example emit_data_regions -- <rom> > regions.toml
//! ```

use contra_native::world::level::{level_header, screen_prg_offset, LocationType, ScrollingType};
use contra_native::audio::sound_code::Slot;
use std::collections::BTreeSet;

const SOUND_TABLE_00_PRG_OFFSET: usize = 0x48E8;
const SOUND_TABLE_00_ENTRIES: usize = 0x5e;
const PULSE_VOLUME_PTR_TBL_PRG_OFFSET: usize = 0x4001;
const PULSE_VOLUME_PTR_TBL_LEN: usize = 108;
const PERCUSSION_TBL_PRG_OFFSET: usize = 0x42CD;
const PERCUSSION_TBL_LEN: usize = 8;
const NOTE_PERIOD_TBL_PRG_OFFSET: usize = 0x46D5;
const NOTE_PERIOD_TBL_LEN: usize = 48;
const GRAPHIC_DATA_ENTRY_COUNT: usize = 27; // documented graphic_data_XX count

const LEVEL_COUNT: usize = 8;

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

/// Physical PRG-ROM offset -> (bank, cpu_addr), matching this 8-bank
/// (128KB) UxROM ROM's real layout: every bank but the last (7, the
/// fixed bank) is switchable and mapped at $8000-$BFFF when active; bank
/// 7 is always mapped at $C000-$FFFF.
fn prg_offset_to_bank_addr(prg_offset: usize) -> (u8, u16) {
    let bank = (prg_offset / 0x4000) as u8;
    let base = if bank == 7 { 0xC000 } else { 0x8000 };
    let addr = (base + (prg_offset % 0x4000)) as u16;
    (bank, addr)
}

fn emit(regions: &mut Vec<(u8, u16, u16)>, prg_offset: usize, len: usize) {
    if len == 0 {
        return;
    }
    let (bank, start) = prg_offset_to_bank_addr(prg_offset);
    let (end_bank, end) = prg_offset_to_bank_addr(prg_offset + len);
    assert_eq!(bank, end_bank, "region at prg={prg_offset:#x} len={len} crosses a bank boundary");
    regions.push((bank, start, end));
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: emit_data_regions <rom>");
    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    let prg = &rom.prg_rom;
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, prg.len() / 1024, rom.md5_hex);

    let mut regions: Vec<(u8, u16, u16)> = Vec::new();

    // ── Sound code (bank 1) ────────────────────────────────────────────
    emit(&mut regions, SOUND_TABLE_00_PRG_OFFSET, SOUND_TABLE_00_ENTRIES * 3);
    emit(&mut regions, PULSE_VOLUME_PTR_TBL_PRG_OFFSET, PULSE_VOLUME_PTR_TBL_LEN);
    emit(&mut regions, PERCUSSION_TBL_PRG_OFFSET, PERCUSSION_TBL_LEN);
    emit(&mut regions, NOTE_PERIOD_TBL_PRG_OFFSET, NOTE_PERIOD_TBL_LEN);

    let mut visited_sound_blobs: BTreeSet<usize> = BTreeSet::new();
    for entry in 0..SOUND_TABLE_00_ENTRIES {
        let base = SOUND_TABLE_00_PRG_OFFSET + entry * 3;
        let byte0 = prg[base];
        let mem_addr = u16::from_le_bytes([prg[base + 1], prg[base + 2]]);
        let prg_offset = 0x4000 + (mem_addr as usize & 0x3FFF);
        let first_byte = prg[prg_offset];
        let all = if first_byte < 0x30 {
            contra_native::audio::sound_code::walk_low_recursive(prg, prg_offset)
        } else {
            contra_native::audio::sound_code::walk_high_recursive(prg, prg_offset, slot_for(byte0))
        };
        for (off, extent) in &all {
            if visited_sound_blobs.insert(*off) {
                emit(&mut regions, *off, extent.length);
            }
        }
    }

    // ── Graphics tables (bank 7, fixed) ────────────────────────────────
    emit(&mut regions, contra_native::world::graphics::LEVEL_GRAPHIC_DATA_TBL_PRG_OFFSET, LEVEL_COUNT * 2 + 5 * 2); // levels + boss/intro/ending contexts, rounded up
    emit(&mut regions, contra_native::world::graphics::GRAPHIC_DATA_PTR_TBL_PRG_OFFSET, GRAPHIC_DATA_ENTRY_COUNT * 3);

    // ── Palette tables ──────────────────────────────────────────────────
    emit(&mut regions, contra_native::world::palette::GAME_PALETTES_PRG_OFFSET, contra_native::world::palette::GAME_PALETTES_LEN);
    emit(&mut regions, contra_native::world::palette::LEVEL_HEADERS_PRG_OFFSET, LEVEL_COUNT * contra_native::world::palette::LEVEL_HEADER_LEN);

    // ── Per-level data: graphics blobs, level screens, enemy spawns ────
    let mut visited_graphics_blobs: BTreeSet<usize> = BTreeSet::new();
    for level_index in 0..LEVEL_COUNT {
        let header = level_header(prg, level_index);
        let expected_len = match header.scrolling_type {
            ScrollingType::Horizontal => 56,
            ScrollingType::Vertical => 64,
        };

        emit(&mut regions, header.screen_ptr_table_prg_offset, header.screen_count * 2);

        for screen_index in 0..header.screen_count {
            let screen_off = screen_prg_offset(prg, &header, screen_index);
            let len = contra_native::world::supertile::decompress_screen_len(&prg[screen_off..], expected_len);
            emit(&mut regions, screen_off, len);
        }

        for entry in contra_native::world::graphics::level_graphic_data_entries(prg, level_index) {
            if visited_graphics_blobs.insert(entry.prg_offset) {
                let len = contra_native::world::graphics::decompressed_len(&prg[entry.prg_offset..], entry.flip);
                emit(&mut regions, entry.prg_offset, len);
            }
        }

        if header.location_type == LocationType::Outdoor {
            let ptr_tbl = contra_native::enemy::enemy_spawn::level_enemy_screen_ptr_tbl_prg_offset(prg, level_index);
            emit(&mut regions, ptr_tbl, header.screen_count * 2);
            for screen_index in 0..header.screen_count {
                let screen_off = contra_native::enemy::enemy_spawn::enemy_screen_prg_offset(prg, ptr_tbl, screen_index);
                let len = contra_native::enemy::enemy_spawn::decompress_outdoor_enemy_screen_len(&prg[screen_off..]);
                emit(&mut regions, screen_off, len);
            }
        }
    }
    emit(&mut regions, contra_native::enemy::enemy_spawn::LEVEL_ENEMY_SCREEN_PTR_PTR_TBL_PRG_OFFSET, LEVEL_COUNT * 2);

    // Merge overlapping/adjacent same-bank regions so the emitted TOML is
    // compact and doesn't exceed nesrecomp's GAME_CFG_MAX_DATA_REGIONS.
    regions.sort();
    let mut merged: Vec<(u8, u16, u16)> = Vec::new();
    for (bank, start, end) in regions {
        if let Some(last) = merged.last_mut() {
            if last.0 == bank && start <= last.2 {
                if end > last.2 {
                    last.2 = end;
                }
                continue;
            }
        }
        merged.push((bank, start, end));
    }

    eprintln!(
        "{} sound blobs + {} graphics blobs -> {} merged data_region entries",
        visited_sound_blobs.len(),
        visited_graphics_blobs.len(),
        merged.len()
    );
    for (bank, start, end) in &merged {
        println!("[[data_region]]");
        println!("bank = {bank}");
        println!("start = {start:#06x}");
        println!("end = {end:#06x}");
        println!();
    }
}
