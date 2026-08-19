//! Native port of the level 2/4 wall core's non-graphics routines,
//! `src/bank0.asm` (`wall_core_routine_ptr_tbl`, `$9124`-`$9250`):
//! [`wall_core_routine_00`] (`$9124`, init HP/collision-box/opening-delay
//! from `ENEMY_ATTRIBUTES`) and [`wall_core_routine_03`] (`$91cf`, "fire
//! at player if conditions met") - the two entries in this 10-routine
//! family with no dependency on the still-unported PPU graphics-buffer
//! subsystem. `wall_core_routine_01`/`_02`/`_04` (core plating/opening
//! animation) call `update_enemy_nametable_tiles`/`update_nametable_
//! tiles_set_delay` directly and stay blocked by that same wall as
//! `claw_routine_02`/`_03`/`fire_beam_02`/`_03`; `wall_core_routine_05`/
//! `07`/`08`/`09` (explosion/boss-appear/cleanup) aren't investigated
//! here and remain unported too.

use crate::enemy::add_with_enemy_pos::set_08_09_to_enemy_pos;
use crate::enemy::create_enemy_bullet::{aim_and_create_enemy_bullet, CreatedBullet};
use crate::enemy::enemy_routine_transition::{set_enemy_delay_adv_routine, DelayedRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::player_enemy_distance::player_enemy_x_dist;

/// `wall_core_hp_tbl` (`$9184`, 4 bytes) - `ENEMY_HP` per core type
/// (index = `(ENEMY_ATTRIBUTES >> 2) & 3`: 0=normal, 1=normal plated,
/// 2=big, 3=big plated (unused - no real core is ever spawned big+
/// plated)).
const WALL_CORE_HP_TBL: [u8; 4] = [0x08, 0x05, 0x10, 0x05];

/// `wall_core_init_dmg_tile_anim_tbl` (`$9194`, 4 bytes) - initial `ENEMY_
/// VAR_2` (offset into `wall_core_tile_anim_tbl`, used once the still-
/// unported `wall_core_routine_04` starts drawing damage tiles), same
/// index as [`WALL_CORE_HP_TBL`].
const WALL_CORE_INIT_DMG_TILE_ANIM_TBL: [u8; 4] = [0x00, 0x03, 0x00, 0x03];

/// `core_opening_delay` (`$9180`, 4 bytes) - initial `ENEMY_ANIMATION_
/// DELAY`, indexed by `ENEMY_ATTRIBUTES & 3` (unplated cores only -
/// plated cores always use entry `0`, see [`wall_core_routine_00`]).
const CORE_OPENING_DELAY: [u8; 4] = [0x20, 0x80, 0xB0, 0xF0];

/// The full result of one [`wall_core_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallCoreRoutine00Result {
    /// `ENEMY_SCORE_COLLISION` - `$25` normally, `$22` for a plated core
    /// (score code `2`, collision box code `2`, vs. the unplated
    /// default's own box code).
    pub score_collision: u8,
    /// `ENEMY_VAR_A` (bullet-collision sound code, see `bullet_hit_sound_
    /// tbl`) - only written at all for a plated core (real ASM: the
    /// unplated path never touches this field here).
    pub plating_collision_sound: Option<u8>,
    pub hp: u8,
    pub var_2: u8,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// Native port of `wall_core_routine_00` (`$9124`) - sets up HP,
/// collision-box/score code, the damage-tile animation start offset, and
/// the opening delay, all from `ENEMY_ATTRIBUTES`, then advances.
pub fn wall_core_routine_00(enemy_attributes: u8, current_routine: u8) -> WallCoreRoutine00Result {
    let core_type = (enemy_attributes >> 2) & 0x03;
    let plated = enemy_attributes & 0x04 != 0;

    let (score_collision, plating_collision_sound) = if plated { (0x22, Some(0x04)) } else { (0x25, None) };

    let hp = WALL_CORE_HP_TBL[core_type as usize];
    let var_2 = WALL_CORE_INIT_DMG_TILE_ANIM_TBL[core_type as usize];

    let opening_delay_index = if plated { 0x00 } else { enemy_attributes & 0x03 };
    let animation_delay = CORE_OPENING_DELAY[opening_delay_index as usize];
    let delayed_routine = set_enemy_delay_adv_routine(animation_delay, current_routine);

    WallCoreRoutine00Result { score_collision, plating_collision_sound, hp, var_2, delayed_routine }
}

/// The real, branchy result of one [`wall_core_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallCoreRoutine03Outcome {
    /// `INDOOR_ENEMY_ATTACK_COUNT < 7` - the core hasn't earned the right
    /// to shoot yet.
    NotEnoughAttackRounds,
    /// `ENEMY_VAR_2 != 0` - the core is still plated (plating destruction
    /// is handled by the still-unported `wall_core_routine_04`).
    StillPlated,
    /// `ENEMY_Y_POS >= $70` - the core is too low on screen; a standing
    /// player couldn't be hit, so it doesn't bother firing.
    TooLow,
    /// Attack delay hadn't elapsed - decremented and stored back.
    Waiting { attack_delay: u8 },
    /// Delay elapsed: reset to `$28` and attempt to fire a bullet aimed
    /// at the closer player (may still be `None` if `ENEMY_ATTACK_FLAG`
    /// is clear - see [`aim_and_create_enemy_bullet`]).
    Fired { attack_delay: u8, bullet: Option<CreatedBullet> },
}

