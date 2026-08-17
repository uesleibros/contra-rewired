//! Native port of the "jumping soldier" enemy type's ($16) own `_00`/
//! `_01`/`_04` table entries (`src/bank0.asm`, `$9380`/`$93a5`/`$9437`) -
//! the other 3 entries of `jumping_soldier_routine_ptr_tbl` this type
//! shares with the rest of the indoor family are already ported in
//! [`crate::enemy::indoor_soldier`]/[`crate::enemy::enemy_explosion`].
//!
//! ## The "red" jumping soldier
//!
//! `ENEMY_ATTRIBUTES` bit 1 marks a jumping soldier as the level's
//! special "red" one (drops a weapon item on death - see [`jumping_
//! soldier_routine_04`]). [`jumping_soldier_routine_00`] only lets the
//! *first* jumping soldier spawned after `INDOOR_ENEMY_ATTACK_COUNT` has
//! advanced past its first round actually keep that bit - every other
//! candidate (round 0, or a red one already created this screen) gets it
//! silently cleared. A red jumping soldier also never fires bullets in
//! [`jumping_soldier_routine_01`] (real ASM: bit 1 set skips the firing
//! check unconditionally, real comment gives no reason - ported as-is).
//!
//! ## `jumping_soldier_routine_04`'s double shift before `play_
//! explosion_sound` reads `ENEMY_ATTRIBUTES` again
//!
//! Real ASM: `lsr ENEMY_ATTRIBUTES,x; lsr ENEMY_ATTRIBUTES,x` (an
//! in-place read-modify-write, twice) runs *before* the tail-jump into
//! [`crate::enemy::enemy_explosion::play_explosion_sound`], which itself
//! reads `ENEMY_ATTRIBUTES,x` again and masks it to the low 3 bits for
//! the weapon item type - so the real weapon type ends up `(original_
//! attributes >> 2) & 7`, not `original_attributes & 7`. This port
//! threads that already-shifted value through explicitly rather than
//! re-deriving it inside `play_explosion_sound` itself.

use crate::enemy::create_enemy_bullet::{aim_and_create_enemy_bullet, CreatedBullet};
use crate::enemy::enemy_explosion::{play_explosion_sound, PlayExplosionSoundResult};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::indoor_soldier::{apply_enemy_velocity_set_bg_priority, init_indoor_enemy_pos_and_vel, ApplyEnemyVelocityResult, InitIndoorEnemyPosAndVelResult};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;

/// `ENEMY_ATTRIBUTES` bit that marks a jumping soldier as the level's
/// "red" one.
const RED_SOLDIER_BIT: u8 = 0x02;

/// The full result of one [`jumping_soldier_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpingSoldierRoutine00Result {
    /// `ENEMY_ATTRIBUTES` after the red-soldier bit is possibly cleared.
    pub attributes: u8,
    /// `Some(1)` only on the one call that actually claims the level's
    /// red jumping soldier slot (real ASM: `sta INDOOR_RED_SOLDIER_
    /// CREATED`) - `None` otherwise (nothing written).
    pub indoor_red_soldier_created: Option<u8>,
    pub init: InitIndoorEnemyPosAndVelResult,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `jumping_soldier_routine_00` (`$9380`) - "see if red
/// soldier, if so mark flag, advance routine". See this module's doc
/// comment for the red-soldier claiming rule. Always initializes with
/// [`init_indoor_enemy_pos_and_vel`]'s logical index `1` (real ASM:
/// `ldy #$02`, a raw byte offset into `indoor_soldier_x_velocity_tbl`'s
/// 2-byte entries - `2 / 2 = 1`, the table's "jumping soldier" row).
pub fn jumping_soldier_routine_00(
    enemy_attributes: u8,
    indoor_red_soldier_created: u8,
    indoor_enemy_attack_count: u8,
    current_routine: u8,
) -> JumpingSoldierRoutine00Result {
    let wants_red = enemy_attributes & RED_SOLDIER_BIT != 0;

    let (attributes, indoor_red_soldier_created) = if !wants_red {
        (enemy_attributes, None)
    } else if indoor_red_soldier_created != 0 || indoor_enemy_attack_count == 0 {
        (enemy_attributes & !RED_SOLDIER_BIT, None)
    } else {
        (enemy_attributes, Some(1u8))
    };

    let init = init_indoor_enemy_pos_and_vel(1, attributes);
    let routine_update = advance_enemy_routine(current_routine);

    JumpingSoldierRoutine00Result { attributes, indoor_red_soldier_created, init, routine_update }
}

