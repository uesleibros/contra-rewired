//! Native port of the sniper ("rifle man")'s full `_00`-`_05` routine
//! table (`src/bank0.asm`, `$8958`-`$8b02`). `_02`-`_05` (attack, re-hide,
//! hit, and destroyed-gravity) were initially assumed blocked behind the
//! `get_rotate_01`/`quadrant_aim_dir_lookup_ptr_tbl` rotation/aiming
//! subsystem the same way `eye_projectile`/`spinning_bubbles` still are -
//! turned out that subsystem (`crate::enemy::quadrant_aim_dir::get_rotate_
//! 00`/`get_rotate_01`) and the shared soldier-family "hit"/"destroyed
//! gravity" tails (`crate::enemy::soldier::init_soldier_hit_vel`/`apply_
//! gravity_to_destroyed_soldier`, factored out of `soldier_routine_04`/
//! `05` this same pass) were both self-contained enough to port in one
//! stage.
//!
//! `ENEMY_ATTRIBUTES` selects one of 3 real sniper types: `0` standing
//! (always visible, fires from a fixed pose), `1` crouching/hiding
//! (only visible - and only takes the extra `+5` Y nudge `_00` applies -
//! while popped up to fire), `2` boss-screen hiding (same shape as type
//! 0 for everything `_00` itself touches, but its own sprite table and
//! `_01`'s own crouch-cycle-then-nudge path).
//!
//! ## `sniper_routine_01`'s 3 ways to reach "activated"
//!
//! Standing snipers (type `0`) skip the crouch-cycle animation entirely
//! and activate immediately once the delay elapses. Crouching/hiding
//! snipers (type `1`/`2`) cycle `ENEMY_FRAME` through 3 pop-up frames
//! first; once that finishes, type `2` (boss screen) applies a real
//! `-14`/`+1` Y/X position nudge before activating, while type `1`
//! (crouching) decrements `ENEMY_FRAME` once more instead and activates
//! directly - *unless* that decrement happens to land on exactly `0`, in
//! which case real ASM falls through into the *same* nudge code type
//! `2` uses. This last case is real, valid control flow this port still
//! models (`ActivatedFrom::CrouchFallthroughNudge`), but isn't
//! independently exercised - tracing the real frame arithmetic, it never
//! actually happens for type `1` starting from `_00`'s own initial
//! frame (`0`).

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::add_with_enemy_pos::add_with_enemy_pos;
use crate::enemy::create_enemy_bullet::{create_enemy_bullet_angle_a, CreatedBullet};
use crate::enemy::enemy_collision_flags::{disable_enemy_collision, enable_enemy_collision};
use crate::enemy::enemy_position_utils::{add_a_to_enemy_x_pos, add_a_to_enemy_y_pos, add_a_with_vert_scroll_to_enemy_y_pos};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_routine_to_a, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::quadrant_aim_dir::get_rotate_01;
use crate::enemy::soldier::{apply_gravity_to_destroyed_soldier, init_soldier_hit_vel, ApplyGravityToDestroyedSoldierResult, InitSoldierHitVelResult};

/// `sniper_animation_delay_tbl` (`$8979`, 3 bytes) - initial `ENEMY_
/// ANIMATION_DELAY`, indexed by sniper type.
const SNIPER_ANIMATION_DELAY_TBL: [u8; 3] = [0x01, 0x30, 0x80];
/// `sniper_frame_tbl` (`$897f`, 3 bytes) - initial `ENEMY_FRAME`
/// (`sniper_sprite_xx` offset), same indexing.
const SNIPER_FRAME_TBL: [u8; 3] = [0x03, 0x00, 0x00];

/// The full result of one [`sniper_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine00Result {
    pub animation_delay: u8,
    pub frame: u8,
    pub y_pos: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `sniper_routine_00` (`$8958`) - "load variables
/// (`ENEMY_ANIMATION_DELAY`, `ENEMY_FRAME`), adjust Y position for
/// crouching sniper". `sniper_type` is `ENEMY_ATTRIBUTES` (`0`/`1`/`2`).
pub fn sniper_routine_00(sniper_type: u8, enemy_y_pos: u8, vertical_scroll: u8, current_routine: u8) -> SniperRoutine00Result {
    let animation_delay = SNIPER_ANIMATION_DELAY_TBL[sniper_type as usize];
    let frame = SNIPER_FRAME_TBL[sniper_type as usize];

    let y_pos = add_a_with_vert_scroll_to_enemy_y_pos(0x04, vertical_scroll, enemy_y_pos);
    let y_pos = if sniper_type == 1 { add_a_to_enemy_y_pos(0x05, y_pos) } else { y_pos };

    SniperRoutine00Result { animation_delay, frame, y_pos, routine_update: advance_enemy_routine(current_routine) }
}

