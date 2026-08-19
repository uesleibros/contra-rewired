//! Native port of Contra's enemy velocity-integration chain, `src/
//! bank7.asm`, CPU `$e809`-`$e969`: [`update_enemy_pos`] (`$e837`) is
//! the enemy-object analog of `player_physics::integrate_y_position` -
//! applies each axis's fixed-point velocity to its position (shared
//! core: [`update_enemy_x_pos`]/[`update_enemy_y_pos`], `$e90a`/`$e8ec`),
//! adds camera scroll to whichever axis matches `LEVEL_SCROLLING_TYPE`
//! (`update_enemy_x_pos_with_scroll`/`update_enemy_y_pos_with_scroll`),
//! and removes the enemy ([`remove_enemy`], `$e809`) if either axis ends
//! up off-screen. Also carries [`set_enemy_y_velocity_to_0`] (`$e8d0`),
//! a small neighboring routine in the same address range reused by
//! `soldier_stop_y_set_x_velocity` (see `soldier.rs`), and [`enemy_
//! routine_remove_enemy`] (`$e806`) - a real, shared enemy-routine-table
//! entry (used by dozens of enemy types, not just the soldier) that
//! unconditionally scrolls-then-removes an enemy.
//!
//! ## Real control flow: both axes update, but a removal short-circuits
//! the second
//!
//! For a horizontal-scrolling level (`LEVEL_SCROLLING_TYPE == 0`, the
//! outdoor/typical case), X updates *with* scroll first; if that alone
//! pushes it off-screen (`< $08`), the enemy is removed immediately and
//! **Y is never touched at all** (not even the plain, no-scroll Y
//! update). Only if X survives does Y update (no scroll - only one axis
//! ever gets the scroll addition) and get its own removal check
//! (`>= $e8`). A vertical level does the mirror image: Y-with-scroll
//! first, X second. Ported exactly this way - [`update_enemy_pos`]'s
//! `removed` field being `Some` means the *other* axis's returned
//! [`AxisUpdate`] is simply the original untouched input, not a
//! computed value, matching the real ASM never running that `jsr`.

/// One axis's position + velocity-accumulator state after one frame's
/// integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisUpdate {
    pub pos: u8,
    pub vel_accum: u8,
}

/// The shared fixed-point integrator both `update_enemy_x_pos` and
/// `update_enemy_y_pos` use: `vel_accum += vel_fract` (with carry),
/// `pos += vel_fast + that carry`.
fn update_axis(pos: u8, vel_accum: u8, vel_fract: u8, vel_fast: u8) -> AxisUpdate {
    let (new_accum, carry) = vel_accum.overflowing_add(vel_fract);
    let new_pos = pos.wrapping_add(vel_fast).wrapping_add(carry as u8);
    AxisUpdate { pos: new_pos, vel_accum: new_accum }
}

/// Native port of `update_enemy_x_pos` (`$e90a`).
pub fn update_enemy_x_pos(pos: u8, vel_accum: u8, vel_fract: u8, vel_fast: u8) -> AxisUpdate {
    update_axis(pos, vel_accum, vel_fract, vel_fast)
}

/// Native port of `update_enemy_y_pos` (`$e8ec`).
pub fn update_enemy_y_pos(pos: u8, vel_accum: u8, vel_fract: u8, vel_fast: u8) -> AxisUpdate {
    update_axis(pos, vel_accum, vel_fract, vel_fast)
}

/// Native port of `update_enemy_x_pos_with_scroll` (`$e900`).
pub fn update_enemy_x_pos_with_scroll(pos: u8, vel_accum: u8, vel_fract: u8, vel_fast: u8, frame_scroll: u8) -> AxisUpdate {
    let u = update_axis(pos, vel_accum, vel_fract, vel_fast);
    AxisUpdate { pos: u.pos.wrapping_sub(frame_scroll), vel_accum: u.vel_accum }
}

/// Native port of `update_enemy_y_pos_with_scroll` (`$e8e2`).
pub fn update_enemy_y_pos_with_scroll(pos: u8, vel_accum: u8, vel_fract: u8, vel_fast: u8, frame_scroll: u8) -> AxisUpdate {
    let u = update_axis(pos, vel_accum, vel_fract, vel_fast);
    AxisUpdate { pos: u.pos.wrapping_add(frame_scroll), vel_accum: u.vel_accum }
}

/// Native port of `remove_enemy`/`set_sprite_0` (`$e809`-`$e813`) - real
/// ASM: two constant stores, `ENEMY_ROUTINE,x = 0` and `ENEMY_SPRITES,x
/// = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemovedEnemy {
    pub routine: u8,
    pub sprites: u8,
}

pub fn remove_enemy() -> RemovedEnemy {
    RemovedEnemy::default()
}