/// `jumping_soldier_y_vel_tbl` (`$9423`, 20 signed bytes) - per-frame Y
/// position offset through a jump arc (rises, levels off, falls),
/// indexed by `ENEMY_VAR_1` (`0..20`, wrapping back to `0` once the jump
/// finishes).
const JUMPING_SOLDIER_Y_VEL_TBL: [u8; 20] = [
    0xFD, 0xFD, 0xFE, 0xFE, 0xFE, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x02, 0x02, 0x02, 0x03, 0x03,
];

/// [`jumping_soldier_routine_01`]'s result once it decided to apply this
/// frame's jump Y offset (real ASM's `@apply_y_vel`, only reached when
/// `ENEMY_ANIMATION_DELAY` is already `0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpingSoldierJumpResult {
    pub velocity: ApplyEnemyVelocityResult,
    pub y_pos: u8,
    pub var_1: u8,
    /// `Some($10)` only on the call whose incremented `var_1` reached the
    /// table's length (`$14`) - the jump sequence just finished.
    pub animation_delay: Option<u8>,
}

/// The real branch [`jumping_soldier_routine_01`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpingSoldierRoutine01Outcome {
    /// `ENEMY_ANIMATION_DELAY` was already nonzero and, after
    /// decrementing, either this is a red soldier (never fires) or the
    /// decremented delay wasn't exactly `$08` (the one frame a shot is
    /// attempted).
    Waiting { animation_delay: u8 },
    /// Decremented delay hit exactly `$08` and this isn't a red soldier -
    /// attempts to fire at whichever player is closer.
    Fired { animation_delay: u8, bullet: Option<CreatedBullet> },
    /// `ENEMY_ANIMATION_DELAY` was already `0` - applies this frame's
    /// jump arc offset instead of the firing logic above.
    Jumping(JumpingSoldierJumpResult),
}

/// The full result of one [`jumping_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JumpingSoldierRoutine01Result {
    pub sprites: u8,
    /// The sprite attribute computed *before* the branch below (real
    /// ASM's `@continue` block) - [`JumpingSoldierRoutine01Outcome::
    /// Jumping`]'s own `velocity.outcome` may still overwrite this again
    /// afterward (same "later write wins" real instruction order as
    /// `indoor_soldier_routine_01`).
    pub sprite_attr: u8,
    pub outcome: JumpingSoldierRoutine01Outcome,
}

