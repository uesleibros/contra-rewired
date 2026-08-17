//! Native port of Contra's generic "enemy just died" explosion-init
//! state, `enemy_routine_init_explosion` (`src/bank7.asm`, CPU `$e74b`-
//! `$e75d`) - a real, *shared* entry in nearly every enemy type's
//! routine table (the plain soldier's own `soldier_routine_ptr_tbl`
//! entry 6, among dozens of others): marks the enemy as destroyed,
//! optionally triggers the destruction sound, re-palettes its sprite,
//! and either removes it immediately (nothing left to show) or hides it
//! for one frame before the actual explosion animation
//! (`enemy_routine_explosion`, not yet ported) takes over.
//!
//! `play_sound` (`$c16b`) itself isn't ported here - it's a real bank-
//! switch wrapper around the sound engine (`jsr load_bank_1; jsr
//! init_sound_code_vars; jsr local_previous_1_bank`), not a pure RAM
//! transform like everything else in this crate. This port instead
//! returns *whether and which* sound code would be triggered
//! ([`EnemyRoutineInitExplosionResult::sound`]) as plain data, the same
//! way [`crate::enemy::create_enemy_bullet`] returns a bullet's fields rather
//! than performing the spawn itself - a caller integrating this into
//! live gameplay is responsible for actually invoking the sound engine.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::enemy::update_enemy_pos::{remove_enemy, RemovedEnemy};

/// `enemy_routine_init_explosion`'s real destruction sound code
/// (`sound_19`).
const EXPLOSION_SOUND: u8 = 0x19;

/// [`enemy_routine_init_explosion`]'s result once it decided the enemy
/// still had a sprite to hide (real ASM's `@continue` path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRoutineInitExplosionHidden {
    /// Always `$ff` - the real ASM's own "no visible animation frame"
    /// sentinel.
    pub enemy_frame: u8,
    /// Always `$01` - an invisible placeholder sprite code, not `$00`
    /// (which would itself mean "no sprite," the very condition this
    /// path exists to avoid re-triggering next frame).
    pub enemy_sprites: u8,
    pub scroll: ScrolledEnemyPos,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// The real branch [`enemy_routine_init_explosion`] takes: nothing left
/// to show (removed immediately) or hidden for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyRoutineInitExplosionOutcome {
    Removed(RemovedEnemy),
    Hidden(EnemyRoutineInitExplosionHidden),
}

/// The full result of one [`enemy_routine_init_explosion`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRoutineInitExplosionResult {
    /// `ENEMY_STATE_WIDTH` after unconditionally setting bits 0 and 7
    /// (real ASM comment: "set boss destroyed bits").
    pub state_width: u8,
    /// `Some($19)` if the *new* `state_width`'s bit 1 is set - real ASM
    /// tests the value it just stored, not the original input.
    pub sound: Option<u8>,
    /// `ENEMY_SPRITE_ATTR` after stripping the palette bits and forcing
    /// palette 2 (`& $fc | $06`).
    pub sprite_attr: u8,
    pub outcome: EnemyRoutineInitExplosionOutcome,
}

/// Native port of `enemy_routine_init_explosion` (`$e74b`).
#[allow(clippy::too_many_arguments)]
pub fn enemy_routine_init_explosion(
    enemy_state_width: u8,
    enemy_sprite_attr: u8,
    enemy_sprites: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    current_routine: u8,
) -> EnemyRoutineInitExplosionResult {
    let state_width = enemy_state_width | 0x81;
    let sound = if state_width & 0x02 != 0 { Some(EXPLOSION_SOUND) } else { None };
    let sprite_attr = (enemy_sprite_attr & 0xFC) | 0x06;

    let outcome = if enemy_sprites == 0 {
        EnemyRoutineInitExplosionOutcome::Removed(remove_enemy())
    } else {
        let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
        let delayed_routine = set_enemy_delay_adv_routine(0x01, current_routine);
        EnemyRoutineInitExplosionOutcome::Hidden(EnemyRoutineInitExplosionHidden {
            enemy_frame: 0xFF,
            enemy_sprites: 0x01,
            scroll,
            delayed_routine,
        })
    };

    EnemyRoutineInitExplosionResult { state_width, sound, sprite_attr, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_the_destroyed_bits_regardless_of_input() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.state_width, 0x81);
    }

    #[test]
    fn plays_the_explosion_sound_when_bit_1_of_the_new_state_width_is_set() {
        // input state_width already has bit 1 set -> survives the |=0x81.
        let r = enemy_routine_init_explosion(0x02, 0x00, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.state_width, 0x83);
        assert_eq!(r.sound, Some(0x19));
    }

    #[test]
    fn no_sound_when_bit_1_is_clear() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.sound, None);
    }

    #[test]
    fn overrides_sprite_attr_palette_to_2_preserving_other_bits() {
        let r = enemy_routine_init_explosion(0x00, 0b0011_0101, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.sprite_attr, 0b0011_0110);
    }

    #[test]
    fn removes_immediately_when_no_sprite_is_present() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, EnemyRoutineInitExplosionOutcome::Removed(remove_enemy()));
    }

    #[test]
    fn hides_and_schedules_the_next_frame_when_a_sprite_is_present() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x05, 0, 0x02, 0x50, 0x60, 3);
        match r.outcome {
            EnemyRoutineInitExplosionOutcome::Hidden(h) => {
                assert_eq!(h.enemy_frame, 0xFF);
                assert_eq!(h.enemy_sprites, 0x01);
                assert_eq!(h.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
                assert_eq!(h.delayed_routine, set_enemy_delay_adv_routine(0x01, 3));
            }
            other => panic!("expected Hidden, got {other:?}"),
        }
    }
}