/// `sniper_sprite_00` (`$8b3b`, 7 bytes) - regular/hiding sniper sprite
/// codes (types `0`/`1`), indexed by `ENEMY_FRAME`.
const SNIPER_SPRITE_00: [u8; 7] = [0x44, 0x45, 0x46, 0x43, 0x42, 0x41, 0x29];
/// `sniper_sprite_01` (`$8b42`, 7 bytes) - boss-screen sniper sprite
/// codes (type `2`), same indexing.
const SNIPER_SPRITE_01: [u8; 7] = [0x44, 0x45, 0x46, 0x2C, 0x42, 0x2D, 0x29];

/// The full result of one [`sniper_set_sprite`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperSetSpriteResult {
    pub sprites: u8,
    pub sprite_attr: u8,
    /// `ENEMY_VAR_3` after decrementing (only when it was already
    /// nonzero - "firing" gun-recoil counter).
    pub var_3: u8,
}

/// Native port of `sniper_set_sprite` (`$8b02`) - "set sprite and
/// attributes based on sniper type and firing angle".
pub fn sniper_set_sprite(sniper_type: u8, enemy_frame: u8, enemy_var_2: u8, enemy_var_3: u8) -> SniperSetSpriteResult {
    let table = if sniper_type < 2 { &SNIPER_SPRITE_00 } else { &SNIPER_SPRITE_01 };
    let sprites = table[enemy_frame as usize];

    let base_attr = if enemy_var_2 & 0x01 == 0 { 0x40 } else { 0x00 };
    let (var_3, sprite_attr) = if enemy_var_3 != 0 { (enemy_var_3 - 1, base_attr | 0x08) } else { (enemy_var_3, base_attr) };

    SniperSetSpriteResult { sprites, sprite_attr, var_3 }
}

/// `sniper_attack_delay_tbl` (`$89cc`, 3 bytes) - delay between attack
/// rounds, indexed by sniper type.
const SNIPER_ATTACK_DELAY_TBL: [u8; 3] = [0x40, 0x04, 0x10];
/// `sniper_bullet_attack_count_tbl` (`$89cf`, 3 bytes) - bullets fired per attack
/// round, same indexing.
const SNIPER_BULLET_ATTACK_COUNT_TBL: [u8; 3] = [0x03, 0x01, 0x03];

/// Which of [`sniper_routine_01`]'s 3 real paths reached "activated" -
/// see this module's doc comment for why the third variant is real,
/// valid control flow that's nonetheless never actually exercised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivatedFrom {
    /// Standing sniper (type `0`) - skipped the crouch-cycle animation
    /// entirely.
    Standing,
    /// Boss-screen sniper (type `2`), crouch-cycle just finished - a
    /// `-14`/`+1` Y/X nudge applied.
    BossNudge { y_pos: u8, x_pos: u8 },
    /// Crouching sniper (type `1`), crouch-cycle just finished, and the
    /// extra frame decrement landed on `0` - real ASM falls through into
    /// the same nudge code `BossNudge` uses.
    CrouchFallthroughNudge { y_pos: u8, x_pos: u8 },
    /// Crouching sniper (type `1`), crouch-cycle just finished, extra
    /// frame decrement left it nonzero - no position nudge.
    Crouching,
}

/// The real branch [`sniper_routine_01`] takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniperRoutine01Outcome {
    /// `ENEMY_ANIMATION_DELAY` hadn't reached `0` yet.
    Waiting { animation_delay: u8 },
    /// Crouching/hiding sniper (type `1`/`2`), still cycling its pop-up
    /// animation.
    CrouchCycling { animation_delay: u8, frame: u8 },
    /// Reached "activated": enables collision, sets score/collision
    /// code, and rolls the next attack round's delay/bullet count.
    Activated {
        from: ActivatedFrom,
        frame: u8,
        state_width: u8,
        /// Always `$30`.
        score_collision: u8,
        attack_delay: u8,
        var_4: u8,
        routine_update: EnemyRoutineUpdate,
    },
}

/// The full result of one [`sniper_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine01Result {
    pub sprite: SniperSetSpriteResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: SniperRoutine01Outcome,
}

