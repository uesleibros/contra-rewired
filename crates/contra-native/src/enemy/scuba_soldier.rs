//! Native port of the scuba diver enemy's `_00`/`_01`/`_02` routine
//! family (`src/bank7.asm`, `$f147`-`$f1c3`).

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_collision_flags::disable_enemy_collision;
use crate::enemy::enemy_routine_transition::{set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::generate_enemy_at_pos::{generate_enemy_at_pos, GeneratedEnemy};

/// Native port of `scuba_soldier_routine_00` (`$f147`) - real ASM: `lda
/// #$80; jmp set_anim_delay_adv_enemy_routine`.
pub fn scuba_soldier_routine_00(current_routine: u8) -> DelayedRoutineUpdate {
    set_enemy_delay_adv_routine(0x80, current_routine)
}

/// One [`scuba_soldier_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScubaSoldierRoutine01Outcome {
    /// Delay hasn't elapsed yet.
    Waiting { animation_delay: u8 },
    /// Delay elapsed but not yet at the real `$b8` attack height - waits
    /// another `$10` frames before checking again (vertical levels
    /// only; the snow field level's own scuba divers start already low
    /// enough that this branch never triggers for them).
    NotYetHighEnough { animation_delay: u8 },
    /// Delay elapsed at/past attack height - enables collision, sets the
    /// real `$10` attack delay, and advances to `_02`.
    Activated { attack_delay: u8, delayed_routine: DelayedRoutineUpdate },
}

/// The full result of one [`scuba_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScubaSoldierRoutine01Result {
    pub sprite: u8,
    /// Real ASM: bit 4 of the *pre-decrement* `ENEMY_ANIMATION_DELAY`
    /// (`asl` x4 into carry) toggles the gun-recoil sprite attribute
    /// flag every 16 frames while hiding, independent of the delay
    /// countdown itself.
    pub sprite_attr: u8,
    pub scroll: ScrolledEnemyPos,
    pub outcome: ScubaSoldierRoutine01Outcome,
}

