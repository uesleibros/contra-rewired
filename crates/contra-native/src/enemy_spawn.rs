//! Native port of Contra's hard-coded enemy placement data - the fixed,
//! same-every-playthrough enemy spawns each level defines per screen (as
//! opposed to level 1/3/5/6/7's *random* soldier generation, a real
//! runtime behavior driven by `exe_soldier_generation`/
//! `soldier_generation_00`-`02` in bank 2, not a decodable static asset -
//! porting that is CPU-logic work, not extraction, and isn't covered
//! here). See `docs/Enemy Routines.md` in `vermiceli/nes-contra-us`'s
//! "Level Enemies" section for the format this is ported from.
//!
//! ## Format (outdoor levels)
//!
//! Each level has its own list of per-screen enemy lists, reached via two
//! levels of indirection: `level_enemy_screen_ptr_ptr_tbl` (bank 2,
//! `$b513`, one 2-byte pointer per level) points to that level's own
//! `level_X_enemy_screen_ptr_tbl` - one 2-byte pointer *per level screen*
//! (entry `i` = screen `i`'s list, confirmed by reading level 1's table
//! raw: 13 entries for its 13 screens, entry 9 resolving to exactly
//! `level_1_enemy_screen_09`'s real address, entry-index-equals-
//! screen-index with no offset). This directly contradicts a literal
//! reading of `docs/Enemy Routines.md`'s prose ("the first entry in this
//! table is associated to the *second* screen of the level"), which would
//! predict an off-by-one shift that the raw bytes don't show; a first
//! attempt trusted that prose, shifted every lookup by one, and produced
//! a garbage/runaway decode at the level's last screen (reading one entry
//! past the table's real end, into whatever data follows it) - the raw
//! bytes are the ground truth here, not the prose. Screen 0's own entry
//! (index 0) is real and resolvable like any other, but per the same doc
//! section is never populated with actual placements (screen 0 never has
//! hard-coded enemies) - callers wanting non-empty data should start at
//! screen 1. A screen's enemy list is plain (uncompressed) data: each
//! entry is `X, (repeat:2|type:6), (Y|
//! attribute:3)` - 3 bytes, or `2 + repeat` bytes when `repeat > 0` (one
//! extra `Y|attribute` byte per repetition, reusing the same `X`/type),
//! terminated by `0xFF`.
//!
//! **The `Y` field's bit layout was verified against the doc's own
//! worked example, and doesn't match a first, plausible-looking read of
//! its diagram**: the diagram (`YYYYY AAA`) suggests `Y = byte >> 3`, but
//! the worked example (`level_1_enemy_screen_09`: `$40` decoded as
//! Y=`$40`, attribute `000`; `$b4` decoded as Y=`$b0`, attribute `100`)
//! only reproduces if `Y = byte & 0xF8` (top 5 bits, *not* shifted down -
//! a coarse Y coordinate in multiples of 8) and `attribute = byte & 0x07`.
//! This module follows the worked example, confirmed by
//! [`tests::level_1_screen_09_matches_the_documented_worked_example`].
//!
//! Indoor/base levels (levels 2 and 4) use a different, 3-byte-fixed
//! format this module doesn't decode yet - see
//! [`decompress_outdoor_enemy_screen`]'s doc comment.

/// `level_enemy_screen_ptr_ptr_tbl` (bank 2, CPU `$b513`): 8 2-byte
/// pointers, one per level, to that level's own screen-enemy pointer
/// table. PRG-ROM offset = `2*0x4000 + (0xb513-0x8000)`.
pub const LEVEL_ENEMY_SCREEN_PTR_PTR_TBL_PRG_OFFSET: usize = 0xB513;

fn mem_addr_at(prg_rom: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([prg_rom[offset], prg_rom[offset + 1]])
}

/// Resolves level `level_index`'s (0-based) own
/// `level_X_enemy_screen_ptr_tbl` to a PRG-ROM offset.
pub fn level_enemy_screen_ptr_tbl_prg_offset(prg_rom: &[u8], level_index: usize) -> usize {
    let entry_offset = LEVEL_ENEMY_SCREEN_PTR_PTR_TBL_PRG_OFFSET + level_index * 2;
    let mem_addr = mem_addr_at(prg_rom, entry_offset);
    2 * 0x4000 + (mem_addr as usize & 0x3FFF)
}

/// Resolves level screen `screen_index`'s (0-based, same convention as
/// `level::screen_prg_offset`) enemy list to a PRG-ROM offset - entry
/// index equals screen index directly (see this module's doc comment for
/// why that's confirmed, not assumed). `screen_index` must be less than
/// the level's real screen count - the table has exactly that many
/// entries, and reading past it lands in unrelated data with no
/// terminator anywhere nearby.
pub fn enemy_screen_prg_offset(prg_rom: &[u8], ptr_tbl_prg_offset: usize, screen_index: usize) -> usize {
    let entry_offset = ptr_tbl_prg_offset + screen_index * 2;
    let mem_addr = mem_addr_at(prg_rom, entry_offset);
    2 * 0x4000 + (mem_addr as usize & 0x3FFF)
}

