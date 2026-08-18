//! Native port of the sniper ("rifle man")'s own `_00`/`_01` table
//! entries (`src/bank0.asm`, `$8958`/`$8982`). `sniper_routine_02`-`_05`
//! (a real bullet-angle-quadrant aiming/firing subsystem built around a
//! new `get_rotate_01`/`quadrant_aim_dir_lookup_ptr_tbl` dependency, a
//! whole further subsystem on its own scale) are **not yet ported** -
//! deferred to a future pass rather than rushed.
//!
//! `ENEMY_ATTRIBUTES` selects one of 3 real sniper types: `0` standing
//! (always visible, fires from a fixed pose), `1` crouching/hiding
//! (only visible - and only takes the extra `+5` Y nudge `_00` applies -
//! while popped up to fire), `2` boss-screen hiding (same shape as type
//! 0 for everything `_00` itself touches, but its own sprite table and
//! `_01`'s own crouch-cycle-then-nudge path).
//!
//! ## `sniper_routine_01`'s 3 ways to reach "activated"
//!
//! Standing snipers (type `0`) skip the crouch-cycle animation entirely
//! and activate immediately once the delay elapses. Crouching/hiding
//! snipers (type `1`/`2`) cycle `ENEMY_FRAME` through 3 pop-up frames
//! first; once that finishes, type `2` (boss screen) applies a real
//! `-14`/`+1` Y/X position nudge before activating, while type `1`
//! (crouching) decrements `ENEMY_FRAME` once more instead and activates
//! directly - *unless* that decrement happens to land on exactly `0`, in
//! which case real ASM falls through into the *same* nudge code type
//! `2` uses. This last case is real, valid control flow this port still
//! models (`ActivatedFrom::CrouchFallthroughNudge`), but isn't
//! independently exercised - tracing the real frame arithmetic, it never
//! actually happens for type `1` starting from `_00`'s own initial
//! frame (`0`).

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_position_utils::{add_a_to_enemy_x_pos, add_a_to_enemy_y_pos, add_a_with_vert_scroll_to_enemy_y_pos};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};

/// `sniper_animation_delay_tbl` (`$8979`, 3 bytes) - initial `ENEMY_
/// ANIMATION_DELAY`, indexed by sniper type.
const SNIPER_ANIMATION_DELAY_TBL: [u8; 3] = [0x01, 0x30, 0x80];
/// `sniper_frame_tbl` (`$897f`, 3 bytes) - initial `ENEMY_FRAME`
/// (`sniper_sprite_xx` offset), same indexing.
const SNIPER_FRAME_TBL: [u8; 3] = [0x03, 0x00, 0x00];

/// The full result of one [`sniper_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine00Result {
    pub animation_delay: u8,
    pub frame: u8,
    pub y_pos: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `sniper_routine_00` (`$8958`) - "load variables
/// (`ENEMY_ANIMATION_DELAY`, `ENEMY_FRAME`), adjust Y position for
/// crouching sniper". `sniper_type` is `ENEMY_ATTRIBUTES` (`0`/`1`/`2`).
pub fn sniper_routine_00(sniper_type: u8, enemy_y_pos: u8, vertical_scroll: u8, current_routine: u8) -> SniperRoutine00Result {
    let animation_delay = SNIPER_ANIMATION_DELAY_TBL[sniper_type as usize];
    let frame = SNIPER_FRAME_TBL[sniper_type as usize];

    let y_pos = add_a_with_vert_scroll_to_enemy_y_pos(0x04, vertical_scroll, enemy_y_pos);
    let y_pos = if sniper_type == 1 { add_a_to_enemy_y_pos(0x05, y_pos) } else { y_pos };

    SniperRoutine00Result { animation_delay, frame, y_pos, routine_update: advance_enemy_routine(current_routine) }
}

