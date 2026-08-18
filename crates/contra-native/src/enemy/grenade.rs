//! Native port of the indoor grenade enemy's `_00`/`_01`/`_02` routine
//! family (`src/bank0.asm`, `$8fd5`-`$90f5`) - thrown by indoor soldiers
//! and grenade launchers on indoor levels. `_00` seeds the falling-arc
//! accumulator from the grenade's spawn Y position; `_01` picks one of 3
//! perspective sprite tables from how far the arc has fallen, cycles
//! through it on a global-frame-counter cadence, grows the arc's own
//! horizontal accumulator (`ENEMY_VAR_4`) by `$0c` every call before
//! handing off to [`crate::enemy::enemy_falling_arc::set_enemy_falling_arc_pos`],
//! then advances once the arc's own `ENEMY_VAR_3` accumulator goes
//! non-negative; `_02` detonates via [`crate::enemy::mortar_shot::
//! mortar_shot_routine_03`] (the same real shared "explosion_sound_hide_
//! enemy" tail split mortars and ice grenades use).
//!
//! ## `_02`'s real double-advance
//!
//! `grenade_routine_02` calls `mortar_shot_routine_03` via `jsr` (not a
//! tail `jmp`, unlike every other real caller of it in this crate so
//! far) and then falls straight into its own `jmp advance_enemy_routine`
//! immediately after. Since `mortar_shot_routine_03` itself always ends
//! by falling through to (or tail-jumping into, which is
//! stack-equivalent) `advance_enemy_routine`'s own `rts`, that `rts`
//! returns control to `grenade_routine_02`'s own next instruction rather
//! than skipping it - so on the "still had a sprite to hide" path, the
//! enemy routine index really does advance *twice* in a single call: once
//! inside `mortar_shot_routine_03`'s own real body, and again from
//! `grenade_routine_02`'s own trailing call. Verified by hand-tracing the
//! real instruction sequence rather than assumed; [`grenade_routine_02`]
//! models this explicitly via `final_routine_update`.

use crate::enemy::enemy_falling_arc::{set_enemy_falling_arc_pos, SetEnemyFallingArcPosOutcome, SetEnemyFallingArcPosResult};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::mortar_shot::{mortar_shot_routine_03, MortarShotRoutine03Outcome, MortarShotRoutine03Result};

/// The full result of one [`grenade_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrenadeRoutine00Result {
    pub var_1: u8,
    pub var_4: u8,
    pub attack_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `grenade_routine_00` (`$8fd5`).
pub fn grenade_routine_00(enemy_y_pos: u8, current_routine: u8) -> GrenadeRoutine00Result {
    GrenadeRoutine00Result { var_1: enemy_y_pos, var_4: 0x00, attack_delay: 0xFD, routine_update: advance_enemy_routine(current_routine) }
}

/// `grenade_sprite_tbl_y_cutoff_tbl` (`$8fea`, 2 bytes) - how far the
/// falling-arc accumulator (`ENEMY_VAR_1`) has to have grown before the
/// "closer to player" perspective sprite tables take over.
const GRENADE_SPRITE_TBL_Y_CUTOFF_TBL: [u8; 2] = [0x80, 0x90];
/// `grenade_sprite_codes_len_tbl` (`$8fec`, 3 bytes) - sprite count per
/// perspective table.
const GRENADE_SPRITE_CODES_LEN_TBL: [u8; 3] = [4, 8, 8];
/// `grenade_sprite_codes_00` (`$9054`, farthest-perspective codes).
const GRENADE_SPRITE_CODES_00_CODE: [u8; 4] = [0xA8, 0xA9, 0xA6, 0xA9];
const GRENADE_SPRITE_CODES_00_ATTR: [u8; 4] = [0x00, 0x00, 0x00, 0xC0];
/// `grenade_sprite_codes_01` (`$905c`, mid-perspective codes).
const GRENADE_SPRITE_CODES_01_CODE: [u8; 8] = [0xA4, 0xA5, 0xA6, 0xA5, 0xA4, 0xA7, 0xA6, 0xA7];
const GRENADE_SPRITE_CODES_01_ATTR: [u8; 8] = [0x00, 0x00, 0x00, 0xC0, 0xC0, 0x00, 0x00, 0xC0];
/// `grenade_sprite_codes_02` (`$906c`, closest-perspective codes).
const GRENADE_SPRITE_CODES_02_CODE: [u8; 8] = [0xA0, 0xA1, 0xA2, 0xA1, 0xA0, 0xA3, 0xA2, 0xA3];
const GRENADE_SPRITE_CODES_02_ATTR: [u8; 8] = [0x00, 0x00, 0x00, 0xC0, 0xC0, 0x00, 0x00, 0xC0];

