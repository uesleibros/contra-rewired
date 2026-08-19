//! Native port of the level 5 (alien lair) white blob enemy,
//! `src/bank0.asm` (`white_blob_routine_ptr_tbl`, `$b874`-`$b940`):
//! `_00` (find a target player, pick an initial aim direction/velocity),
//! `_01` (float down under a fixed velocity until "gaining sentience",
//! then freeze and lock onto the target with a burst of 4 rotation
//! steps), `_02` (rush the target at 3x speed, re-aiming and re-dashing
//! periodically). The other real caller of `aim_var_1_for_quadrant_aim_
//! dir_01`/`quadrant_aim_dir_01` alongside `crate::enemy::
//! spinning_bubbles`, previously assumed blocked the same way. `white_
//! blob_routine_ptr_tbl` entries `3`-`5` (explosion/removal) are the same
//! real shared `bank7.asm` routines most enemy families use and aren't
//! ported here.
//!
//! ## `white_blob_spider_set_sprite`'s nibble-packed dual state
//!
//! Shared with the (not yet ported) alien spider, `$b9ad` packs *two*
//! independent counters into one byte, `ENEMY_ANIMATION_DELAY`: the high
//! nibble is a `0..8` countdown to the next sprite change, the low nibble
//! is which entry of `white_blob_spider_sprite_tbl` (a small, `0xff`-
//! terminated, wrapping cycle) to show next. [`white_blob_spider_set_
//! sprite`] models this as one function returning both the repacked byte
//! and (only on the call the sprite actually changes) the new sprite
//! code.
//!
//! ## A real "coincidental 8" this port does *not* simplify away
//!
//! `white_blob_routine_01`'s own velocity-adjustment trigger checks
//! whether the *current* high-nibble timer value is exactly `8` - not
//! "the sprite table just cycled". Since `white_blob_routine_00` seeds
//! `ENEMY_ANIMATION_DELAY` with a high nibble of `0xc` (`12`, not the
//! usual post-cycle reset value of `8`), the timer naturally counts down
//! *through* `8` four calls after spawn, well before the sprite-cycle's
//! own reset-to-`8` would ever fire - a real, deliberate way to trigger
//! one early velocity adjustment. This port checks the returned timer
//! value directly (`timer_after == 8`), not `sprite.is_some()`, which
//! would have silently dropped this case.
//!
//! ## `white_blob_routine_02`'s own velocity phase offset
//!
//! Its re-dash also reads from `white_blob_alien_fetus_vel_tbl`, but at
//! *different* fixed pair offsets than [`crate::enemy::alien_fetus::
//! set_white_blob_alien_fetus_vel`] uses (`Y = table[aim_dir + 1]`, `X =
//! table[aim_dir + 4]`, vs. that function's own `+0`/`+6`) - a real,
//! different phase relationship for this specific caller, ported
//! literally rather than assumed to match the other two callers.

use crate::enemy::add_with_enemy_pos::set_08_09_to_enemy_pos;
use crate::enemy::alien_fetus::{set_white_blob_alien_fetus_vel, WHITE_BLOB_ALIEN_FETUS_VEL_TBL};
use crate::enemy::quadrant_aim_dir::{aim_var_1_for_quadrant_aim_dir_01, get_rotate_01, RotateEnemyVar1Result};
use crate::enemy::update_enemy_pos::{set_enemy_x_velocity_to_0, set_enemy_y_velocity_to_0, update_enemy_pos, UpdatedEnemyPos};

/// `mv_low_nibble_to_high` (`$b446`) - `%01101100 -> %11000000`.
fn mv_low_nibble_to_high(v: u8) -> u8 {
    v << 4
}

/// Native port of `white_blob_init_velocity` (`$b8e8`) - a thin wrapper:
/// its single `asl` (vs. `alien_fetus_set_velocity`'s own `asl;asl`)
/// means its raw byte offset is already a pair index with no extra
/// doubling needed.
fn white_blob_init_velocity(aim_dir: u8) -> ((u8, u8), (u8, u8)) {
    set_white_blob_alien_fetus_vel(aim_dir)
}

