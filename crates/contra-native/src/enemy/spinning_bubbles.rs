//! Native port of the spinning bubbles projectile (`src/bank0.asm`,
//! `spinning_bubbles_routine_ptr_tbl`, `$a05b`-`$a094`): `_00` launches
//! it toward the closer player with a random initial speed, `_01`
//! animates its spin and periodically re-aims toward the player (up to
//! 20 times per bubble) via `crate::enemy::quadrant_aim_dir::aim_var_1_
//! for_quadrant_aim_dir_01` - a one-step-per-call rotation that visibly
//! sweeps rather than snapping. Previously assumed blocked behind the
//! rotation/aiming subsystem the same way `sniper_02`-`_05` were -
//! unblocked in the same pass. `spinning_bubbles_routine_ptr_tbl`
//! entries `2`-`4` (explosion/removal) are the same real shared
//! `bank7.asm` routines most enemy families use and aren't ported here.

use crate::enemy::add_with_enemy_pos::set_08_09_to_enemy_pos;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::quadrant_aim_dir::{aim_var_1_for_quadrant_aim_dir_01, get_quadrant_aim_dir_for_player, get_rotate_dir, QUADRANT_AIM_DIR_01};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};
use crate::physics::bullet_physics::calc_bullet_velocities;

/// `spinning_bubbles_speed_tbl` (`$a086`, 4 bytes) - initial bullet speed
/// code, indexed by a random `0..4` value (`FRAME_COUNTER & 3`) rolled at
/// launch. Values are `bullet_velocity_adjust_XX` selectors (`.75x`,
/// `1.25x`, `1.5x`, `1.62x`), not a simple linear scale.
const SPINNING_BUBBLES_SPEED_TBL: [u8; 4] = [0x01, 0x03, 0x04, 0x05];

/// The full result of one [`spinning_bubbles_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinningBubblesRoutine00Result {
    /// `ENEMY_VAR_2` - the closer player's index, remembered for `_01`'s
    /// own re-aiming calls.
    pub var_2: u8,
    /// `ENEMY_ATTRIBUTES` - a random `0..4` value that both picks the
    /// initial speed here and (in `_01`) the spin-animation threshold.
    pub attributes: u8,
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    /// `ENEMY_VAR_1` - the aim direction [`crate::enemy::quadrant_aim_
    /// dir::get_rotate_dir`] computed (already reflected into the
    /// enemy's own facing convention, not the raw table lookup).
    pub aim_dir: u8,
    /// Always `0x20`.
    pub attack_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `spinning_bubbles_routine_00` (`$a05b`) - launches
/// toward the closer player: rolls a random initial speed code, aims via
/// `quadrant_aim_dir_01`, sets velocity, and stores the rotated aim
/// direction for `_01`'s own re-aiming to build on.
#[allow(clippy::too_many_arguments)]
pub fn spinning_bubbles_routine_00(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_var_1: u8,
    frame_counter: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_routine: u8,
) -> SpinningBubblesRoutine00Result {
    let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
    let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);

    let attributes = frame_counter & 0x03;
    let speed_code = SPINNING_BUBBLES_SPEED_TBL[attributes as usize];

    let aimed = get_quadrant_aim_dir_for_player(
        source_y,
        source_x,
        closest.player_index,
        player_state,
        sprite_y_pos,
        sprite_x_pos,
        level_location_type,
        &QUADRANT_AIM_DIR_01,
    );
    let v = calc_bullet_velocities(aimed.aim_dir, speed_code, aimed.quadrant);
    let rotate = get_rotate_dir(aimed.aim_dir, aimed.quadrant, 0x01, enemy_var_1);

    SpinningBubblesRoutine00Result {
        var_2: closest.player_index,
        attributes,
        y_velocity: (v.frac_y, v.fast_y),
        x_velocity: (v.frac_x, v.fast_x),
        aim_dir: rotate.new_aim_dir,
        attack_delay: 0x20,
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// `spinning_bullet_spin_tbl` (`$a091`, 4 bytes) - animation-delay
/// threshold before the spin frame advances, indexed by `ENEMY_
/// ATTRIBUTES` (the same random `0..4` value `_00` rolled - lower values
/// spin faster).
const SPINNING_BULLET_SPIN_TBL: [u8; 4] = [0x08, 0x06, 0x04, 0x02];

/// The result of [`spinning_bubbles_routine_01`]'s own animation step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinningBubblesAnimation {
    pub animation_delay: u8,
    pub enemy_frame: u8,
    pub sprite: u8,
}