fn sniper_activated(sniper_type: u8, from: ActivatedFrom, frame: u8, enemy_state_width: u8, current_routine: u8) -> SniperRoutine01Outcome {
    SniperRoutine01Outcome::Activated {
        from,
        frame,
        state_width: enable_enemy_collision(enemy_state_width),
        score_collision: 0x30,
        attack_delay: SNIPER_ATTACK_DELAY_TBL[sniper_type as usize],
        var_4: SNIPER_BULLET_ATTACK_COUNT_TBL[sniper_type as usize],
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// Native port of `sniper_routine_01` (`$8982`) - "cycle crouch
/// animation (if crouching sniper), enable collision (when standing
/// only for crouching snipers)". See this module's doc comment for the
/// 3 ways to reach "activated".
#[allow(clippy::too_many_arguments)]
pub fn sniper_routine_01(
    sniper_type: u8,
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
    enemy_animation_delay: u8,
    enemy_state_width: u8,
    current_routine: u8,
) -> SniperRoutine01Result {
    let sprite = sniper_set_sprite(sniper_type, enemy_frame, enemy_var_2, enemy_var_3);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return SniperRoutine01Result { sprite, scroll, outcome: SniperRoutine01Outcome::Waiting { animation_delay: delay } };
    }

    let outcome = if sniper_type == 0 {
        sniper_activated(sniper_type, ActivatedFrom::Standing, enemy_frame, enemy_state_width, current_routine)
    } else {
        let new_frame = enemy_frame.wrapping_add(1);
        if new_frame < 3 {
            SniperRoutine01Outcome::CrouchCycling { animation_delay: 0x08, frame: new_frame }
        } else if sniper_type != 1 {
            let nudged_y = add_a_to_enemy_y_pos(0xF2, scroll.y_pos);
            let nudged_x = add_a_to_enemy_x_pos(0x01, scroll.x_pos);
            sniper_activated(sniper_type, ActivatedFrom::BossNudge { y_pos: nudged_y, x_pos: nudged_x }, new_frame, enemy_state_width, current_routine)
        } else {
            let decremented = new_frame.wrapping_sub(1);
            if decremented != 0 {
                sniper_activated(sniper_type, ActivatedFrom::Crouching, decremented, enemy_state_width, current_routine)
            } else {
                let nudged_y = add_a_to_enemy_y_pos(0xF2, scroll.y_pos);
                let nudged_x = add_a_to_enemy_x_pos(0x01, scroll.x_pos);
                sniper_activated(sniper_type, ActivatedFrom::CrouchFallthroughNudge { y_pos: nudged_y, x_pos: nudged_x }, decremented, enemy_state_width, current_routine)
            }
        }
    };

    SniperRoutine01Result { sprite, scroll, outcome }
}

/// `sniper_standing_sprite_tbl` (`$8b48`, 3 bytes) - firing sprite,
/// indexed by [`sniper_angle_bucket`]'s own 3-way bucket.
const SNIPER_STANDING_SPRITE_TBL: [u8; 3] = [0x04, 0x03, 0x05];
/// `sniper_bullet_y_offset` (`$8b4b`, 3 bytes) - bullet spawn Y offset
/// from `ENEMY_Y_POS`, same indexing.
const SNIPER_BULLET_Y_OFFSET: [u8; 3] = [0xEE, 0xF5, 0x06];
/// `sniper_bullet_x_offset` (`$8b4e`, 3 bytes) - bullet spawn X offset
/// magnitude from `ENEMY_X_POS` (sign flipped separately based on which
/// side the target is on), same indexing.
const SNIPER_BULLET_X_OFFSET: [u8; 3] = [0xF3, 0xF1, 0xF1];
/// `sniper_bullet_speed` (`$8b51`, 3 bytes) - bullet speed code, indexed
/// by sniper type.
const SNIPER_BULLET_SPEED: [u8; 3] = [0x03, 0x05, 0x03];
/// `sniper_animation_delay_2_tbl` (`$8971`, 3 bytes) - `ENEMY_ANIMATION_
/// DELAY` set once `sniper_routine_03`'s re-hide cycle finishes, indexed
/// by sniper type.
const SNIPER_ANIMATION_DELAY_2_TBL: [u8; 3] = [0x01, 0x60, 0x80];

/// Native port of `sniper_routine_02`'s own local angle-bucketing math
/// (`$8a20`-`$8a3c`, inline in the real ASM, factored out here for
/// clarity) - takes the raw aim direction [`get_rotate_01`] (or the
/// crouching sniper's own fixed left/right code) produced, rotates it a
/// quarter-turn and mirrors it into a `0..12` half-range, then buckets it
/// into one of 3 firing-sprite/offset-table rows. Note this bucketed
/// value is used *only* to index [`SNIPER_STANDING_SPRITE_TBL`]/[`SNIPER_
/// BULLET_Y_OFFSET`]/[`SNIPER_BULLET_X_OFFSET`] - the bullet itself is
/// still created with the raw, unbucketed aim direction (see
/// [`sniper_routine_02`]'s own use of `aim_dir` vs this function's
/// return value).
fn sniper_angle_bucket(aim_dir: u8) -> u8 {
    let mut a = aim_dir.wrapping_add(0x06);
    if a >= 0x18 {
        a = a.wrapping_sub(0x18);
    }
    if a >= 0x0C {
        a = 0x18u8.wrapping_sub(a);
    }
    if a < 0x05 {
        0
    } else if a < 0x08 {
        1
    } else {
        2
    }
}

/// The full result of one [`sniper_routine_02`] call's `Fired` outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine02FiredResult {
    /// `ENEMY_VAR_2` - `0` if the closer player is to the enemy's left,
    /// `1` if to the right (or exactly level).
    pub var_2: u8,
    /// The raw aim direction (`$0c`) the bullet is actually created
    /// with - [`get_rotate_01`]'s own output for a standing/boss sniper,
    /// or a fixed `0x00`/`0x0c` (left/right) for a crouching one, which
    /// never calls [`get_rotate_01`] at all.
    pub aim_dir: u8,
    /// `ENEMY_FRAME` - only set for a standing/boss sniper (`Some`);
    /// left untouched for a crouching one.
    pub enemy_frame: Option<u8>,
    pub bullet_y_pos: u8,
    pub bullet_x_pos: u8,
    pub bullet: Option<CreatedBullet>,
    /// `ENEMY_VAR_3` (gun-recoil timer) - only set to `0x06` if the
    /// bullet was actually created.
    pub var_3: Option<u8>,
}

