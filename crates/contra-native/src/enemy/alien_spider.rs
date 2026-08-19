//! Native port of the level 5 (alien lair) alien spider enemy,
//! `src/bank0.asm` (`alien_spider_routine_ptr_tbl`, `$ba3b`-`$bb68`):
//! `_00` (spawn already on the ground), `_01`/`_02` (spawn as a
//! descending egg that hatches on landing), `_03` (walk on the ground/
//! ceiling, occasionally jump toward the target player), `_04` (mid-jump,
//! land on ground/ceiling collision). Genuinely self-contained - no PPU
//! graphics-buffer or rotation-subsystem dependency at all, the first
//! family this session ported for neither reason. Shares [`crate::enemy::
//! white_blob::white_blob_spider_set_sprite`]'s nibble-packed sprite/
//! delay cycling. `alien_spider_routine_ptr_tbl` entries `5`-`7`
//! (explosion/removal) are the same real shared `bank7.asm` routines
//! most enemy families use and aren't ported here.
//!
//! ## `set_enemy_routine_to_a`'s real "off by one" - resolved, not a bug
//!
//! `alien_spider_set_ground_vel_and_routine`'s own `lda #$04; jmp set_
//! enemy_routine_to_a` looked, on first read, like it contradicted its
//! own real ASM comment ("set routine to alien_spider_routine_03") and
//! this project's established `set_enemy_routine_to_a(current_routine,
//! a)` convention (which elsewhere always uses `a` matching the target
//! state's own numeric suffix, e.g. `wall_core`/`sniper`). The real
//! disassembly's own comment on `set_enemy_routine_to_a` itself resolves
//! this: "remember enemy routines are off by one, so setting ENEMY_
//! ROUTINE to #$03, results in the 2nd routine being run" - i.e. the raw
//! `ENEMY_ROUTINE` byte is always the *table index + 1* (`initialize_
//! enemy` starts fresh spawns at `1`, dispatching table index `0`).
//! `a=4` therefore really does target table index `3` = `alien_spider_
//! routine_03`, matching the comment. This project's own `current_
//! routine`/`set_enemy_routine_to_a`/`advance_enemy_routine` Rust
//! functions were *already* correct throughout this session regardless -
//! they always mirror the literal raw ASM immediate/register value
//! bit-for-bit (`advance_enemy_routine` is a raw `+1`, matching the real
//! `inc`), so the "off by one" is inherent in the raw values themselves
//! and needs no special handling in code - only in how this doc comment
//! (and others like it) describes *which named state* a given raw value
//! reaches.
//!
//! ## The one-shot jump flag
//!
//! `ENEMY_VAR_3` doubles as "has this spider ever jumped" once `_03`'s
//! own jump-trigger path runs (`inc ENEMY_VAR_3,x`, never reset back to
//! `0` anywhere in this family) - after a spider's first jump, every
//! future visit to `_03` skips the jump-trigger check entirely and just
//! walks. A real, deliberate one-shot, ported faithfully.
//!
//! ## A second real inter-instruction carry dependency
//!
//! `alien_spider_routine_03`'s own ceiling-descent Y-velocity calc reads
//! `mv_low_nibble_to_high`'s carry-out (bit `4` of the *original*,
//! pre-shift `PLAYER_WEAPON_STRENGTH` - a side effect of chaining 4
//! `asl`s, not part of that routine's own documented return value) into
//! an immediately-following `adc` with no `clc` in between - the same
//! category of real, deliberate carry-threading this crate already
//! modeled precisely in `crate::enemy::alien_fetus::alien_fetus_
//! routine_00`'s own HP-then-RNG calculation.

use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_routine_to_a, EnemyRoutineUpdate};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};
use crate::enemy::white_blob::WhiteBlobSpiderSpriteResult;
use crate::physics::collision::{add_a_y_to_enemy_pos_get_bg_collision, CollisionCode, BG_COLLISION_DATA_LEN};

