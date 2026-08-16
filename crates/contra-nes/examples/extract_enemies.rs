//! Debug-only verification tool (not part of the library or any shipped
//! binary) for `contra_native::enemy_spawn`: walks the real two-level
//! pointer indirection (`level_enemy_screen_ptr_ptr_tbl` ->
//! `level_X_enemy_screen_ptr_tbl` -> per-screen enemy list) straight from
//! PRG-ROM for level 1, and checks the result against the exact bytes
//! `docs/Enemy Routines.md`'s worked example describes (already confirmed
//! to match the real ROM verbatim via `cmp`/`od`) - this is the same
//! decoder, exercised through the *real* pointer walk instead of a
//! synthetic ROM, unlike this module's own unit tests.
//!
//! ```text
//! cargo run -p contra-nes --release --example extract_enemies -- <rom>
//! ```

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = args.get(1).expect("usage: extract_enemies <rom>");
    let rom = contra_assets::NesRom::load(rom_path).expect("failed to load ROM");
    eprintln!("mapper={} prg_kib={} md5={}", rom.mapper, rom.prg_rom.len() / 1024, rom.md5_hex);

    let header = contra_native::level::level_header(&rom.prg_rom, 0);
    let ptr_tbl = contra_native::enemy_spawn::level_enemy_screen_ptr_tbl_prg_offset(&rom.prg_rom, 0);
    println!("level 1: {} screens, enemy ptr table @ prg[{ptr_tbl:#06x}]", header.screen_count);

    // Screen 0 is skipped (always empty per the doc, and this level's own
    // ptr table has exactly `screen_count` entries - 0..screen_count, not
    // 0..=screen_count; going one past crashed into unrelated data on a
    // first attempt, since there's no terminator to stop it).
    let mut total = 0usize;
    for screen_index in 1..header.screen_count {
        let offset = contra_native::enemy_spawn::enemy_screen_prg_offset(&rom.prg_rom, ptr_tbl, screen_index);
        let spawns = contra_native::enemy_spawn::decompress_outdoor_enemy_screen(&rom.prg_rom[offset..]);
        println!("  screen {screen_index}: {} enemies @ prg[{offset:#06x}]", spawns.len());
        for spawn in &spawns {
            println!("    type={:#04x} x={:#04x} y={:#04x} attr={:#03b}", spawn.enemy_type, spawn.x, spawn.y, spawn.attribute);
        }
        total += spawns.len();
    }
    println!("total: {total} hard-coded enemies across level 1");

    // Cross-check: screen 9 must match docs/Enemy Routines.md's worked
    // example exactly (already independently confirmed to match the real
    // ROM's raw bytes via `od`/`cmp`).
    let screen_9_offset = contra_native::enemy_spawn::enemy_screen_prg_offset(&rom.prg_rom, ptr_tbl, 9);
    let screen_9 = contra_native::enemy_spawn::decompress_outdoor_enemy_screen(&rom.prg_rom[screen_9_offset..]);
    let expected = [
        contra_native::enemy_spawn::EnemySpawn { x: 0x10, y: 0x40, enemy_type: 0x03, attribute: 0b000 },
        contra_native::enemy_spawn::EnemySpawn { x: 0x10, y: 0xb0, enemy_type: 0x03, attribute: 0b100 },
        contra_native::enemy_spawn::EnemySpawn { x: 0xe0, y: 0x80, enemy_type: 0x07, attribute: 0b001 },
    ];
    if screen_9 == expected {
        println!("MATCH: screen 9's real-ROM decode via the full pointer-table walk matches the documented worked example exactly.");
    } else {
        println!("MISMATCH: screen 9 decoded as {screen_9:?}, expected {expected:?}");
    }
}