/// `sniper_sprite_00` (`$8b3b`, 7 bytes) - regular/hiding sniper sprite
/// codes (types `0`/`1`), indexed by `ENEMY_FRAME`.
const SNIPER_SPRITE_00: [u8; 7] = [0x44, 0x45, 0x46, 0x43, 0x42, 0x41, 0x29];
/// `sniper_sprite_01` (`$8b42`, 7 bytes) - boss-screen sniper sprite
/// codes (type `2`), same indexing.
const SNIPER_SPRITE_01: [u8; 7] = [0x44, 0x45, 0x46, 0x2C, 0x42, 0x2D, 0x29];

/// The full result of one [`sniper_set_sprite`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperSetSpriteResult {
    pub sprites: u8,
    pub sprite_attr: u8,
    /// `ENEMY_VAR_3` after decrementing (only when it was already
    /// nonzero - "firing" gun-recoil counter).
    pub var_3: u8,
}

/// Native port of `sniper_set_sprite` (`$8b02`) - "set sprite and
/// attributes based on sniper type and firing angle".
pub fn sniper_set_sprite(sniper_type: u8, enemy_frame: u8, enemy_var_2: u8, enemy_var_3: u8) -> SniperSetSpriteResult {
    let table = if sniper_type < 2 { &SNIPER_SPRITE_00 } else { &SNIPER_SPRITE_01 };
    let sprites = table[enemy_frame as usize];

    let base_attr = if enemy_var_2 & 0x01 == 0 { 0x40 } else { 0x00 };
    let (var_3, sprite_attr) = if enemy_var_3 != 0 { (enemy_var_3 - 1, base_attr | 0x08) } else { (enemy_var_3, base_attr) };

    SniperSetSpriteResult { sprites, sprite_attr, var_3 }
}

/// `sniper_attack_delay_tbl` (`$89cc`, 3 bytes) - delay between attack
/// rounds, indexed by sniper type.
const SNIPER_ATTACK_DELAY_TBL: [u8; 3] = [0x40, 0x04, 0x10];
/// `sniper_bullet_attack_count_tbl` (`$89cf`, 3 bytes) - bullets fired per attack
/// round, same indexing.
const SNIPER_BULLET_ATTACK_COUNT_TBL: [u8; 3] = [0x03, 0x01, 0x03];

/// Which of [`sniper_routine_01`]'s 3 real paths reached "activated" -
/// see this module's doc comment for why the third variant is real,
/// valid control flow that's nonetheless never actually exercised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivatedFrom {
    /// Standing sniper (type `0`) - skipped the crouch-cycle animation
    /// entirely.
    Standing,
    /// Boss-screen sniper (type `2`), crouch-cycle just finished - a
    /// `-14`/`+1` Y/X nudge applied.
    BossNudge { y_pos: u8, x_pos: u8 },
    /// Crouching sniper (type `1`), crouch-cycle just finished, and the
    /// extra frame decrement landed on `0` - real ASM falls through into
    /// the same nudge code `BossNudge` uses.
    CrouchFallthroughNudge { y_pos: u8, x_pos: u8 },
    /// Crouching sniper (type `1`), crouch-cycle just finished, extra
    /// frame decrement left it nonzero - no position nudge.
    Crouching,
}

/// The real branch [`sniper_routine_01`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniperRoutine01Outcome {
    /// `ENEMY_ANIMATION_DELAY` hadn't reached `0` yet.
    Waiting { animation_delay: u8 },
    /// Crouching/hiding sniper (type `1`/`2`), still cycling its pop-up
    /// animation.
    CrouchCycling { animation_delay: u8, frame: u8 },
    /// Reached "activated": enables collision, sets score/collision
    /// code, and rolls the next attack round's delay/bullet count.
    Activated {
        from: ActivatedFrom,
        frame: u8,
        state_width: u8,
        /// Always `$30`.
        score_collision: u8,
        attack_delay: u8,
        var_4: u8,
        routine_update: EnemyRoutineUpdate,
    },
}

