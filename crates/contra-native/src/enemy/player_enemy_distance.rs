//! Native port of Contra's "which player is closer to this enemy"
//! utility: `player_enemy_x_dist`/`player_enemy_y_dist` (`src/bank7.asm`,
//! CPU `$ecf5`-`$ed4b`, sharing a tail at `lda_closer_distance`,
//! `$ed24`). The single most-reused routine ported in this crate so far
//! (21 real `jsr player_enemy_x_dist` call sites alone, across nearly
//! every enemy AI routine in `bank0.asm`) - almost any enemy that aims,
//! chases, or decides which side of the screen to act on calls one of
//! these first.
//!
//! Both compute the absolute distance from each player to the enemy on
//! one axis, then pick whichever is smaller - except a player not in
//! `PLAYER_STATE_NORMAL` has their real distance overridden with a
//! sentinel (`$fe` for player 1, `$ff` for player 2) *before* the
//! comparison, so an inactive/dead/falling player is never picked as
//! "closer" unless *both* players are inactive (in which case player 1's
//! sentinel, `$fe`, still wins over player 2's `$ff` - real, deliberate
//! tie-breaking behavior, not an oversight).

use crate::enemy::quadrant_aim_dir::PLAYER_STATE_NORMAL;

// Live-verified via `VERIFY_PLAYER_ENEMY_DIST=1` in `crates/contra-nes/
// examples/dump_frames.rs`: 207 real calls (both axes) across a
// 9000-frame session, zero mismatches after fixing one bug in the
// verification harness itself (the harness had swapped which RAM
// address fed the X-axis vs Y-axis branch - not a bug in this port).

/// Sentinel distance used when a player isn't in
/// [`PLAYER_STATE_NORMAL`] - real ASM: `ldy #$fe`/`sty $08` for player 1.
const PLAYER_1_INACTIVE_SENTINEL: u8 = 0xFE;
/// Same, for player 2 (`ldy #$ff`/`sty $09`) - larger than player 1's
/// sentinel so player 1 wins the "closer" comparison when both are
/// inactive.
const PLAYER_2_INACTIVE_SENTINEL: u8 = 0xFF;

/// Which player is closer to the enemy on the queried axis, and by how
/// much (or a sentinel distance - see this module's doc comment - if
/// that player wasn't in a normal state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosestPlayer {
    pub player_index: u8,
    pub distance: u8,
}

/// `lda_closer_distance`'s own comparison (`$ed24`-`$ed4b`): picks the
/// smaller of the two already-computed (and already sentinel-overridden)
/// per-player distances.
fn closest_of(dist_p1: u8, dist_p2: u8) -> ClosestPlayer {
    if dist_p2 < dist_p1 {
        ClosestPlayer { player_index: 1, distance: dist_p2 }
    } else {
        ClosestPlayer { player_index: 0, distance: dist_p1 }
    }
}

fn per_player_distance(sprite_pos: [u8; 2], enemy_pos: u8, player_state: [u8; 2]) -> (u8, u8) {
    let d0 = if player_state[0] == PLAYER_STATE_NORMAL { sprite_pos[0].abs_diff(enemy_pos) } else { PLAYER_1_INACTIVE_SENTINEL };
    let d1 = if player_state[1] == PLAYER_STATE_NORMAL { sprite_pos[1].abs_diff(enemy_pos) } else { PLAYER_2_INACTIVE_SENTINEL };
    (d0, d1)
}

/// Native port of `player_enemy_x_dist` (`$ecf5`).
pub fn player_enemy_x_dist(sprite_x_pos: [u8; 2], enemy_x_pos: u8, player_state: [u8; 2]) -> ClosestPlayer {
    let (d0, d1) = per_player_distance(sprite_x_pos, enemy_x_pos, player_state);
    closest_of(d0, d1)
}

/// Native port of `player_enemy_y_dist` (`$ed0e`).
pub fn player_enemy_y_dist(sprite_y_pos: [u8; 2], enemy_y_pos: u8, player_state: [u8; 2]) -> ClosestPlayer {
    let (d0, d1) = per_player_distance(sprite_y_pos, enemy_y_pos, player_state);
    closest_of(d0, d1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_player_1_when_closer() {
        let r = player_enemy_x_dist([100, 50], 80, [1, 1]);
        assert_eq!(r, ClosestPlayer { player_index: 0, distance: 20 });
    }

    #[test]
    fn picks_player_2_when_closer() {
        let r = player_enemy_x_dist([50, 100], 80, [1, 1]);
        assert_eq!(r, ClosestPlayer { player_index: 1, distance: 20 });
    }

    #[test]
    fn inactive_player_1_falls_back_to_player_2_even_if_computed_distance_would_be_smaller() {
        // p1 would be distance 0 (right on top of the enemy) but isn't
        // normal - must not be picked.
        let r = player_enemy_x_dist([80, 80], 80, [0, 1]);
        assert_eq!(r, ClosestPlayer { player_index: 1, distance: 0 });
    }

    #[test]
    fn inactive_player_2_falls_back_to_player_1() {
        let r = player_enemy_x_dist([80, 80], 80, [1, 0]);
        assert_eq!(r, ClosestPlayer { player_index: 0, distance: 0 });
    }

    #[test]
    fn both_inactive_defaults_to_player_1_with_the_0xfe_sentinel() {
        // Matches the disassembly's own documented behavior for this
        // exact case ("if both players are in non-normal state, a is
        // set to #$fe").
        let r = player_enemy_x_dist([10, 200], 5, [0, 0]);
        assert_eq!(r, ClosestPlayer { player_index: 0, distance: 0xFE });
    }

    #[test]
    fn y_axis_variant_behaves_identically_on_its_own_axis() {
        let r = player_enemy_y_dist([30, 90], 40, [1, 1]);
        assert_eq!(r, ClosestPlayer { player_index: 0, distance: 10 });
    }
}
