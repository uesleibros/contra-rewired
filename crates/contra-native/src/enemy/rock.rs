//! Native port of the level-4 (Energy Zone) rock/flame family (`src/
//! bank0.asm`, `$97e9`-`$992a`): `floating_rock`/`moving_flame` (share
//! the same `_00` entry and the same "bounce off a boundary" tail),
//! `rock_cave` (a stationary generator that periodically spawns a
//! falling rock, enemy type `$13`, via `generate_enemy_at_pos`), and
//! `falling_rock` itself (wobbles in place, then falls and bounces off
//! the ground once). `boss_mouth` (level 3's own dragon boss, sharing
//! this same address range) is not ported here - its own animation
//! routines depend on the unported PPU graphics-buffer subsystem.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_position_utils::{add_10_to_enemy_y_fract_vel, reverse_enemy_x_direction};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::generate_enemy_at_pos::{generate_enemy_at_pos, GeneratedEnemy};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};
use crate::physics::collision::{add_y_to_y_pos_get_bg_collision, CollisionCode, BG_COLLISION_DATA_LEN};

/// `rock_moving_flame_init_vel_tbl` (`$9827`, 4 `(x_vel_fract, x_vel_fast)`
/// entries) - indexed by `ENEMY_ATTRIBUTES` (`0`/`1` rock platform slow/
/// fast, `2`/`3` moving flame left/right).
const ROCK_MOVING_FLAME_INIT_VEL_TBL: [(u8, u8); 4] = [(0x80, 0xFF), (0xC0, 0x00), (0x80, 0xFF), (0x80, 0x00)];
/// `rock_moving_flame_boundaries_tbl` (`$982f`, 4 `(left, right)` entries),
/// same indexing.
const ROCK_MOVING_FLAME_BOUNDARIES_TBL: [(u8, u8); 4] = [(0x50, 0xB0), (0x70, 0xC0), (0x48, 0xB8), (0x48, 0xB8)];

/// The full result of one [`floating_rock_routine_00`] call (also real
/// ASM's own entry for `moving_flame_routine_00` - the identical
/// function, not a separate port).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingRockRoutine00Result {
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    /// `ENEMY_VAR_2` - left boundary.
    pub var_2: u8,
    /// `ENEMY_VAR_1` - right boundary.
    pub var_1: u8,
    pub scroll: ScrolledEnemyPos,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `floating_rock_routine_00` (`$97e9`) - real ASM
/// comment: "also used for moving flame enemy".
pub fn floating_rock_routine_00(attributes: u8, level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> FloatingRockRoutine00Result {
    let (x_vel_fract, x_vel_fast) = ROCK_MOVING_FLAME_INIT_VEL_TBL[attributes as usize];
    let (var_2, var_1) = ROCK_MOVING_FLAME_BOUNDARIES_TBL[attributes as usize];
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    FloatingRockRoutine00Result { x_vel_fract, x_vel_fast, var_2, var_1, scroll, routine_update: advance_enemy_routine(current_routine) }
}

/// One [`update_pos_turn_around_if_needed`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnAroundOutcome {
    NoTurn,
    TurnedAround { x_vel_fract: u8, x_vel_fast: u8 },
}

/// The full result of one [`update_pos_turn_around_if_needed`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdatePosTurnAroundResult {
    pub position: UpdatedEnemyPos,
    pub outcome: TurnAroundOutcome,
}

/// Native port of `update_pos_turn_around_if_needed` (`$9826`) - shared
/// by `floating_rock_routine_01` and `moving_flame_routine_01`: applies
/// velocity/scroll, then reverses direction once the *post-update* X
/// position crosses whichever boundary (`ENEMY_VAR_1`/`_2`) is ahead of
/// it in the current direction of travel (tested against the *entry-
/// time* `x_vel_fast`, not anything `update_enemy_pos` itself touches).
#[allow(clippy::too_many_arguments)]
pub fn update_pos_turn_around_if_needed(
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
    var_1: u8,
    var_2: u8,
) -> UpdatePosTurnAroundResult {
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);

    let hit_barrier = if (x_vel_fast as i8) < 0 { position.x.pos < var_2 } else { position.x.pos >= var_1 };

    let outcome = if hit_barrier {
        let (new_x_vel_fract, new_x_vel_fast) = reverse_enemy_x_direction(x_vel_fract, x_vel_fast);
        TurnAroundOutcome::TurnedAround { x_vel_fract: new_x_vel_fract, x_vel_fast: new_x_vel_fast }
    } else {
        TurnAroundOutcome::NoTurn
    };

    UpdatePosTurnAroundResult { position, outcome }
}