/// The full result of one [`white_blob_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBlobRoutine00Result {
    /// `ENEMY_VAR_2` - delay before "gaining sentience", `[0x50-0x6f]`.
    pub var_2: u8,
    /// Always `0xc0` - see this module's own doc comment for why this
    /// particular value (not the usual `8`) matters later.
    pub animation_delay: u8,
    /// `ENEMY_VAR_3` (which player to target) - `None` if `P2_GAME_OVER_
    /// STATUS` was set (real ASM skips this whole roll, leaving the field
    /// at whatever it already was).
    pub var_3: Option<u8>,
    /// Always `0xb0`.
    pub sprite: u8,
    pub var_1: u8,
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    /// `ENEMY_ROUTINE` after this call - a real, literal `inc`, not the
    /// guarded `advance_enemy_routine` helper (same reasoning as `crate::
    /// enemy::alien_fetus::AlienFetusRoutine00Result::new_routine`).
    pub new_routine: u8,
}

/// Native port of `white_blob_routine_00` (`$b874`) - spawn init.
#[allow(clippy::too_many_arguments)]
pub fn white_blob_routine_00(
    enemy_var_3: u8,
    enemy_var_1: u8,
    random_num: u8,
    frame_counter: u8,
    p1_game_over_status: u8,
    p2_game_over_status: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_routine: u8,
) -> WhiteBlobRoutine00Result {
    let var_2 = (random_num & 0x1F).wrapping_add(0x50);

    let new_var_3 = if p2_game_over_status != 0 {
        None
    } else {
        let target = random_num.wrapping_add(frame_counter) & 0x01;
        Some(if p1_game_over_status != 0 { 0x01 } else { target })
    };
    let effective_var_3 = new_var_3.unwrap_or(enemy_var_3);

    let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
    let rotate = get_rotate_01(source_y, source_x, effective_var_3, player_state, sprite_y_pos, sprite_x_pos, level_location_type, enemy_var_1);
    let var_1 = rotate.new_aim_dir;
    let (y_velocity, x_velocity) = white_blob_init_velocity(var_1);

    WhiteBlobRoutine00Result {
        var_2,
        animation_delay: 0xC0,
        var_3: new_var_3,
        sprite: 0xB0,
        var_1,
        y_velocity,
        x_velocity,
        new_routine: current_routine.wrapping_add(1),
    }
}

/// `white_blob_spider_sprite_tbl` (`$b9ea`, 5 bytes, `0xff`-terminated) -
/// shared by white blob and the (unported) alien spider: `sprite_b0`,
/// `_b1`, `_b2`, `_b1` for white blob (indices `0..4`, wrapping via the
/// terminator).
const WHITE_BLOB_SPIDER_SPRITE_TBL: [u8; 5] = [0x00, 0x01, 0x02, 0x01, 0xFF];

/// The full result of one `white_blob_spider_set_sprite` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBlobSpiderSpriteResult {
    /// Repacked `ENEMY_ANIMATION_DELAY` (timer in the high nibble, sprite-
    /// cycle index in the low nibble).
    pub animation_delay: u8,
    /// The un-packed timer value used for this call's own `cmp #$08`-
    /// style checks (`8` right after a fresh reset, `1..8` while
    /// counting down otherwise) - see this module's doc comment for why
    /// this is exposed separately from `sprite`.
    pub timer_after: u8,
    /// `Some(new_sprite_code)` only the call the sprite actually changes.
    pub sprite: Option<u8>,
}

/// Native port of `white_blob_spider_set_sprite` (`$b9ad`) - see this
/// module's own doc comment for the nibble-packing shape.
pub(crate) fn white_blob_spider_set_sprite(base_sprite_offset: u8, enemy_animation_delay: u8) -> WhiteBlobSpiderSpriteResult {
    let mut sprite_index = enemy_animation_delay & 0x0F;
    let timer = enemy_animation_delay >> 4;
    let decremented_timer = timer.wrapping_sub(1);

    let (timer_after, sprite) = if decremented_timer != 0 {
        (decremented_timer, None)
    } else {
        let mut idx = sprite_index;
        let val = loop {
            let v = WHITE_BLOB_SPIDER_SPRITE_TBL[idx as usize];
            if v == 0xFF {
                idx = 0;
                continue;
            }
            break v;
        };
        sprite_index = idx.wrapping_add(1);
        (0x08, Some(val.wrapping_add(base_sprite_offset)))
    };

    let animation_delay = mv_low_nibble_to_high(timer_after).wrapping_add(sprite_index);
    WhiteBlobSpiderSpriteResult { animation_delay, timer_after, sprite }
}