/// Native port of `wall_core_routine_03` (`$91cf`) - "fire at player if
/// conditions met": gates on the indoor attack-round counter, plating
/// state, and screen height, then a per-instance attack-delay countdown,
/// before aiming at the closer player via `player_enemy_x_dist` +
/// [`aim_and_create_enemy_bullet`] (bullet type `$60`, speed code `5` -
/// the real ASM's own `lda #$60 / ldy #$05`). `set_08_09_to_enemy_pos` is
/// called for faithfulness (a real `jsr` in the ASM) even though it's
/// mathematically an identity here - the bullet's source position is
/// simply the core's own position.
#[allow(clippy::too_many_arguments)]
pub fn wall_core_routine_03(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    indoor_enemy_attack_count: u8,
    enemy_var_2: u8,
    enemy_y_pos: u8,
    enemy_attack_delay: u8,
    enemy_x_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
) -> WallCoreRoutine03Outcome {
    if indoor_enemy_attack_count < 0x07 {
        return WallCoreRoutine03Outcome::NotEnoughAttackRounds;
    }
    if enemy_var_2 != 0 {
        return WallCoreRoutine03Outcome::StillPlated;
    }
    if enemy_y_pos >= 0x70 {
        return WallCoreRoutine03Outcome::TooLow;
    }

    let attack_delay = enemy_attack_delay.wrapping_sub(1);
    if attack_delay != 0 {
        return WallCoreRoutine03Outcome::Waiting { attack_delay };
    }

    let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
    let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
    let bullet = aim_and_create_enemy_bullet(
        prg_rom,
        enemy_routine,
        current_level,
        enemy_attack_flag,
        0x60,
        0x05,
        source_y,
        source_x,
        closest.player_index,
        0,
        0,
        player_state,
        sprite_y_pos,
        sprite_x_pos,
        level_location_type,
    );
    WallCoreRoutine03Outcome::Fired { attack_delay: 0x28, bullet }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enemy::enemy_routine_transition::advance_enemy_routine;

    #[test]
    fn routine_00_unplated_normal_core() {
        // core_type = (attrs>>2)&3 = 0, plated = attrs&4 = 0.
        let r = wall_core_routine_00(0x00, 5);
        assert_eq!(r.score_collision, 0x25);
        assert_eq!(r.plating_collision_sound, None);
        assert_eq!(r.hp, 0x08);
        assert_eq!(r.var_2, 0x00);
        // opening_delay_index = attrs&3 = 0 -> CORE_OPENING_DELAY[0] = 0x20
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x20, 5));
    }

    #[test]
    fn routine_00_plated_normal_core_uses_the_plated_score_code_and_sound() {
        // attrs = 0b0000_0100: core_type = 1, plated = true.
        let r = wall_core_routine_00(0b0000_0100, 5);
        assert_eq!(r.score_collision, 0x22);
        assert_eq!(r.plating_collision_sound, Some(0x04));
        assert_eq!(r.hp, WALL_CORE_HP_TBL[1]);
        assert_eq!(r.var_2, WALL_CORE_INIT_DMG_TILE_ANIM_TBL[1]);
        // plated -> opening_delay_index forced to 0 regardless of low bits.
        let r_with_low_bits = wall_core_routine_00(0b0000_0111, 5);
        assert_eq!(r_with_low_bits.delayed_routine, set_enemy_delay_adv_routine(CORE_OPENING_DELAY[0], 5));
        assert_eq!(r.delayed_routine, r_with_low_bits.delayed_routine);
    }

    #[test]
    fn routine_00_big_unplated_core() {
        // attrs = 0b0000_1000: core_type = (attrs>>2)&3 = 2, plated = false.
        let r = wall_core_routine_00(0b0000_1000, 5);
        assert_eq!(r.score_collision, 0x25);
        assert_eq!(r.hp, WALL_CORE_HP_TBL[2]);
        assert_eq!(r.var_2, WALL_CORE_INIT_DMG_TILE_ANIM_TBL[2]);
        // unplated -> opening_delay_index = attrs&3 = 0.
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(CORE_OPENING_DELAY[0], 5));
    }

    #[test]
    fn routine_00_unplated_core_reads_the_opening_delay_from_the_low_attribute_bits() {
        // attrs&3 = 2 -> CORE_OPENING_DELAY[2] = 0xb0.
        let r = wall_core_routine_00(0b0000_0010, 5);
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0xB0, 5));
    }

    #[test]
    fn routine_00_advances_the_routine_guarded_the_usual_way() {
        let r = wall_core_routine_00(0x00, 5);
        assert_eq!(r.delayed_routine.routine_update.routine, advance_enemy_routine(5).routine);
        let guarded = wall_core_routine_00(0x00, 0);
        assert_eq!(guarded.delayed_routine.routine_update.routine, 0);
    }

    #[test]
    fn routine_03_not_enough_attack_rounds_exits_immediately() {
        let r = wall_core_routine_03(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x06, 0, 0x50, 0x05, 0x50, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r, WallCoreRoutine03Outcome::NotEnoughAttackRounds);
    }

    #[test]
    fn routine_03_still_plated_exits_before_touching_the_attack_delay() {
        let r = wall_core_routine_03(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x07, 0x01, 0x50, 0x05, 0x50, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r, WallCoreRoutine03Outcome::StillPlated);
    }

    #[test]
    fn routine_03_too_low_on_screen_exits() {
        let r = wall_core_routine_03(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x07, 0x00, 0x70, 0x05, 0x50, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r, WallCoreRoutine03Outcome::TooLow);
    }

    #[test]
    fn routine_03_waits_and_decrements_the_attack_delay() {
        let r = wall_core_routine_03(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x07, 0x00, 0x50, 0x05, 0x50, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r, WallCoreRoutine03Outcome::Waiting { attack_delay: 0x04 });
    }

    fn synthetic_prg_rom() -> Vec<u8> {
        // Same shape as `create_enemy_bullet`'s own synthetic-ROM test
        // fixture: a shared property-table pointer with a recognizable
        // record at enemy_type=1's (bullets') offset.
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 4;
        rom[record_off..record_off + 4].copy_from_slice(&[0x80, 0x00, 0x01, 0x00]);
        rom
    }

    #[test]
    fn routine_03_fires_at_the_closer_player_once_the_delay_elapses() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[9] = 0; // free slot
        let r = wall_core_routine_03(
            &rom,
            &routine,
            0,
            1, // attack flag on
            0x07,
            0x00,
            0x50, // y_pos, < 0x70
            0x01, // attack delay -> decrements to 0 this call
            0x60, // enemy x_pos
            [1, 0],
            [0, 0],
            [0x70, 0x00], // player 1 closer on X
            0,
        );
        match r {
            WallCoreRoutine03Outcome::Fired { attack_delay, bullet } => {
                assert_eq!(attack_delay, 0x28);
                assert!(bullet.is_some());
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_respects_the_attack_flag_gate_on_fire() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = wall_core_routine_03(&rom, &routine, 0, 0, 0x07, 0x00, 0x50, 0x01, 0x60, [1, 0], [0, 0], [0x70, 0x00], 0);
        match r {
            WallCoreRoutine03Outcome::Fired { bullet, .. } => assert_eq!(bullet, None),
            other => panic!("expected Fired, got {other:?}"),
        }
    }
}
