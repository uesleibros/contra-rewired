//! Native port of the turret man enemy's `_00`/`_01`/`_02` routine
//! family (`src/bank7.asm`, `$f0c9`-`$f116`) and its bullet's own `_00`/
//! `_01` family (`$f11f`-`$f136`).

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::generate_enemy_at_pos::{generate_enemy_at_pos, GeneratedEnemy};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};

/// The full result of one [`turret_man_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurretManRoutine00Result {
    pub sprite: u8,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `turret_man_routine_00` (`$f0c9`) - real ASM shifts
/// `ENEMY_ATTRIBUTES`'s low nibble into the high nibble (`asl` x4,
/// dropping the original high nibble) then adds `1` to get the initial
/// animation delay.
pub fn turret_man_routine_00(enemy_attributes: u8, current_routine: u8) -> TurretManRoutine00Result {
    let delay = (enemy_attributes << 4).wrapping_add(1);
    TurretManRoutine00Result { sprite: 0xBD, delayed_routine: set_enemy_delay_adv_routine(delay, current_routine) }
}

/// One [`turret_man_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurretManRoutine01Outcome {
    Waiting { animation_delay: u8 },
    RecoilStarted { sprite: u8, delayed_routine: DelayedRoutineUpdate },
}

/// The full result of one [`turret_man_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurretManRoutine01Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: TurretManRoutine01Outcome,
}

/// Native port of `turret_man_routine_01` (`$f0db`).
pub fn turret_man_routine_01(
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    animation_delay: u8,
    enemy_sprites: u8,
    current_routine: u8,
) -> TurretManRoutine01Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        TurretManRoutine01Outcome::Waiting { animation_delay: delay }
    } else {
        TurretManRoutine01Outcome::RecoilStarted {
            sprite: enemy_sprites.wrapping_add(1),
            delayed_routine: set_enemy_delay_adv_routine(0x05, current_routine),
        }
    };
    TurretManRoutine01Result { scroll, outcome }
}

/// One [`turret_man_routine_02`] call's outcome - real ASM: same
/// scroll-then-countdown shape as [`turret_man_routine_01`] (`jsr add_
/// scroll_to_enemy_pos`, `dec ENEMY_ANIMATION_DELAY,x`, exit if nonzero)
/// before firing. `bullet` is `None` when [`generate_enemy_at_pos`]
/// found no free slot - the rest of the routine's own state updates
/// still run either way (real ASM never checks the spawn's own success/
/// failure carry flag here). `routine_update`'s `a = #$02` sets *this*
/// turret man's own routine index to `2` (array position `1`, i.e. back
/// to `_01`'s own recoil-wait state - real off-by-one indexing), not a
/// self-jump to `_02`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurretManRoutine02Outcome {
    Waiting { animation_delay: u8 },
    Fired {
        sound: u8,
        bullet: Option<GeneratedEnemy>,
        animation_delay: u8,
        sprite: u8,
        routine_update: EnemyRoutineUpdate,
    },
}

/// The full result of one [`turret_man_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurretManRoutine02Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: TurretManRoutine02Outcome,
}

/// Native port of `turret_man_routine_02` (`$f0ec`).
#[allow(clippy::too_many_arguments)]
pub fn turret_man_routine_02(
    prg_rom: &[u8],
    enemy_routine_slots: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    enemy_attributes: u8,
    enemy_sprites: u8,
    animation_delay: u8,
    current_routine: u8,
) -> TurretManRoutine02Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        TurretManRoutine02Outcome::Waiting { animation_delay: delay }
    } else {
        let bullet = generate_enemy_at_pos(prg_rom, enemy_routine_slots, 0x0F, current_level, scroll.x_pos, scroll.y_pos, 0xF0, 0xFC);
        TurretManRoutine02Outcome::Fired {
            sound: 0x0C,
            bullet,
            animation_delay: (enemy_attributes << 4).wrapping_add(0x30),
            sprite: enemy_sprites.wrapping_sub(1),
            routine_update: set_enemy_routine_to_a(current_routine, 0x02),
        }
    };
    TurretManRoutine02Result { scroll, outcome }
}

/// The full result of one [`turret_man_bullet_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurretManBulletRoutine00Result {
    pub x_vel_fast: u8,
    pub x_vel_fract: u8,
    pub sprite: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `turret_man_bullet_routine_00` (`$f11f`) - sets a