/// `white_blob_y_vel_adj_tbl` (`$b8f8`) + `white_blob_x_vel_adj_tbl`
/// (`$b8fe`) - declared as two separate labels in the ROM (`6` and `24`
/// bytes), but `white_blob_y_vel_adj_tbl` is read with indices up to
/// `0x16` (`22`) - past its own declared length, directly into `white_
/// blob_x_vel_adj_tbl`'s own bytes (the two are adjacent in ROM with no
/// gap). Ported as the single contiguous 30-byte array this produces,
/// the same "don't fix the overlapping read" policy as `crate::enemy::
/// spiked_wall::SPIKED_WALL_DESTROYED_DATA_TBL`. `Y_ADJUST[dir] =
/// COMBINED[dir]`, `X_ADJUST[dir] = COMBINED[dir + 6]` (`white_blob_x_
/// vel_adj_tbl`'s own reads never need the spillover, since it already
/// has all `24` real entries on its own).
const WHITE_BLOB_VEL_ADJ_TBL: [u8; 30] = [
    0x00, 0xFD, 0xFA, 0xF8, 0xF6, 0xF5, // white_blob_y_vel_adj_tbl's own 6 bytes
    0xF4, 0xF5, 0xF6, 0xF8, 0xFA, 0xFD, // white_blob_x_vel_adj_tbl (4 groups of 6)
    0x00, 0x03, 0x06, 0x08, 0x0A, 0x0B, //
    0x0C, 0x0B, 0x0A, 0x08, 0x06, 0x03, //
    0x00, 0xFD, 0xFA, 0xF8, 0xF6, 0xF5, //
];

/// Native port of `white_blob_routine_01`'s own `@adjust_velocity`
/// (`$b8cb`-`$b8dc`) - nudges the fractional Y/X velocity components
/// (fast components untouched) toward the current aim direction.
fn white_blob_adjust_velocity(enemy_var_1: u8, y_vel_fract: u8, x_vel_fract: u8) -> (u8, u8) {
    let idx = enemy_var_1 as usize;
    (y_vel_fract.wrapping_add(WHITE_BLOB_VEL_ADJ_TBL[idx]), x_vel_fract.wrapping_add(WHITE_BLOB_VEL_ADJ_TBL[idx + 6]))
}

/// Native port of `white_blob_aim_to_player` (`$b9a2`).
#[allow(clippy::too_many_arguments)]
fn white_blob_aim_to_player(
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    target_player: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_aim_dir: u8,
) -> RotateEnemyVar1Result {
    let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
    aim_var_1_for_quadrant_aim_dir_01(source_y, source_x, target_player, player_state, sprite_y_pos, sprite_x_pos, level_location_type, current_aim_dir)
}

/// [`white_blob_routine_01`]'s own `Frozen` (`ENEMY_VAR_4 != 0`) branch
/// outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteBlobFrozenOutcome {
    /// Freeze timer decremented, still nonzero.
    StillFreezing { var_4: u8 },
    /// Freeze timer just reached `0`: locks onto the target with 4
    /// rotation steps in a row and advances to `white_blob_routine_02`
    /// (real, literal `inc`, not the guarded helper).
    LockedOn { var_4: u8, var_2: u8, aim_dir: u8, new_routine: u8 },
}

/// [`white_blob_routine_01`]'s own `Floating` (`ENEMY_VAR_4 == 0`) branch
/// outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBlobFloatingOutcome {
    /// `Some((y_frac, x_frac))` only when the animation timer reads
    /// exactly `8` this call - see this module's own doc comment.
    pub adjusted_velocity: Option<(u8, u8)>,
    pub var_2: u8,
    /// `Some(new_var_4)` only the call `var_2` reaches `0`.
    pub freeze_length: Option<u8>,
}

/// [`white_blob_routine_01`]'s `Frozen` branch full result - `white_blob_
/// freeze` (`$b921`) unconditionally zeroes both velocity axes (`jsr
/// set_enemy_velocity_to_0`) before even decrementing the freeze timer,
/// on *both* of [`WhiteBlobFrozenOutcome`]'s own sub-cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBlobFrozenResult {
    pub y_velocity: (u8, u8),
    pub x_velocity: (u8, u8),
    pub outcome: WhiteBlobFrozenOutcome,
}

/// The real, branchy result of one [`white_blob_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteBlobRoutine01Outcome {
    Floating(WhiteBlobFloatingOutcome),
    Frozen(WhiteBlobFrozenResult),
}

/// The full result of one [`white_blob_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBlobRoutine01Result {
    pub sprite: WhiteBlobSpiderSpriteResult,
    pub position: UpdatedEnemyPos,
    pub outcome: WhiteBlobRoutine01Outcome,
}

