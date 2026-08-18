//! Native port of the mortar shot enemy's `_00`/`_01`/`_02`/`_03`
//! routine family (`src/bank7.asm`, `$f1d6`-`$f2a6`, plus `_03` itself
//! at the real, shared `$e752` in the fixed bank - reused by ice
//! grenades too, per the real ASM comment).

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_position_utils::add_10_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::{initialize_enemy, InitializedEnemy};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::update_enemy_pos::{remove_enemy, update_enemy_pos, RemovedEnemy, UpdatedEnemyPos};
use crate::physics::collision::{add_y_to_y_pos_get_bg_collision, CollisionCode, BG_COLLISION_DATA_LEN};

/// `mortar_shot_velocity_tbl` (`$f200`, 32 bytes / 8 `(y_fract, y_fast,
/// x_fract, x_fast)` entries) - entry `0` is the default main shot
/// (straight up fast), `1`-`3` the 3 split-mortar directions (straight/
/// right/left), `4`-`7` the hangar zone boss's own aimed launch
/// directions (indexed by `ENEMY_VAR_1 + 3`, `ENEMY_VAR_1` itself being
/// `1`-`4`).
const MORTAR_SHOT_VELOCITY_TBL: [(u8, u8, u8, u8); 8] = [
    (0x00, 0xFB, 0x00, 0x00),
    (0x00, 0xFE, 0x00, 0x00),
    (0x40, 0xFE, 0x90, 0x00),
    (0x40, 0xFE, 0x70, 0xFF),
    (0x00, 0xFB, 0xC0, 0xFF),
    (0x00, 0xFB, 0x80, 0xFF),
    (0x00, 0xFB, 0x40, 0xFF),
    (0x00, 0xFB, 0x00, 0xFF),
];