/// The full result of one [`floating_rock_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FloatingRockRoutine01Result {
    pub sprite: u8,
    pub inner: UpdatePosTurnAroundResult,
}

/// Native port of `floating_rock_routine_01` (`$981c`).
#[allow(clippy::too_many_arguments)]
pub fn floating_rock_routine_01(
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
    var_1: u8,
    var_2: u8,
) -> FloatingRockRoutine01Result {
    let inner = update_pos_turn_around_if_needed(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast, var_1, var_2);
    FloatingRockRoutine01Result { sprite: 0x48, inner }
}

/// The full result of one [`moving_flame_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovingFlameRoutine01Result {
    pub sprite: u8,
    pub sprite_attr: u8,
    pub inner: UpdatePosTurnAroundResult,
}

/// Native port of `moving_flame_routine_01` (`$9840`) - same "bounce off
/// a boundary" tail as `floating_rock_routine_01`, plus a flashing
/// palette (real ASM: `lsr` x4, testing bit 3 of `FRAME_COUNTER` in the
/// resulting carry - a real, if slightly counterintuitive, bit position
/// since a *right* shift's carry after `N` shifts holds original bit
/// `N-1`, not bit `N`).
#[allow(clippy::too_many_arguments)]
pub fn moving_flame_routine_01(
    frame_counter: u8,
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
    var_1: u8,
    var_2: u8,
) -> MovingFlameRoutine01Result {
    let sprite_attr = if frame_counter & 0x08 != 0 { 0x40 } else { 0x00 };
    let inner = update_pos_turn_around_if_needed(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast, var_1, var_2);
    MovingFlameRoutine01Result { sprite: 0x49, sprite_attr, inner }
}

/// Native port of `rock_cave_routine_00` (`$985d`) - real ASM: `jsr add_
/// scroll_to_enemy_pos; jmp advance_enemy_routine`.
pub fn rock_cave_routine_00(level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> (ScrolledEnemyPos, EnemyRoutineUpdate) {
    (add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos), advance_enemy_routine(current_routine))
}

/// Native port of `rock_cave_routine_01` (`$9863`) - real ASM: scroll,
/// then `a = $08; jmp set_anim_delay_adv_routine` (the initial delay
/// before the first falling rock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RockCaveRoutine01Result {
    pub scroll: ScrolledEnemyPos,
    pub delayed_routine: DelayedRoutineUpdate,
}

pub fn rock_cave_routine_01(level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> RockCaveRoutine01Result {
    RockCaveRoutine01Result { scroll: add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos), delayed_routine: set_enemy_delay_adv_routine(0x08, current_routine) }
}

/// One [`rock_cave_routine_02`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RockCaveRoutine02Outcome {
    Waiting { animation_delay: u8 },
    /// Delay elapsed - spawns a falling rock (enemy type `$13`) at the
    /// generator's own (already scroll-updated) position, no offset.
    /// `None` when no free slot was available (real ASM never checks
    /// the spawn's own success/failure here either).
    Spawned { animation_delay: u8, rock: Option<GeneratedEnemy> },
}

/// The full result of one [`rock_cave_routine_02`] call. Real ASM's own
/// tail is a `jmp generate_enemy_a`, so (like `moving_cart_routine_00`)
/// this routine never advances its own routine index - it stays here
/// permanently, just periodically spawning rocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RockCaveRoutine02Result {
    pub position: UpdatedEnemyPos,
    pub outcome: RockCaveRoutine02Outcome,
}

/// Native port of `rock_cave_routine_02` (`$986b`).
#[allow(clippy::too_many_arguments)]
pub fn rock_cave_routine_02(
    prg_rom: &[u8],
    enemy_routine_slots: &[u8; ENEMY_SLOT_COUNT],
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
    animation_delay: u8,
) -> RockCaveRoutine02Result {
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);

    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        RockCaveRoutine02Outcome::Waiting { animation_delay: delay }
    } else {
        let rock = generate_enemy_at_pos(prg_rom, enemy_routine_slots, 0x13, current_level, position.x.pos, position.y.pos, 0x00, 0x00);
        RockCaveRoutine02Outcome::Spawned { animation_delay: 0xE0, rock }
    };

    RockCaveRoutine02Result { position, outcome }
}

/// Native port of `falling_rock_routine_00` (`$9889`) - real ASM: `a =
/// $40; jmp set_anim_delay_adv_routine`.
pub fn falling_rock_routine_00(current_routine: u8) -> DelayedRoutineUpdate {
    set_enemy_delay_adv_routine(0x40, current_routine)
}

