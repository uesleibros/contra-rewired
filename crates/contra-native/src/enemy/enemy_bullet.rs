//! Native port of the enemy bullet's own routine table (`src/bank0.asm`,
//! `$814f`-`$8202`) - `enemy_bullet_routine_00`/`_01`/`_02` (entry 3,
//! `remove_enemy`, is already ported). This is the bullet *entity's* own
//! per-frame state machine (collision code, sprite/velocity/position,
//! and per-`ENEMY_VAR_1` "bullet type" behavior) - a different thing
//! from [`crate::enemy::create_enemy_bullet`], which only *spawns* one.
//!
//! ## Five real bullet types, one shared position update
//! `ENEMY_VAR_1` selects the bullet type: `0` regular, `1` large
//! cannonball (falls with gravity, explodes into `_02`'s animation at
//! the ground), `2` unused/no-op here, `3` indoor regular bullet (no
//! gravity, just an on-screen bounds check), `4` level-3 dragon boss's
//! fire ball (recolors/flips every 4 frames to animate). All 5 share one
//! `update_enemy_pos` call in `enemy_bullet_routine_01` before branching
//! - real ASM's own removal from *that* call (off the normal enemy
//! bounds) can happen independently of, and *before*, whatever the
//! bullet-type branch below it does; execution keeps going regardless
//! (same "removal doesn't stop the rest of the routine" quirk this
//! crate has documented before for other routines) - callers should
//! check [`EnemyBulletRoutine01Result::position`]'s own `removed` field
//! in addition to `outcome`.
//!
//! ## The snow-field sprite override doesn't change the real bullet type
//! Level 5 (`CURRENT_LEVEL == 4`, 0-indexed) recolors regular bullets
//! (`ENEMY_VAR_1 == 0`) red by looking up sprite/palette at table index
//! `5` instead of `0` - but only for that one lookup; every later check
//! in the same call re-reads the real, unmodified `ENEMY_VAR_1` value,
//! so the bullet still behaves exactly like a regular bullet otherwise.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_position_utils::add_a_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::update_enemy_pos::{remove_enemy, update_enemy_pos, RemovedEnemy, UpdatedEnemyPos};
use crate::physics::collision::{check_enemy_collision_solid_bg, CollisionCode, BG_COLLISION_DATA_LEN};

/// `bullet_collision_code_tbl` (`$815b`, 6 bytes) - real comment:
/// "#$01 regular bullets (types 0,3), #$05 larger cannonball bullets
/// (types 1,2), #$02 level 3 dragon boss fire ball (type 4)"; index 5's
/// `$00` is never documented as a real bullet type, only ever reached if
/// something stores `ENEMY_VAR_1 = 5` directly (not done by any spawn
/// path this crate has ported so far).
const BULLET_COLLISION_CODE_TBL: [u8; 6] = [0x01, 0x05, 0x05, 0x01, 0x02, 0x00];

/// Native port of `enemy_bullet_routine_00` (`$814f`) - "initialize
/// collision code".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyBulletRoutine00Result {
    pub score_collision: u8,
    pub routine_update: EnemyRoutineUpdate,
}

pub fn enemy_bullet_routine_00(bullet_type: u8, current_routine: u8) -> EnemyBulletRoutine00Result {
    EnemyBulletRoutine00Result { score_collision: BULLET_COLLISION_CODE_TBL[bullet_type as usize], routine_update: advance_enemy_routine(current_routine) }
}

/// `bullet_sprite_tbl` (`$81d8`, 6 bytes) - real bullet types 0-4, plus
/// index 5's snow-field red-bullet override for type 0.
const BULLET_SPRITE_TBL: [u8; 6] = [0x1E, 0x21, 0x21, 0x1E, 0x79, 0x07];
/// `bullet_palette_tbl` (`$81de`, 6 bytes) - same indexing as
/// [`BULLET_SPRITE_TBL`].
const BULLET_PALETTE_TBL: [u8; 6] = [0x01, 0x02, 0x02, 0x01, 0x01, 0x02];
/// `bullet_04_palette_mirror_tbl` (`$81a6`, 4 bytes) - the dragon-orb
/// fireball's own 4-frame flip/palette animation cycle.
const BULLET_04_PALETTE_MIRROR_TBL: [u8; 4] = [0x01, 0x41, 0xC1, 0x81];
/// Level index (`CURRENT_LEVEL`, 0-based) that recolors regular bullets
/// red - level 5 ("snow field").
const SNOW_FIELD_LEVEL: u8 = 4;