fn spinning_bubbles_animate(attributes: u8, enemy_animation_delay: u8, enemy_frame: u8) -> SpinningBubblesAnimation {
    let delay = enemy_animation_delay.wrapping_add(1);
    let threshold = SPINNING_BULLET_SPIN_TBL[attributes as usize];

    let (animation_delay, enemy_frame) = if delay >= threshold {
        let next_frame = enemy_frame.wrapping_add(1);
        (0x00, if next_frame >= 0x06 { 0x00 } else { next_frame })
    } else {
        (delay, enemy_frame)
    };

    SpinningBubblesAnimation { animation_delay, enemy_frame, sprite: enemy_frame.wrapping_add(0x6D) }
}

/// `spinning_bullet_vel_tbl` (`$a03c`, 30 entries/60 bytes) - a full
/// sine-wave velocity table over the 24-step `quadrant_aim_dir_01` wheel
/// (indices `0..24`), with 6 extra entries (`24..30`) letting the real
/// ASM read the X velocity from the *same* array at a `+6`-entry offset
/// from the Y velocity - `cos(dir) == sin(dir + 6)` for a 24-step wheel
/// (a quarter turn = 6 steps), so the ROM never stores a separate cosine
/// table at all. Ported as the literal overlapping-window array rather
/// than two conceptually-separate tables, matching this crate's standing
/// policy for this exact kind of real data-table trick (see `spiked_
/// wall::SPIKED_WALL_DESTROYED_DATA_TBL`'s own doc comment).
const SPINNING_BULLET_VEL_TBL: [(u8, u8); 30] = [
    (0x00, 0x00), // 0
    (0x63, 0x00), // .39
    (0xC0, 0x00), // .75
    (0x0F, 0x01), // 1.06
    (0x4B, 0x01), // 1.29
    (0x72, 0x01), // 1.44
    (0x7E, 0x01), // 1.49
    (0x72, 0x01), // 1.44
    (0x4B, 0x01), // 1.29
    (0x0F, 0x01), // 1.06
    (0xC0, 0x00), // .75
    (0x63, 0x00), // .39
    (0x00, 0x00), // 0
    (0x9D, 0xFF), // -.39
    (0x40, 0xFF), // -.75
    (0xF1, 0xFE), // -1.06
    (0xB5, 0xFE), // -1.29
    (0x8E, 0xFE), // -1.44
    (0x82, 0xFE), // -1.49
    (0x8E, 0xFE), // -1.44
    (0xB5, 0xFE), // -1.29
    (0xF1, 0xFE), // -1.06
    (0x40, 0xFF), // -.75
    (0x9D, 0xFF), // -.39
    (0x00, 0x00), // 0 (wraps back to entry 0's value - the +6 window's own start)
    (0x63, 0x00), // .39
    (0xC0, 0x00), // .75
    (0x0F, 0x01), // 1.06
    (0x4B, 0x01), // 1.29
    (0x72, 0x01), // 1.44
];

/// The real, branchy result of [`spinning_bubbles_routine_01`]'s own
/// periodic re-aiming attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinningBubblesRoutine01Outcome {
    /// `ENEMY_VAR_3 >= 0x14` - already readjusted 20 times, never
    /// attempts again.
    NoReadjustment,
    /// Still within the 20-readjustment budget, but the delay between
    /// attempts hasn't elapsed.
    WaitingToReadjust { attack_delay: u8 },
    /// Attempted a readjustment, but the bubble was already aiming at the
    /// target direction - velocity untouched.
    ReadjustedAlreadyAiming { var_3: u8, attack_delay: u8 },
    /// Rotated one step toward the target: forces the fast spin-animation
    /// threshold (`ENEMY_ATTRIBUTES |= 3`) and re-derives velocity from
    /// the new aim direction via `SPINNING_BULLET_VEL_TBL`.
    Readjusted { var_3: u8, attack_delay: u8, attributes: u8, aim_dir: u8, y_velocity: (u8, u8), x_velocity: (u8, u8) },
}

/// The full result of one [`spinning_bubbles_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpinningBubblesRoutine01Result {
    pub animation: SpinningBubblesAnimation,
    pub position: UpdatedEnemyPos,
    pub outcome: SpinningBubblesRoutine01Outcome,
}