/// Native port of `update_enemy_x_pos_rem_off_screen` (`$e842`) - a
/// real, shared "X-only" position updater reused by several enemy
/// types whose own routines only ever move along X (`set_enemy_falling_
/// arc_pos`'s own tail, `mini_ufo_routine_01`): applies X velocity with
/// scroll, then removes the enemy if that alone pushed it off the left
/// edge.
pub fn update_enemy_x_pos_rem_off_screen(pos: u8, vel_accum: u8, vel_fract: u8, vel_fast: u8, frame_scroll: u8) -> (AxisUpdate, Option<RemovedEnemy>) {
    let x = update_enemy_x_pos_with_scroll(pos, vel_accum, vel_fract, vel_fast, frame_scroll);
    let removed = if x.pos < 0x08 { Some(remove_enemy()) } else { None };
    (x, removed)
}

/// The result of one [`enemy_routine_remove_enemy`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRoutineRemoveEnemyResult {
    pub scroll: crate::enemy::add_scroll_to_enemy_pos::ScrolledEnemyPos,
    pub removed: RemovedEnemy,
}

/// Native port of `enemy_routine_remove_enemy` (`$e806`) - a real, shared
/// enemy-routine-table entry: `jsr add_scroll_to_enemy_pos` (real ASM
/// keeps its position-writing side effect even though the *result* is
/// about to be discarded - `add_scroll_to_enemy_pos` may itself already
/// trigger a nested removal if its own scroll pushes the enemy off-
/// screen, harmlessly redundant with the unconditional [`remove_enemy`]
/// call right after it either way), then falls straight into [`remove_
/// enemy`] unconditionally.
pub fn enemy_routine_remove_enemy(
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
) -> EnemyRoutineRemoveEnemyResult {
    let scroll = crate::enemy::add_scroll_to_enemy_pos::add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
    let removed = remove_enemy();
    EnemyRoutineRemoveEnemyResult { scroll, removed }
}

/// Native port of `set_enemy_y_velocity_to_0` (`$e8d0`) - real ASM:
/// two constant stores, `ENEMY_Y_VELOCITY_FRACT,x = 0` and
/// `ENEMY_Y_VELOCITY_FAST,x = 0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ZeroedVelocity {
    pub vel_fract: u8,
    pub vel_fast: u8,
}

pub fn set_enemy_y_velocity_to_0() -> ZeroedVelocity {
    ZeroedVelocity::default()
}

/// Native port of `set_enemy_x_velocity_to_0` (`$e8d9`) - real ASM:
/// two constant stores, `ENEMY_X_VELOCITY_FRACT,x = 0` and
/// `ENEMY_X_VELOCITY_FAST,x = 0`.
pub fn set_enemy_x_velocity_to_0() -> ZeroedVelocity {
    ZeroedVelocity::default()
}

/// The full result of one frame's `update_enemy_pos` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdatedEnemyPos {
    pub x: AxisUpdate,
    pub y: AxisUpdate,
    pub removed: Option<RemovedEnemy>,
}

