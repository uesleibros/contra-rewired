//! Wires `contra_native::enemy_spawn` into a real extraction command:
//! writes every level's hard-coded enemy placements to a plain text
//! file, straight from PRG-ROM, no emulation - both outdoor levels'
//! per-screen-scroll-position format and indoor/base levels' (2 and 4)
//! fixed 3-byte-per-enemy-plus-core-count format.

/// Writes `<out_dir>/level{1..8}_enemies.txt` for every level.
/// Returns `files_written` (always 8 now that both formats are decoded).
pub fn dump_all(prg_rom: &[u8], out_dir: &std::path::Path) -> anyhow::Result<usize> {
    std::fs::create_dir_all(out_dir)?;
    let mut written = 0usize;

    for level_index in 0..8 {
        let header = contra_native::world::level::level_header(prg_rom, level_index);
        let path = out_dir.join(format!("level{}_enemies.txt", level_index + 1));
        let ptr_tbl = contra_native::enemy::enemy_spawn::level_enemy_screen_ptr_tbl_prg_offset(prg_rom, level_index);

        let text = if header.location_type == contra_native::world::level::LocationType::Outdoor {
            let mut text = String::new();
            let mut total = 0usize;
            for screen_index in 1..header.screen_count {
                let offset = contra_native::enemy::enemy_spawn::enemy_screen_prg_offset(prg_rom, ptr_tbl, screen_index);
                let spawns = contra_native::enemy::enemy_spawn::decompress_outdoor_enemy_screen(&prg_rom[offset..]);
                if spawns.is_empty() {
                    continue;
                }
                text.push_str(&format!("screen {screen_index}:\n"));
                for spawn in &spawns {
                    text.push_str(&format!("  type={:#04x} x={:#04x} y={:#04x} attribute={:#05b}\n", spawn.enemy_type, spawn.x, spawn.y, spawn.attribute));
                }
                total += spawns.len();
            }
            format!("{total} hard-coded enemies across {} screens\n\n{text}", header.screen_count)
        } else {
            // Indoor/base level: unlike outdoor, every screen (including
            // screen 0) can have real placements - both real indoor
            // levels' own screen 0 do (confirmed via live gameplay
            // capture, see `contra_native::enemy::enemy_spawn::
            // decompress_indoor_enemy_screen`'s doc comment) - and a
            // screen's whole enemy list is read in one pass, not
            // incrementally by scroll position, so there's no "skip
            // screen 0" convention to mirror here.
            let mut text = String::new();
            let mut total = 0usize;
            for screen_index in 0..header.screen_count {
                let offset = contra_native::enemy::enemy_spawn::enemy_screen_prg_offset(prg_rom, ptr_tbl, screen_index);
                let Some(screen) = contra_native::enemy::enemy_spawn::decompress_indoor_enemy_screen(&prg_rom[offset..]) else {
                    continue;
                };
                if screen.spawns.is_empty() {
                    continue;
                }
                text.push_str(&format!("screen {screen_index} (cores to destroy: {}):\n", screen.cores_to_destroy));
                for spawn in &screen.spawns {
                    text.push_str(&format!("  type={:#04x} x={:#04x} y={:#04x} attribute={:#04x}\n", spawn.enemy_type, spawn.x, spawn.y, spawn.attribute));
                }
                total += screen.spawns.len();
            }
            format!("{total} hard-coded enemies across {} screens (indoor/base level)\n\n{text}", header.screen_count)
        };

        std::fs::write(&path, text)?;
        log::info!("level {}: wrote enemy placements", level_index + 1);
        written += 1;
    }

    Ok(written)
}