// `white_blob_spider_set_sprite` is private to `white_blob.rs` - re-call
// it through this small re-export so `alien_spider` (its other real
// caller) can share it without duplicating the nibble-packing logic.
use crate::enemy::white_blob::white_blob_spider_set_sprite as shared_spider_set_sprite;

/// `mv_low_nibble_to_high` (`$b446`) plus its own real carry-out (see
/// this module's own doc comment) - `crate::enemy::white_blob`'s own
/// copy of this routine only ever needs the plain value, not this carry,
/// so it's kept as a separate, local helper here rather than shared.
fn mv_low_nibble_to_high_with_carry(v: u8) -> (u8, bool) {
    (v << 4, (v >> 4) & 1 != 0)
}

/// Native port of `set_alien_spider_hp_sprite_attr` (`$ba58`) - `hp =
/// weapon_strength + completion_count + 2`; the real ASM's own `+2`
/// (rather than `+1`) relies on the carry flag already being set by the
/// enemy-dispatch mechanism's own `cmp #$10` check before *any* level-
/// specific (`ENEMY_TYPE >= $10`) enemy routine runs - always true for
/// every real call site of this routine, so this port bakes it in as a
/// constant `+2` rather than threading a synthetic "carry-in" parameter
/// nothing else in this crate models. Returns `(hp, animation_delay,
/// sprite_attr)`.
pub fn set_alien_spider_hp_sprite_attr(player_weapon_strength: u8, game_completion_count: u8, enemy_y_pos: u8) -> (u8, u8, u8) {
    let hp = player_weapon_strength.wrapping_add(game_completion_count).wrapping_add(2);
    let sprite_attr = if enemy_y_pos >= 0x80 { 0x00 } else { 0x80 };
    (hp, 0x60, sprite_attr)
}

/// The full result of one [`alien_spider_set_ground_vel_and_routine`]
/// call - the real shared tail both `alien_spider_routine_00` (spawn
/// already grounded) and `alien_spider_routine_02` (egg just hatched)
/// fall into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienSpiderGroundVelResult {
    /// Always `0xb3`.
    pub sprite: u8,
    pub x_velocity: (u8, u8),
    pub y_velocity: (u8, u8),
    /// `ENEMY_VAR_1` - target player index.
    pub var_1: u8,
    /// `ENEMY_VAR_2` - `0x00` won't jump, `0x02` will jump.
    pub var_2: u8,
    pub var_3: u8,
    pub var_4: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `alien_spider_set_ground_vel_and_routine` (`$ba52`) -
/// `clear_enemy_custom_vars`'s own effect (`ENEMY_VAR_1`-`_4` all zeroed
/// first) is folded directly into this function's own baseline output
/// rather than threading a separate call, since every field it clears is
/// unconditionally overwritten or left at its cleared value here anyway.
pub fn alien_spider_set_ground_vel_and_routine(
    random_num: u8,
    frame_counter: u8,
    p2_game_over_status: u8,
    current_routine: u8,
) -> AlienSpiderGroundVelResult {
    let var_1 = if p2_game_over_status != 0 { 0x00 } else { random_num.wrapping_add(frame_counter) & 0x01 };

    let (shifted, carry) = (random_num >> 1, random_num & 0x01 != 0);
    let sum = shifted as u16 + frame_counter as u16 + carry as u16;
    let var_2 = (sum as u8) & 0x02;

    AlienSpiderGroundVelResult {
        sprite: 0xB3,
        x_velocity: (0x80, 0xFE),
        y_velocity: (0x00, 0x00),
        var_1,
        var_2,
        var_3: 0x00,
        var_4: 0x00,
        routine_update: set_enemy_routine_to_a(current_routine, 0x04),
    }
}

/// The full result of one [`alien_spider_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienSpiderRoutine00Result {
    pub hp: u8,
    pub animation_delay: u8,
    pub sprite_attr: u8,
    pub ground: AlienSpiderGroundVelResult,
}

