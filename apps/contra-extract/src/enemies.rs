//! Wires `contra_native::enemy_spawn` into a real extraction command:
//! writes each outdoor level's hard-coded enemy placements to a plain
//! text file, straight from PRG-ROM, no emulation. Indoor/base levels
//! (2 and 4) use a different, not-yet-decoded format (see
//! `contra_native::enemy_spawn`'s doc comment) and are skipped, noted as
//! such in the output rather than silently producing nothing.

/// Writes `<out_dir>/level{1..8}_enemies.txt` for every outdoor level.
/// Returns `(files_written, levels_skipped)`.
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<(usize, usize)> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = 0usize;
    let mut skipped = 0usize;

    for level_index in 0..8 {
        let header = contra_native::level::level_header(prg_rom, level_index);
        let path = out_dir.join(format!("level{}_enemies.txt", level_index + 1));

        if header.location_type != contra_native::level::LocationType::Outdoor {
            std::fs::write(&path, "indoor/base level - enemy placement format not decoded yet, see contra_native::enemy_spawn's doc comment\n")?;
            skipped += 1;
            continue;
        }

        let ptr_tbl = contra_native::enemy_spawn::level_enemy_screen_ptr_tbl_prg_offset(prg_rom, level_index);
        let mut text = String::new();
        let mut total = 0usize;
        for screen_index in 1..header.screen_count {
            let offset = contra_native::enemy_spawn::enemy_screen_prg_offset(prg_rom, ptr_tbl, screen_index);
            let spawns = contra_native::enemy_spawn::decompress_outdoor_enemy_screen(&prg_rom[offset..]);
            if spawns.is_empty() {
                continue;
            }
            text.push_str(&format!("screen {screen_index}:\n"));
            for spawn in &spawns {
                text.push_str(&format!("  type={:#04x} x={:#04x} y={:#04x} attribute={:#05b}\n", spawn.enemy_type, spawn.x, spawn.y, spawn.attribute));
            }
            total += spawns.len();
        }
        std::fs::write(&path, format!("{total} hard-coded enemies across {} screens\n\n{text}", header.screen_count))?;
        log::info!("level {}: {total} hard-coded enemies", level_index + 1);
        written += 1;
    }

    Ok((written, skipped))
}