/// fixed leftward X velocity and sprite, then advances.
pub fn turret_man_bullet_routine_00(current_routine: u8) -> TurretManBulletRoutine00Result {
    TurretManBulletRoutine00Result {
        x_vel_fast: 0xFD,
        x_vel_fract: 0x80,
        sprite: 0x1F,
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// One [`turret_man_bullet_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurretManBulletRoutine01Outcome {
    /// `ENEMY_X_POS >= $f0` - off the left edge, advance without moving.
    Advanced(EnemyRoutineUpdate),
    Position(UpdatedEnemyPos),
}

/// Native port of `turret_man_bullet_routine_01` (`$f131`).
#[allow(clippy::too_many_arguments)]
pub fn turret_man_bullet_routine_01(
    x_pos: u8,
    current_routine: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> TurretManBulletRoutine01Outcome {
    if x_pos >= 0xF0 {
        TurretManBulletRoutine01Outcome::Advanced(advance_enemy_routine(current_routine))
    } else {
        TurretManBulletRoutine01Outcome::Position(update_enemy_pos(
            level_scrolling_type,
            frame_scroll,
            x_pos,
            x_vel_accum,
            x_vel_fract,
            x_vel_fast,
            y_pos,
            y_vel_accum,
            y_vel_fract,
            y_vel_fast,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let shared_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 0x0f * 4;
        rom[shared_off..shared_off + 4].copy_from_slice(&[0x81, 0x0d, 0x01, 0x00]);
        rom
    }

    #[test]
    fn routine_00_shifts_low_nibble_to_high_and_adds_one() {
        let r = turret_man_routine_00(0x02, 3);
        assert_eq!(r.sprite, 0xBD);
        assert_eq!(r.delayed_routine.animation_delay, 0x21);
        assert_eq!(r.delayed_routine.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn routine_00_drops_the_original_high_nibble() {
        // 0xF2 << 4 wraps to 0x20, matching the real 6502 ASL x4's own
        // truncation (not a saturating/clamped shift).
        let r = turret_man_routine_00(0xF2, 3);
        assert_eq!(r.delayed_routine.animation_delay, 0x21);
    }

    #[test]
    fn routine_01_waits_while_delay_has_not_elapsed() {
        let r = turret_man_routine_01(0, 0x05, 0x50, 0x40, 0x03, 0x10, 3);
        assert_eq!(r.outcome, TurretManRoutine01Outcome::Waiting { animation_delay: 0x02 });
    }

    #[test]
    fn routine_01_starts_recoil_when_delay_elapses() {
        let r = turret_man_routine_01(0, 0x05, 0x50, 0x40, 0x01, 0x10, 3);
        match r.outcome {
            TurretManRoutine01Outcome::RecoilStarted { sprite, delayed_routine } => {
                assert_eq!(sprite, 0x11);
                assert_eq!(delayed_routine.animation_delay, 0x05);
                assert_eq!(delayed_routine.routine_update, advance_enemy_routine(3));
            }
            other => panic!("expected RecoilStarted, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_waits_while_delay_has_not_elapsed() {
        let rom = synthetic_prg_rom();
        let routine_slots = [0u8; ENEMY_SLOT_COUNT];
        let r = turret_man_routine_02(&rom, &routine_slots, 0, 0, 0x05, 0x50, 0x60, 0x02, 0x10, 0x03, 3);
        assert_eq!(r.outcome, TurretManRoutine02Outcome::Waiting { animation_delay: 0x02 });
    }

    #[test]
    fn routine_02_fires_bullet_relative_to_the_post_scroll_position() {
        let rom = synthetic_prg_rom();
        let routine_slots = [0u8; ENEMY_SLOT_COUNT];
        let r = turret_man_routine_02(&rom, &routine_slots, 0, 0, 0x05, 0x50, 0x60, 0x02, 0x10, 0x01, 3);
        assert_eq!(r.scroll.x_pos, 0x4B); // 0x50 - 0x05 scroll
        match r.outcome {
            TurretManRoutine02Outcome::Fired { sound, bullet, animation_delay, sprite, routine_update } => {
                assert_eq!(sound, 0x0C);
                let bullet = bullet.unwrap();
                assert_eq!(bullet.x_pos, 0x3B); // 0x4B (post-scroll) + 0xf0 wraps
                assert_eq!(bullet.y_pos, 0x5C); // 0x60 + 0xfc wraps
                assert_eq!(animation_delay, 0x50); // (0x02 << 4) + 0x30
                assert_eq!(sprite, 0x0F);
                assert_eq!(routine_update, set_enemy_routine_to_a(3, 0x02));
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_still_updates_state_when_no_slot_is_free() {
        let rom = synthetic_prg_rom();
        let routine_slots = [1u8; ENEMY_SLOT_COUNT];
        let r = turret_man_routine_02(&rom, &routine_slots, 0, 0, 0x05, 0x50, 0x60, 0x00, 0x10, 0x01, 3);
        match r.outcome {
            TurretManRoutine02Outcome::Fired { bullet, routine_update, .. } => {
                assert!(bullet.is_none());
                assert_eq!(routine_update, set_enemy_routine_to_a(3, 0x02));
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn bullet_routine_00_sets_fixed_leftward_velocity() {
        let r = turret_man_bullet_routine_00(3);
        assert_eq!(r.x_vel_fast, 0xFD);
        assert_eq!(r.x_vel_fract, 0x80);
        assert_eq!(r.sprite, 0x1F);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn bullet_routine_01_advances_without_moving_past_0xf0() {
        let r = turret_man_bullet_routine_01(0xF0, 3, 0, 0x02, 0, 0x80, 0xFD, 0x60, 0, 0, 0);
        assert_eq!(r, TurretManBulletRoutine01Outcome::Advanced(advance_enemy_routine(3)));
    }

    #[test]
    fn bullet_routine_01_updates_position_below_0xf0() {
        let r = turret_man_bullet_routine_01(0x80, 3, 0, 0x02, 0, 0x80, 0xFD, 0x60, 0, 0, 0);
        assert_eq!(r, TurretManBulletRoutine01Outcome::Position(update_enemy_pos(0, 0x02, 0x80, 0, 0x80, 0xFD, 0x60, 0, 0, 0)));
    }
}