/// The real `@determine_sprite_code_loop`/`@sprite_code_tbl_found`
/// Y-cutoff scan (`$8fe8`-`$8ff6`) - same "walk down from the largest
/// matching cutoff" shape as [`crate::enemy::roller::roller_routine_01`]'s
/// own sprite-size scan, just a 2-entry table (3 possible table
/// indices: `0`, `1`, `2`).
fn grenade_sprite_table_index(var_1: u8) -> u8 {
    let mut y = 2u8;
    while y != 0 {
        if var_1 >= GRENADE_SPRITE_TBL_Y_CUTOFF_TBL[(y - 1) as usize] {
            return y;
        }
        y -= 1;
    }
    0
}

fn grenade_sprite_lookup(table_index: u8, frame: u8) -> (u8, u8) {
    match table_index {
        0 => (GRENADE_SPRITE_CODES_00_CODE[frame as usize], GRENADE_SPRITE_CODES_00_ATTR[frame as usize]),
        1 => (GRENADE_SPRITE_CODES_01_CODE[frame as usize], GRENADE_SPRITE_CODES_01_ATTR[frame as usize]),
        _ => (GRENADE_SPRITE_CODES_02_CODE[frame as usize], GRENADE_SPRITE_CODES_02_ATTR[frame as usize]),
    }
}

/// The full result of one [`grenade_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrenadeRoutine01Result {
    pub frame: u8,
    pub sprite: u8,
    pub sprite_attr: u8,
    pub var_4: u8,
    pub attack_delay: u8,
    pub arc: SetEnemyFallingArcPosResult,
    /// `Some` when `ENEMY_VAR_3` (post-arc-update) is non-negative (real
    /// `bpl grenade_adv_routine`) - `None` means the real ASM's own
    /// `rts`, staying in this routine another frame.
    pub routine_update: Option<EnemyRoutineUpdate>,
}

/// Native port of `grenade_routine_01` (`$8fe8`).
#[allow(clippy::too_many_arguments)]
pub fn grenade_routine_01(
    var_1: u8,
    var_2: u8,
    var_3: u8,
    var_4: u8,
    var_b: u8,
    enemy_frame: u8,
    frame_counter: u8,
    attack_delay: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    current_routine: u8,
) -> GrenadeRoutine01Result {
    let table_index = grenade_sprite_table_index(var_1);
    let len = GRENADE_SPRITE_CODES_LEN_TBL[table_index as usize];

    let mut frame = enemy_frame;
    if frame_counter & 0x07 == 0 {
        frame = frame.wrapping_add(1);
    }
    if frame >= len {
        frame = 0;
    }

    let (sprite, sprite_attr) = grenade_sprite_lookup(table_index, frame);

    let (var_4, carry) = var_4.overflowing_add(0x0C);
    let attack_delay = attack_delay.wrapping_add(carry as u8);

    let arc = set_enemy_falling_arc_pos(var_1, var_2, var_3, var_4, var_b, y_vel_accum, y_vel_fract, y_vel_fast, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast);

    // `set_enemy_falling_arc_pos` reaches its own removal via a real
    // tail `jmp`, so a removal there still returns into this routine's
    // own remaining code (same `jsr`-return quirk documented throughout
    // this crate) - `current_routine` must be treated as already-zeroed
    // in that case, not the stale entry value.
    let effective_routine = match arc.outcome {
        SetEnemyFallingArcPosOutcome::Position { .. } => current_routine,
        _ => 0,
    };
    let routine_update = if (arc.vars.var_3 as i8) >= 0 { Some(advance_enemy_routine(effective_routine)) } else { None };

    GrenadeRoutine01Result { frame, sprite, sprite_attr, var_4, attack_delay, arc, routine_update }
}

/// The full result of one [`grenade_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrenadeRoutine02Result {
    pub sound: u8,
    pub y_pos: u8,
    pub mortar_result: MortarShotRoutine03Result,
    /// `advance_enemy_routine` applied *again* on top of `mortar_
    /// result`'s own outcome - see this module's doc comment for why.
    pub final_routine_update: EnemyRoutineUpdate,
}