/// Native port of `spinning_bubbles_routine_01` (`$a094`) - animates the
/// spin, applies velocity, and (up to 20 times per bubble, gated by its
/// own delay) rotates one step closer to the player, re-deriving velocity
/// from `SPINNING_BULLET_VEL_TBL` whenever that rotation actually
/// changes the aim direction.
#[allow(clippy::too_many_arguments)]
pub fn spinning_bubbles_routine_01(
    attributes: u8,
    enemy_animation_delay: u8,
    enemy_frame: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    enemy_y_pos: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    enemy_var_3: u8,
    enemy_attack_delay: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
) -> SpinningBubblesRoutine01Result {
    let animation = spinning_bubbles_animate(attributes, enemy_animation_delay, enemy_frame);

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

    let outcome = if enemy_var_3 >= 0x14 {
        SpinningBubblesRoutine01Outcome::NoReadjustment
    } else {
        let attack_delay = enemy_attack_delay.wrapping_sub(1);
        if attack_delay != 0 {
            SpinningBubblesRoutine01Outcome::WaitingToReadjust { attack_delay }
        } else {
            let var_3 = enemy_var_3.wrapping_add(1);
            let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
            let rotate = aim_var_1_for_quadrant_aim_dir_01(
                source_y,
                source_x,
                enemy_var_2,
                player_state,
                sprite_y_pos,
                sprite_x_pos,
                level_location_type,
                enemy_var_1,
            );
            if rotate.already_aiming {
                SpinningBubblesRoutine01Outcome::ReadjustedAlreadyAiming { var_3, attack_delay: 0x08 }
            } else {
                let (y_velocity, x_velocity) =
                    (SPINNING_BULLET_VEL_TBL[rotate.new_aim_dir as usize], SPINNING_BULLET_VEL_TBL[rotate.new_aim_dir as usize + 6]);
                SpinningBubblesRoutine01Outcome::Readjusted {
                    var_3,
                    attack_delay: 0x08,
                    attributes: attributes | 0x03,
                    aim_dir: rotate.new_aim_dir,
                    y_velocity,
                    x_velocity,
                }
            }
        }
    };

    SpinningBubblesRoutine01Result { animation, position, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_matches_a_direct_composition_of_the_aiming_and_velocity_primitives() {
        let closest = player_enemy_x_dist([0x90, 0x00], 0x50, [1, 0]);
        let aimed = get_quadrant_aim_dir_for_player(0x60, 0x50, closest.player_index, [1, 0], [0x60, 0x00], [0x90, 0x00], 0, &QUADRANT_AIM_DIR_01);
        let expected_v = calc_bullet_velocities(aimed.aim_dir, SPINNING_BUBBLES_SPEED_TBL[0x03 & 0x03], aimed.quadrant);
        let expected_rotate = get_rotate_dir(aimed.aim_dir, aimed.quadrant, 0x01, 0x00);

        let r = spinning_bubbles_routine_00(0x50, 0x60, 0x00, 0x03, [1, 0], [0x60, 0x00], [0x90, 0x00], 0, 5);
        assert_eq!(r.var_2, closest.player_index);
        assert_eq!(r.attributes, 0x03);
        assert_eq!(r.y_velocity, (expected_v.frac_y, expected_v.fast_y));
        assert_eq!(r.x_velocity, (expected_v.frac_x, expected_v.fast_x));
        assert_eq!(r.aim_dir, expected_rotate.new_aim_dir);
        assert_eq!(r.attack_delay, 0x20);
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn routine_00_random_attribute_selects_the_matching_speed_code() {
        for frame_counter in 0..4u8 {
            let r = spinning_bubbles_routine_00(0x50, 0x60, 0x00, frame_counter, [1, 0], [0x60, 0x00], [0x90, 0x00], 0, 5);
            assert_eq!(r.attributes, frame_counter);
        }
    }

    #[test]
    fn animate_does_not_advance_before_the_threshold() {
        // attributes=0 -> threshold 8; delay 5+1=6 < 8.
        let r = spinning_bubbles_animate(0x00, 0x05, 0x02);
        assert_eq!(r, SpinningBubblesAnimation { animation_delay: 0x06, enemy_frame: 0x02, sprite: 0x02 + 0x6D });
    }

    #[test]
    fn animate_advances_and_resets_the_delay_at_the_threshold() {
        // attributes=3 -> threshold 2; delay 1+1=2 >= 2.
        let r = spinning_bubbles_animate(0x03, 0x01, 0x02);
        assert_eq!(r, SpinningBubblesAnimation { animation_delay: 0x00, enemy_frame: 0x03, sprite: 0x03 + 0x6D });
    }

    #[test]
    fn animate_wraps_the_frame_at_6() {
        let r = spinning_bubbles_animate(0x03, 0x01, 0x05);
        assert_eq!(r.enemy_frame, 0x00);
        assert_eq!(r.sprite, 0x6D);
    }

    #[test]
    fn routine_01_no_readjustment_once_the_budget_is_spent() {
        let r = spinning_bubbles_routine_01(0, 0, 0, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0, 0, 0x14, 0x01, 0, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r.outcome, SpinningBubblesRoutine01Outcome::NoReadjustment);
    }

    #[test]
    fn routine_01_waits_when_the_readjustment_delay_has_not_elapsed() {
        let r = spinning_bubbles_routine_01(0, 0, 0, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0, 0, 0x00, 0x05, 0, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(r.outcome, SpinningBubblesRoutine01Outcome::WaitingToReadjust { attack_delay: 0x04 });
    }

    #[test]
    fn routine_01_already_aiming_skips_the_velocity_readjustment() {
        // player straight below source (0x50,0x60) targeting (0x50,0x90):
        // aim_dir 0 with quadrant 1 -> get_rotate_01 should already match
        // current_aim_dir once the wheel settles; instead force it
        // directly via source==target position (dy=dx=0) -> aim_dir=0,
        // quadrant=0 -> new_dir=0 == current_aim_dir=0 -> NoChangeNeeded.
        let r = spinning_bubbles_routine_01(0, 0, 0, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x00, 0x01, 0, 0x00, [1, 0], [0x50, 0], [0x50, 0], 0);
        match r.outcome {
            SpinningBubblesRoutine01Outcome::ReadjustedAlreadyAiming { var_3, attack_delay } => {
                assert_eq!(var_3, 0x01);
                assert_eq!(attack_delay, 0x08);
            }
            other => panic!("expected ReadjustedAlreadyAiming, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_readjusts_velocity_from_the_new_aim_direction() {
        // player far to the right and below -> a real rotation step is needed.
        let r = spinning_bubbles_routine_01(0, 0, 0, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x00, 0x01, 0, 0x00, [1, 0], [0x90, 0], [0x90, 0], 0);
        match r.outcome {
            SpinningBubblesRoutine01Outcome::Readjusted { var_3, attack_delay, attributes, aim_dir, y_velocity, x_velocity } => {
                assert_eq!(var_3, 0x01);
                assert_eq!(attack_delay, 0x08);
                assert_eq!(attributes, 0x00 | 0x03);
                assert_eq!(y_velocity, SPINNING_BULLET_VEL_TBL[aim_dir as usize]);
                assert_eq!(x_velocity, SPINNING_BULLET_VEL_TBL[aim_dir as usize + 6]);
            }
            other => panic!("expected Readjusted, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_position_matches_update_enemy_pos_directly() {
        let r = spinning_bubbles_routine_01(0, 0, 0, 0, 0x02, 0x50, 0, 0x10, 0x00, 0x60, 0, 0x08, 0x00, 0x14, 0x00, 0, 0, [0, 0], [0, 0], [0, 0], 0);
        let expected = update_enemy_pos(0, 0x02, 0x50, 0, 0x10, 0x00, 0x60, 0, 0x08, 0x00);
        assert_eq!(r.position, expected);
    }

    #[test]
    fn velocity_table_matches_the_transcribed_real_data_and_repeats_after_24_entries() {
        assert_eq!(SPINNING_BULLET_VEL_TBL[0], (0x00, 0x00));
        assert_eq!(SPINNING_BULLET_VEL_TBL[6], (0x7E, 0x01)); // peak positive
        assert_eq!(SPINNING_BULLET_VEL_TBL[18], (0x82, 0xFE)); // peak negative
        assert_eq!(SPINNING_BULLET_VEL_TBL[29], (0x72, 0x01));
        // entries 24..30 are a literal repeat of entries 0..6 - the
        // window `X_velocity = table[aim_dir + 6]` needs for aim_dir up
        // to 23 (the highest real aim direction on this 24-step wheel).
        for i in 0..6usize {
            assert_eq!(SPINNING_BULLET_VEL_TBL[24 + i], SPINNING_BULLET_VEL_TBL[i], "i={i}");
        }
    }
}