/// Native port of `alien_spider_routine_00` (`$ba3b`) - spawn already
/// on the ground.
pub fn alien_spider_routine_00(
    player_weapon_strength: u8,
    game_completion_count: u8,
    enemy_y_pos: u8,
    random_num: u8,
    frame_counter: u8,
    p2_game_over_status: u8,
    current_routine: u8,
) -> AlienSpiderRoutine00Result {
    let (hp, animation_delay, sprite_attr) = set_alien_spider_hp_sprite_attr(player_weapon_strength, game_completion_count, enemy_y_pos);
    let ground = alien_spider_set_ground_vel_and_routine(random_num, frame_counter, p2_game_over_status, current_routine);
    AlienSpiderRoutine00Result { hp, animation_delay, sprite_attr, ground }
}

/// The full result of one [`alien_spider_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienSpiderRoutine01Result {
    /// Always `0x33`.
    pub score_collision: u8,
    pub hp: u8,
    pub animation_delay: u8,
    pub sprite_attr: u8,
    /// Always `0xb6` (the egg sprite).
    pub sprite: u8,
    pub x_velocity: (u8, u8),
    /// `ENEMY_VAR_3` - the egg's own Y fast velocity accumulator.
    pub var_3: u8,
    /// `ENEMY_VAR_4` - the egg's own Y fractional velocity accumulator.
    pub var_4: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `alien_spider_routine_01` (`$bb44`) - spawn as an egg,
/// out of the spider-generator.
pub fn alien_spider_routine_01(
    player_weapon_strength: u8,
    game_completion_count: u8,
    enemy_y_pos: u8,
    current_routine: u8,
) -> AlienSpiderRoutine01Result {
    let (hp, animation_delay, sprite_attr) = set_alien_spider_hp_sprite_attr(player_weapon_strength, game_completion_count, enemy_y_pos);
    AlienSpiderRoutine01Result {
        score_collision: 0x33,
        hp,
        animation_delay,
        sprite_attr,
        sprite: 0xB6,
        x_velocity: (0xB0, 0xFF),
        var_3: 0xFC,
        var_4: 0x00,
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// The real, branchy result of one [`alien_spider_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlienSpiderRoutine02Outcome {
    /// Not yet close enough to the ground or ceiling.
    StillFloating,
    /// Hatches: falls into [`alien_spider_set_ground_vel_and_routine`].
    SpawnedFromEgg(AlienSpiderGroundVelResult),
}

/// The full result of one [`alien_spider_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienSpiderRoutine02Result {
    pub y_velocity: (u8, u8),
    pub position: UpdatedEnemyPos,
    pub outcome: AlienSpiderRoutine02Outcome,
}

/// Native port of `alien_spider_routine_02` (`$bb68`) - the egg floats
/// toward whichever of the ground/ceiling is closer (`ENEMY_Y_POS < 0x80`
/// picks the ceiling), accelerating under a fixed "gravity" pulling it
/// that direction, and hatches once it gets close.
#[allow(clippy::too_many_arguments)]
pub fn alien_spider_routine_02(
    enemy_var_3: u8,
    enemy_var_4: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    random_num: u8,
    frame_counter: u8,
    p2_game_over_status: u8,
    current_routine: u8,
) -> AlienSpiderRoutine02Result {
    let (var_4, carry) = enemy_var_4.overflowing_add(0x28);
    let var_3 = enemy_var_3.wrapping_add(carry as u8);

    let y_velocity = if enemy_y_pos >= 0x80 { (var_4, var_3) } else { (0x00u8.wrapping_sub(var_4), !var_3) };

    let position = update_enemy_pos(
        level_scrolling_type,
        frame_scroll,
        enemy_x_pos,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        enemy_y_pos,
        y_vel_accum,
        y_velocity.0,
        y_velocity.1,
    );

    let outcome = if position.y.pos >= 0xC1 || position.y.pos < 0x30 {
        AlienSpiderRoutine02Outcome::SpawnedFromEgg(alien_spider_set_ground_vel_and_routine(random_num, frame_counter, p2_game_over_status, current_routine))
    } else {
        AlienSpiderRoutine02Outcome::StillFloating
    };

    AlienSpiderRoutine02Result { y_velocity, position, outcome }
}