/// Native port of `update_enemy_pos` (`$e837`) - see this module's doc
/// comment for the real control-flow subtlety (a removal on the first-
/// checked axis skips the second axis's update entirely).
#[allow(clippy::too_many_arguments)]
pub fn update_enemy_pos(
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
) -> UpdatedEnemyPos {
    if level_scrolling_type == 0 {
        let x = update_enemy_x_pos_with_scroll(x_pos, x_vel_accum, x_vel_fract, x_vel_fast, frame_scroll);
        if x.pos < 0x08 {
            return UpdatedEnemyPos { x, y: AxisUpdate { pos: y_pos, vel_accum: y_vel_accum }, removed: Some(remove_enemy()) };
        }
        let y = update_enemy_y_pos(y_pos, y_vel_accum, y_vel_fract, y_vel_fast);
        let removed = if y.pos >= 0xE8 { Some(remove_enemy()) } else { None };
        UpdatedEnemyPos { x, y, removed }
    } else {
        let y = update_enemy_y_pos_with_scroll(y_pos, y_vel_accum, y_vel_fract, y_vel_fast, frame_scroll);
        if y.pos >= 0xE8 {
            return UpdatedEnemyPos { x: AxisUpdate { pos: x_pos, vel_accum: x_vel_accum }, y, removed: Some(remove_enemy()) };
        }
        let x = update_enemy_x_pos(x_pos, x_vel_accum, x_vel_fract, x_vel_fast);
        let removed = if x.pos < 0x08 { Some(remove_enemy()) } else { None };
        UpdatedEnemyPos { x, y, removed }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_axis_carries_fractional_overflow_into_position() {
        // vel_accum=$f0, vel_fract=$20 -> accum wraps to $10 with carry;
        // pos gets vel_fast + 1.
        let r = update_enemy_x_pos(0x50, 0xF0, 0x20, 0x03);
        assert_eq!(r, AxisUpdate { pos: 0x54, vel_accum: 0x10 });
    }

    #[test]
    fn update_axis_without_overflow_adds_plain_vel_fast() {
        let r = update_enemy_x_pos(0x50, 0x10, 0x20, 0x03);
        assert_eq!(r, AxisUpdate { pos: 0x53, vel_accum: 0x30 });
    }

    #[test]
    fn with_scroll_variants_apply_scroll_after_velocity() {
        let plain = update_enemy_x_pos(0x50, 0x10, 0x20, 0x03);
        let scrolled = update_enemy_x_pos_with_scroll(0x50, 0x10, 0x20, 0x03, 0x05);
        assert_eq!(scrolled.pos, plain.pos.wrapping_sub(0x05));
        assert_eq!(scrolled.vel_accum, plain.vel_accum);

        let plain_y = update_enemy_y_pos(0x50, 0x10, 0x20, 0x03);
        let scrolled_y = update_enemy_y_pos_with_scroll(0x50, 0x10, 0x20, 0x03, 0x05);
        assert_eq!(scrolled_y.pos, plain_y.pos.wrapping_add(0x05));
    }

    #[test]
    fn horizontal_level_removal_on_x_skips_y_update_entirely() {
        // x_pos=0x0A, frame_scroll=0x05 -> 0x0A - 0x05 = 0x05, < 0x08.
        let r = update_enemy_pos(0, 0x05, 0x0A, 0, 0, 0, 0x50, 0x10, 0x20, 0x03);
        assert!(r.removed.is_some());
        assert!(r.x.pos < 0x08);
        // y untouched - matches the *original* input exactly, not a
        // computed value (the real ASM never runs that jsr).
        assert_eq!(r.y, AxisUpdate { pos: 0x50, vel_accum: 0x10 });
    }

    #[test]
    fn horizontal_level_x_survives_then_y_gets_checked() {
        let r = update_enemy_pos(0, 0x01, 0x50, 0, 0, 0, 0xE0, 0, 0, 0x10);
        assert!(r.removed.is_some());
        assert_eq!(r.y.pos, 0xF0); // 0xE0 + 0x10, off the bottom
    }

    #[test]
    fn horizontal_level_both_axes_survive_no_removal() {
        let r = update_enemy_pos(0, 0x01, 0x50, 0, 0, 0, 0x50, 0, 0, 0x01);
        assert!(r.removed.is_none());
        assert_eq!(r.x.pos, 0x4F); // 0x50 - 0x01 scroll
        assert_eq!(r.y.pos, 0x51); // 0x50 + 0x01 velocity
    }

    #[test]
    fn vertical_level_removal_on_y_skips_x_update_entirely() {
        let r = update_enemy_pos(1, 0x10, 0x50, 0x10, 0, 0, 0xE0, 0, 0, 0);
        assert!(r.removed.is_some());
        assert!(r.y.pos >= 0xE8);
        assert_eq!(r.x, AxisUpdate { pos: 0x50, vel_accum: 0x10 });
    }

    #[test]
    fn remove_enemy_zeroes_both_fields() {
        assert_eq!(remove_enemy(), RemovedEnemy { routine: 0, sprites: 0 });
    }

    #[test]
    fn update_enemy_x_pos_rem_off_screen_survives_mid_screen() {
        let (x, removed) = update_enemy_x_pos_rem_off_screen(0x50, 0, 0, 0, 0x01);
        assert_eq!(x.pos, 0x4F);
        assert_eq!(removed, None);
    }

    #[test]
    fn update_enemy_x_pos_rem_off_screen_removes_past_the_left_edge() {
        let (x, removed) = update_enemy_x_pos_rem_off_screen(0x0A, 0, 0, 0, 0x05);
        assert_eq!(x.pos, 0x05);
        assert_eq!(removed, Some(remove_enemy()));
    }

    #[test]
    fn set_enemy_y_velocity_to_0_zeroes_both_fields() {
        assert_eq!(set_enemy_y_velocity_to_0(), ZeroedVelocity { vel_fract: 0, vel_fast: 0 });
    }

    #[test]
    fn set_enemy_x_velocity_to_0_zeroes_both_fields() {
        assert_eq!(set_enemy_x_velocity_to_0(), ZeroedVelocity { vel_fract: 0, vel_fast: 0 });
    }

    #[test]
    fn enemy_routine_remove_enemy_scrolls_then_removes() {
        use crate::enemy::add_scroll_to_enemy_pos::add_scroll_to_enemy_pos;
        let r = enemy_routine_remove_enemy(0, 0x02, 0x50, 0x60);
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
        assert_eq!(r.removed, remove_enemy());
    }

    #[test]
    fn enemy_routine_remove_enemy_keeps_the_scrolled_position_even_when_that_scroll_itself_would_remove() {
        // x_pos=0x0A, frame_scroll=0x05 -> 0x0A-0x05=0x05, < 0x08: add_scroll_to_enemy_pos's
        // own removal condition, but the position write still happened.
        let r = enemy_routine_remove_enemy(0, 0x05, 0x0A, 0x60);
        assert!(r.scroll.should_remove);
        assert_eq!(r.scroll.x_pos, 0x05);
    }
}