/// The real, branchy result of one [`sniper_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniperRoutine02Outcome {
    /// Attack delay hadn't elapsed.
    Waiting { attack_delay: u8 },
    /// Delay elapsed and bullets remain: fires one.
    Fired(SniperRoutine02FiredResult),
    /// Delay elapsed and `ENEMY_VAR_4` just went negative (all bullets
    /// fired), standing sniper (type `0`): immediately re-arms for
    /// another attack round *without* changing routine (real ASM: `rts`,
    /// not `jmp advance_enemy_routine`) - it keeps attacking in place.
    AllFiredStanding { var_4: u8, attack_delay: u8 },
    /// Delay elapsed and all bullets fired, crouching/boss sniper (type
    /// `1`/`2`): sets the re-hide sprite frame and advances to `sniper_
    /// routine_03`.
    AllFiredHiding { enemy_frame: u8, animation_delay: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`sniper_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine02Result {
    pub sprite: SniperSetSpriteResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: SniperRoutine02Outcome,
}

/// Native port of `sniper_routine_02` (`$89d2`) - "attack": counts down
/// between bullets, aiming each one at the closer player (standing/boss
/// snipers rotate a real aim direction via [`get_rotate_01`]; crouching
/// snipers just fire straight at whichever side the player is on), until
/// `ENEMY_VAR_4` (the per-round bullet counter `sniper_routine_01` set)
/// runs out.
#[allow(clippy::too_many_arguments)]
pub fn sniper_routine_02(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    sniper_type: u8,
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    enemy_var_1: u8,
    enemy_var_4: u8,
    enemy_attack_delay: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_routine: u8,
) -> SniperRoutine02Result {
    let sprite = sniper_set_sprite(sniper_type, enemy_frame, enemy_var_2, enemy_var_3);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);

    let attack_delay = enemy_attack_delay.wrapping_sub(1);
    let outcome = if attack_delay != 0 {
        SniperRoutine02Outcome::Waiting { attack_delay }
    } else {
        let var_4 = enemy_var_4.wrapping_sub(1);
        if (var_4 as i8) < 0 {
            if sniper_type == 0 {
                SniperRoutine02Outcome::AllFiredStanding { var_4: SNIPER_BULLET_ATTACK_COUNT_TBL[0], attack_delay: 0x80 }
            } else {
                let enemy_frame = if sniper_type == 1 { 0x02 } else { 0x03 };
                SniperRoutine02Outcome::AllFiredHiding {
                    enemy_frame,
                    animation_delay: 0x80,
                    routine_update: advance_enemy_routine(current_routine),
                }
            }
        } else {
            let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
            let var_2: u8 = if sprite_x_pos[closest.player_index as usize] < enemy_x_pos { 0 } else { 1 };

            let aim_dir = if sniper_type == 1 {
                if var_2 == 0 { 0x0C } else { 0x00 }
            } else {
                let y_offset = if sniper_type == 2 { 0xF0 } else { 0x00 };
                let (source_x, source_y) = add_with_enemy_pos(0x00, y_offset, enemy_x_pos, enemy_y_pos);
                get_rotate_01(source_y, source_x, closest.player_index, player_state, sprite_y_pos, sprite_x_pos, level_location_type, enemy_var_1)
                    .new_aim_dir
            };

            let bucket = sniper_angle_bucket(aim_dir) as usize;
            let enemy_frame = if sniper_type != 1 { Some(SNIPER_STANDING_SPRITE_TBL[bucket]) } else { None };

            let bullet_y_pos = enemy_y_pos.wrapping_add(SNIPER_BULLET_Y_OFFSET[bucket]);
            let x_offset = SNIPER_BULLET_X_OFFSET[bucket];
            let x_offset = if var_2 != 0 { x_offset.wrapping_neg() } else { x_offset };
            let bullet_x_pos = enemy_x_pos.wrapping_add(x_offset);

            let speed = SNIPER_BULLET_SPEED[sniper_type as usize];
            let bullet = create_enemy_bullet_angle_a(prg_rom, enemy_routine, current_level, enemy_attack_flag, aim_dir, speed, bullet_y_pos, bullet_x_pos);
            let var_3 = if bullet.is_some() { Some(0x06) } else { None };

            SniperRoutine02Outcome::Fired(SniperRoutine02FiredResult { var_2, aim_dir, enemy_frame, bullet_y_pos, bullet_x_pos, bullet, var_3 })
        }
    };

    SniperRoutine02Result { sprite, scroll, outcome }
}

/// The real, branchy result of one [`sniper_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SniperRoutine03Outcome {
    /// `ENEMY_ANIMATION_DELAY` hadn't reached `0` yet.
    Waiting { animation_delay: u8 },
    /// Delay reached `0`: disables collision, decrements `ENEMY_FRAME`
    /// through the un-hiding animation, and (once that reaches `0`) jumps
    /// straight back to `sniper_routine_02` (`set_enemy_routine_to_a`,
    /// *not* an advance) to attack again.
    Active { state_width: u8, animation_delay: u8, routine_update: Option<EnemyRoutineUpdate> },
}

/// The full result of one [`sniper_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine03Result {
    pub enemy_frame: u8,
    pub y_pos: u8,
    pub x_pos: u8,
    pub sprite: SniperSetSpriteResult,
    pub scroll: ScrolledEnemyPos,
    pub outcome: SniperRoutine03Outcome,
}