/// One attempted jump [`alien_spider_routine_03`] computed - either
/// toward the ground (from the ceiling) or up (from the ground).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AlienAttemptedJump {
    y_velocity: (u8, u8),
    /// `Some` only for the ceiling-descent path - the ground-jump path
    /// keeps whatever X velocity the spider already had (its existing
    /// walking speed).
    x_velocity: Option<(u8, u8)>,
}

/// The real, branchy result of one [`alien_spider_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlienSpiderRoutine03Outcome {
    /// No jump attempted (or attempted and rejected by a distance/height
    /// gate): clamps `ENEMY_Y_POS` to the ground/ceiling line if it
    /// overshot, applies velocity, and zeroes Y velocity afterward.
    Walking { y_pos: u8, position: UpdatedEnemyPos },
    /// A jump (or ceiling-descent) was triggered: sets `ENEMY_VAR_3`'s
    /// one-shot "has jumped" flag and moves to `alien_spider_routine_04`
    /// (`$04`, real, literal raw `inc` - see this module's own doc
    /// comment for why that reaches the state the ROM calls `_04`).
    Jumping {
        sprite: u8,
        sprite_attr: u8,
        y_velocity: (u8, u8),
        x_velocity: Option<(u8, u8)>,
        var_3: u8,
        new_routine: u8,
        position: UpdatedEnemyPos,
    },
}

/// The full result of one [`alien_spider_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienSpiderRoutine03Result {
    pub sprite: WhiteBlobSpiderSpriteResult,
    pub outcome: AlienSpiderRoutine03Outcome,
}

/// Native port of `alien_spider_routine_03` (`$ba8c`) - see this
/// module's own doc comment for the one-shot jump flag and the real
/// inter-instruction carry dependency in the ceiling-descent branch.
#[allow(clippy::too_many_arguments)]
pub fn alien_spider_routine_03(
    enemy_animation_delay: u8,
    enemy_var_1: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_sprite_attr: u8,
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    player_weapon_strength: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    current_routine: u8,
) -> AlienSpiderRoutine03Result {
    let sprite = shared_spider_set_sprite(0xB3, enemy_animation_delay);

    let jump = 'attempt: {
        if enemy_var_3 != 0 || enemy_var_2 == 0 {
            break 'attempt None;
        }
        let target_idx = enemy_var_1 as usize;
        let target_y = sprite_y_pos[target_idx];
        if target_y < 0x20 {
            break 'attempt None;
        }
        let dist_x = enemy_x_pos.wrapping_sub(sprite_x_pos[target_idx]);
        if dist_x >= 0x30 {
            break 'attempt None;
        }
        if enemy_y_pos >= target_y {
            if dist_x >= 0x20 {
                break 'attempt None;
            }
            Some(AlienAttemptedJump { y_velocity: (0x00, 0xFF), x_velocity: None })
        } else {
            let (strength_boost, carry_in) = mv_low_nibble_to_high_with_carry(player_weapon_strength);
            let sum = strength_boost as u16 + 0x40u16 + carry_in as u16;
            let y_frac = sum as u8;
            let y_fast = 0x02u8.wrapping_add((sum > 0xFF) as u8);
            Some(AlienAttemptedJump { y_velocity: (y_frac, y_fast), x_velocity: Some((0x80, 0xFF)) })
        }
    };

    let outcome = match jump {
        Some(j) => {
            let sprite_attr = enemy_sprite_attr & 0x3F;
            let var_3 = enemy_var_3.wrapping_add(1);
            let new_routine = current_routine.wrapping_add(1);
            let (x_fract, x_fast) = j.x_velocity.unwrap_or((x_vel_fract, x_vel_fast));
            let position = update_enemy_pos(
                level_scrolling_type,
                frame_scroll,
                enemy_x_pos,
                x_vel_accum,
                x_fract,
                x_fast,
                enemy_y_pos,
                y_vel_accum,
                j.y_velocity.0,
                j.y_velocity.1,
            );
            AlienSpiderRoutine03Outcome::Jumping {
                sprite: 0xB3,
                sprite_attr,
                y_velocity: j.y_velocity,
                x_velocity: j.x_velocity,
                var_3,
                new_routine,
                position,
            }
        }
        None => {
            let y_pos = if enemy_y_pos > 0xB8 {
                0xB8
            } else if enemy_y_pos <= 0x38 {
                0x38
            } else {
                enemy_y_pos
            };
            let position = update_enemy_pos(
                level_scrolling_type,
                frame_scroll,
                enemy_x_pos,
                x_vel_accum,
                x_vel_fract,
                x_vel_fast,
                y_pos,
                y_vel_accum,
                y_vel_fract,
                y_vel_fast,
            );
            AlienSpiderRoutine03Outcome::Walking { y_pos, position }
        }
    };

    AlienSpiderRoutine03Result { sprite, outcome }
}