/// One [`falling_rock_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallingRockRoutine01Outcome {
    Waiting { animation_delay: u8 },
    Activated { state_width: u8, delayed_routine: DelayedRoutineUpdate },
}

/// The full result of one [`falling_rock_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallingRockRoutine01Result {
    pub sprite: u8,
    pub position: UpdatedEnemyPos,
    /// `ENEMY_X_POS` after `position.x.pos`'s own ±1 wobble nudge -
    /// real ASM applies this *on top of* `update_enemy_pos`'s own
    /// write, only every 4th frame (`FRAME_COUNTER & 3 == 0`), direction
    /// from bit 2 of `FRAME_COUNTER`.
    pub x_pos: u8,
    pub outcome: FallingRockRoutine01Outcome,
}

/// Native port of `falling_rock_routine_01` (`$988e`).
#[allow(clippy::too_many_arguments)]
pub fn falling_rock_routine_01(
    frame_counter: u8,
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
    animation_delay: u8,
    state_width: u8,
    current_routine: u8,
) -> FallingRockRoutine01Result {
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);

    let swayed_x = if frame_counter & 0x03 == 0 {
        if frame_counter & 0x04 != 0 { position.x.pos.wrapping_add(1) } else { position.x.pos.wrapping_sub(1) }
    } else {
        position.x.pos
    };

    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        FallingRockRoutine01Outcome::Waiting { animation_delay: delay }
    } else {
        FallingRockRoutine01Outcome::Activated { state_width: enable_enemy_collision(state_width), delayed_routine: set_enemy_delay_adv_routine(0x01, current_routine) }
    };

    FallingRockRoutine01Result { sprite: 0x4A, position, x_pos: swayed_x, outcome }
}

/// `falling_rock_sprite_attr_tbl` (`$98c9`, 4 bytes) - mirroring codes
/// the tumbling boulder cycles through.
const FALLING_ROCK_SPRITE_ATTR_TBL: [u8; 4] = [0x00, 0x40, 0xC0, 0x80];

/// Native port of `falling_rock_set_sprite_and_attr` (`$98b3`) - real
/// ASM keeps sprite `$4a` fixed and only cycles the mirroring bits every
/// 4 frames (`(FRAME_COUNTER >> 2) & 3`), preserving every other sprite-
/// attribute bit (`& $3f` before OR-ing in the new mirror bits).
fn falling_rock_set_sprite_and_attr(frame_counter: u8, sprite_attr: u8) -> (u8, u8) {
    let idx = (frame_counter >> 2) & 0x03;
    (0x4A, (sprite_attr & 0x3F) | FALLING_ROCK_SPRITE_ATTR_TBL[idx as usize])
}

/// A real ground-impact event from [`falling_rock_routine_02`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallingRockBounce {
    pub sound: u8,
    pub animation_delay: u8,
}

/// The full result of one [`falling_rock_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FallingRockRoutine02Result {
    pub sprite: u8,
    pub sprite_attr: u8,
    pub bounce: Option<FallingRockBounce>,
    /// `ENEMY_VAR_1` - the rock's own tracked ground-collision Y
    /// position, after both the bounce recompute (if any) and the real
    /// per-frame scroll adjustment that always runs regardless.
    pub var_1: u8,
    pub position: UpdatedEnemyPos,
}

