//! Native port of `initialize_enemy` (`src/bank7.asm`, CPU `$ee47`-`$ee79`)
//! - the universal "set up a freshly-claimed enemy slot" helper almost
//! every spawn path in the game calls right after finding a free slot
//! (see [`crate::enemy::enemy_slots`]) and setting `ENEMY_TYPE[x]`: sets
//! `ENEMY_ROUTINE`/`ENEMY_SPRITES` to 1, clears most other per-slot
//! fields via [`crate::enemy::enemy_clear::clear_enemy_pt_2`], then looks up
//! `ENEMY_STATE_WIDTH`/`ENEMY_SCORE_COLLISION`/`ENEMY_HP`/`ENEMY_VAR_A`
//! from a per-(level, enemy-type) property table.
//!
//! ## Why the property lookup reads raw PRG-ROM bytes instead of a
//! hand-transcribed table
//!
//! The real property data (`enemy_prop_ptr_tbl` + `enemy_prop_00`-`_07`,
//! `$ee8d`-`$efb7`, 9 pointers into a combined 0x118-byte/70-entry data
//! block) is addressed by a **raw pointer chase**: `ENEMY_TYPE < $10`
//! picks a shared table, otherwise `y = CURRENT_LEVEL*2` selects one of
//! 8 per-level pointers - but the entry actually read is at
//! `table_ptr + ENEMY_TYPE*4`, using the *unadjusted* type byte as an
//! offset from wherever that level's pointer happens to point, into one
//! physically contiguous block the individual `enemy_prop_XX` labels
//! merely subdivide for the disassembler's own bookkeeping. The
//! disassembly's own inline comments (parenthesized numbers next to each
//! entry) are **not reliable evidence of that offset** - they restart
//! partway through several tables (`enemy_prop_06`/`_07` each visibly
//! concatenate multiple levels' worth of entries under one label, with
//! their comment numbers resetting at each internal boundary), and one
//! table is prefixed with a section comment (`"; level 3 enemies"`
//! above `enemy_prop_06`, which `enemy_prop_ptr_tbl` actually assigns to
//! level 7) that flatly contradicts the pointer table itself. Rather
//! than guess which of these clues (if any) reflects the real indexing,
//! this port replicates the CPU's own byte-offset arithmetic directly
//! against the ROM's raw bytes - the same "don't hand-transcribe
//! ambiguous data, walk the real bytes" approach `graphics`/`level`/
//! `enemy_spawn` already use - and lets live-gameplay verification
//! (this module's own hook, not prose) be the actual source of truth.

use crate::enemy::enemy_clear::{clear_enemy_pt_2, EnemyClearFields};

/// `enemy_prop_ptr_tbl`'s own CPU address (bank 7, fixed at `$c000`-
/// `$ffff`) - 9 2-byte pointers (levels 1-8, then one shared entry at
/// offset `$10` for `ENEMY_TYPE < $10`).
const ENEMY_PROP_PTR_TBL_ADDR: u16 = 0xEE8D;

/// Converts a bank-7 (fixed bank) CPU address to a PRG-ROM byte offset.
/// Bank 7 is UxROM's fixed bank, always mapped at `$c000`-`$ffff`,
/// occupying the last 16KiB of PRG-ROM.
fn bank7_prg_offset(addr: u16) -> usize {
    7 * 0x4000 + (addr as usize - 0xC000)
}

/// Native port of the property-table half of `initialize_enemy`: given
/// the real ROM's PRG data, `ENEMY_TYPE[x]`, and `CURRENT_LEVEL`,
/// replicates the exact real pointer chase and returns the raw
/// `(state_width, score_collision, hp, var_a)` 4-byte record - see this
/// module's doc comment for why it reads `prg_rom` directly rather than
/// a hand-transcribed table.
pub fn enemy_prop_lookup(prg_rom: &[u8], enemy_type: u8, current_level: u8) -> (u8, u8, u8, u8) {
    let y: u16 = if enemy_type < 0x10 { 0x10 } else { current_level as u16 * 2 };
    let ptr_off = bank7_prg_offset(ENEMY_PROP_PTR_TBL_ADDR) + y as usize;
    let table_addr = u16::from_le_bytes([prg_rom[ptr_off], prg_rom[ptr_off + 1]]);
    let entry_off = bank7_prg_offset(table_addr) + enemy_type as usize * 4;
    (prg_rom[entry_off], prg_rom[entry_off + 1], prg_rom[entry_off + 2], prg_rom[entry_off + 3])
}