/// The full result of one [`alien_spider_routine_04`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlienSpiderRoutine04Result {
    /// `Some((x_frac, x_fast))` only the call the spider lands (`0x80,
    /// 0xfe` - Y velocity is also zeroed on this same call).
    pub landed: Option<(u8, u8)>,
    /// `Some(0x04)` only the call the spider lands - a real, literal raw
    /// `sta ENEMY_ROUTINE,x`, *not* even the guarded [`set_enemy_routine_
    /// to_a`] helper (see this module's own doc comment for why raw `4`
    /// reaches the state the ROM calls `_03`).
    pub new_routine: Option<u8>,
    pub position: UpdatedEnemyPos,
}

/// Native port of `alien_spider_routine_04` (`$bb2a`) - mid-jump: checks
/// for floor/ceiling collision at the current position, and if found,
/// lands (stopping Y velocity, resetting to the walking X velocity, and
/// returning to `_03`).
#[allow(clippy::too_many_arguments)]
pub fn alien_spider_routine_04(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> AlienSpiderRoutine04Result {
    let collision =
        add_a_y_to_enemy_pos_get_bg_collision(0, 0, enemy_x_pos, enemy_y_pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);

    let (landed, new_routine, y_vel_fract, y_vel_fast, x_vel_fract, x_vel_fast) = if collision == CollisionCode::Floor {
        (Some((0x80u8, 0xFEu8)), Some(0x04u8), 0x00u8, 0x00u8, 0x80u8, 0xFEu8)
    } else {
        (None, None, y_vel_fract, y_vel_fast, x_vel_fract, x_vel_fast)
    };

    let position = update_enemy_pos(
        level_scrolling_type,
        frame_scroll,
        enemy_x_pos,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        enemy_y_pos,
        y_vel_accum,
        y_vel_fract,
        y_vel_fast,
    );

    AlienSpiderRoutine04Result { landed, new_routine, position }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0u8; BG_COLLISION_DATA_LEN]
    }

    #[test]
    fn hp_sprite_attr_adds_weapon_strength_completion_and_2() {
        let (hp, delay, _) = set_alien_spider_hp_sprite_attr(0x02, 0x03, 0x50);
        assert_eq!(hp, 0x07);
        assert_eq!(delay, 0x60);
    }

    #[test]
    fn hp_sprite_attr_flips_vertically_in_the_top_half() {
        let (_, _, top) = set_alien_spider_hp_sprite_attr(0, 0, 0x40);
        assert_eq!(top, 0x80);
        let (_, _, bottom) = set_alien_spider_hp_sprite_attr(0, 0, 0x90);
        assert_eq!(bottom, 0x00);
    }

    #[test]
    fn ground_vel_p2_game_over_targets_player_1() {
        let r = alien_spider_set_ground_vel_and_routine(0xFF, 0xFF, 0x01, 5);
        assert_eq!(r.var_1, 0x00);
        assert_eq!(r.x_velocity, (0x80, 0xFE));
        assert_eq!(r.routine_update, set_enemy_routine_to_a(5, 0x04));
    }

    #[test]
    fn ground_vel_rolls_target_player_when_p2_is_playing() {
        let r = alien_spider_set_ground_vel_and_routine(0x01, 0x00, 0x00, 5);
        assert_eq!(r.var_1, 0x01);
    }

    #[test]
    fn routine_00_composes_hp_and_ground_vel() {
        let r = alien_spider_routine_00(0x02, 0x03, 0x50, 0x00, 0x00, 0x01, 5);
        assert_eq!(r.hp, 0x07);
        assert_eq!(r.ground.sprite, 0xB3);
    }

    #[test]
    fn routine_01_sets_egg_state() {
        let r = alien_spider_routine_01(0x02, 0x03, 0x50, 5);
        assert_eq!(r.score_collision, 0x33);
        assert_eq!(r.sprite, 0xB6);
        assert_eq!(r.x_velocity, (0xB0, 0xFF));
        assert_eq!(r.var_3, 0xFC);
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn routine_02_still_floating_mid_screen() {
        let r = alien_spider_routine_02(0x00, 0x00, 0x50, 0x80, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 0x00, 5);
        assert_eq!(r.outcome, AlienSpiderRoutine02Outcome::StillFloating);
    }

    #[test]
    fn routine_02_falls_toward_ground_below_midscreen() {
        // enemy_y_pos=0x90 (>=0x80) -> falls (positive-ish gravity direction).
        let r = alien_spider_routine_02(0x00, 0x00, 0x50, 0x90, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 0x00, 5);
        assert_eq!(r.y_velocity, (0x28, 0x00)); // var_4=0+0x28, var_3=0+carry(0)
    }

    #[test]
    fn routine_02_floats_toward_ceiling_above_midscreen() {
        let r = alien_spider_routine_02(0x00, 0x00, 0x50, 0x40, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 0x00, 5);
        // var_4=0x28, var_3=0 -> y_fract = 0-0x28 (wrapping), y_fast = !0 = 0xff
        assert_eq!(r.y_velocity, (0x00u8.wrapping_sub(0x28), 0xFF));
    }

    #[test]
    fn routine_02_hatches_near_the_ceiling() {
        // enemy_y_pos=0x30 (< 0x80, ceiling path), fast=-1 fract-carry
        // pushes the post-update Y position below the 0x30 threshold.
        let r = alien_spider_routine_02(0x00, 0x00, 0x50, 0x30, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 0x00, 5);
        assert!(matches!(r.outcome, AlienSpiderRoutine02Outcome::SpawnedFromEgg(_)), "position={:?}", r.position);
    }

    #[test]
    fn routine_03_walks_when_not_flagged_to_jump() {
        let r = alien_spider_routine_03(0x50, 0, 0x00, 0x00, 0x50, 0x60, 0x00, [0, 0], [0, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        match r.outcome {
            AlienSpiderRoutine03Outcome::Walking { y_pos, .. } => assert_eq!(y_pos, 0x60),
            other => panic!("expected Walking, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_walking_clamps_past_the_ground_line() {
        let r = alien_spider_routine_03(0x50, 0, 0x00, 0x00, 0x50, 0xC0, 0x00, [0, 0], [0, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        match r.outcome {
            AlienSpiderRoutine03Outcome::Walking { y_pos, .. } => assert_eq!(y_pos, 0xB8),
            other => panic!("expected Walking, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_walking_clamps_past_the_ceiling_line() {
        let r = alien_spider_routine_03(0x50, 0, 0x00, 0x00, 0x50, 0x20, 0x00, [0, 0], [0, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        match r.outcome {
            AlienSpiderRoutine03Outcome::Walking { y_pos, .. } => assert_eq!(y_pos, 0x38),
            other => panic!("expected Walking, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_never_reattempts_a_jump_once_the_one_shot_flag_is_set() {
        // var_2 flagged to jump, but var_3 already nonzero (has jumped before).
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x01, 0x50, 0x60, 0x00, [0x60, 0], [0x50, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        assert!(matches!(r.outcome, AlienSpiderRoutine03Outcome::Walking { .. }));
    }

    #[test]
    fn routine_03_ground_jump_triggers_when_close_and_player_at_or_below() {
        // target player at same X (dist=0), enemy_y_pos(0x60) >= target_y(0x50).
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x00, 0x50, 0x60, 0b0011_0000, [0x50, 0], [0x50, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        match r.outcome {
            AlienSpiderRoutine03Outcome::Jumping { y_velocity, x_velocity, var_3, new_routine, sprite_attr, .. } => {
                assert_eq!(y_velocity, (0x00, 0xFF));
                assert_eq!(x_velocity, None);
                assert_eq!(var_3, 0x01);
                assert_eq!(new_routine, 6);
                assert_eq!(sprite_attr, 0b0011_0000 & 0x3F);
            }
            other => panic!("expected Jumping, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_ceiling_descent_triggers_when_player_is_below() {
        // target player below the spider: target_y(0x90) > enemy_y_pos(0x50).
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x00, 0x50, 0x50, 0x00, [0x90, 0], [0x50, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        match r.outcome {
            AlienSpiderRoutine03Outcome::Jumping { y_velocity, x_velocity, .. } => {
                assert_eq!(y_velocity, (0x40, 0x02));
                assert_eq!(x_velocity, Some((0x80, 0xFF)));
            }
            other => panic!("expected Jumping, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_ceiling_descent_boosts_y_velocity_by_weapon_strength() {
        // weapon_strength high nibble contributes a carry into the fract add.
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x00, 0x50, 0x50, 0x00, [0x90, 0], [0x50, 0], 0x1F, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        let (strength_boost, carry_in) = mv_low_nibble_to_high_with_carry(0x1F);
        let sum = strength_boost as u16 + 0x40u16 + carry_in as u16;
        let expected = (sum as u8, 0x02u8.wrapping_add((sum > 0xFF) as u8));
        match r.outcome {
            AlienSpiderRoutine03Outcome::Jumping { y_velocity, .. } => assert_eq!(y_velocity, expected),
            other => panic!("expected Jumping, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_rejects_jump_when_target_player_too_high() {
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x00, 0x50, 0x60, 0x00, [0x10, 0], [0x50, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        assert!(matches!(r.outcome, AlienSpiderRoutine03Outcome::Walking { .. }));
    }

    #[test]
    fn routine_03_rejects_jump_when_too_far_on_the_coarse_gate() {
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x00, 0x50, 0x60, 0x00, [0x50, 0], [0x00, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        assert!(matches!(r.outcome, AlienSpiderRoutine03Outcome::Walking { .. }));
    }

    #[test]
    fn routine_03_rejects_ground_jump_on_the_tighter_gate() {
        // dist_x = 0x28 (within the coarse 0x30 gate but not the tighter 0x20 ground-jump gate).
        let r = alien_spider_routine_03(0x50, 0, 0x02, 0x00, 0x78, 0x60, 0x00, [0x50, 0], [0x50, 0], 0x00, 0, 0x00, 0, 0, 0, 0, 0x00, 0x00, 5);
        assert!(matches!(r.outcome, AlienSpiderRoutine03Outcome::Walking { .. }));
    }

    #[test]
    fn routine_04_no_collision_keeps_flying() {
        let bg = empty_bg_collision_data();
        let r = alien_spider_routine_04(0x50, 0x60, 0, 0, 0, &bg, 0, 0x00, 0, 0x00, 0xFF, 0, 0x00, 0x02);
        assert_eq!(r.landed, None);
        assert_eq!(r.new_routine, None);
    }
}