/// The full result of one [`sniper_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine01Result {
    pub sprite: SniperSetSpriteResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: SniperRoutine01Outcome,
}

fn sniper_activated(sniper_type: u8, from: ActivatedFrom, frame: u8, enemy_state_width: u8, current_routine: u8) -> SniperRoutine01Outcome {
    SniperRoutine01Outcome::Activated {
        from,
        frame,
        state_width: enable_enemy_collision(enemy_state_width),
        score_collision: 0x30,
        attack_delay: SNIPER_ATTACK_DELAY_TBL[sniper_type as usize],
        var_4: SNIPER_BULLET_ATTACK_COUNT_TBL[sniper_type as usize],
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// Native port of `sniper_routine_01` (`$8982`) - "cycle crouch
/// animation (if crouching sniper), enable collision (when standing
/// only for crouching snipers)". See this module's doc comment for the
/// 3 ways to reach "activated".
#[allow(clippy::too_many_arguments)]
pub fn sniper_routine_01(
    sniper_type: u8,
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    enemy_animation_delay: u8,
    enemy_state_width: u8,
    current_routine: u8,
) -> SniperRoutine01Result {
    let sprite = sniper_set_sprite(sniper_type, enemy_frame, enemy_var_2, enemy_var_3);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return SniperRoutine01Result { sprite, scroll, outcome: SniperRoutine01Outcome::Waiting { animation_delay: delay } };
    }

    let outcome = if sniper_type == 0 {
        sniper_activated(sniper_type, ActivatedFrom::Standing, enemy_frame, enemy_state_width, current_routine)
    } else {
        let new_frame = enemy_frame.wrapping_add(1);
        if new_frame < 3 {
            SniperRoutine01Outcome::CrouchCycling { animation_delay: 0x08, frame: new_frame }
        } else if sniper_type != 1 {
            let nudged_y = add_a_to_enemy_y_pos(0xF2, scroll.y_pos);
            let nudged_x = add_a_to_enemy_x_pos(0x01, scroll.x_pos);
            sniper_activated(sniper_type, ActivatedFrom::BossNudge { y_pos: nudged_y, x_pos: nudged_x }, new_frame, enemy_state_width, current_routine)
        } else {
            let decremented = new_frame.wrapping_sub(1);
            if decremented != 0 {
                sniper_activated(sniper_type, ActivatedFrom::Crouching, decremented, enemy_state_width, current_routine)
            } else {
                let nudged_y = add_a_to_enemy_y_pos(0xF2, scroll.y_pos);
                let nudged_x = add_a_to_enemy_x_pos(0x01, scroll.x_pos);
                sniper_activated(sniper_type, ActivatedFrom::CrouchFallthroughNudge { y_pos: nudged_y, x_pos: nudged_x }, decremented, enemy_state_width, current_routine)
            }
        }
    };

    SniperRoutine01Result { sprite, scroll, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_sniper_uses_type_0_row_and_no_extra_nudge() {
        let r = sniper_routine_00(0, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x01);
        assert_eq!(r.frame, 0x03);
        assert_eq!(r.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60));
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn crouching_sniper_gets_the_extra_5_pixel_nudge() {
        let r = sniper_routine_00(1, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x30);
        assert_eq!(r.frame, 0x00);
        let after_4 = add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60);
        assert_eq!(r.y_pos, add_a_to_enemy_y_pos(0x05, after_4));
    }

    #[test]
    fn boss_screen_sniper_uses_type_2_row_and_no_extra_nudge() {
        let r = sniper_routine_00(2, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x80);
        assert_eq!(r.frame, 0x00);
        assert_eq!(r.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60));
    }

    #[test]
    fn vertical_scroll_is_threaded_through_the_first_add() {
        let with_scroll = sniper_routine_00(0, 0x60, 0x08, 5);
        assert_eq!(with_scroll.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x08, 0x60));
    }

    #[test]
    fn set_sprite_uses_type_0_table_for_standing_and_crouching() {
        let standing = sniper_set_sprite(0, 3, 0x00, 0);
        let crouching = sniper_set_sprite(1, 3, 0x00, 0);
        assert_eq!(standing.sprites, SNIPER_SPRITE_00[3]);
        assert_eq!(crouching.sprites, SNIPER_SPRITE_00[3]);
    }

    #[test]
    fn set_sprite_uses_type_1_table_for_boss_screen() {
        let r = sniper_set_sprite(2, 3, 0x00, 0);
        assert_eq!(r.sprites, SNIPER_SPRITE_01[3]);
    }

    #[test]
    fn set_sprite_attr_reflects_firing_angle_bit_0() {
        let even = sniper_set_sprite(0, 0, 0x00, 0);
        let odd = sniper_set_sprite(0, 0, 0x01, 0);
        assert_eq!(even.sprite_attr, 0x40);
        assert_eq!(odd.sprite_attr, 0x00);
    }

    #[test]
    fn set_sprite_decrements_var_3_and_sets_the_recoil_bit_only_when_nonzero() {
        let firing = sniper_set_sprite(0, 0, 0x00, 0x03);
        assert_eq!(firing.var_3, 0x02);
        assert_eq!(firing.sprite_attr, 0x40 | 0x08);
        let idle = sniper_set_sprite(0, 0, 0x00, 0x00);
        assert_eq!(idle.var_3, 0x00);
        assert_eq!(idle.sprite_attr, 0x40);
    }

    #[test]
    fn routine_01_standing_activates_immediately_once_delay_elapses() {
        let r = sniper_routine_01(0, 0x03, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        match r.outcome {
            SniperRoutine01Outcome::Activated { from: ActivatedFrom::Standing, frame, state_width, score_collision, attack_delay, var_4, routine_update } => {
                assert_eq!(frame, 0x03);
                assert_eq!(state_width, enable_enemy_collision(0x00));
                assert_eq!(score_collision, 0x30);
                assert_eq!(attack_delay, SNIPER_ATTACK_DELAY_TBL[0]);
                assert_eq!(var_4, SNIPER_BULLET_ATTACK_COUNT_TBL[0]);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Activated/Standing, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_waits_while_delay_has_not_elapsed() {
        let r = sniper_routine_01(0, 0x03, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x05, 0x00, 5);
        assert_eq!(r.outcome, SniperRoutine01Outcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn routine_01_crouching_cycles_through_3_frames_then_activates_without_a_nudge() {
        // Starting frame 0, delay elapses on 3 successive calls.
        let step1 = sniper_routine_01(1, 0x00, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(step1.outcome, SniperRoutine01Outcome::CrouchCycling { animation_delay: 0x08, frame: 0x01 });
        let step2 = sniper_routine_01(1, 0x01, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(step2.outcome, SniperRoutine01Outcome::CrouchCycling { animation_delay: 0x08, frame: 0x02 });
        let step3 = sniper_routine_01(1, 0x02, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        match step3.outcome {
            SniperRoutine01Outcome::Activated { from: ActivatedFrom::Crouching, frame, .. } => assert_eq!(frame, 0x02),
            other => panic!("expected Activated/Crouching, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_boss_screen_applies_the_nudge_after_the_crouch_cycle() {
        let r = sniper_routine_01(2, 0x02, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        match r.outcome {
            SniperRoutine01Outcome::Activated { from: ActivatedFrom::BossNudge { y_pos, x_pos }, frame, .. } => {
                assert_eq!(frame, 0x03);
                assert_eq!(y_pos, add_a_to_enemy_y_pos(0xF2, r.scroll.y_pos));
                assert_eq!(x_pos, add_a_to_enemy_x_pos(0x01, r.scroll.x_pos));
            }
            other => panic!("expected Activated/BossNudge, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_scroll_matches_add_scroll_to_enemy_pos_directly() {
        let r = sniper_routine_01(0, 0x03, 0x00, 0x00, 0, 0x02, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
    }
}