/// Native port of `white_blob_routine_01` (`$b8b3`) - see this module's
/// own doc comment for the real "coincidental 8" trigger and the freeze/
/// lock-on shape.
#[allow(clippy::too_many_arguments)]
pub fn white_blob_routine_01(
    enemy_animation_delay: u8,
    enemy_var_1: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    enemy_var_4: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    random_num: u8,
    current_routine: u8,
) -> WhiteBlobRoutine01Result {
    let sprite = white_blob_spider_set_sprite(0xB0, enemy_animation_delay);
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

    let outcome = if enemy_var_4 != 0 {
        let zx = set_enemy_x_velocity_to_0();
        let zy = set_enemy_y_velocity_to_0();
        let var_4 = enemy_var_4.wrapping_sub(1);
        let frozen_outcome = if var_4 != 0 {
            WhiteBlobFrozenOutcome::StillFreezing { var_4 }
        } else {
            let mut aim_dir = enemy_var_1;
            for _ in 0..4 {
                let r = white_blob_aim_to_player(
                    enemy_x_pos,
                    enemy_y_pos,
                    enemy_var_3,
                    player_state,
                    sprite_y_pos,
                    sprite_x_pos,
                    level_location_type,
                    aim_dir,
                );
                aim_dir = r.new_aim_dir;
            }
            WhiteBlobFrozenOutcome::LockedOn { var_4: 0x08, var_2: 0x08, aim_dir, new_routine: current_routine.wrapping_add(1) }
        };
        WhiteBlobRoutine01Outcome::Frozen(WhiteBlobFrozenResult {
            y_velocity: (zy.vel_fract, zy.vel_fast),
            x_velocity: (zx.vel_fract, zx.vel_fast),
            outcome: frozen_outcome,
        })
    } else {
        let adjusted_velocity =
            if sprite.timer_after == 0x08 { Some(white_blob_adjust_velocity(enemy_var_1, y_vel_fract, x_vel_fract)) } else { None };
        let var_2 = enemy_var_2.wrapping_sub(1);
        let freeze_length = if var_2 == 0 { Some((random_num & 0x20).wrapping_add(0x02)) } else { None };
        WhiteBlobRoutine01Outcome::Floating(WhiteBlobFloatingOutcome { adjusted_velocity, var_2, freeze_length })
    };

    WhiteBlobRoutine01Result { sprite, position, outcome }
}

/// Native port of `mult_velocity_by_3` (`$b98f`) - returns `(new_fast,
/// new_frac)`, matching the real ASM's own `a`/`$08` outputs exactly
/// (bit-for-bit, including the real carry-propagation chain through the
/// `asl`/`rol`/`adc` sequence - not a closed-form "multiply by 3", to
/// guarantee an exact match even though for this specific instruction
/// sequence the two happen to coincide).
fn mult_velocity_by_3(fast: u8, frac: u8) -> (u8, u8) {
    let doubled_frac = frac.wrapping_mul(2);
    let carry_from_double = frac & 0x80 != 0;
    let r09 = (fast << 1) | carry_from_double as u8;
    let (new_frac, carry5) = doubled_frac.overflowing_add(frac);
    let new_fast = (r09 as u16 + fast as u16 + carry5 as u16) as u8;
    (new_fast, new_frac)
}

/// [`white_blob_routine_02`]'s own real, branchy result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhiteBlobRoutine02Outcome {
    /// `ENEMY_VAR_4 == 0` - nothing to do beyond the position update.
    Idle,
    /// Pause between dashes hasn't elapsed.
    StillPaused { var_2: u8 },
    /// Pause elapsed: re-aims and re-dashes at 3x the table velocity.
    Redashed { var_4: u8, var_2: u8, aim_dir: u8, y_velocity: (u8, u8), x_velocity: (u8, u8) },
}

/// The full result of one [`white_blob_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WhiteBlobRoutine02Result {
    pub sprite: WhiteBlobSpiderSpriteResult,
    pub position: UpdatedEnemyPos,
    pub outcome: WhiteBlobRoutine02Outcome,
}

