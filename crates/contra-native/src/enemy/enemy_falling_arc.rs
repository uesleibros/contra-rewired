//! Native port of `set_enemy_falling_arc_pos` (`src/bank7.asm`, CPU
//! `$ee08`-`$ee2e`) - a real, shared "parabolic falling arc" position
//! updater that repurposes `ENEMY_VAR_1`/`_2`/`_3`/`_4`/`_b` as two
//! accumulator pairs on top of the normal Y-velocity fields: `VAR_2`/
//! `VAR_4` track a fractional horizontal-ish delta (the caller grows
//! `VAR_4` itself frame-to-frame to make the arc accelerate), `VAR_3`/
//! `VAR_B` are that pair's own carry destination, and `VAR_1`/`Y_VEL_
//! ACCUM`/`Y_VELOCITY_FRACT`/`Y_VELOCITY_FAST` integrate the actual fall
//! speed the same way [`crate::enemy::update_enemy_pos::update_enemy_y_pos`]
//! does. Used by `grenade_routine_01` (indoor grenades) and, once
//! ported, the outdoor weapon item's own falling-arc drop.
//!
//! Real ASM's `ENEMY_Y_POS` write and the X-position update are
//! **skipped entirely** if the Y-side accumulator (`VAR_1`) overflows
//! past `$f0` (fallen off the bottom of the screen) - ported the same
//! way [`crate::enemy::update_enemy_pos::update_enemy_pos`] short-
//! circuits its own second axis on an early removal.

use crate::enemy::update_enemy_pos::{remove_enemy, update_enemy_x_pos_with_scroll, AxisUpdate, RemovedEnemy};

/// The 3 real accumulator fields this routine writes back every call,
/// regardless of outcome (`VAR_2` unconditionally; `VAR_1`/`VAR_3`/
/// `Y_VEL_ACCUM` before the removal check even runs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallingArcVars {
    pub var_1: u8,
    pub var_2: u8,
    pub var_3: u8,
    pub y_vel_accum: u8,
}

/// One [`set_enemy_falling_arc_pos`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetEnemyFallingArcPosOutcome {
    /// `VAR_1` (the integrated fall accumulator) reached `$f0` or past -
    /// real ASM's own Y-side removal check; `ENEMY_Y_POS` and the X
    /// update are never touched this call.
    RemovedFallenOffBottom(RemovedEnemy),
    /// Y survived; the X update then pushed it off the left edge
    /// (`< $08`) - `y_pos` was still written before this happened.
    RemovedOffScreenLeft { y_pos: u8, removed: RemovedEnemy },
    /// Both axes survived.
    Position { y_pos: u8, x: AxisUpdate },
}

/// The full result of one [`set_enemy_falling_arc_pos`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetEnemyFallingArcPosResult {
    pub vars: FallingArcVars,
    pub outcome: SetEnemyFallingArcPosOutcome,
}

/// Native port of `set_enemy_falling_arc_pos` (`$ee08`).
#[allow(clippy::too_many_arguments)]
pub fn set_enemy_falling_arc_pos(
    var_1: u8,
    var_2: u8,
    var_3: u8,
    var_4: u8,
    var_b: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
) -> SetEnemyFallingArcPosResult {
    let (var_2, carry1) = var_2.overflowing_add(var_4);
    let var_3 = var_3.wrapping_add(var_b).wrapping_add(carry1 as u8); // carry chain ends here - real ASM never propagates it further

    let (y_vel_accum, carry3) = y_vel_accum.overflowing_add(y_vel_fract);
    let var_1 = var_1.wrapping_add(y_vel_fast).wrapping_add(carry3 as u8);

    let vars = FallingArcVars { var_1, var_2, var_3, y_vel_accum };

    if var_1 >= 0xF0 {
        return SetEnemyFallingArcPosResult { vars, outcome: SetEnemyFallingArcPosOutcome::RemovedFallenOffBottom(remove_enemy()) };
    }

    let y_pos = var_1.wrapping_add(var_3);
    let x = update_enemy_x_pos_with_scroll(x_pos, x_vel_accum, x_vel_fract, x_vel_fast, frame_scroll);

    let outcome = if x.pos < 0x08 {
        SetEnemyFallingArcPosOutcome::RemovedOffScreenLeft { y_pos, removed: remove_enemy() }
    } else {
        SetEnemyFallingArcPosOutcome::Position { y_pos, x }
    };

    SetEnemyFallingArcPosResult { vars, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_2_and_var_3_accumulate_with_carry_chained_between_them() {
        // var_2 = 0xF0 + 0x20 overflows -> carry 1 into var_3.
        let r = set_enemy_falling_arc_pos(0x00, 0xF0, 0x05, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50, 0, 0, 0);
        assert_eq!(r.vars.var_2, 0x10);
        assert_eq!(r.vars.var_3, 0x06);
    }

    #[test]
    fn y_side_integrates_like_the_normal_axis_updater() {
        // y_vel_accum overflow carries into var_1, same as update_enemy_pos's own fixed-point integrator.
        let r = set_enemy_falling_arc_pos(0x10, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x20, 0x03, 0x00, 0x50, 0, 0, 0);
        assert_eq!(r.vars.y_vel_accum, 0x10);
        assert_eq!(r.vars.var_1, 0x14); // 0x10 + 0x03 fast + 1 carry
    }

    #[test]
    fn removed_when_var_1_reaches_0xf0_before_touching_position() {
        let r = set_enemy_falling_arc_pos(0xE0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x50, 0, 0, 0);
        assert_eq!(r.vars.var_1, 0xF0);
        assert_eq!(r.outcome, SetEnemyFallingArcPosOutcome::RemovedFallenOffBottom(remove_enemy()));
    }

    #[test]
    fn survives_at_the_removal_boundary() {
        // var_1 lands at 0xef (one less than the 0xf0 removal threshold).
        let r = set_enemy_falling_arc_pos(0xDF, 0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x50, 0, 0, 0);
        assert_eq!(r.vars.var_1, 0xEF);
        match r.outcome {
            SetEnemyFallingArcPosOutcome::Position { y_pos, .. } => assert_eq!(y_pos, 0xEF_u8.wrapping_add(0x10)),
            other => panic!("expected Position, got {other:?}"),
        }
    }

    #[test]
    fn removed_off_screen_left_after_y_survives() {
        // x_pos - frame_scroll ends up < 0x08.
        let r = set_enemy_falling_arc_pos(0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x0A, 0, 0, 0);
        match r.outcome {
            SetEnemyFallingArcPosOutcome::RemovedOffScreenLeft { y_pos, removed } => {
                assert_eq!(y_pos, 0x10);
                assert_eq!(removed, remove_enemy());
            }
            other => panic!("expected RemovedOffScreenLeft, got {other:?}"),
        }
    }
}