/// The real result of initializing one enemy slot - `ENEMY_ROUTINE` is
/// always `1` (real comment: "always off by one", i.e. index 1 selects
/// `..._routine_00`), `ENEMY_HP` isn't part of [`EnemyClearFields`] (the
/// `clear_enemy_*` family never touches it), and `fields` holds
/// everything [`clear_enemy_pt_2`] clears plus the 3 of those fields
/// (`state_width`, `score_collision`, `var_a`) the property lookup
/// immediately overwrites afterward, and `sprites` (set to `1` *before*
/// `clear_enemy_pt_2` runs, which never touches it - real instruction
/// order, not incidental).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitializedEnemy {
    pub routine: u8,
    pub hp: u8,
    pub fields: EnemyClearFields,
}

/// Native port of `initialize_enemy` (`$ee47`). `enemy_type`/
/// `current_level` are inputs the real routine reads but never writes
/// (`ENEMY_TYPE[x]` must already be set by *this* routine's own caller,
/// same as the real ASM).
pub fn initialize_enemy(prg_rom: &[u8], enemy_type: u8, current_level: u8) -> InitializedEnemy {
    let mut fields = EnemyClearFields { sprites: 1, ..EnemyClearFields::default() };
    clear_enemy_pt_2(&mut fields);
    let (state_width, score_collision, hp, var_a) = enemy_prop_lookup(prg_rom, enemy_type, current_level);
    fields.state_width = state_width;
    fields.score_collision = score_collision;
    fields.var_a = var_a;
    InitializedEnemy { routine: 1, hp, fields }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal synthetic PRG-ROM: just enough bytes at the real bank-7
    /// offsets to exercise the pointer chase without needing the real
    /// ROM - one shared-table pointer (offset `$10`) and one per-level
    /// pointer (level 0, offset `$00`), each pointing at a small,
    /// distinct data block with recognizable, non-overlapping bytes.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = bank7_prg_offset(ENEMY_PROP_PTR_TBL_ADDR);
        let shared_table_addr: u16 = 0xEF00;
        let level0_table_addr: u16 = 0xEF10;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level0_table_addr.to_le_bytes());

        // shared table: enemy_type=2 -> record at offset 2*4=8
        let shared_off = bank7_prg_offset(shared_table_addr) + 8;
        rom[shared_off..shared_off + 4].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);

        // level-0 table: enemy_type=0x12 -> record at offset 0x12*4=0x48
        let level0_off = bank7_prg_offset(level0_table_addr) + 0x48;
        rom[level0_off..level0_off + 4].copy_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);

        rom
    }

    #[test]
    fn shared_enemy_type_uses_the_shared_pointer() {
        let rom = synthetic_prg_rom();
        assert_eq!(enemy_prop_lookup(&rom, 2, 5), (0x11, 0x22, 0x33, 0x44));
    }

    #[test]
    fn non_shared_enemy_type_uses_the_current_level_pointer() {
        let rom = synthetic_prg_rom();
        assert_eq!(enemy_prop_lookup(&rom, 0x12, 0), (0xAA, 0xBB, 0xCC, 0xDD));
    }

    #[test]
    fn initialize_enemy_sets_routine_1_sprites_1_and_the_looked_up_fields() {
        let rom = synthetic_prg_rom();
        let result = initialize_enemy(&rom, 2, 5);
        assert_eq!(result.routine, 1);
        assert_eq!(result.fields.sprites, 1);
        assert_eq!(result.fields.state_width, 0x11);
        assert_eq!(result.fields.score_collision, 0x22);
        assert_eq!(result.hp, 0x33);
        assert_eq!(result.fields.var_a, 0x44);
        // everything else clear_enemy_pt_2 touches is still zeroed
        assert_eq!(result.fields.attributes, 0);
        assert_eq!(result.fields.var_1, 0);
    }
}