/// Native port of `white_blob_routine_02` (`$b940`) - see this module's
/// own doc comment for the real velocity-table phase offset this uses.
#[allow(clippy::too_many_arguments)]
pub fn white_blob_routine_02(
    enemy_animation_delay: u8,
    enemy_var_4: u8,
    enemy_var_2: u8,
    enemy_var_1: u8,
    enemy_var_3: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
) -> WhiteBlobRoutine02Result {
    let sprite = white_blob_spider_set_sprite(0xB0, enemy_animation_delay);

    let outcome = if enemy_var_4 == 0 {
        WhiteBlobRoutine02Outcome::Idle
    } else {
        let var_2 = enemy_var_2.wrapping_sub(1);
        if var_2 != 0 {
            WhiteBlobRoutine02Outcome::StillPaused { var_2 }
        } else {
            let var_4 = enemy_var_4.wrapping_add(2);
            let (source_x, source_y) = set_08_09_to_enemy_pos(enemy_x_pos, enemy_y_pos);
            let rotate = aim_var_1_for_quadrant_aim_dir_01(
                source_y,
                source_x,
                enemy_var_3,
                player_state,
                sprite_y_pos,
                sprite_x_pos,
                level_location_type,
                enemy_var_1,
            );
            let aim_dir = rotate.new_aim_dir;
            let (y_fast, y_frac) = WHITE_BLOB_ALIEN_FETUS_VEL_TBL[aim_dir as usize + 1];
            let (x_fast, x_frac) = WHITE_BLOB_ALIEN_FETUS_VEL_TBL[aim_dir as usize + 4];
            let (new_y_fast, new_y_frac) = mult_velocity_by_3(y_fast, y_frac);
            let (new_x_fast, new_x_frac) = mult_velocity_by_3(x_fast, x_frac);
            WhiteBlobRoutine02Outcome::Redashed {
                var_4,
                var_2: var_4,
                aim_dir,
                y_velocity: (new_y_frac, new_y_fast),
                x_velocity: (new_x_frac, new_x_fast),
            }
        }
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

    WhiteBlobRoutine02Result { sprite, position, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routine_00_var_2_is_bounded_correctly() {
        let r = white_blob_routine_00(0, 0, 0x1F, 0, 0, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r.var_2, 0x1F + 0x50);
    }

    #[test]
    fn routine_00_p2_game_over_leaves_var_3_unchanged() {
        let r = white_blob_routine_00(0x07, 0, 0x00, 0x00, 0, 1, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r.var_3, None);
        // aiming still uses the existing (input) var_3 = 0x07 -> masked implicitly by callers, here just confirm it composes.
    }

    #[test]
    fn routine_00_p1_game_over_forces_target_to_player_2() {
        let r = white_blob_routine_00(0x00, 0, 0x00, 0x00, 1, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r.var_3, Some(0x01));
    }

    #[test]
    fn routine_00_advances_the_routine_raw() {
        let r = white_blob_routine_00(0, 0, 0, 0, 0, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r.new_routine, 6);
    }

    #[test]
    fn spider_set_sprite_counts_down_without_changing_sprite() {
        // timer nibble = 5 -> decremented to 4, nonzero -> no sprite change.
        let r = white_blob_spider_set_sprite(0xB0, 0x53);
        assert_eq!(r.timer_after, 0x04);
        assert_eq!(r.sprite, None);
        assert_eq!(r.animation_delay, 0x43); // timer(4)<<4 | index(3, unchanged)
    }

    #[test]
    fn spider_set_sprite_changes_sprite_and_resets_when_timer_hits_zero() {
        // timer nibble = 1 -> decremented to 0 -> reset to 8, sprite index 0 -> table[0]=0x00.
        let r = white_blob_spider_set_sprite(0xB0, 0x10);
        assert_eq!(r.timer_after, 0x08);
        assert_eq!(r.sprite, Some(0xB0));
        assert_eq!(r.animation_delay, 0x81); // timer(8)<<4 | new_index(1)
    }

    #[test]
    fn spider_set_sprite_wraps_the_cycle_at_the_terminator() {
        // sprite index 4 -> table[4]=0xff (terminator) -> wraps to table[0]=0x00, new index=1.
        let r = white_blob_spider_set_sprite(0xB0, 0x14);
        assert_eq!(r.sprite, Some(0xB0));
        assert_eq!(r.animation_delay, 0x81);
    }

    #[test]
    fn routine_01_floating_adjusts_velocity_only_when_timer_reads_exactly_8() {
        // enemy_animation_delay high nibble = 9 -> decrements to 8 -> timer_after=8 (the "coincidental 8" case, no sprite change).
        let r = white_blob_routine_01(0x90, 0x00, 0x05, 0, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0x10, 0x00, 0, 0x08, 0x00, 0, 5);
        match r.outcome {
            WhiteBlobRoutine01Outcome::Floating(f) => {
                assert!(f.adjusted_velocity.is_some());
                let expected = white_blob_adjust_velocity(0x00, 0x08, 0x10);
                assert_eq!(f.adjusted_velocity, Some(expected));
            }
            other => panic!("expected Floating, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_floating_no_adjustment_when_timer_is_not_8() {
        let r = white_blob_routine_01(0x50, 0x00, 0x05, 0, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0, 0, 5);
        match r.outcome {
            WhiteBlobRoutine01Outcome::Floating(f) => assert_eq!(f.adjusted_velocity, None),
            other => panic!("expected Floating, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_floating_rolls_freeze_length_once_var_2_reaches_zero() {
        let r = white_blob_routine_01(0x50, 0x00, 0x01, 0, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0, 0x21, 5);
        match r.outcome {
            WhiteBlobRoutine01Outcome::Floating(f) => {
                assert_eq!(f.var_2, 0x00);
                assert_eq!(f.freeze_length, Some((0x21 & 0x20) + 2));
            }
            other => panic!("expected Floating, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_frozen_still_counting_down() {
        let r = white_blob_routine_01(0x50, 0, 0, 0, 0x05, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0, 0, 5);
        match r.outcome {
            WhiteBlobRoutine01Outcome::Frozen(f) => {
                assert_eq!(f.y_velocity, (0x00, 0x00));
                assert_eq!(f.x_velocity, (0x00, 0x00));
                assert_eq!(f.outcome, WhiteBlobFrozenOutcome::StillFreezing { var_4: 0x04 });
            }
            other => panic!("expected Frozen, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_frozen_locks_on_once_the_timer_elapses() {
        let r = white_blob_routine_01(0x50, 0x00, 0, 0, 0x01, 0x50, 0x60, [1, 0], [0x30, 0], [0x90, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0, 0, 5);
        match r.outcome {
            WhiteBlobRoutine01Outcome::Frozen(WhiteBlobFrozenResult {
                outcome: WhiteBlobFrozenOutcome::LockedOn { var_4, var_2, new_routine, .. },
                ..
            }) => {
                assert_eq!(var_4, 0x08);
                assert_eq!(var_2, 0x08);
                assert_eq!(new_routine, 6);
            }
            other => panic!("expected LockedOn, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_idle_when_var_4_is_zero() {
        let r = white_blob_routine_02(0x50, 0x00, 0, 0, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.outcome, WhiteBlobRoutine02Outcome::Idle);
    }

    #[test]
    fn routine_02_still_paused_when_var_2_has_not_elapsed() {
        let r = white_blob_routine_02(0x50, 0x08, 0x05, 0, 0, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        assert_eq!(r.outcome, WhiteBlobRoutine02Outcome::StillPaused { var_2: 0x04 });
    }

    #[test]
    fn routine_02_redashes_using_the_plus_1_plus_4_phase_offset() {
        let r = white_blob_routine_02(0x50, 0x08, 0x01, 0x00, 0x00, 0x50, 0x60, [1, 0], [0x30, 0], [0x90, 0], 0, 0, 0x00, 0, 0, 0, 0, 0, 0);
        match r.outcome {
            WhiteBlobRoutine02Outcome::Redashed { var_4, var_2, aim_dir, y_velocity, x_velocity } => {
                assert_eq!(var_4, 0x0A); // 0x08 + 2
                assert_eq!(var_2, var_4);
                let (y_fast, y_frac) = WHITE_BLOB_ALIEN_FETUS_VEL_TBL[aim_dir as usize + 1];
                let (x_fast, x_frac) = WHITE_BLOB_ALIEN_FETUS_VEL_TBL[aim_dir as usize + 4];
                assert_eq!(y_velocity, { let (f, s) = mult_velocity_by_3(y_fast, y_frac); (s, f) });
                assert_eq!(x_velocity, { let (f, s) = mult_velocity_by_3(x_fast, x_frac); (s, f) });
            }
            other => panic!("expected Redashed, got {other:?}"),
        }
    }

    #[test]
    fn mult_velocity_by_3_matches_the_disassembly_worked_example() {
        // real ASM comment's own example: fast=$ff, fract=$23 (-.863) -> fast=$fd, fract=$69 (-2.589).
        let (fast, frac) = mult_velocity_by_3(0xFF, 0x23);
        assert_eq!((fast, frac), (0xFD, 0x69));
    }

    #[test]
    fn mult_velocity_by_3_zero_stays_zero() {
        assert_eq!(mult_velocity_by_3(0x00, 0x00), (0x00, 0x00));
    }
}
