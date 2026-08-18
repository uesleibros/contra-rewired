//! Native port of `generate_enemy_at_pos` (`src/bank7.asm`, CPU
//! `$e94a`-`$e972`) - the shared "spawn a new enemy relative to the
//! calling enemy's own position" primitive: finds a free slot
//! ([`find_next_enemy_slot`]), initializes it ([`initialize_enemy`]),
//! then offsets its position from the *caller's* current `ENEMY_X_POS`/
//! `ENEMY_Y_POS` by the given `(x_offset, y_offset)`. Real callers set
//! `$0a` (the new enemy's type) themselves before calling - this port
//! takes `enemy_type` as a plain parameter instead.
//!
//! This is a pure composition of two already-ported primitives - no new
//! real logic beyond the position-offset arithmetic and the real
//! find/fail branch (`bne @exit`, ported as [`Option::None`]).

use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::{initialize_enemy, InitializedEnemy};

/// One successful [`generate_enemy_at_pos`] call's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedEnemy {
    pub slot: u8,
    pub initialized: InitializedEnemy,
    pub x_pos: u8,
    pub y_pos: u8,
}

/// Native port of `generate_enemy_at_pos` (`$e94a`). `caller_x_pos`/
/// `caller_y_pos` are the *spawning* enemy's own current position
/// (`ENEMY_X_POS,y`/`ENEMY_Y_POS,y` in the real ASM, `y` still holding
/// the caller's own slot at that point); `x_offset`/`y_offset` are the
/// real `$09`/`$08` relative-position parameters. Returns [`None`] when
/// no free slot exists (real `bne @exit`), matching [`find_next_enemy_
/// slot`]'s own `Option`.
#[allow(clippy::too_many_arguments)]
pub fn generate_enemy_at_pos(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    enemy_type: u8,
    current_level: u8,
    caller_x_pos: u8,
    caller_y_pos: u8,
    x_offset: u8,
    y_offset: u8,
) -> Option<GeneratedEnemy> {
    let slot = find_next_enemy_slot(enemy_routine)?;
    let initialized = initialize_enemy(prg_rom, enemy_type, current_level);
    Some(GeneratedEnemy {
        slot,
        initialized,
        x_pos: caller_x_pos.wrapping_add(x_offset),
        y_pos: caller_y_pos.wrapping_add(y_offset),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        // Reuses initialize_enemy's own real bank-7 property-table
        // layout so enemy_type=0x0f (turret man bullet) resolves to
        // something non-zero and distinguishable in tests.
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let shared_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 0x0f * 4;
        rom[shared_off..shared_off + 4].copy_from_slice(&[0x81, 0x0d, 0x01, 0x00]);
        rom
    }

    #[test]
    fn spawns_at_caller_position_plus_offset() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = generate_enemy_at_pos(&rom, &routine, 0x0f, 0, 0x50, 0x60, 0x01, 0xFC).unwrap();
        assert_eq!(r.slot, 15); // highest free slot
        assert_eq!(r.x_pos, 0x51);
        assert_eq!(r.y_pos, 0x5C); // 0x60 + 0xFC wraps
        assert_eq!(r.initialized.routine, 1);
        assert_eq!(r.initialized.fields.state_width, 0x81);
        assert_eq!(r.initialized.fields.score_collision, 0x0d);
        assert_eq!(r.initialized.hp, 0x01);
    }

    #[test]
    fn no_free_slot_returns_none() {
        let rom = synthetic_prg_rom();
        let routine = [1u8; ENEMY_SLOT_COUNT];
        assert!(generate_enemy_at_pos(&rom, &routine, 0x0f, 0, 0x50, 0x60, 0x01, 0xFC).is_none());
    }
}