/// The real, per-bullet-type branch [`enemy_bullet_routine_01`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyBulletRoutine01Outcome {
    /// Bullet type `0`, `LEVEL_SOLID_BG_COLLISION_CHECK` bit 7 set, and
    /// the bullet is colliding with a solid background object.
    RemovedBySolidCollision(RemovedEnemy),
    /// Bullet type `0` with no solid-collision removal (either the level
    /// doesn't check, or it checked and found no solid collision), or
    /// any bullet type this routine has no special behavior for (e.g.
    /// type `2`) - a real no-op tail.
    Exited,
    /// Bullet type `1` (cannonball), hasn't reached the ground yet.
    StillFalling { y_velocity: (u8, u8) },
    /// Bullet type `1`, reached the ground (`ENEMY_Y_POS >= $d0`) -
    /// starts the `enemy_bullet_routine_02` explosion animation.
    Exploded { frame: u8, animation_delay: u8, routine_update: EnemyRoutineUpdate },
    /// Bullet type `3` (indoor), past the indoor screen's own bounds.
    IndoorRemoved(RemovedEnemy),
    /// Bullet type `3`, still on screen.
    IndoorOnScreen,
    /// Bullet type `4` (dragon boss fire ball) - recolors/flips this
    /// frame's sprite.
    DragonOrbAnimated { sprite_attr: u8 },
}

/// The full result of one [`enemy_bullet_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyBulletRoutine01Result {
    pub sprites: u8,
    pub sprite_attr: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: EnemyBulletRoutine01Outcome,
}

/// Native port of `enemy_bullet_routine_01` (`$8161`) - "init palette,
/// sprite, and velocity". See this module's doc comment for the 5 real
/// bullet types and the snow-field sprite override.
#[allow(clippy::too_many_arguments)]
pub fn enemy_bullet_routine_01(
    bullet_type: u8,
    current_level: u8,
    level_solid_bg_collision_check: u8,
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
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    frame_counter: u8,
    current_routine: u8,
) -> EnemyBulletRoutine01Result {
    let sprite_index = if bullet_type == 0 && current_level == SNOW_FIELD_LEVEL { 5 } else { bullet_type };
    let sprites = BULLET_SPRITE_TBL[sprite_index as usize];
    let sprite_attr = BULLET_PALETTE_TBL[sprite_index as usize];

    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);

    // `update_enemy_pos`'s own removal (a real tail `jmp remove_enemy`)
    // zeroes `ENEMY_ROUTINE,x` in place before this call's own `rts`
    // returns straight back into this routine's remaining code (same
    // "removal doesn't stop the rest of the routine" quirk documented
    // above) - so a later `advance_enemy_routine` call in *this same
    // call* must see that already-zeroed value, not the entry-time
    // `current_routine` this function was handed. Caught by live
    // verification: without this, `Exploded`'s `routine_update` came out
    // wrong on the (real, observed) frames where a cannonball's own
    // `update_enemy_pos` call removed it in the same frame it exploded.
    let effective_routine = if position.removed.is_some() { 0 } else { current_routine };

    let outcome = match bullet_type {
        0 => {
            if (level_solid_bg_collision_check as i8) < 0 {
                let collision = check_enemy_collision_solid_bg(position.x.pos, position.y.pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
                if collision == CollisionCode::Solid {
                    EnemyBulletRoutine01Outcome::RemovedBySolidCollision(remove_enemy())
                } else {
                    EnemyBulletRoutine01Outcome::Exited
                }
            } else {
                EnemyBulletRoutine01Outcome::Exited
            }
        }
        1 => {
            let y_velocity = add_a_to_enemy_y_fract_vel(0x14, y_vel_fract, y_vel_fast);
            if position.y.pos >= 0xD0 {
                EnemyBulletRoutine01Outcome::Exploded { frame: 0, animation_delay: 1, routine_update: advance_enemy_routine(effective_routine) }
            } else {
                EnemyBulletRoutine01Outcome::StillFalling { y_velocity }
            }
        }
        3 => {
            let off_screen = position.y.pos >= 0xB4 || position.x.pos < 0x20 || position.x.pos >= 0xE0;
            if off_screen {
                EnemyBulletRoutine01Outcome::IndoorRemoved(remove_enemy())
            } else {
                EnemyBulletRoutine01Outcome::IndoorOnScreen
            }
        }
        4 => {
            let idx = (frame_counter >> 2) & 0x03;
            EnemyBulletRoutine01Outcome::DragonOrbAnimated { sprite_attr: BULLET_04_PALETTE_MIRROR_TBL[idx as usize] }
        }
        _ => EnemyBulletRoutine01Outcome::Exited,
    };

    EnemyBulletRoutine01Result { sprites, sprite_attr, position, outcome }
}

/// `cannonball_explosion_sprite_tbl` (`$8202`, 3 bytes).
const CANNONBALL_EXPLOSION_SPRITE_TBL: [u8; 3] = [0x37, 0x36, 0x37];

