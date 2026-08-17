//! Native port of Contra's enemy state-machine transition primitives -
//! by far the most-reused routines found in this codebase:
//! `advance_enemy_routine` (`src/bank7.asm`, `$e78e`-`$e796`, **75 real
//! call sites**), `set_enemy_routine_to_a` (`$e81a`-`$e822`, 29 real call
//! sites), and `set_enemy_delay_adv_routine` (`$e78b`-`$e78d`, a real ASM
//! fallthrough into `advance_enemy_routine`, 29 more real call sites) -
//! virtually every enemy AI routine in the game ends by calling one of
//! these to move to its next state.
//!
//! All three share the same real guard: **an enemy slot whose
//! `ENEMY_ROUTINE` is already `0` never gets a new routine value** -
//! instead `set_sprite_0` runs (`ENEMY_SPRITES = 0`, shared real exit for
//! all three), leaving `ENEMY_ROUTINE` at `0`. Real ASM comment: "enemy
//! routines are off by one, so setting `ENEMY_ROUTINE` to `#$03` results
//! in the 2nd routine being run" - `0` itself means "no active routine
//! for this slot", not "routine index 0".

/// The real, shared result of any of this module's 3 entry points: either
/// the guard rejected the update (`ENEMY_ROUTINE` was already `0`, only
/// `ENEMY_SPRITES` gets cleared) or `ENEMY_ROUTINE` took on a new value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRoutineUpdate {
    pub routine: u8,
    /// `Some(0)` when the guard rejected the update (real `set_sprite_0`
    /// ran); `None` when it didn't touch `ENEMY_SPRITES` at all.
    pub sprites: Option<u8>,
}

fn set_enemy_routine_guarded(current_routine: u8, new_routine: u8) -> EnemyRoutineUpdate {
    if current_routine == 0 {
        EnemyRoutineUpdate { routine: 0, sprites: Some(0) }
    } else {
        EnemyRoutineUpdate { routine: new_routine, sprites: None }
    }
}

/// Native port of `advance_enemy_routine` (`$e78e`) - moves to the next
/// routine index (`+1`), guarded.
pub fn advance_enemy_routine(current_routine: u8) -> EnemyRoutineUpdate {
    set_enemy_routine_guarded(current_routine, current_routine.wrapping_add(1))
}

/// Native port of `set_enemy_routine_to_a` (`$e81a`) - jumps directly to
/// routine index `a`, guarded (unlike [`advance_enemy_routine`], this can
/// move to any routine, not just the next one).
pub fn set_enemy_routine_to_a(current_routine: u8, a: u8) -> EnemyRoutineUpdate {
    set_enemy_routine_guarded(current_routine, a)
}

/// Native port of `set_enemy_delay_adv_routine` (`$e78b`) - sets
/// `ENEMY_ANIMATION_DELAY` to `a` **unconditionally** (real ASM: this
/// store runs before the guard, unlike the routine update itself), then
/// falls through to [`advance_enemy_routine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelayedRoutineUpdate {
    pub animation_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

pub fn set_enemy_delay_adv_routine(a: u8, current_routine: u8) -> DelayedRoutineUpdate {
    DelayedRoutineUpdate { animation_delay: a, routine_update: advance_enemy_routine(current_routine) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_increments_when_routine_is_nonzero() {
        assert_eq!(advance_enemy_routine(5), EnemyRoutineUpdate { routine: 6, sprites: None });
    }

    #[test]
    fn advance_does_nothing_but_clear_sprite_when_routine_is_zero() {
        assert_eq!(advance_enemy_routine(0), EnemyRoutineUpdate { routine: 0, sprites: Some(0) });
    }

    #[test]
    fn advance_wraps_at_0xff() {
        assert_eq!(advance_enemy_routine(0xFF), EnemyRoutineUpdate { routine: 0, sprites: None });
    }

    #[test]
    fn set_to_a_jumps_directly_when_routine_is_nonzero() {
        assert_eq!(set_enemy_routine_to_a(1, 0x0A), EnemyRoutineUpdate { routine: 0x0A, sprites: None });
    }

    #[test]
    fn set_to_a_does_nothing_but_clear_sprite_when_routine_is_zero() {
        assert_eq!(set_enemy_routine_to_a(0, 0x0A), EnemyRoutineUpdate { routine: 0, sprites: Some(0) });
    }

    #[test]
    fn delayed_advance_sets_delay_unconditionally_even_when_guard_rejects() {
        let r = set_enemy_delay_adv_routine(0x20, 0);
        assert_eq!(r.animation_delay, 0x20);
        assert_eq!(r.routine_update, EnemyRoutineUpdate { routine: 0, sprites: Some(0) });
    }

    #[test]
    fn delayed_advance_sets_delay_and_advances_when_routine_nonzero() {
        let r = set_enemy_delay_adv_routine(0x20, 3);
        assert_eq!(r.animation_delay, 0x20);
        assert_eq!(r.routine_update, EnemyRoutineUpdate { routine: 4, sprites: None });
    }
}