/// The full result of one [`mortar_shot_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MortarShotRoutine00Result {
    pub state_width: u8,
    pub sprite: u8,
    pub sprite_attr: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `mortar_shot_routine_00` (`$f1d6`) - `ENEMY_ATTRIBUTES
/// != 0` selects a split-mortar velocity directly (values `1`-`3`);
/// `== 0` is the main/aimed shot, using `ENEMY_VAR_1` (`0` = default,
/// `1`-`4` = hangar zone aim direction, offset `+3`) instead.
pub fn mortar_shot_routine_00(enemy_attributes: u8, enemy_var_1: u8, current_routine: u8) -> MortarShotRoutine00Result {
    let state_width = if enemy_attributes == 0 { 0x8A } else { 0x80 };

    let index = if enemy_attributes != 0 {
        enemy_attributes
    } else if enemy_var_1 == 0 {
        0
    } else {
        enemy_var_1.wrapping_add(3)
    };
    let (y_vel_fract, y_vel_fast, x_vel_fract, x_vel_fast) = MORTAR_SHOT_VELOCITY_TBL[index as usize];

    MortarShotRoutine00Result {
        state_width,
        sprite: 0x20,
        sprite_attr: 0x06,
        y_vel_fract,
        y_vel_fast,
        x_vel_fract,
        x_vel_fast,
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// One [`mortar_shot_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MortarShotRoutine01Outcome {
    /// Main shot, still rising and hasn't reached the `$30` divide
    /// height yet.
    StillRising,
    /// Main shot reached its apex (or is already falling) - advances to
    /// `_02` to split into 3 mortars.
    Advanced(EnemyRoutineUpdate),
    /// Split mortar, still rising.
    SplitStillRising,
    /// Split mortar falling, but still above the closest player.
    SplitAboveClosestPlayer,
    /// Split mortar falling, at/below the closest player's height, but
    /// no background collision yet.
    SplitNoBgCollision,
    /// Split mortar hit the background - plays the collision sound and
    /// jumps to `_03`.
    SplitCollided { sound: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`mortar_shot_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MortarShotRoutine01Result {
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: MortarShotRoutine01Outcome,
}

/// Native port of `mortar_shot_routine_01` (`$f237`) - real ASM applies
/// gravity and integrates position via a plain `jsr` (not a tail `jmp`),
/// so [`update_enemy_pos`]'s own removal (off-screen) is just a side
/// effect that doesn't stop the rest of this routine from running - if
/// it happened, any routine-index update below must see the
/// already-zeroed routine, not the stale `current_routine` input (same
/// real quirk this crate already caught in `enemy_bullet_routine_01`).
#[allow(clippy::too_many_arguments)]
pub fn mortar_shot_routine_01(
    enemy_attributes: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    sprite_x_pos: [u8; 2],
    sprite_y_pos: [u8; 2],
    player_state: [u8; 2],
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    current_routine: u8,
) -> MortarShotRoutine01Result {
    let (y_vel_fract, y_vel_fast) = add_10_to_enemy_y_fract_vel(y_vel_fract, y_vel_fast);
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);
    let effective_routine = if position.removed.is_some() { 0 } else { current_routine };
    let falling = (y_vel_fast as i8) >= 0;

    let outcome = if enemy_attributes == 0 {
        if falling || position.y.pos < 0x30 {
            MortarShotRoutine01Outcome::Advanced(advance_enemy_routine(effective_routine))
        } else {
            MortarShotRoutine01Outcome::StillRising
        }
    } else if !falling {
        MortarShotRoutine01Outcome::SplitStillRising
    } else {
        let closest = player_enemy_x_dist(sprite_x_pos, position.x.pos, player_state);
        if position.y.pos < sprite_y_pos[closest.player_index as usize] {
            MortarShotRoutine01Outcome::SplitAboveClosestPlayer
        } else {
            let collision = add_y_to_y_pos_get_bg_collision(0, position.x.pos, position.y.pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
            if collision == CollisionCode::Empty {
                MortarShotRoutine01Outcome::SplitNoBgCollision
            } else {
                MortarShotRoutine01Outcome::SplitCollided { sound: 0x24, routine_update: set_enemy_routine_to_a(effective_routine, 0x07) }
            }
        }
    };

    MortarShotRoutine01Result { y_vel_fract, y_vel_fast, position, outcome }
}

/// One spawned split mortar from a [`mortar_shot_routine_02`] call -
/// real ASM: `ENEMY_TYPE = $0b`, `ENEMY_X_POS`/`ENEMY_Y_POS` copied
/// unchanged from the falling mortar's own (already-updated) position,
/// `ENEMY_ATTRIBUTES` set to the split-direction index (`3`, `2`, then
/// `1`, matching `MORTAR_SHOT_VELOCITY_TBL`'s own entries `1`-`3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MortarShotSplit {
    pub slot: u8,
    pub initialized: InitializedEnemy,
    pub x_pos: u8,
    pub y_pos: u8,
    pub attributes: u8,
}

/// The full result of one [`mortar_shot_routine_02`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MortarShotRoutine02Result {
    pub position: UpdatedEnemyPos,
    /// Up to 3 split mortars, in the real spawn order (`3`, `2`, `1`) -
    /// shorter than 3 only if [`find_next_enemy_slot`] ran out of free
    /// slots partway through (real ASM: `bne @advance_enemy_routine`
    /// stops the loop early, same effect [`crate::enemy::generate_enemy_at_pos::generate_enemy_at_pos`]'s
    /// own `None` models elsewhere).
    pub splits: Vec<MortarShotSplit>,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `mortar_shot_routine_02` (`$f26e`) - unlike other
/// spawn paths in this crate, this one doesn't go through [`crate::
/// enemy::generate_enemy_at_pos::generate_enemy_at_pos`] (real ASM
/// inlines its own `find_next_enemy_slot`/`initialize_enemy` loop
/// instead, claiming up to 3 slots against the *same* evolving
/// `ENEMY_ROUTINE` snapshot before any of this frame's other spawns can
/// see them - `initialize_enemy` writing `ENEMY_ROUTINE = 1` is what
/// keeps the loop from picking the same slot twice).
#[allow(clippy::too_many_arguments)]
pub fn mortar_shot_routine_02(
    prg_rom: &[u8],
    mut enemy_routine_slots: [u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    current_routine: u8,
) -> MortarShotRoutine02Result {
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);
    let effective_routine = if position.removed.is_some() { 0 } else { current_routine };

    let mut splits = Vec::new();
    for attributes in (1..=3).rev() {
        let Some(slot) = find_next_enemy_slot(&enemy_routine_slots) else { break };
        let initialized = initialize_enemy(prg_rom, 0x0B, current_level);
        enemy_routine_slots[slot as usize] = initialized.routine;
        splits.push(MortarShotSplit { slot, initialized, x_pos: position.x.pos, y_pos: position.y.pos, attributes });
    }

    MortarShotRoutine02Result { position, splits, routine_update: advance_enemy_routine(effective_routine) }
}

/// [`mortar_shot_routine_03`]'s own "still had a sprite to hide" branch,
/// the same real shared tail [`crate::enemy::enemy_explosion::
/// enemy_routine_init_explosion`]'s own `Hidden` outcome falls into
/// (`explosion_sound_hide_enemy`, `$e752`), reproduced locally rather
/// than cross-called since the two entry points compute their own,
/// different incoming `ENEMY_STATE_WIDTH` value first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MortarShotRoutine03Hidden {
    pub enemy_frame: u8,
    pub enemy_sprites: u8,
    pub scroll: ScrolledEnemyPos,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// The real branch [`mortar_shot_routine_03`] takes: nothing left to
/// show (removed immediately) or hidden for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MortarShotRoutine03Outcome {
    Removed(RemovedEnemy),
    Hidden(MortarShotRoutine03Hidden),
}

/// The full result of one [`mortar_shot_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MortarShotRoutine03Result {
    /// Always `$0d` - real ASM: `ENEMY_SCORE_COLLISION,x = #$0d`
    /// unconditionally.
    pub score_collision: u8,
    /// `ENEMY_STATE_WIDTH` after `(state_width & $be) | $80`.
    pub state_width: u8,
    /// `Some($19)` when the *new* `state_width`'s bit 1 is set.
    pub sound: Option<u8>,
    pub sprite_attr: u8,
    pub outcome: MortarShotRoutine03Outcome,
}

/// Native port of `mortar_shot_routine_03` (`$e752`, fixed bank - also
/// reused by ice grenades per the real ASM comment).
#[allow(clippy::too_many_arguments)]
pub fn mortar_shot_routine_03(
    enemy_state_width: u8,
    enemy_sprite_attr: u8,
    enemy_sprites: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    current_routine: u8,
) -> MortarShotRoutine03Result {
    let state_width = (enemy_state_width & 0xBE) | 0x80;
    let sound = if state_width & 0x02 != 0 { Some(0x19) } else { None };
    let sprite_attr = (enemy_sprite_attr & 0xFC) | 0x06;

    let outcome = if enemy_sprites == 0 {
        MortarShotRoutine03Outcome::Removed(remove_enemy())
    } else {
        let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
        MortarShotRoutine03Outcome::Hidden(MortarShotRoutine03Hidden {
            enemy_frame: 0xFF,
            enemy_sprites: 0x01,
            scroll,
            delayed_routine: set_enemy_delay_adv_routine(0x01, current_routine),
        })
    };

    MortarShotRoutine03Result { score_collision: 0x0D, state_width, sound, sprite_attr, outcome }
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

    fn no_scroll_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0u8; BG_COLLISION_DATA_LEN]
    }

    #[test]
    fn routine_00_split_mortar_indexes_the_table_directly_by_attributes() {
        let r = mortar_shot_routine_00(0x02, 0x00, 3);
        assert_eq!(r.state_width, 0x80);
        assert_eq!((r.y_vel_fract, r.y_vel_fast, r.x_vel_fract, r.x_vel_fast), MORTAR_SHOT_VELOCITY_TBL[2]);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn routine_00_default_main_shot_uses_index_0() {
        let r = mortar_shot_routine_00(0x00, 0x00, 3);
        assert_eq!(r.state_width, 0x8A);
        assert_eq!((r.y_vel_fract, r.y_vel_fast, r.x_vel_fract, r.x_vel_fast), MORTAR_SHOT_VELOCITY_TBL[0]);
    }

    #[test]
    fn routine_00_hangar_aim_direction_offsets_by_3() {
        let r = mortar_shot_routine_00(0x00, 0x02, 3);
        assert_eq!((r.y_vel_fract, r.y_vel_fast, r.x_vel_fract, r.x_vel_fast), MORTAR_SHOT_VELOCITY_TBL[5]);
    }

    #[test]
    fn routine_01_main_shot_still_rising_below_apex_height() {
        // y_vel_fast negative (rising) and y_pos still >= 0x30.
        let r = mortar_shot_routine_01(0x00, 0, 0x02, 0x50, 0, 0, 0, 0x50, 0, 0, 0xFA, [0, 0], [0, 0], [0, 0], 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(r.outcome, MortarShotRoutine01Outcome::StillRising);
    }

    #[test]
    fn routine_01_main_shot_advances_once_falling() {
        let r = mortar_shot_routine_01(0x00, 0, 0x02, 0x50, 0, 0, 0, 0x50, 0, 0, 0x01, [0, 0], [0, 0], [0, 0], 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(r.outcome, MortarShotRoutine01Outcome::Advanced(advance_enemy_routine(3)));
    }

    #[test]
    fn routine_01_split_still_rising_exits() {
        let r = mortar_shot_routine_01(0x02, 0, 0x02, 0x50, 0, 0, 0, 0x50, 0, 0, 0xFA, [0, 0], [0, 0], [0, 0], 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(r.outcome, MortarShotRoutine01Outcome::SplitStillRising);
    }

    #[test]
    fn routine_01_split_falling_above_closest_player() {
        // player 0 is closer (distance 10 vs 200) and sits below the mortar (higher Y value).
        let r = mortar_shot_routine_01(0x02, 0, 0x00, 0x50, 0, 0, 0, 0x20, 0, 0, 0x01, [0x40, 0x00], [0x90, 0x00], [1, 0], 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(r.outcome, MortarShotRoutine01Outcome::SplitAboveClosestPlayer);
    }

    #[test]
    fn routine_01_split_falling_no_bg_collision_below_player() {
        let r = mortar_shot_routine_01(0x02, 0, 0x00, 0x50, 0, 0, 0, 0x90, 0, 0, 0x01, [0x40, 0x00], [0x40, 0x00], [1, 0], 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(r.outcome, MortarShotRoutine01Outcome::SplitNoBgCollision);
    }

    #[test]
    fn routine_02_spawns_up_to_3_splits_at_the_updated_position() {
        let rom = synthetic_prg_rom();
        let slots = [0u8; ENEMY_SLOT_COUNT];
        let r = mortar_shot_routine_02(&rom, slots, 0, 0, 0x02, 0x50, 0, 0, 0, 0x50, 0, 0, 0x01, 3);
        assert_eq!(r.splits.len(), 3);
        assert_eq!(r.splits[0].attributes, 3);
        assert_eq!(r.splits[1].attributes, 2);
        assert_eq!(r.splits[2].attributes, 1);
        // 3 distinct slots claimed, highest-first.
        assert_eq!(r.splits[0].slot, 15);
        assert_eq!(r.splits[1].slot, 14);
        assert_eq!(r.splits[2].slot, 13);
        assert_eq!(r.splits[0].x_pos, r.position.x.pos);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn routine_02_stops_early_when_slots_run_out() {
        let rom = synthetic_prg_rom();
        let mut slots = [1u8; ENEMY_SLOT_COUNT];
        slots[0] = 0; // exactly one free slot
        let r = mortar_shot_routine_02(&rom, slots, 0, 0, 0x02, 0x50, 0, 0, 0, 0x50, 0, 0, 0x01, 3);
        assert_eq!(r.splits.len(), 1);
        assert_eq!(r.splits[0].slot, 0);
    }

    #[test]
    fn routine_03_removes_immediately_when_no_sprite() {
        let r = mortar_shot_routine_03(0x00, 0x00, 0x00, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.score_collision, 0x0D);
        assert_eq!(r.outcome, MortarShotRoutine03Outcome::Removed(remove_enemy()));
    }

    #[test]
    fn routine_03_hides_and_delays_when_sprite_present() {
        let r = mortar_shot_routine_03(0x00, 0x00, 0x20, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.state_width, 0x80);
        match r.outcome {
            MortarShotRoutine03Outcome::Hidden(h) => {
                assert_eq!(h.enemy_frame, 0xFF);
                assert_eq!(h.enemy_sprites, 0x01);
                assert_eq!(h.delayed_routine, set_enemy_delay_adv_routine(0x01, 3));
            }
            other => panic!("expected Hidden, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_plays_sound_when_new_state_width_bit_1_set() {
        // bit 1 (0x02) present in the input survives the (& 0xbe) mask.
        let r = mortar_shot_routine_03(0x02, 0x00, 0x20, 0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.sound, Some(0x19));
    }
}