/// One hard-coded enemy spawn: raw fields, undecoded beyond the
/// documented bit-packing (which specific `enemy_type`/`attribute`
/// values mean is `Enemy Glossary.md`'s job, out of scope here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemySpawn {
    pub x: u8,
    /// Coarse Y coordinate, already in multiples of 8 (`byte & 0xF8` -
    /// see this module's doc comment for why that's the confirmed
    /// decoding, not a `>>3` shift).
    pub y: u8,
    pub enemy_type: u8,
    pub attribute: u8,
}

/// Decodes one outdoor-level screen's hard-coded enemy list. Indoor/base
/// levels (2 and 4) use a different, fixed 3-byte-per-enemy format
/// (`XXXX YYYY`, `CDTT TTTT`, `AAAAAAAA` - 4-bit coarse X/Y plus `+8`
/// adjustment flags, a 6-bit type, and a full 8-bit attribute byte) that
/// isn't decoded here yet: `docs/Enemy Routines.md` documents the bit
/// layout but (unlike the outdoor format) doesn't include a worked
/// example to verify a final pixel-offset formula against, and this
/// module's standard is not shipping an unverified guess.
pub fn decompress_outdoor_enemy_screen(data: &[u8]) -> Vec<EnemySpawn> {
    let mut pos = 0usize;
    let mut spawns = Vec::new();
    loop {
        let x = data[pos];
        pos += 1;
        if x == 0xff {
            break;
        }
        let rt = data[pos];
        pos += 1;
        let repeat = (rt >> 6) & 0x03;
        let enemy_type = rt & 0x3f;
        for _ in 0..=repeat {
            let ya = data[pos];
            pos += 1;
            spawns.push(EnemySpawn { x, y: ya & 0xF8, enemy_type, attribute: ya & 0x07 });
        }
    }
    spawns
}

/// How many bytes [`decompress_outdoor_enemy_screen`] would consume from
/// `data` before hitting its terminating `0xFF` - same control flow,
/// tracking only the position. Useful for callers that need a screen
/// blob's exact byte extent (e.g. to declare it as a known data region).
pub fn decompress_outdoor_enemy_screen_len(data: &[u8]) -> usize {
    let mut pos = 0usize;
    loop {
        let x = data[pos];
        pos += 1;
        if x == 0xff {
            return pos;
        }
        let rt = data[pos];
        pos += 1;
        let repeat = (rt >> 6) & 0x03;
        pos += repeat as usize + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_1_screen_09_matches_the_documented_worked_example() {
        // docs/Enemy Routines.md's own worked example, verbatim:
        //   level_1_enemy_screen_09:
        //       .byte $10,$43,$40,$b4 ; flying capsule (type #$03), attribute 000,
        //                             ; location (#$10, #$40), repeat 1 [(y=#$b0, attr=100)]
        //       .byte $e0,$07,$81     ; red turret (type #$07), attribute 001, location (#$e0, #$80)
        //       .byte $ff
        let data = [0x10, 0x43, 0x40, 0xb4, 0xe0, 0x07, 0x81, 0xff];
        let spawns = decompress_outdoor_enemy_screen(&data);
        assert_eq!(
            spawns,
            vec![
                EnemySpawn { x: 0x10, y: 0x40, enemy_type: 0x03, attribute: 0b000 },
                EnemySpawn { x: 0x10, y: 0xb0, enemy_type: 0x03, attribute: 0b100 },
                EnemySpawn { x: 0xe0, y: 0x80, enemy_type: 0x07, attribute: 0b001 },
            ]
        );
    }

    #[test]
    fn no_repeat_is_exactly_one_spawn() {
        let data = [0x50, 0x05, 0x60, 0xff]; // soldier, no repeat
        let spawns = decompress_outdoor_enemy_screen(&data);
        assert_eq!(spawns, vec![EnemySpawn { x: 0x50, y: 0x60, enemy_type: 0x05, attribute: 0 }]);
        assert_eq!(decompress_outdoor_enemy_screen_len(&data), data.len());
    }

    #[test]
    fn ptr_tbl_lookups_resolve_bank_2_addresses() {
        let mut rom = vec![0u8; 0x20000];
        rom[LEVEL_ENEMY_SCREEN_PTR_PTR_TBL_PRG_OFFSET..LEVEL_ENEMY_SCREEN_PTR_PTR_TBL_PRG_OFFSET + 2].copy_from_slice(&0xB82Bu16.to_le_bytes());
        let level_ptr_tbl = level_enemy_screen_ptr_tbl_prg_offset(&rom, 0);
        assert_eq!(level_ptr_tbl, 0xB82B);

        // level_1_enemy_screen_ptr_tbl's real entry 9 (confirmed via raw
        // ROM bytes, entry index == screen index) points to
        // level_1_enemy_screen_09.
        rom[level_ptr_tbl + 9 * 2..level_ptr_tbl + 9 * 2 + 2].copy_from_slice(&0xB88Du16.to_le_bytes());
        assert_eq!(enemy_screen_prg_offset(&rom, level_ptr_tbl, 9), 0xB88D);
    }
}