/// Native port of `sniper_routine_03` (`$8ab3`) - "re-hide": counts down
/// a delay, then plays the un-hiding animation in reverse (`ENEMY_FRAME`
/// decrementing) until it reaches `0`, at which point it jumps back to
/// `sniper_routine_02` to attack again. Boss-screen snipers (type `2`)
/// get one extra real quirk: exactly when the decrementing frame lands on
/// `2`, a `-14`/`+1` Y/X position nudge is applied every remaining call
/// this state runs (not just once) - real ASM re-checks `ENEMY_FRAME == 2`
/// unconditionally each call, it doesn't gate on "just reached 2".
#[allow(clippy::too_many_arguments)]
pub fn sniper_routine_03(
    sniper_type: u8,
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    enemy_animation_delay: u8,
    enemy_state_width: u8,
    enemy_y_pos: u8,
    enemy_x_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
) -> SniperRoutine03Result {
    let delay = enemy_animation_delay.wrapping_sub(1);

    let (enemy_frame, y_pos, x_pos, outcome) = if delay != 0 {
        (enemy_frame, enemy_y_pos, enemy_x_pos, SniperRoutine03Outcome::Waiting { animation_delay: delay })
    } else {
        let state_width = disable_enemy_collision(enemy_state_width);
        let new_frame = enemy_frame.wrapping_sub(1);
        let (animation_delay, routine_update) = if new_frame == 0 {
            (SNIPER_ANIMATION_DELAY_2_TBL[sniper_type as usize], Some(set_enemy_routine_to_a(current_routine, 0x02)))
        } else {
            (0x08, None)
        };
        let (y_pos, x_pos) = if sniper_type == 2 && new_frame == 2 {
            (add_a_to_enemy_y_pos(0x0E, enemy_y_pos), add_a_to_enemy_x_pos(0xFF, enemy_x_pos))
        } else {
            (enemy_y_pos, enemy_x_pos)
        };
        (new_frame, y_pos, x_pos, SniperRoutine03Outcome::Active { state_width, animation_delay, routine_update })
    };

    let sprite = sniper_set_sprite(sniper_type, enemy_frame, enemy_var_2, enemy_var_3);
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    SniperRoutine03Result { enemy_frame, y_pos, x_pos, sprite, scroll, outcome }
}

/// The full result of one [`sniper_routine_04`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine04Result {
    /// Always `0x06` - the destroyed-sniper sprite frame.
    pub enemy_frame: u8,
    pub sprite: SniperSetSpriteResult,
    pub hit_vel: InitSoldierHitVelResult,
}

/// Native port of `sniper_routine_04` (`$8af1`) - "hit": sets the
/// destroyed-sniper sprite frame, then falls into the same shared
/// `init_soldier_hit_vel` tail `soldier_routine_04` uses.
#[allow(clippy::too_many_arguments)]
pub fn sniper_routine_04(
    sniper_type: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    enemy_state_width: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
) -> SniperRoutine04Result {
    let enemy_frame = 0x06;
    let sprite = sniper_set_sprite(sniper_type, enemy_frame, enemy_var_2, enemy_var_3);
    let hit_vel = init_soldier_hit_vel(enemy_x_pos, enemy_y_pos, enemy_var_2, enemy_state_width, level_scrolling_type, frame_scroll, current_routine);
    SniperRoutine04Result { enemy_frame, sprite, hit_vel }
}