/// Native port of `scuba_soldier_routine_01` (`$f14c`).
pub fn scuba_soldier_routine_01(
    animation_delay: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> ScubaSoldierRoutine01Result {
    let sprite_attr = if animation_delay & 0x10 != 0 { 0x00 } else { 0x08 };
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let delay = animation_delay.wrapping_sub(1);

    let outcome = if delay != 0 {
        ScubaSoldierRoutine01Outcome::Waiting { animation_delay: delay }
    } else if scroll.y_pos < 0xB8 {
        ScubaSoldierRoutine01Outcome::NotYetHighEnough { animation_delay: 0x10 }
    } else {
        ScubaSoldierRoutine01Outcome::Activated {
            attack_delay: 0x10,
            delayed_routine: set_enemy_delay_adv_routine(0x30, current_routine),
        }
    };

    ScubaSoldierRoutine01Result { sprite: 0x4B, sprite_attr, scroll, outcome }
}

/// One [`scuba_soldier_routine_02`] call's outcome - `var_1`/`sprite_
/// attr` (top-of-routine recoil-timer countdown) and `scroll` apply
/// regardless of which of these is reached, so they live on [`ScubaSoldierRoutine02Result`]
/// instead of being duplicated per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScubaSoldierRoutine02Outcome {
    /// Visible-duration timer still running, attack-delay timer hasn't
    /// elapsed either.
    Firing { animation_delay: u8, attack_delay: u8 },
    /// Visible-duration timer still running, attack-delay timer
    /// elapsed - fires a mortar shot (enemy type `$0b`) at relative
    /// position `($05, $e8)`, and resets `ENEMY_VAR_1` (recoil timer)
    /// to `$07`, overriding this call's own top-of-routine countdown.
    Fired { animation_delay: u8, mortar: Option<GeneratedEnemy> },
    /// Visible-duration timer elapsed - hides back underwater, disables
    /// collision, and returns to `_01`. Real ASM: the animation-delay
    /// store and the routine-index jump are two separate, unconditional
    /// operations here (`lda #$c0; sta ENEMY_ANIMATION_DELAY,x`, then
    /// `lda #$02; jmp set_enemy_routine_to_a`), *not* the combined `set_
    /// anim_delay_adv_enemy_routine` both other outcomes in this family
    /// use - `animation_delay` and `routine_update` are independent
    /// fields rather than a single [`DelayedRoutineUpdate`] for that
    /// reason.
    Submerging { animation_delay: u8, disabled_state_width: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`scuba_soldier_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScubaSoldierRoutine02Result {
    pub sprite: u8,
    /// `ENEMY_VAR_1` after the top-of-routine recoil countdown -
    /// overridden to `$07` by [`ScubaSoldierRoutine02Outcome::Fired`].
    pub var_1: u8,
    pub sprite_attr: u8,
    pub scroll: ScrolledEnemyPos,
    pub outcome: ScubaSoldierRoutine02Outcome,
}

/// Native port of `scuba_soldier_routine_02` (`$f183`) - real ASM: the
/// recoil (`ENEMY_VAR_1`) and visible-duration (`ENEMY_ANIMATION_DELAY`)
/// countdowns both run and get written back before the branch that
/// decides submerge-vs-fire-vs-wait, and `add_scroll_to_enemy_pos` runs
/// last regardless of which path is taken (real ASM: both
/// `@disable_and_dec_enemy_routine` and `@add_scroll_exit` end with the
/// same call).
#[allow(clippy::too_many_arguments)]
pub fn scuba_soldier_routine_02(
    prg_rom: &[u8],
    enemy_routine_slots: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    var_1: u8,
    animation_delay: u8,
    attack_delay: u8,
    state_width: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> ScubaSoldierRoutine02Result {
    let (mut var_1, sprite_attr) = if var_1 != 0 { (var_1.wrapping_sub(1), 0x08) } else { (0, 0x00) };
    let delay = animation_delay.wrapping_sub(1);

    let outcome = if delay == 0 {
        ScubaSoldierRoutine02Outcome::Submerging {
            animation_delay: 0xC0,
            disabled_state_width: disable_enemy_collision(state_width),
            routine_update: set_enemy_routine_to_a(current_routine, 0x02),
        }
    } else {
        let attack_delay = attack_delay.wrapping_sub(1);
        if attack_delay != 0 {
            ScubaSoldierRoutine02Outcome::Firing { animation_delay: delay, attack_delay }
        } else {
            var_1 = 0x07;
            let mortar = generate_enemy_at_pos(prg_rom, enemy_routine_slots, 0x0B, current_level, x_pos, y_pos, 0x05, 0xE8);
            ScubaSoldierRoutine02Outcome::Fired { animation_delay: delay, mortar }
        }
    };

    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    ScubaSoldierRoutine02Result { sprite: 0x4C, var_1, sprite_attr, scroll, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let shared_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 0x0b * 4;
        rom[shared_off..shared_off + 4].copy_from_slice(&[0x81, 0x0d, 0x01, 0x00]);
        rom
    }

    #[test]
    fn routine_00_sets_the_initial_hiding_delay() {
        let r = scuba_soldier_routine_00(3);
        assert_eq!(r, set_enemy_delay_adv_routine(0x80, 3));
    }

    #[test]
    fn routine_01_waits_while_delay_has_not_elapsed() {
        let r = scuba_soldier_routine_01(0x05, 0, 0x02, 0x50, 0x40, 3);
        assert_eq!(r.outcome, ScubaSoldierRoutine01Outcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn routine_01_flashes_gun_recoil_attr_on_bit_4_of_the_pre_decrement_delay() {
        let flashing = scuba_soldier_routine_01(0x10, 0, 0x02, 0x50, 0x40, 3);
        assert_eq!(flashing.sprite_attr, 0x00);
        let not_flashing = scuba_soldier_routine_01(0x05, 0, 0x02, 0x50, 0x40, 3);
        assert_eq!(not_flashing.sprite_attr, 0x08);
    }

    #[test]
    fn routine_01_waits_longer_when_not_yet_high_enough_vertical() {
        // vertical level (type 1): scroll adds to Y.
        let r = scuba_soldier_routine_01(0x01, 1, 0x05, 0x50, 0x50, 3);
        assert_eq!(r.scroll.y_pos, 0x55);
        assert_eq!(r.outcome, ScubaSoldierRoutine01Outcome::NotYetHighEnough { animation_delay: 0x10 });
    }

    #[test]
    fn routine_01_activates_at_or_past_attack_height() {
        let r = scuba_soldier_routine_01(0x01, 1, 0x05, 0x50, 0xB5, 3);
        assert_eq!(r.scroll.y_pos, 0xBA);
        match r.outcome {
            ScubaSoldierRoutine01Outcome::Activated { attack_delay, delayed_routine } => {
                assert_eq!(attack_delay, 0x10);
                assert_eq!(delayed_routine, set_enemy_delay_adv_routine(0x30, 3));
            }
            other => panic!("expected Activated, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_counts_down_recoil_and_visible_timers_while_firing() {
        let rom = synthetic_prg_rom();
        let routine_slots = [0u8; ENEMY_SLOT_COUNT];
        let r = scuba_soldier_routine_02(&rom, &routine_slots, 0, 0x03, 0x05, 0x05, 0, 0, 0x02, 0x50, 0x40, 3);
        assert_eq!(r.var_1, 0x02);
        assert_eq!(r.sprite_attr, 0x08);
        match r.outcome {
            ScubaSoldierRoutine02Outcome::Firing { animation_delay, attack_delay } => {
                assert_eq!(animation_delay, 0x04);
                assert_eq!(attack_delay, 0x04);
            }
            other => panic!("expected Firing, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_fires_mortar_when_attack_delay_elapses() {
        let rom = synthetic_prg_rom();
        let routine_slots = [0u8; ENEMY_SLOT_COUNT];
        let r = scuba_soldier_routine_02(&rom, &routine_slots, 0, 0x00, 0x05, 0x01, 0, 0, 0x02, 0x50, 0x40, 3);
        assert_eq!(r.var_1, 0x07); // overridden even though the top-of-routine countdown left it at 0
        match r.outcome {
            ScubaSoldierRoutine02Outcome::Fired { animation_delay, mortar } => {
                assert_eq!(animation_delay, 0x04);
                let mortar = mortar.unwrap();
                assert_eq!(mortar.x_pos, 0x55);
                assert_eq!(mortar.y_pos, 0x28); // 0x40 + 0xe8 wraps
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_submerges_when_visible_duration_elapses() {
        let rom = synthetic_prg_rom();
        let routine_slots = [0u8; ENEMY_SLOT_COUNT];
        let r = scuba_soldier_routine_02(&rom, &routine_slots, 0, 0x00, 0x01, 0x01, 0x00, 0, 0x02, 0x50, 0x40, 3);
        assert_eq!(r.sprite, 0x4C);
        match r.outcome {
            ScubaSoldierRoutine02Outcome::Submerging { animation_delay, disabled_state_width, routine_update } => {
                assert_eq!(animation_delay, 0xC0);
                assert_eq!(disabled_state_width, disable_enemy_collision(0x00));
                assert_eq!(routine_update, set_enemy_routine_to_a(3, 0x02));
            }
            other => panic!("expected Submerging, got {other:?}"),
        }
    }
}
