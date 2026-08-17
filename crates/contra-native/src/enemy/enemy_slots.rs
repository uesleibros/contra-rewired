//! Native port of Contra's enemy-object-slot finder: `find_next_enemy_slot`
//! / `find_next_enemy_slot_6_to_0` (`src/bank7.asm`, CPU `$edca`-`$edd9`,
//! sharing a loop body at `find_next_enemy_slot_x_to_0`, `$edd0`) - scans
//! `ENEMY_ROUTINE` (16 bytes, one per enemy object slot, `$04b8`-`$04c7`)
//! for a free slot before spawning anything into the shared enemy/bullet/
//! pickup object pool. One of the most-called routines in the whole ROM
//! (13 real `jsr` sites across banks 0, 2, and 7) - almost every spawn
//! path (enemy AI, player/enemy bullets, random soldier generation, item
//! drops) starts here.

/// `ENEMY_ROUTINE` and its sibling per-slot arrays (`ENEMY_TYPE`,
/// `ENEMY_HP`, etc.) are all this long - 16 enemy object slots.
pub const ENEMY_SLOT_COUNT: usize = 16;

/// Shared core of both real entry points (`find_next_enemy_slot_x_to_0`,
/// `$edd0`-`$edd7`): scans `enemy_routine` from `start` down to 0
/// inclusive, returning the *first* slot found free (i.e. the
/// highest-indexed free slot at or below `start`) - `None` if every slot
/// down to 0 is occupied. Matches the real routine's own register
/// convention (`x` = result, zero flag = found/not-found) via `Option`.
fn find_slot_from(enemy_routine: &[u8; ENEMY_SLOT_COUNT], start: u8) -> Option<u8> {
    let mut x = start as i16;
    loop {
        if enemy_routine[x as usize] == 0 {
            return Some(x as u8);
        }
        x -= 1;
        if x < 0 {
            return None;
        }
    }
}

/// Native port of `find_next_enemy_slot` (`$edce`) - the general-purpose
/// free-slot scan used by most spawn paths: all 16 slots, 15 down to 0.
pub fn find_next_enemy_slot(enemy_routine: &[u8; ENEMY_SLOT_COUNT]) -> Option<u8> {
    find_slot_from(enemy_routine, 15)
}

/// Native port of `find_next_enemy_slot_6_to_0` (`$edca`) - the same scan
/// restricted to slots 6 down to 0. Real caller: level 1/3/5/6/7's random
/// soldier generation (`bank2.asm`'s `exe_soldier_generation` path),
/// which reserves the top slots (7-15) for everything else so a level
/// that's already busy (bosses, players' own bullets, hard-coded enemy
/// placements) can't get crowded out of spawning more soldiers.
pub fn find_next_enemy_slot_6_to_0(enemy_routine: &[u8; ENEMY_SLOT_COUNT]) -> Option<u8> {
    find_slot_from(enemy_routine, 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_slots_free_returns_the_highest_index_first() {
        let routine = [0u8; ENEMY_SLOT_COUNT];
        assert_eq!(find_next_enemy_slot(&routine), Some(15));
        assert_eq!(find_next_enemy_slot_6_to_0(&routine), Some(6));
    }

    #[test]
    fn skips_occupied_slots_scanning_downward() {
        let mut routine = [0u8; ENEMY_SLOT_COUNT];
        routine[15] = 1;
        routine[14] = 1;
        assert_eq!(find_next_enemy_slot(&routine), Some(13));
    }

    #[test]
    fn no_free_slots_returns_none() {
        let routine = [1u8; ENEMY_SLOT_COUNT];
        assert_eq!(find_next_enemy_slot(&routine), None);
        assert_eq!(find_next_enemy_slot_6_to_0(&routine), None);
    }

    #[test]
    fn slot_0_free_is_still_found() {
        // Real routine's edge case: dex from x=0 wraps to 0xff (bpl not
        // taken) only *after* slot 0 itself has already been checked and
        // missed - so slot 0 being free must still return Some(0), not
        // be skipped.
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[0] = 0;
        assert_eq!(find_next_enemy_slot(&routine), Some(0));
    }

    #[test]
    fn restricted_scan_never_returns_above_6() {
        let routine = [0u8; ENEMY_SLOT_COUNT]; // every slot free
        assert_eq!(find_next_enemy_slot_6_to_0(&routine), Some(6));
    }
}