/// Native port of `jumping_soldier_routine_01` (`$93a5`) - "set sprite,
/// and perform jump animation". See this module's doc comment for the
/// red-soldier firing exception.
#[allow(clippy::too_many_arguments)]
pub fn jumping_soldier_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_animation_delay: u8,
    enemy_attributes: u8,
    enemy_sprite_attr: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    x_pos: u8,
    y_pos: u8,
    var_1: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
) -> JumpingSoldierRoutine01Result {
    let sprites = if enemy_animation_delay == 0 {
        0x97
    } else if enemy_animation_delay < 0x04 {
        0x93
    } else {
        0x98
    };

    let is_red = enemy_attributes & RED_SOLDIER_BIT != 0;
    let red_palette: u8 = if is_red { 0x05 } else { 0x00 };

    let moving_left = (x_vel_fast as i8) < 0;
    let flipped = if moving_left { enemy_sprite_attr | 0x40 } else { enemy_sprite_attr & 0xBF };
    let sprite_attr = (flipped & 0xF8) | red_palette;

    let outcome = if enemy_animation_delay == 0 {
        let velocity = apply_enemy_velocity_set_bg_priority(x_vel_accum, x_vel_fract, x_vel_fast, x_pos, sprite_attr);
        let offset = JUMPING_SOLDIER_Y_VEL_TBL[var_1 as usize];
        let new_y_pos = y_pos.wrapping_add(offset);
        let advanced_var_1 = var_1.wrapping_add(1);
        let (final_var_1, animation_delay) =
            if advanced_var_1 >= 0x14 { (0, Some(0x10)) } else { (advanced_var_1, None) };
        JumpingSoldierRoutine01Outcome::Jumping(JumpingSoldierJumpResult { velocity, y_pos: new_y_pos, var_1: final_var_1, animation_delay })
    } else {
        let animation_delay = enemy_animation_delay.wrapping_sub(1);
        if is_red || animation_delay != 0x08 {
            JumpingSoldierRoutine01Outcome::Waiting { animation_delay }
        } else {
            let closest = player_enemy_x_dist(sprite_x_pos, x_pos, player_state);
            let bullet = aim_and_create_enemy_bullet(
                prg_rom,
                enemy_routine,
                current_level,
                enemy_attack_flag,
                0x60,
                4,
                y_pos,
                x_pos,
                closest.player_index,
                0,
                0,
                player_state,
                sprite_y_pos,
                sprite_x_pos,
                level_location_type,
            );
            JumpingSoldierRoutine01Outcome::Fired { animation_delay, bullet }
        }
    };

    JumpingSoldierRoutine01Result { sprites, sprite_attr, outcome }
}

/// The real branch [`jumping_soldier_routine_04`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpingSoldierRoutine04Outcome {
    /// Not a red soldier, outside the `$64..$9c` X range, or `ENEMY_
    /// ATTRIBUTES` bit 7 was set (real comment: "not sure when this
    /// happens" - ported as-is) - just advances to the next routine.
    AdvancedOnly(EnemyRoutineUpdate),
    /// Red soldier, in range, bit 7 clear - explodes and drops a weapon
    /// item. See this module's doc comment for why `play_result.
    /// attributes` reflects the already-shifted value, not a fresh `&7`
    /// of the original.
    ExplodedAndDroppedWeapon {
        /// `ENEMY_ATTRIBUTES` after the real double `lsr` (`>> 2`).
        attributes_after_shift: u8,
        play_result: PlayExplosionSoundResult,
    },
}