/// Native port of `grenade_routine_02` (`$907c`).
#[allow(clippy::too_many_arguments)]
pub fn grenade_routine_02(
    enemy_state_width: u8,
    enemy_sprite_attr: u8,
    enemy_sprites: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    current_routine: u8,
) -> GrenadeRoutine02Result {
    let y_pos = 0xAC;
    let mortar_result = mortar_shot_routine_03(enemy_state_width, enemy_sprite_attr, enemy_sprites, level_scrolling_type, frame_scroll, x_pos, y_pos, current_routine);

    let routine_after_first_advance = match &mortar_result.outcome {
        MortarShotRoutine03Outcome::Removed(removed) => removed.routine,
        MortarShotRoutine03Outcome::Hidden(hidden) => hidden.delayed_routine.routine_update.routine,
    };
    let final_routine_update = advance_enemy_routine(routine_after_first_advance);

    GrenadeRoutine02Result { sound: 0x24, y_pos, mortar_result, final_routine_update }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_seeds_var_1_from_entry_y_pos() {
        let r = grenade_routine_00(0x60, 3);
        assert_eq!(r.var_1, 0x60);
        assert_eq!(r.var_4, 0x00);
        assert_eq!(r.attack_delay, 0xFD);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn sprite_table_index_walks_down_from_closest_perspective() {
        assert_eq!(grenade_sprite_table_index(0x95), 2);
        assert_eq!(grenade_sprite_table_index(0x85), 1);
        assert_eq!(grenade_sprite_table_index(0x10), 0);
    }

    #[test]
    fn routine_01_advances_frame_only_on_the_frame_counter_cadence() {
        let no_advance = grenade_routine_01(0x10, 0, 0, 0, 0, 0x01, 0x03, 0x00, 0, 0, 0, 0, 0x50, 0, 0, 0, 3);
        assert_eq!(no_advance.frame, 0x01);
        let advance = grenade_routine_01(0x10, 0, 0, 0, 0, 0x01, 0x08, 0x00, 0, 0, 0, 0, 0x50, 0, 0, 0, 3);
        assert_eq!(advance.frame, 0x02);
    }

    #[test]
    fn routine_01_wraps_frame_at_the_table_length() {
        // table_index 0 (var_1 < 0x80) has length 4.
        let r = grenade_routine_01(0x10, 0, 0, 0, 0, 0x03, 0x08, 0x00, 0, 0, 0, 0, 0x50, 0, 0, 0, 3);
        assert_eq!(r.frame, 0x00);
        assert_eq!((r.sprite, r.sprite_attr), (GRENADE_SPRITE_CODES_00_CODE[0], GRENADE_SPRITE_CODES_00_ATTR[0]));
    }

    #[test]
    fn routine_01_grows_var_4_by_0x0c_before_the_arc_update() {
        let r = grenade_routine_01(0x10, 0, 0, 0xFA, 0, 0x00, 0x01, 0x00, 0, 0, 0, 0, 0x50, 0, 0, 0, 3);
        assert_eq!(r.var_4, 0x06); // 0xfa + 0x0c wraps
        assert_eq!(r.attack_delay, 0x01); // carried
        assert_eq!(r.arc.vars.var_2, 0x06); // arc used the already-grown var_4
    }

    #[test]
    fn routine_01_advances_when_var_3_is_non_negative_after_the_arc_update() {
        let r = grenade_routine_01(0x10, 0, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0, 0x50, 0, 0, 0, 3);
        assert!(r.arc.vars.var_3 as i8 >= 0);
        assert_eq!(r.routine_update, Some(advance_enemy_routine(3)));
    }

    #[test]
    fn routine_01_stays_when_var_3_is_negative_after_the_arc_update() {
        let r = grenade_routine_01(0x10, 0, 0x80, 0x00, 0x00, 0x00, 0x01, 0x00, 0, 0, 0, 0, 0x50, 0, 0, 0, 3);
        assert!((r.arc.vars.var_3 as i8) < 0);
        assert_eq!(r.routine_update, None);
    }

    #[test]
    fn routine_01_treats_a_removed_arc_as_routine_zero_before_the_advance_check() {
        // var_1 = 0xe0, y_vel_fast = 0x10 -> new var_1 = 0xf0, triggers RemovedFallenOffBottom.
        let r = grenade_routine_01(0xE0, 0, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0, 0, 0x10, 0, 0x50, 0, 0, 0, 3);
        assert!(matches!(r.arc.outcome, SetEnemyFallingArcPosOutcome::RemovedFallenOffBottom(_)));
        if (r.arc.vars.var_3 as i8) >= 0 {
            assert_eq!(r.routine_update, Some(advance_enemy_routine(0)));
        }
    }

    #[test]
    fn routine_02_detonates_and_double_advances_when_hidden() {
        let r = grenade_routine_02(0x00, 0x00, 0x20, 0, 0x02, 0x50, 3);
        assert_eq!(r.sound, 0x24);
        assert_eq!(r.y_pos, 0xAC);
        match &r.mortar_result.outcome {
            MortarShotRoutine03Outcome::Hidden(h) => {
                let after_first = h.delayed_routine.routine_update.routine;
                assert_eq!(r.final_routine_update, advance_enemy_routine(after_first));
                // genuinely a second, distinct increment on top of the first.
                assert_eq!(r.final_routine_update.routine, after_first.wrapping_add(1));
            }
            other => panic!("expected Hidden, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_guard_rejects_the_second_advance_when_removed() {
        let r = grenade_routine_02(0x00, 0x00, 0x00, 0, 0x02, 0x50, 3);
        assert!(matches!(r.mortar_result.outcome, MortarShotRoutine03Outcome::Removed(_)));
        assert_eq!(r.final_routine_update, advance_enemy_routine(0));
        assert_eq!(r.final_routine_update, EnemyRoutineUpdate { routine: 0, sprites: Some(0) });
    }
}