/// The real branch [`enemy_bullet_routine_02`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyBulletRoutine02Outcome {
    Waiting { animation_delay: u8 },
    Animating { frame: u8, animation_delay: u8, sprites: u8 },
    Advanced(EnemyRoutineUpdate),
}

/// The full result of one [`enemy_bullet_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyBulletRoutine02Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: EnemyBulletRoutine02Outcome,
}

/// Native port of `enemy_bullet_routine_02` (`$81e4`) - "only used for
/// bullet type `3` (level 1 boss cannonball) explosion animation" (real
/// comment numbering is off by one from `ENEMY_VAR_1`'s own type `1`;
/// the type that reaches here is whichever one `enemy_bullet_routine_01`
/// actually transitions from, i.e. type `1`, the large cannonball).
pub fn enemy_bullet_routine_02(
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    enemy_animation_delay: u8,
    enemy_frame: u8,
    current_routine: u8,
) -> EnemyBulletRoutine02Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let delay = enemy_animation_delay.wrapping_sub(1);

    let outcome = if delay != 0 {
        EnemyBulletRoutine02Outcome::Waiting { animation_delay: delay }
    } else {
        let frame = enemy_frame.wrapping_add(1);
        if frame >= 3 {
            EnemyBulletRoutine02Outcome::Advanced(advance_enemy_routine(current_routine))
        } else {
            EnemyBulletRoutine02Outcome::Animating { frame, animation_delay: 0x08, sprites: CANNONBALL_EXPLOSION_SPRITE_TBL[frame as usize] }
        }
    };

    EnemyBulletRoutine02Result { scroll, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_looks_up_the_collision_code_and_advances() {
        let r = enemy_bullet_routine_00(1, 5);
        assert_eq!(r.score_collision, 0x05);
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn routine_00_matches_the_real_table_for_every_documented_type() {
        assert_eq!(enemy_bullet_routine_00(0, 5).score_collision, 0x01);
        assert_eq!(enemy_bullet_routine_00(1, 5).score_collision, 0x05);
        assert_eq!(enemy_bullet_routine_00(2, 5).score_collision, 0x05);
        assert_eq!(enemy_bullet_routine_00(3, 5).score_collision, 0x01);
        assert_eq!(enemy_bullet_routine_00(4, 5).score_collision, 0x02);
    }

    fn no_scroll_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0u8; BG_COLLISION_DATA_LEN]
    }

    #[test]
    fn routine_01_type_0_skips_solid_check_when_level_flag_is_positive() {
        let data = no_scroll_bg_collision_data();
        let r = enemy_bullet_routine_01(0, 0, 0x00, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert_eq!(r.outcome, EnemyBulletRoutine01Outcome::Exited);
        assert_eq!(r.sprites, BULLET_SPRITE_TBL[0]);
    }

    #[test]
    fn routine_01_type_0_uses_the_snow_field_sprite_override_only_on_level_5() {
        let data = no_scroll_bg_collision_data();
        let normal = enemy_bullet_routine_01(0, 0, 0x00, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        let snow = enemy_bullet_routine_01(0, SNOW_FIELD_LEVEL, 0x00, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert_eq!(normal.sprites, BULLET_SPRITE_TBL[0]);
        assert_eq!(snow.sprites, BULLET_SPRITE_TBL[5]);
        assert_eq!(snow.sprite_attr, BULLET_PALETTE_TBL[5]);
    }

    #[test]
    fn routine_01_type_0_removed_when_flag_negative_and_solid_collision() {
        // Same worked example as `physics::collision`'s own
        // `no_scroll_no_overflow_reads_the_expected_offset_and_column`
        // test: x=0x10, y=0x10, no scroll -> offset=0x04, column=1
        // (shift 4). Stamp a solid (raw 2-bit code 3) nibble there, and
        // use a position where `update_enemy_pos` (zero velocity) leaves
        // the bullet at exactly (0x10, 0x10).
        let mut data = no_scroll_bg_collision_data();
        data[0x04] = 0b11 << 4;
        let r = enemy_bullet_routine_01(0, 0, 0x80, 0, 0, 0x10, 0, 0, 0, 0x10, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert!(matches!(r.outcome, EnemyBulletRoutine01Outcome::RemovedBySolidCollision(_)));
    }

    #[test]
    fn routine_01_type_1_still_falling_below_ground() {
        let data = no_scroll_bg_collision_data();
        let r = enemy_bullet_routine_01(1, 0, 0, 0, 0, 0x50, 0, 0, 0, 0x80, 0, 0x10, 0x01, 0, 0, 0, &data, 0, 5);
        match r.outcome {
            EnemyBulletRoutine01Outcome::StillFalling { y_velocity } => assert_eq!(y_velocity, add_a_to_enemy_y_fract_vel(0x14, 0x10, 0x01)),
            other => panic!("expected StillFalling, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_type_1_exploded_routine_update_reflects_a_same_call_removal_by_update_enemy_pos() {
        // x_pos=0x05, no velocity, horizontal level -> update_enemy_pos's
        // own X-off-screen removal fires (x < 0x08), short-circuiting Y
        // entirely (y_pos passes through untouched at 0xd0, still
        // satisfying the explode threshold). `advance_enemy_routine`
        // must see the *already-zeroed* routine, not the entry-time one.
        let data = no_scroll_bg_collision_data();
        let r = enemy_bullet_routine_01(1, 0, 0, 0, 0, 0x05, 0, 0, 0, 0xD0, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert!(r.position.removed.is_some());
        match r.outcome {
            EnemyBulletRoutine01Outcome::Exploded { routine_update, .. } => {
                assert_eq!(routine_update, advance_enemy_routine(0), "must use the post-removal routine (0), not the entry-time current_routine (5)");
            }
            other => panic!("expected Exploded, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_type_1_explodes_once_it_reaches_the_ground() {
        let data = no_scroll_bg_collision_data();
        // y_pos=0xd0, y_vel_fast=0 -> update_enemy_pos leaves it at/above 0xd0.
        let r = enemy_bullet_routine_01(1, 0, 0, 0, 0, 0x50, 0, 0, 0, 0xD0, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        match r.outcome {
            EnemyBulletRoutine01Outcome::Exploded { frame, animation_delay, routine_update } => {
                assert_eq!(frame, 0);
                assert_eq!(animation_delay, 1);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Exploded, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_type_3_removed_outside_the_indoor_screen_bounds() {
        let data = no_scroll_bg_collision_data();
        let too_low = enemy_bullet_routine_01(3, 0, 0, 0, 0, 0x50, 0, 0, 0, 0xB4, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert!(matches!(too_low.outcome, EnemyBulletRoutine01Outcome::IndoorRemoved(_)));
        let too_left = enemy_bullet_routine_01(3, 0, 0, 0, 0, 0x10, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert!(matches!(too_left.outcome, EnemyBulletRoutine01Outcome::IndoorRemoved(_)));
        let too_right = enemy_bullet_routine_01(3, 0, 0, 0, 0, 0xE0, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert!(matches!(too_right.outcome, EnemyBulletRoutine01Outcome::IndoorRemoved(_)));
        let on_screen = enemy_bullet_routine_01(3, 0, 0, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert_eq!(on_screen.outcome, EnemyBulletRoutine01Outcome::IndoorOnScreen);
    }

    #[test]
    fn routine_01_type_4_animates_every_4_frames() {
        let data = no_scroll_bg_collision_data();
        let r0 = enemy_bullet_routine_01(4, 0, 0, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0x00, 5);
        let r4 = enemy_bullet_routine_01(4, 0, 0, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0x04, 5);
        match (r0.outcome, r4.outcome) {
            (EnemyBulletRoutine01Outcome::DragonOrbAnimated { sprite_attr: a }, EnemyBulletRoutine01Outcome::DragonOrbAnimated { sprite_attr: b }) => {
                assert_eq!(a, BULLET_04_PALETTE_MIRROR_TBL[0]);
                assert_eq!(b, BULLET_04_PALETTE_MIRROR_TBL[1]);
            }
            other => panic!("expected both DragonOrbAnimated, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_type_2_is_a_real_documented_no_op() {
        let data = no_scroll_bg_collision_data();
        let r = enemy_bullet_routine_01(2, 0, 0, 0, 0, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, &data, 0, 5);
        assert_eq!(r.outcome, EnemyBulletRoutine01Outcome::Exited);
    }

    #[test]
    fn routine_02_waits_while_delay_has_not_elapsed() {
        let r = enemy_bullet_routine_02(0, 0x01, 0x50, 0x60, 0x05, 0x00, 5);
        assert_eq!(r.outcome, EnemyBulletRoutine02Outcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn routine_02_animates_the_next_explosion_frame() {
        let r = enemy_bullet_routine_02(0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(r.outcome, EnemyBulletRoutine02Outcome::Animating { frame: 1, animation_delay: 0x08, sprites: CANNONBALL_EXPLOSION_SPRITE_TBL[1] });
    }

    #[test]
    fn routine_02_advances_once_the_explosion_sequence_finishes() {
        let r = enemy_bullet_routine_02(0, 0x01, 0x50, 0x60, 0x01, 0x02, 5);
        assert_eq!(r.outcome, EnemyBulletRoutine02Outcome::Advanced(advance_enemy_routine(5)));
    }

    #[test]
    fn routine_02_scroll_matches_add_scroll_to_enemy_pos_directly() {
        let r = enemy_bullet_routine_02(0, 0x02, 0x50, 0x60, 0x05, 0x00, 5);
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
    }
}