/// Native port of `jumping_soldier_routine_04` (`$9437`) - "soldier
/// destroyed, if red soldier play explosion and create weapon item".
pub fn jumping_soldier_routine_04(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attributes: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    current_routine: u8,
) -> JumpingSoldierRoutine04Outcome {
    let is_red = enemy_attributes & RED_SOLDIER_BIT != 0;
    let in_range = enemy_x_pos >= 0x64 && enemy_x_pos < 0x9C;
    let bit7_clear = enemy_attributes & 0x80 == 0;

    if !is_red || !in_range || !bit7_clear {
        return JumpingSoldierRoutine04Outcome::AdvancedOnly(advance_enemy_routine(current_routine));
    }

    let attributes_after_shift = enemy_attributes >> 2;
    let play_result = play_explosion_sound(prg_rom, enemy_routine, current_level, enemy_x_pos, enemy_y_pos, attributes_after_shift);

    JumpingSoldierRoutine04Outcome::ExplodedAndDroppedWeapon { attributes_after_shift, play_result }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Same shape as `create_enemy_bullet`'s own synthetic-ROM test
    /// fixture: a shared property-table pointer with recognizable
    /// records at enemy_type=1's (bullets', used by `routine_01`'s own
    /// firing test) and enemy_type=2's (`ENEMY_TYPE_EXPLOSION_SENSOR`,
    /// used by `routine_04`'s `play_explosion_sound` chain) offsets.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let bullet_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 1 * 4;
        rom[bullet_off..bullet_off + 4].copy_from_slice(&[0x80, 0x00, 0x01, 0x00]);
        let explosion_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 0x02 * 4;
        rom[explosion_off..explosion_off + 4].copy_from_slice(&[0x80, 0x00, 0x05, 0x00]);
        rom
    }

    #[test]
    fn routine_00_non_red_passes_through_unchanged() {
        let r = jumping_soldier_routine_00(0x00, 0, 5, 3);
        assert_eq!(r.attributes, 0x00);
        assert_eq!(r.indoor_red_soldier_created, None);
        assert_eq!(r.init, init_indoor_enemy_pos_and_vel(1, 0x00));
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn routine_00_wants_red_but_round_zero_gets_demoted() {
        let r = jumping_soldier_routine_00(0x02, 0, 0, 3);
        assert_eq!(r.attributes, 0x00);
        assert_eq!(r.indoor_red_soldier_created, None);
    }

    #[test]
    fn routine_00_wants_red_but_already_created_gets_demoted() {
        let r = jumping_soldier_routine_00(0x02, 1, 5, 3);
        assert_eq!(r.attributes, 0x00);
        assert_eq!(r.indoor_red_soldier_created, None);
    }

    #[test]
    fn routine_00_claims_the_red_soldier_slot_when_eligible() {
        let r = jumping_soldier_routine_00(0x02, 0, 5, 3);
        assert_eq!(r.attributes, 0x02);
        assert_eq!(r.indoor_red_soldier_created, Some(1));
    }

    #[test]
    fn routine_00_preserves_other_attribute_bits_when_demoting() {
        let r = jumping_soldier_routine_00(0b0000_0111, 1, 5, 3);
        assert_eq!(r.attributes, 0b0000_0101);
    }

    #[test]
    fn routine_01_sprite_cadence_matches_the_3_delay_bands() {
        assert_eq!(
            jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x00, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0).sprites,
            0x97
        );
        assert_eq!(
            jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x02, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0).sprites,
            0x93
        );
        assert_eq!(
            jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x09, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0).sprites,
            0x98
        );
    }

    #[test]
    fn routine_01_sprite_attr_uses_red_palette_only_when_the_red_bit_is_set() {
        let plain = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x02, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(plain.sprite_attr & 0x07, 0x00);
        let red = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x02, 0x02, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(red.sprite_attr & 0x07, 0x05);
    }

    #[test]
    fn routine_01_sprite_attr_flips_horizontally_when_moving_left() {
        let right = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x02, 0x00, 0x00, 0, 0, 0x01, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(right.sprite_attr & 0x40, 0x00);
        let left = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x02, 0x00, 0x00, 0, 0, 0xFF, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(left.sprite_attr & 0x40, 0x40);
    }

    #[test]
    fn routine_01_waits_when_delay_is_nonzero_and_not_the_fire_frame() {
        let r = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x05, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r.outcome, JumpingSoldierRoutine01Outcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn routine_01_red_soldier_never_fires_even_on_the_fire_frame() {
        // enemy_animation_delay=0x09 -> decremented to 0x08 (the fire frame), but is_red=true.
        let r = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 1, 0x09, 0x02, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r.outcome, JumpingSoldierRoutine01Outcome::Waiting { animation_delay: 0x08 });
    }

    #[test]
    fn routine_01_fires_exactly_on_the_decremented_0x08_frame() {
        let rom = synthetic_prg_rom();
        let r = jumping_soldier_routine_01(&rom, &[0; ENEMY_SLOT_COUNT], 0, 1, 0x09, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [1, 0], [0, 0], [0x60, 0], 0);
        match r.outcome {
            JumpingSoldierRoutine01Outcome::Fired { animation_delay, .. } => assert_eq!(animation_delay, 0x08),
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_jumps_when_animation_delay_is_already_zero() {
        let r = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x00, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0, [0, 0], [0, 0], [0, 0], 0);
        match r.outcome {
            JumpingSoldierRoutine01Outcome::Jumping(j) => {
                assert_eq!(j.y_pos, 0x6D_u8.wrapping_add(JUMPING_SOLDIER_Y_VEL_TBL[0]));
                assert_eq!(j.var_1, 1);
                assert_eq!(j.animation_delay, None);
            }
            other => panic!("expected Jumping, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_jump_sequence_resets_var_1_and_animation_delay_once_finished() {
        let r = jumping_soldier_routine_01(&[], &[0; ENEMY_SLOT_COUNT], 0, 0, 0x00, 0x00, 0x00, 0, 0, 0x00, 0x50, 0x6D, 0x13, [0, 0], [0, 0], [0, 0], 0);
        match r.outcome {
            JumpingSoldierRoutine01Outcome::Jumping(j) => {
                assert_eq!(j.var_1, 0);
                assert_eq!(j.animation_delay, Some(0x10));
            }
            other => panic!("expected Jumping, got {other:?}"),
        }
    }

    #[test]
    fn routine_04_non_red_just_advances() {
        let rom = synthetic_prg_rom();
        let r = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x00, 0x80, 0x6D, 5);
        assert_eq!(r, JumpingSoldierRoutine04Outcome::AdvancedOnly(advance_enemy_routine(5)));
    }

    #[test]
    fn routine_04_red_but_outside_x_range_just_advances() {
        let rom = synthetic_prg_rom();
        let too_far_left = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x02, 0x50, 0x6D, 5);
        assert_eq!(too_far_left, JumpingSoldierRoutine04Outcome::AdvancedOnly(advance_enemy_routine(5)));
        let too_far_right = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x02, 0xA0, 0x6D, 5);
        assert_eq!(too_far_right, JumpingSoldierRoutine04Outcome::AdvancedOnly(advance_enemy_routine(5)));
    }

    #[test]
    fn routine_04_red_with_bit_7_set_just_advances() {
        let rom = synthetic_prg_rom();
        let r = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0b1000_0010, 0x80, 0x6D, 5);
        assert_eq!(r, JumpingSoldierRoutine04Outcome::AdvancedOnly(advance_enemy_routine(5)));
    }

    #[test]
    fn routine_04_red_in_range_explodes_and_shifts_attributes_by_2() {
        let rom = synthetic_prg_rom();
        // attrs = 0b0001_0110 (0x16): red bit (bit1) set, bit7 clear.
        let r = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0b0001_0110, 0x80, 0x6D, 5);
        match r {
            JumpingSoldierRoutine04Outcome::ExplodedAndDroppedWeapon { attributes_after_shift, play_result } => {
                assert_eq!(attributes_after_shift, 0b0001_0110 >> 2);
                assert_eq!(play_result.attributes, (0b0001_0110_u8 >> 2) & 0x07);
                assert_eq!(play_result.routine, 1);
                assert_eq!(play_result.enemy_type, 0);
                let explosion = play_result.explosion.unwrap();
                assert_eq!(explosion.fields.x_pos, 0x80);
                assert_eq!(explosion.fields.y_pos, 0x6D);
            }
            other => panic!("expected ExplodedAndDroppedWeapon, got {other:?}"),
        }
    }

    #[test]
    fn routine_04_x_range_is_exclusive_on_the_right_edge() {
        let rom = synthetic_prg_rom();
        let at_left_edge = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x02, 0x64, 0x6D, 5);
        assert!(matches!(at_left_edge, JumpingSoldierRoutine04Outcome::ExplodedAndDroppedWeapon { .. }));
        let at_right_edge = jumping_soldier_routine_04(&rom, &[0; ENEMY_SLOT_COUNT], 0, 0x02, 0x9C, 0x6D, 5);
        assert_eq!(at_right_edge, JumpingSoldierRoutine04Outcome::AdvancedOnly(advance_enemy_routine(5)));
    }
}