/// The full result of one [`sniper_routine_05`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SniperRoutine05Result {
    pub sprite: SniperSetSpriteResult,
    pub gravity: ApplyGravityToDestroyedSoldierResult,
}

/// Native port of `sniper_routine_05` (`$8afc`) - "destroyed, apply
/// gravity": sets the sprite, then falls into the same shared `apply_
/// gravity_to_destroyed_soldier` tail `soldier_routine_05` uses.
#[allow(clippy::too_many_arguments)]
pub fn sniper_routine_05(
    sniper_type: u8,
    enemy_frame: u8,
    enemy_var_2: u8,
    enemy_var_3: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    y_vel_fract: u8,
    y_vel_fast: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    y_vel_accum: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_animation_delay: u8,
    current_routine: u8,
) -> SniperRoutine05Result {
    let sprite = sniper_set_sprite(sniper_type, enemy_frame, enemy_var_2, enemy_var_3);
    let gravity = apply_gravity_to_destroyed_soldier(
        enemy_x_pos,
        enemy_y_pos,
        y_vel_fract,
        y_vel_fast,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        y_vel_accum,
        level_scrolling_type,
        frame_scroll,
        enemy_animation_delay,
        current_routine,
    );
    SniperRoutine05Result { sprite, gravity }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_sniper_uses_type_0_row_and_no_extra_nudge() {
        let r = sniper_routine_00(0, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x01);
        assert_eq!(r.frame, 0x03);
        assert_eq!(r.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60));
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    #[test]
    fn crouching_sniper_gets_the_extra_5_pixel_nudge() {
        let r = sniper_routine_00(1, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x30);
        assert_eq!(r.frame, 0x00);
        let after_4 = add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60);
        assert_eq!(r.y_pos, add_a_to_enemy_y_pos(0x05, after_4));
    }

    #[test]
    fn boss_screen_sniper_uses_type_2_row_and_no_extra_nudge() {
        let r = sniper_routine_00(2, 0x60, 0x00, 5);
        assert_eq!(r.animation_delay, 0x80);
        assert_eq!(r.frame, 0x00);
        assert_eq!(r.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x00, 0x60));
    }

    #[test]
    fn vertical_scroll_is_threaded_through_the_first_add() {
        let with_scroll = sniper_routine_00(0, 0x60, 0x08, 5);
        assert_eq!(with_scroll.y_pos, add_a_with_vert_scroll_to_enemy_y_pos(0x04, 0x08, 0x60));
    }

    #[test]
    fn set_sprite_uses_type_0_table_for_standing_and_crouching() {
        let standing = sniper_set_sprite(0, 3, 0x00, 0);
        let crouching = sniper_set_sprite(1, 3, 0x00, 0);
        assert_eq!(standing.sprites, SNIPER_SPRITE_00[3]);
        assert_eq!(crouching.sprites, SNIPER_SPRITE_00[3]);
    }

    #[test]
    fn set_sprite_uses_type_1_table_for_boss_screen() {
        let r = sniper_set_sprite(2, 3, 0x00, 0);
        assert_eq!(r.sprites, SNIPER_SPRITE_01[3]);
    }

    #[test]
    fn set_sprite_attr_reflects_firing_angle_bit_0() {
        let even = sniper_set_sprite(0, 0, 0x00, 0);
        let odd = sniper_set_sprite(0, 0, 0x01, 0);
        assert_eq!(even.sprite_attr, 0x40);
        assert_eq!(odd.sprite_attr, 0x00);
    }

    #[test]
    fn set_sprite_decrements_var_3_and_sets_the_recoil_bit_only_when_nonzero() {
        let firing = sniper_set_sprite(0, 0, 0x00, 0x03);
        assert_eq!(firing.var_3, 0x02);
        assert_eq!(firing.sprite_attr, 0x40 | 0x08);
        let idle = sniper_set_sprite(0, 0, 0x00, 0x00);
        assert_eq!(idle.var_3, 0x00);
        assert_eq!(idle.sprite_attr, 0x40);
    }

    #[test]
    fn routine_01_standing_activates_immediately_once_delay_elapses() {
        let r = sniper_routine_01(0, 0x03, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        match r.outcome {
            SniperRoutine01Outcome::Activated { from: ActivatedFrom::Standing, frame, state_width, score_collision, attack_delay, var_4, routine_update } => {
                assert_eq!(frame, 0x03);
                assert_eq!(state_width, enable_enemy_collision(0x00));
                assert_eq!(score_collision, 0x30);
                assert_eq!(attack_delay, SNIPER_ATTACK_DELAY_TBL[0]);
                assert_eq!(var_4, SNIPER_BULLET_ATTACK_COUNT_TBL[0]);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Activated/Standing, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_waits_while_delay_has_not_elapsed() {
        let r = sniper_routine_01(0, 0x03, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x05, 0x00, 5);
        assert_eq!(r.outcome, SniperRoutine01Outcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn routine_01_crouching_cycles_through_3_frames_then_activates_without_a_nudge() {
        // Starting frame 0, delay elapses on 3 successive calls.
        let step1 = sniper_routine_01(1, 0x00, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(step1.outcome, SniperRoutine01Outcome::CrouchCycling { animation_delay: 0x08, frame: 0x01 });
        let step2 = sniper_routine_01(1, 0x01, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(step2.outcome, SniperRoutine01Outcome::CrouchCycling { animation_delay: 0x08, frame: 0x02 });
        let step3 = sniper_routine_01(1, 0x02, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        match step3.outcome {
            SniperRoutine01Outcome::Activated { from: ActivatedFrom::Crouching, frame, .. } => assert_eq!(frame, 0x02),
            other => panic!("expected Activated/Crouching, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_boss_screen_applies_the_nudge_after_the_crouch_cycle() {
        let r = sniper_routine_01(2, 0x02, 0x00, 0x00, 0, 0x01, 0x50, 0x60, 0x01, 0x00, 5);
        match r.outcome {
            SniperRoutine01Outcome::Activated { from: ActivatedFrom::BossNudge { y_pos, x_pos }, frame, .. } => {
                assert_eq!(frame, 0x03);
                assert_eq!(y_pos, add_a_to_enemy_y_pos(0xF2, r.scroll.y_pos));
                assert_eq!(x_pos, add_a_to_enemy_x_pos(0x01, r.scroll.x_pos));
            }
            other => panic!("expected Activated/BossNudge, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_scroll_matches_add_scroll_to_enemy_pos_directly() {
        let r = sniper_routine_01(0, 0x03, 0x00, 0x00, 0, 0x02, 0x50, 0x60, 0x01, 0x00, 5);
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
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
    fn routine_02_waits_when_attack_delay_has_not_elapsed() {
        let rom = synthetic_prg_rom();
        let r = sniper_routine_02(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0, 0x00, 0x00, 0x00, 0x00, 0x02, 0x05, 0x50, 0x50, 0, 0x00, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r.outcome, SniperRoutine02Outcome::Waiting { attack_delay: 0x04 });
    }

    #[test]
    fn routine_02_standing_sniper_fires_using_get_rotate_01() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[9] = 0; // free slot
        let r = sniper_routine_02(
            &rom,
            &routine,
            0,
            1,
            0x00, // standing sniper
            0x00,
            0x00,
            0x00,
            0x00, // enemy_var_1 (current aim dir)
            0x01, // 1 bullet left
            0x01, // attack delay -> decrements to 0
            0x50,
            0x50,
            0,
            0x00,
            [1, 0],
            [0x50, 0x00],
            [0x90, 0x00], // player to the right
            0,
            5,
        );
        match r.outcome {
            SniperRoutine02Outcome::Fired(f) => {
                let closest = player_enemy_x_dist([0x90, 0x00], 0x50, [1, 0]);
                assert_eq!(f.var_2, 1); // player X (0x90) >= enemy X (0x50)
                let (source_x, source_y) = add_with_enemy_pos(0x00, 0x00, 0x50, 0x50);
                let expected_aim =
                    get_rotate_01(source_y, source_x, closest.player_index, [1, 0], [0x50, 0x00], [0x90, 0x00], 0, 0x00).new_aim_dir;
                assert_eq!(f.aim_dir, expected_aim);
                assert!(f.enemy_frame.is_some());
                assert!(f.bullet.is_some());
                assert_eq!(f.var_3, Some(0x06));
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_crouching_sniper_fires_straight_without_rotating() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[9] = 0;
        let r = sniper_routine_02(
            &rom,
            &routine,
            0,
            1,
            0x01, // crouching sniper
            0x00,
            0x00,
            0x00,
            0x00,
            0x01,
            0x01,
            0x50,
            0x50,
            0,
            0x00,
            [1, 0],
            [0x50, 0x00],
            [0x20, 0x00], // player to the left
            0,
            5,
        );
        match r.outcome {
            SniperRoutine02Outcome::Fired(f) => {
                assert_eq!(f.var_2, 0); // player left of enemy
                assert_eq!(f.aim_dir, 0x0C); // var_2==0 -> fixed left-facing code, no get_rotate_01 call
                assert_eq!(f.enemy_frame, None); // crouching sniper never sets ENEMY_FRAME here
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_all_fired_standing_rearms_without_changing_routine() {
        let rom = synthetic_prg_rom();
        let r =
            sniper_routine_02(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x00, 0, 0, 0, 0, 0x00, 0x01, 0x50, 0x50, 0, 0x00, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r.outcome, SniperRoutine02Outcome::AllFiredStanding { var_4: SNIPER_BULLET_ATTACK_COUNT_TBL[0], attack_delay: 0x80 });
    }

    #[test]
    fn routine_02_all_fired_hiding_advances_to_routine_03() {
        let rom = synthetic_prg_rom();
        let r =
            sniper_routine_02(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x01, 0, 0, 0, 0, 0x00, 0x01, 0x50, 0x50, 0, 0x00, [0, 0], [0, 0], [0, 0], 0, 5);
        match r.outcome {
            SniperRoutine02Outcome::AllFiredHiding { enemy_frame, animation_delay, routine_update } => {
                assert_eq!(enemy_frame, 0x02);
                assert_eq!(animation_delay, 0x80);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected AllFiredHiding, got {other:?}"),
        }
        let boss = sniper_routine_02(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x02, 0, 0, 0, 0, 0x00, 0x01, 0x50, 0x50, 0, 0x00, [0, 0], [0, 0], [0, 0], 0, 5);
        match boss.outcome {
            SniperRoutine02Outcome::AllFiredHiding { enemy_frame, .. } => assert_eq!(enemy_frame, 0x03),
            other => panic!("expected AllFiredHiding, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_waits_when_delay_has_not_elapsed() {
        let r = sniper_routine_03(0, 0x02, 0x00, 0x00, 0x05, 0x00, 0x50, 0x60, 0, 0x01, 5);
        assert_eq!(r.outcome, SniperRoutine03Outcome::Waiting { animation_delay: 0x04 });
        assert_eq!(r.enemy_frame, 0x02);
    }

    #[test]
    fn routine_03_active_frame_still_nonzero_after_decrement() {
        let r = sniper_routine_03(0, 0x02, 0x00, 0x00, 0x01, 0x00, 0x50, 0x60, 0, 0x01, 5);
        assert_eq!(r.enemy_frame, 0x01);
        match r.outcome {
            SniperRoutine03Outcome::Active { animation_delay, routine_update, .. } => {
                assert_eq!(animation_delay, 0x08);
                assert_eq!(routine_update, None);
            }
            other => panic!("expected Active, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_frame_reaches_zero_jumps_back_to_routine_02() {
        let r = sniper_routine_03(0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x50, 0x60, 0, 0x01, 5);
        assert_eq!(r.enemy_frame, 0x00);
        match r.outcome {
            SniperRoutine03Outcome::Active { animation_delay, routine_update, .. } => {
                assert_eq!(animation_delay, SNIPER_ANIMATION_DELAY_2_TBL[1]);
                assert_eq!(routine_update, Some(set_enemy_routine_to_a(5, 0x02)));
            }
            other => panic!("expected Active, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_boss_screen_nudges_position_when_frame_is_2() {
        let r = sniper_routine_03(0x02, 0x03, 0x00, 0x00, 0x01, 0x00, 0x50, 0x60, 0, 0x01, 5);
        assert_eq!(r.enemy_frame, 0x02);
        assert_eq!(r.y_pos, add_a_to_enemy_y_pos(0x0E, 0x50));
        assert_eq!(r.x_pos, add_a_to_enemy_x_pos(0xFF, 0x60));
    }

    #[test]
    fn routine_03_no_nudge_when_not_boss_or_frame_not_2() {
        let r = sniper_routine_03(0x00, 0x03, 0x00, 0x00, 0x01, 0x00, 0x50, 0x60, 0, 0x01, 5);
        assert_eq!(r.y_pos, 0x50);
        assert_eq!(r.x_pos, 0x60);
    }

    #[test]
    fn routine_04_sets_frame_6_and_delegates_to_init_soldier_hit_vel() {
        let r = sniper_routine_04(0x00, 0x00, 0x00, 0x00, 0x50, 0x60, 0, 0x01, 5);
        assert_eq!(r.enemy_frame, 0x06);
        assert_eq!(r.sprite, sniper_set_sprite(0x00, 0x06, 0x00, 0x00));
        assert_eq!(r.hit_vel, init_soldier_hit_vel(0x50, 0x60, 0x00, 0x00, 0, 0x01, 5));
    }

    #[test]
    fn routine_05_sets_sprite_and_delegates_to_apply_gravity() {
        let r = sniper_routine_05(0x00, 0x06, 0x00, 0x00, 0x50, 0x60, 0x00, 0x00, 0, 0x00, 0xFF, 0, 0, 0x01, 0x05, 5);
        assert_eq!(r.sprite, sniper_set_sprite(0x00, 0x06, 0x00, 0x00));
        let expected = apply_gravity_to_destroyed_soldier(0x50, 0x60, 0x00, 0x00, 0, 0x00, 0xFF, 0, 0, 0x01, 0x05, 5);
        assert_eq!(r.gravity, expected);
    }
}
