//! Native port of the (outdoor) roller enemy's `_00`/`_01` routine
//! (`src/bank0.asm`, `$8f8c`-`$8fb2`) - distinct from [`crate::enemy::
//! indoor_roller_gen`], which only *spawns* rollers on indoor levels;
//! this is the roller object's own AI, reused across outdoor levels
//! too: it grows through 4 sprite sizes as it nears the bottom of the
//! screen, only counts for score/collision purposes once large enough,
//! and enables player collision (then later removes itself) only once
//! it's rolled close enough to be hit. `roller_routine_04` (`$e7a4`,
//! the destroyed-explosion entry) is not a new port - it's already
//! covered by [`crate::enemy::enemy_explosion::show_explosion_a`] with
//! a fixed `(explosion_type_override=3, max_sprites=2)` pair, exactly
//! like [`crate::enemy::enemy_explosion::shared_enemy_routine_03`]'s own
//! wrapper, so a caller integrating this live should just call that
//! directly rather than needing a roller-specific wrapper here.

use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::update_enemy_pos::{remove_enemy, update_enemy_pos, RemovedEnemy, UpdatedEnemyPos};

/// `roller_sprite_y_cutoff_tbl` (`$8fb3`, 3 bytes) - the Y positions
/// where the roller's sprite grows to the next size up.
const ROLLER_SPRITE_Y_CUTOFF_TBL: [u8; 3] = [0x7C, 0x8C, 0x9C];

/// Native port of `roller_routine_00` (`$8f8c`) - real ASM: `lda #$72;
/// sta ENEMY_Y_POS,x; jmp advance_enemy_routine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollerRoutine00Result {
    pub y_pos: u8,
    pub routine_update: EnemyRoutineUpdate,
}

pub fn roller_routine_00(current_routine: u8) -> RollerRoutine00Result {
    RollerRoutine00Result { y_pos: 0x72, routine_update: advance_enemy_routine(current_routine) }
}

/// The real Y-cutoff scan (`@sprite_y_check`/`@found_size`, `$8f94`-
/// `$8fa2`): starts at index `3` (the largest size) and walks *down*
/// looking for the first cutoff the roller's Y position has already
/// passed, falling back to `0` (smallest sprite) if none match.
fn roller_sprite_size_index(y_pos: u8) -> u8 {
    let mut y = 3u8;
    while y != 0 {
        if y_pos >= ROLLER_SPRITE_Y_CUTOFF_TBL[(y - 1) as usize] {
            return y;
        }
        y -= 1;
    }
    0
}

/// One [`roller_routine_01`] call's outcome, checked against the
/// *post-`update_enemy_pos`* Y position (real ASM reads `ENEMY_Y_POS,x`
/// again after that call, not the entry-time value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollerRoutine01Outcome {
    /// Not yet close enough (`< $ac`) to matter.
    NotCloseEnough,
    /// Close enough (`$ac`-`$bb`) to enable player collision.
    CollisionEnabled { state_width: u8 },
    /// Rolled past the player (`>= $bc`) - remove it.
    Removed(RemovedEnemy),
}

/// The full result of one [`roller_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollerRoutine01Result {
    pub sprite: u8,
    /// `Some($2e)` only once the roller has grown to one of its 2
    /// largest sizes (`cpy #$02` / `bcc`) - real ASM leaves `ENEMY_
    /// SCORE_COLLISION` untouched otherwise.
    pub score_collision: Option<u8>,
    pub position: UpdatedEnemyPos,
    pub outcome: RollerRoutine01Outcome,
}

/// Native port of `roller_routine_01` (`$8f94`).
#[allow(clippy::too_many_arguments)]
pub fn roller_routine_01(
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    state_width: u8,
) -> RollerRoutine01Result {
    let size_index = roller_sprite_size_index(enemy_y_pos);
    let sprite = 0x99 + size_index;
    let score_collision = if size_index >= 2 { Some(0x2E) } else { None };

    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, enemy_y_pos, y_vel_accum, y_vel_fract, y_vel_fast);

    let outcome = if position.y.pos < 0xAC {
        RollerRoutine01Outcome::NotCloseEnough
    } else if position.y.pos >= 0xBC {
        RollerRoutine01Outcome::Removed(remove_enemy())
    } else {
        RollerRoutine01Outcome::CollisionEnabled { state_width: enable_enemy_collision(state_width) }
    };

    RollerRoutine01Result { sprite, score_collision, position, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_sets_fixed_initial_y_and_advances() {
        let r = roller_routine_00(3);
        assert_eq!(r.y_pos, 0x72);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn size_index_walks_down_from_largest_matching_cutoff() {
        assert_eq!(roller_sprite_size_index(0xA0), 3);
        assert_eq!(roller_sprite_size_index(0x9C), 3);
        assert_eq!(roller_sprite_size_index(0x90), 2);
        assert_eq!(roller_sprite_size_index(0x80), 1);
        assert_eq!(roller_sprite_size_index(0x50), 0);
    }

    #[test]
    fn routine_01_small_sizes_never_set_score_collision() {
        let r = roller_routine_01(0x80, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, 0x00);
        assert_eq!(r.sprite, 0x99 + 1);
        assert_eq!(r.score_collision, None);
    }

    #[test]
    fn routine_01_large_sizes_set_score_collision() {
        let r = roller_routine_01(0xA0, 0, 0, 0x50, 0, 0, 0, 0, 0, 0, 0x00);
        assert_eq!(r.sprite, 0x99 + 3);
        assert_eq!(r.score_collision, Some(0x2E));
    }

    #[test]
    fn routine_01_not_close_enough_below_0xac() {
        let r = roller_routine_01(0xA0, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0x00, 0x00);
        assert_eq!(r.position.y.pos, 0xA0);
        assert_eq!(r.outcome, RollerRoutine01Outcome::NotCloseEnough);
    }

    #[test]
    fn routine_01_enables_collision_in_the_ac_to_bb_band() {
        let r = roller_routine_01(0xAC, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0x00, 0x81);
        assert_eq!(r.position.y.pos, 0xAC);
        assert_eq!(r.outcome, RollerRoutine01Outcome::CollisionEnabled { state_width: enable_enemy_collision(0x81) });
    }

    #[test]
    fn routine_01_removed_at_or_past_0xbc() {
        let r = roller_routine_01(0xBC, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0x00, 0x00);
        assert_eq!(r.position.y.pos, 0xBC);
        assert_eq!(r.outcome, RollerRoutine01Outcome::Removed(remove_enemy()));
    }
}