/// Native port of `falling_rock_routine_02` (`$98ce`) - only checks for
/// a real floor collision once `ENEMY_Y_POS` has reached or passed its
/// own tracked ground level (`ENEMY_VAR_1`); if that specific check
/// isn't a real `Floor` collision (e.g. the ground segment already
/// destroyed), the rock just keeps falling through rather than bouncing.
#[allow(clippy::too_many_arguments)]
pub fn falling_rock_routine_02(
    frame_counter: u8,
    sprite_attr: u8,
    y_pos: u8,
    var_1: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> FallingRockRoutine02Result {
    let (sprite, new_sprite_attr) = falling_rock_set_sprite_and_attr(frame_counter, sprite_attr);

    let bounce_state = if y_pos >= var_1 {
        let collision = add_y_to_y_pos_get_bg_collision(0x08, x_pos, y_pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
        if collision == CollisionCode::Floor {
            let (new_ground_y, overflowed) = y_pos.overflowing_add(0x10);
            Some((if overflowed { 0xFF } else { new_ground_y }, 0xC0u8, 0xFEu8))
        } else {
            None
        }
    } else {
        None
    };

    let (var_1_for_gravity, y_vel_fract_in, y_vel_fast_in) = match bounce_state {
        Some((new_var_1, fract, fast)) => (new_var_1, fract, fast),
        None => (var_1, y_vel_fract, y_vel_fast),
    };

    let (y_vel_fract, y_vel_fast) = add_10_to_enemy_y_fract_vel(y_vel_fract_in, y_vel_fast_in);
    let (scrolled_var_1, overflowed2) = var_1_for_gravity.overflowing_add(frame_scroll);
    let var_1 = if overflowed2 { 0xFF } else { scrolled_var_1 };

    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);

    FallingRockRoutine02Result {
        sprite,
        sprite_attr: new_sprite_attr,
        bounce: bounce_state.map(|_| FallingRockBounce { sound: 0x05, animation_delay: 0x40 }),
        var_1,
        position,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let level0_table_addr: u16 = 0xEF10;
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level0_table_addr.to_le_bytes());
        let level0_off = 7 * 0x4000 + (level0_table_addr as usize - 0xC000) + 0x13 * 4;
        rom[level0_off..level0_off + 4].copy_from_slice(&[0x81, 0x0d, 0x01, 0x00]);
        rom
    }

    fn no_scroll_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0u8; BG_COLLISION_DATA_LEN]
    }

    #[test]
    fn routine_00_loads_velocity_and_boundaries_by_attribute_index() {
        let r = floating_rock_routine_00(0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!((r.x_vel_fract, r.x_vel_fast), (0xC0, 0x00));
        assert_eq!((r.var_2, r.var_1), (0x70, 0xC0));
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn turn_around_reverses_at_the_right_boundary_moving_right() {
        let r = update_pos_turn_around_if_needed(0, 0x00, 0xBF, 0, 0, 0x01, 0x50, 0, 0, 0, 0xC0, 0x70);
        assert_eq!(r.position.x.pos, 0xC0);
        match r.outcome {
            TurnAroundOutcome::TurnedAround { x_vel_fract, x_vel_fast } => assert_eq!((x_vel_fract, x_vel_fast), reverse_enemy_x_direction(0, 0x01)),
            TurnAroundOutcome::NoTurn => panic!("expected TurnedAround"),
        }
    }

    #[test]
    fn turn_around_reverses_past_the_left_boundary_moving_left() {
        // x=0x70, velocity -1 -> lands at 0x6f, strictly below the 0x70
        // left boundary (the real ASM's own check is `< var_2`, not
        // `<=`, so landing exactly on the boundary must NOT turn around).
        let r = update_pos_turn_around_if_needed(0, 0x00, 0x70, 0, 0, 0xFF, 0x50, 0, 0, 0, 0xC0, 0x70);
        assert_eq!(r.position.x.pos, 0x6F);
        assert!(matches!(r.outcome, TurnAroundOutcome::TurnedAround { .. }));

        let at_boundary = update_pos_turn_around_if_needed(0, 0x00, 0x71, 0, 0, 0xFF, 0x50, 0, 0, 0, 0xC0, 0x70);
        assert_eq!(at_boundary.position.x.pos, 0x70);
        assert_eq!(at_boundary.outcome, TurnAroundOutcome::NoTurn);
    }

    #[test]
    fn turn_around_does_nothing_mid_range() {
        let r = update_pos_turn_around_if_needed(0, 0x00, 0x80, 0, 0, 0x01, 0x50, 0, 0, 0, 0xC0, 0x70);
        assert_eq!(r.outcome, TurnAroundOutcome::NoTurn);
    }

    #[test]
    fn moving_flame_flashes_palette_on_bit_3_of_frame_counter() {
        let dark = moving_flame_routine_01(0x00, 0, 0x00, 0x50, 0, 0, 0x01, 0x50, 0, 0, 0, 0xC0, 0x70);
        let bright = moving_flame_routine_01(0x08, 0, 0x00, 0x50, 0, 0, 0x01, 0x50, 0, 0, 0, 0xC0, 0x70);
        assert_eq!(dark.sprite_attr, 0x00);
        assert_eq!(bright.sprite_attr, 0x40);
        assert_eq!(dark.sprite, 0x49);
    }

    #[test]
    fn rock_cave_routine_00_scrolls_and_advances() {
        let (scroll, update) = rock_cave_routine_00(0, 0x02, 0x50, 0x60, 3);
        assert_eq!(scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
        assert_eq!(update, advance_enemy_routine(3));
    }

    #[test]
    fn rock_cave_routine_01_sets_initial_delay() {
        let r = rock_cave_routine_01(0, 0x02, 0x50, 0x60, 3);
        assert_eq!(r.delayed_routine, set_enemy_delay_adv_routine(0x08, 3));
    }

    #[test]
    fn rock_cave_routine_02_waits_then_spawns_at_its_own_updated_position() {
        let rom = synthetic_prg_rom();
        let slots = [0u8; ENEMY_SLOT_COUNT];
        let waiting = rock_cave_routine_02(&rom, &slots, 0, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0, 0, 0x05);
        assert_eq!(waiting.outcome, RockCaveRoutine02Outcome::Waiting { animation_delay: 0x04 });

        let spawning = rock_cave_routine_02(&rom, &slots, 0, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0, 0, 0x01);
        match spawning.outcome {
            RockCaveRoutine02Outcome::Spawned { animation_delay, rock } => {
                assert_eq!(animation_delay, 0xE0);
                let rock = rock.unwrap();
                assert_eq!(rock.x_pos, spawning.position.x.pos);
                assert_eq!(rock.y_pos, spawning.position.y.pos);
            }
            other => panic!("expected Spawned, got {other:?}"),
        }
    }

    #[test]
    fn falling_rock_routine_00_sets_the_pre_fall_delay() {
        assert_eq!(falling_rock_routine_00(3), set_enemy_delay_adv_routine(0x40, 3));
    }

    #[test]
    fn falling_rock_routine_01_sways_every_4th_frame_by_direction_bit() {
        let left = falling_rock_routine_01(0x00, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x05, 0x00, 3);
        assert_eq!(left.x_pos, 0x4F);
        let right = falling_rock_routine_01(0x04, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x05, 0x00, 3);
        assert_eq!(right.x_pos, 0x51);
        let no_sway = falling_rock_routine_01(0x01, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x05, 0x00, 3);
        assert_eq!(no_sway.x_pos, 0x50);
    }

    #[test]
    fn falling_rock_routine_01_activates_once_delay_elapses() {
        let r = falling_rock_routine_01(0x01, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x01, 0x00, 3);
        match r.outcome {
            FallingRockRoutine01Outcome::Activated { state_width, delayed_routine } => {
                assert_eq!(state_width, enable_enemy_collision(0x00));
                assert_eq!(delayed_routine, set_enemy_delay_adv_routine(0x01, 3));
            }
            other => panic!("expected Activated, got {other:?}"),
        }
    }

    #[test]
    fn falling_rock_routine_02_bounces_off_a_real_floor_collision() {
        // BG_COLLISION_DATA all zero -> code 0 (Empty), not Floor - use a
        // handcrafted "column 3, code 1" byte at whatever offset (x=0x50,
        // y=y_pos+8) resolves to with zero scroll/ppuctrl.
        // offset = (hx>>6)|((vy>>2)&0x3c)|nt_off ; column=(hx>>4)&3
        // x=0x50 -> hx=0x50 -> hx>>6=1, column=(0x50>>4)&3=1
        // y_pos=0x40, +8=0x48 -> vy=0x48 -> (0x48>>2)&0x3c=0x10
        // offset = 1|0x10|0 = 0x11, column 1 -> shift 4, code 1 (floor) = 0b01 << 4 = 0x10
        let mut data = [0u8; BG_COLLISION_DATA_LEN];
        data[0x11] = 0x10;
        let r = falling_rock_routine_02(0x00, 0x00, 0x40, 0x40, 0, 0, 0, &data, 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
        assert!(r.bounce.is_some());
        let b = r.bounce.unwrap();
        assert_eq!(b.sound, 0x05);
        assert_eq!(b.animation_delay, 0x40);
        assert_eq!(r.var_1, 0x50); // 0x40+0x10 new ground, +0 scroll
    }

    #[test]
    fn falling_rock_routine_02_falls_through_when_no_real_floor_below() {
        let r = falling_rock_routine_02(0x00, 0x00, 0x40, 0x40, 0, 0, 0, &no_scroll_bg_collision_data(), 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
        assert!(r.bounce.is_none());
        assert_eq!(r.var_1, 0x40); // unchanged ground Y, +0 scroll
    }

    #[test]
    fn falling_rock_routine_02_skips_the_floor_check_while_still_above_ground() {
        let r = falling_rock_routine_02(0x00, 0x00, 0x10, 0x40, 0, 0, 0, &no_scroll_bg_collision_data(), 0, 0x00, 0x50, 0, 0, 0, 0, 0, 0);
        assert!(r.bounce.is_none());
    }
}
