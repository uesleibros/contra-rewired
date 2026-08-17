//! Native port of the "indoor soldier" enemy family's shared position/
//! velocity/sprite helpers, plus the indoor soldier's own `_00`/`_01`
//! routines. `indoor_soldier_routine_ptr_tbl` (`src/bank0.asm`, `$92c8`-
//! onward) is actually shared by **4 real enemy types** ($15 indoor
//! soldier, $16 jumping soldier, $17 grenade launcher, $18 group of four
//! soldiers) - this module only carries the pieces reachable from
//! `indoor_soldier_routine_00`/`_01`; the other 3 enemy types' own
//! `_00`/`_01` entries are not yet ported (they reuse some of the same
//! shared helpers here, e.g. `apply_enemy_velocity_set_bg_priority`, with
//! different callers still missing).
//!
//! ## `indoor_soldier_routine_01`'s weapon-type branch and its 3 new
//! sub-routines
//!
//! [`indoor_soldier_routine_01`] (`$92d5`) waits for `ENEMY_ATTACK_DELAY`
//! to elapse, then fires one of 3 weapons based on `(ENEMY_ATTRIBUTES >>
//! 1) & 3`: [`create_indoor_bullet`] (`$9784`, type `0`), a grenade via
//! [`enemy_launch_grenade`] (`$9743`, type `1` - gated by `ENEMY_VAR_1`'s
//! own parity, so it only actually fires every *other* time this branch
//! is reached, effectively doubling the attack delay for this weapon
//! type specifically), or [`create_roller`]/[`create_roller_with_segment_a`]
//! (`$9700`/`$9703`, types `2` *and* `3` alike - real ASM: `dey; bne
//! @create_roller` only special-cases `y == 1` for the grenade, so type
//! `3` silently falls into the same roller path as type `2`, not a 4th
//! weapon).
//!
//! ## The stale-`$0a` roller quirk
//!
//! `create_roller_with_segment_a`'s real body writes `ENEMY_ATTRIBUTES,x`
//! for the new roller from `$0a` (real comment: "load ENEMY_ATTRIBUTES") -
//! but tracing every real caller shows `indoor_soldier_routine_01`'s own
//! `@create_roller` path (`ldy #$08; lda #$00; jsr add_with_enemy_pos;
//! jmp create_roller`) never writes `$0a` itself, and the master enemy
//! dispatch loop (`exe_enemy_routine_loop`, `bank7.asm`) doesn't either -
//! so the roller's `ENEMY_ATTRIBUTES` ends up being whatever `$0a`
//! happened to hold left over from a *different, unrelated* routine
//! earlier in the same frame. This port makes that explicit rather than
//! guessing a value: [`create_roller`]/[`create_roller_with_segment_a`]
//! take it as a plain `attributes_scratch` parameter, and [`indoor_
//! soldier_routine_01`] passes its own same-named parameter straight
//! through untouched - faithfully preserving the real quirk instead of
//! "fixing" it.
//!
//! ## Attack-delay continues even through a same-frame removal
//!
//! `apply_enemy_velocity_set_bg_priority`'s off-screen removal path is a
//! plain `jmp remove_enemy` (not a `jsr`), so `remove_enemy`'s own `rts`
//! returns straight back into `indoor_soldier_routine_01` right after the
//! `jsr apply_enemy_velocity_set_bg_priority` line - meaning the routine
//! goes on to decrement `ENEMY_ATTACK_DELAY` and can even fire a weapon
//! in the same frame an enemy was just removed (harmless in practice,
//! since `ENEMY_ROUTINE` is now `0` and this routine won't run again next
//! frame, but real, faithfully preserved control flow: [`indoor_soldier_
//! routine_01`] keeps going regardless of [`ApplyEnemyVelocityOutcome`]).

use crate::enemy::add_with_enemy_pos::add_with_enemy_pos;
use crate::enemy::create_enemy_bullet::ENEMY_TYPE_BULLET;
use crate::enemy::enemy_clear::EnemyClearFields;
use crate::enemy::enemy_position_utils::reverse_enemy_x_direction;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::find_far_segment::find_far_segment_for_x_pos;
use crate::enemy::initialize_enemy::initialize_enemy;
use crate::enemy::update_enemy_pos::{remove_enemy, RemovedEnemy};

/// `indoor_soldier_x_velocity_tbl` (`$96b9`, 8 bytes) - `(x_vel_fract,
/// x_vel_fast)` per real enemy type sharing this init helper, indexed by
/// the `y` value each type's own `routine_00` passes in (indoor soldier
/// itself always uses index `0`).
const INDOOR_SOLDIER_X_VELOCITY_TBL: [(u8, u8); 4] = [
    (0x20, 0xFF), // indoor soldier (-.875)
    (0x40, 0xFF), // jumping soldier (-.75)
    (0x40, 0xFF), // group of 4 (-.75)
    (0x40, 0xFF), // grenade launcher (-.75)
];

/// The full result of one [`init_indoor_enemy_pos_and_vel`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitIndoorEnemyPosAndVelResult {
    pub x_velocity: (u8, u8),
    pub x_pos: u8,
    /// Always `$6d` - indoor levels use a fixed enemy Y position.
    pub y_pos: u8,
}

/// Native port of `init_indoor_enemy_pos_and_vel` (`$9697`) - places the
/// enemy at one of 2 fixed X positions (screen edges) based on `ENEMY_
/// ATTRIBUTES` bit 0, with an initial velocity from the shared table
/// above, reversed to run the opposite direction when spawning from the
/// left instead of the right.
pub fn init_indoor_enemy_pos_and_vel(y_index: u8, enemy_attributes: u8) -> InitIndoorEnemyPosAndVelResult {
    let (fract, fast) = INDOOR_SOLDIER_X_VELOCITY_TBL[y_index as usize];
    let (x_velocity, x_pos) =
        if enemy_attributes & 0x01 == 0 { ((fract, fast), 0xA8) } else { (reverse_enemy_x_direction(fract, fast), 0x58) };
    InitIndoorEnemyPosAndVelResult { x_velocity, x_pos, y_pos: 0x6D }
}

/// The real, branchy result of one [`apply_enemy_velocity_set_bg_priority`]
/// call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyEnemyVelocityOutcome {
    /// Past the real disappearance limit for the enemy's travel
    /// direction (`>= $b0` moving right, `< $50` moving left).
    Removed(RemovedEnemy),
    /// Still on screen: `ENEMY_SPRITE_ATTR` after the background-
    /// priority bit is set or cleared depending on X position (real ASM
    /// draws the enemy *behind* the background near either screen edge,
    /// same "behind pillar" shape as `red_blue_soldier_set_bg_priority`).
    BgPriority(u8),
}

/// The full result of one [`apply_enemy_velocity_set_bg_priority`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyEnemyVelocityResult {
    pub x_vel_accum: u8,
    pub x_pos: u8,
    pub outcome: ApplyEnemyVelocityOutcome,
}

/// Native port of `apply_enemy_velocity_set_bg_priority` (`$96c1`) - the
/// indoor soldier family's X-only velocity integrator (indoor enemies
/// never move vertically), shared by every routine in this family that
/// needs to move and stay on screen.
pub fn apply_enemy_velocity_set_bg_priority(x_vel_accum: u8, x_vel_fract: u8, x_vel_fast: u8, x_pos: u8, sprite_attr: u8) -> ApplyEnemyVelocityResult {
    let (new_accum, carry) = x_vel_accum.overflowing_add(x_vel_fract);
    let new_x_pos = x_pos.wrapping_add(x_vel_fast).wrapping_add(carry as u8);

    let moving_left = (x_vel_fast as i8) < 0;
    let removed = if moving_left { new_x_pos < 0x50 } else { new_x_pos >= 0xB0 };

    let outcome = if removed {
        ApplyEnemyVelocityOutcome::Removed(remove_enemy())
    } else {
        let behind_bg = new_x_pos >= 0xA0 || new_x_pos < 0x60;
        let attr = if behind_bg { sprite_attr | 0x20 } else { sprite_attr & 0xDF };
        ApplyEnemyVelocityOutcome::BgPriority(attr)
    };

    ApplyEnemyVelocityResult { x_vel_accum: new_accum, x_pos: new_x_pos, outcome }
}

/// The full result of one [`init_sprite_from_frame`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitSpriteFromFrameResult {
    pub enemy_frame: u8,
    pub sprites: u8,
    pub sprite_attr: u8,
}

/// Native port of `init_sprite_from_frame` (`$9316`) - cycles `ENEMY_
/// FRAME` through `0..3` every 4th frame (same cadence as `red_blue_
/// soldier_set_run_frame`, a different real routine with the same
/// shape), then sets the sprite code and horizontal-flip bit from the
/// current travel direction.
pub fn init_sprite_from_frame(frame_counter: u8, enemy_frame: u8, enemy_sprite_attr: u8, x_vel_fast: u8) -> InitSpriteFromFrameResult {
    let new_frame = if frame_counter & 0x03 != 0 {
        enemy_frame
    } else {
        let incremented = enemy_frame.wrapping_add(1);
        if incremented >= 0x03 {
            0x00
        } else {
            incremented
        }
    };
    let sprites = new_frame.wrapping_add(0x93);
    let moving_left = (x_vel_fast as i8) < 0;
    let sprite_attr = if moving_left { enemy_sprite_attr | 0x40 } else { enemy_sprite_attr & 0xBF };
    InitSpriteFromFrameResult { enemy_frame: new_frame, sprites, sprite_attr }
}

/// The full result of one [`indoor_soldier_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndoorSoldierRoutine00Result {
    pub init: InitIndoorEnemyPosAndVelResult,
    /// Always `$08`.
    pub attack_delay: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `indoor_soldier_routine_00` (`$92c8`) - "initializes
/// indoor soldier: sets position, velocity and attack delay".
pub fn indoor_soldier_routine_00(enemy_attributes: u8, current_routine: u8) -> IndoorSoldierRoutine00Result {
    let init = init_indoor_enemy_pos_and_vel(0, enemy_attributes);
    let routine_update = advance_enemy_routine(current_routine);
    IndoorSoldierRoutine00Result { init, attack_delay: 0x08, routine_update }
}

/// `indoor_bullet_velocity_tbl` (`$97c6`, 7 `(fract, fast)` entries) -
/// indexed by [`crate::enemy::find_far_segment::find_far_segment_for_x_pos`]'s
/// 0-6 horizontal-segment code.
const INDOOR_BULLET_VELOCITY_TBL: [(u8, u8); 7] =
    [(0xD4, 0x00), (0x8D, 0x00), (0x46, 0x00), (0x00, 0x00), (0xBA, 0xFF), (0x73, 0xFF), (0x2C, 0xFF)];

/// `roller_vel_code_tbl` (`$9735`, 7 entries) - byte-for-byte identical to
/// [`GRENADE_VEL_CODE_TBL`] in the real ROM (two separate copies of the
/// same data at different addresses), kept as its own const to match the
/// real ROM's own duplication rather than sharing one table.
const ROLLER_VEL_CODE_TBL: [(u8, u8); 7] =
    [(0x55, 0x00), (0x38, 0x00), (0x1C, 0x00), (0x00, 0x00), (0xE4, 0xFF), (0xC8, 0xFF), (0xAB, 0xFF)];

/// `grenade_vel_code_tbl` (`$9776`, 7 entries) - see [`ROLLER_VEL_CODE_TBL`]'s
/// doc comment.
const GRENADE_VEL_CODE_TBL: [(u8, u8); 7] =
    [(0x55, 0x00), (0x38, 0x00), (0x1C, 0x00), (0x00, 0x00), (0xE4, 0xFF), (0xC8, 0xFF), (0xAB, 0xFF)];

/// `ENEMY_TYPE` code for rollers (`$11`).
pub const ENEMY_TYPE_ROLLER: u8 = 0x11;
/// `ENEMY_TYPE` code for grenades (`$12`).
pub const ENEMY_TYPE_GRENADE: u8 = 0x12;

/// A successfully created indoor bullet/grenade/roller's full real field
/// set - one shared shape for all 3 (mirrors [`crate::enemy::create_enemy_bullet::CreatedBullet`]'s
/// own convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedIndoorEnemy {
    pub slot: u8,
    pub enemy_type: u8,
    pub hp: u8,
    pub fields: EnemyClearFields,
}

/// Native port of `create_indoor_bullet` (`$9784`) - fires a regular
/// indoor bullet from `(enemy_x_pos, enemy_y_pos)` (the calling enemy's
/// own position; real ASM reaches these via `set_08_09_to_enemy_pos`'s
/// zero-offset case). Unlike [`enemy_launch_grenade`]/[`create_roller`],
/// the on-screen X-range check (`$60..$a0`) runs *before* the attack-flag
/// check and doesn't consume an enemy slot either way - ported as two
/// separate early-outs in the same order.
pub fn create_indoor_bullet(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
) -> Option<CreatedIndoorEnemy> {
    if enemy_x_pos >= 0xA0 || enemy_x_pos < 0x60 {
        return None;
    }
    if enemy_attack_flag == 0 {
        return None;
    }
    let segment = find_far_segment_for_x_pos(enemy_x_pos);
    let slot = find_next_enemy_slot(enemy_routine)?;

    let init = initialize_enemy(prg_rom, ENEMY_TYPE_BULLET, current_level);
    let mut fields = init.fields;
    fields.var_1 = 0x03; // real comment: "indoor regular bullet type"
    let (frac, fast) = INDOOR_BULLET_VELOCITY_TBL[segment as usize];
    fields.x_velocity_fract = frac;
    fields.x_velocity_fast = fast;
    fields.y_velocity_fract = 0x40;
    fields.y_velocity_fast = 0x01;
    fields.x_pos = enemy_x_pos;
    fields.y_pos = enemy_y_pos;

    Some(CreatedIndoorEnemy { slot, enemy_type: ENEMY_TYPE_BULLET, hp: init.hp, fields })
}

/// Native port of `enemy_launch_grenade` (`$9743`) - shared by the indoor
/// soldier and the grenade launcher enemy. Real callers pass the calling
/// enemy's own position (`set_08_09_to_enemy_pos`'s zero-offset case),
/// same as [`create_indoor_bullet`]. Unlike that routine, there's no
/// on-screen range check here - only the attack-flag gate.
pub fn enemy_launch_grenade(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
) -> Option<CreatedIndoorEnemy> {
    let segment = find_far_segment_for_x_pos(enemy_x_pos);
    if enemy_attack_flag == 0 {
        return None;
    }
    let slot = find_next_enemy_slot(enemy_routine)?;

    let init = initialize_enemy(prg_rom, ENEMY_TYPE_GRENADE, current_level);
    let mut fields = init.fields;
    let (frac, fast) = GRENADE_VEL_CODE_TBL[segment as usize];
    fields.x_velocity_fract = frac;
    fields.x_velocity_fast = fast;
    fields.y_velocity_fract = 0x80;
    fields.y_velocity_fast = 0x00;
    fields.x_pos = enemy_x_pos;
    fields.y_pos = enemy_y_pos;

    Some(CreatedIndoorEnemy { slot, enemy_type: ENEMY_TYPE_GRENADE, hp: init.hp, fields })
}

/// Native port of `create_roller_with_segment_a` (`$9703`) - `x_pos`/
/// `y_pos` here are the roller's own *already offset* spawn position
/// (real ASM's `$09`/`$08`, set by the caller before this runs - see
/// [`create_roller`] for the one real case that computes them via
/// [`find_far_segment_for_x_pos`] directly from an un-offset position,
/// and this module's own doc comment for what `attributes_scratch`
/// really is).
pub fn create_roller_with_segment_a(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    segment: u8,
    x_pos: u8,
    y_pos: u8,
    attributes_scratch: u8,
) -> Option<CreatedIndoorEnemy> {
    if enemy_attack_flag == 0 {
        return None;
    }
    let slot = find_next_enemy_slot(enemy_routine)?;

    let init = initialize_enemy(prg_rom, ENEMY_TYPE_ROLLER, current_level);
    let mut fields = init.fields;
    fields.attributes = attributes_scratch;
    let (frac, fast) = ROLLER_VEL_CODE_TBL[segment as usize];
    fields.x_velocity_fract = frac;
    fields.x_velocity_fast = fast;
    fields.y_velocity_fract = 0x80;
    fields.y_velocity_fast = 0x00;
    fields.x_pos = x_pos;
    fields.y_pos = y_pos;

    Some(CreatedIndoorEnemy { slot, enemy_type: ENEMY_TYPE_ROLLER, hp: init.hp, fields })
}

/// Native port of `create_roller` (`$9700`) - computes the horizontal
/// segment from `x_pos` itself, then falls straight into [`create_roller_
/// with_segment_a`] (real ASM: `jsr find_far_segment_for_x_pos` then no
/// `rts` before the next label).
pub fn create_roller(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    x_pos: u8,
    y_pos: u8,
    attributes_scratch: u8,
) -> Option<CreatedIndoorEnemy> {
    let segment = find_far_segment_for_x_pos(x_pos);
    create_roller_with_segment_a(prg_rom, enemy_routine, current_level, enemy_attack_flag, segment, x_pos, y_pos, attributes_scratch)
}

/// What [`indoor_soldier_routine_01`] did on one call, once `ENEMY_ATTACK_
/// DELAY` had already been decremented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndoorSoldierAttack {
    /// `ENEMY_ATTACK_DELAY` hadn't reached 0 yet - nothing else ran.
    StillWaiting,
    /// Delay reset to `$10`, but `ENEMY_X_POS` (post-movement) was
    /// outside the `$68..$98` attack range.
    OutOfRange,
    /// Weapon type `0`.
    Bullet(Option<CreatedIndoorEnemy>),
    /// Weapon type `1`, but `ENEMY_VAR_1`'s new value was even - the
    /// "skip every other time" gate, no grenade fired.
    GrenadeSkipped { var_1: u8 },
    /// Weapon type `1`, `ENEMY_VAR_1`'s new value was odd - grenade
    /// fired (or slot allocation failed).
    Grenade { var_1: u8, grenade: Option<CreatedIndoorEnemy> },
    /// Weapon type `2` or `3` (both alike - see this module's doc
    /// comment).
    Roller(Option<CreatedIndoorEnemy>),
}

/// The full result of one [`indoor_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndoorSoldierRoutine01Result {
    pub sprite: InitSpriteFromFrameResult,
    pub velocity: ApplyEnemyVelocityResult,
    pub attack_delay: u8,
    pub attack: IndoorSoldierAttack,
}

/// Native port of `indoor_soldier_routine_01` (`$92d5`) - see this
/// module's doc comment for the weapon-type branch, the grenade parity
/// gate, and the stale-`$0a` (`attributes_scratch`) roller quirk.
#[allow(clippy::too_many_arguments)]
pub fn indoor_soldier_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    frame_counter: u8,
    enemy_frame: u8,
    enemy_sprite_attr: u8,
    x_vel_accum: u8,
    x_vel_fract: u8,
    x_vel_fast: u8,
    x_pos: u8,
    y_pos: u8,
    enemy_attack_delay: u8,
    enemy_attributes: u8,
    enemy_var_1: u8,
    enemy_attack_flag: u8,
    attributes_scratch: u8,
) -> IndoorSoldierRoutine01Result {
    let sprite = init_sprite_from_frame(frame_counter, enemy_frame, enemy_sprite_attr, x_vel_fast);
    let velocity = apply_enemy_velocity_set_bg_priority(x_vel_accum, x_vel_fract, x_vel_fast, x_pos, sprite.sprite_attr);

    let new_delay = enemy_attack_delay.wrapping_sub(1);
    if new_delay != 0 {
        return IndoorSoldierRoutine01Result { sprite, velocity, attack_delay: new_delay, attack: IndoorSoldierAttack::StillWaiting };
    }

    let attack_delay = 0x10;
    let updated_x_pos = velocity.x_pos;
    if updated_x_pos < 0x68 || updated_x_pos >= 0x98 {
        return IndoorSoldierRoutine01Result { sprite, velocity, attack_delay, attack: IndoorSoldierAttack::OutOfRange };
    }

    let weapon_type = (enemy_attributes >> 1) & 0x03;
    let attack = if weapon_type == 0 {
        IndoorSoldierAttack::Bullet(create_indoor_bullet(prg_rom, enemy_routine, current_level, enemy_attack_flag, updated_x_pos, y_pos))
    } else if weapon_type == 1 {
        let var_1 = enemy_var_1.wrapping_add(1);
        if var_1 & 1 == 0 {
            IndoorSoldierAttack::GrenadeSkipped { var_1 }
        } else {
            let grenade = enemy_launch_grenade(prg_rom, enemy_routine, current_level, enemy_attack_flag, updated_x_pos, y_pos);
            IndoorSoldierAttack::Grenade { var_1, grenade }
        }
    } else {
        let (roller_x, roller_y) = add_with_enemy_pos(0x00, 0x08, updated_x_pos, y_pos);
        let roller = create_roller(prg_rom, enemy_routine, current_level, enemy_attack_flag, roller_x, roller_y, attributes_scratch);
        IndoorSoldierAttack::Roller(roller)
    };

    IndoorSoldierRoutine01Result { sprite, velocity, attack_delay, attack }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_pos_and_vel_from_the_right_uses_the_table_velocity_directly() {
        let r = init_indoor_enemy_pos_and_vel(0, 0x00);
        assert_eq!(r.x_velocity, (0x20, 0xFF));
        assert_eq!(r.x_pos, 0xA8);
        assert_eq!(r.y_pos, 0x6D);
    }

    #[test]
    fn init_pos_and_vel_from_the_left_reverses_the_table_velocity() {
        let r = init_indoor_enemy_pos_and_vel(0, 0x01);
        assert_eq!(r.x_velocity, reverse_enemy_x_direction(0x20, 0xFF));
        assert_eq!(r.x_pos, 0x58);
    }

    #[test]
    fn init_pos_and_vel_uses_the_right_table_row_per_enemy_type() {
        let r = init_indoor_enemy_pos_and_vel(1, 0x00); // jumping soldier
        assert_eq!(r.x_velocity, (0x40, 0xFF));
    }

    #[test]
    fn apply_velocity_removes_past_the_right_limit_moving_right() {
        let r = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0xAF, 0x00);
        assert_eq!(r.x_pos, 0xB0);
        assert_eq!(r.outcome, ApplyEnemyVelocityOutcome::Removed(remove_enemy()));
    }

    #[test]
    fn apply_velocity_removes_past_the_left_limit_moving_left() {
        let r = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0xFF, 0x50, 0x00);
        assert_eq!(r.x_pos, 0x4F);
        assert_eq!(r.outcome, ApplyEnemyVelocityOutcome::Removed(remove_enemy()));
    }

    #[test]
    fn apply_velocity_sets_bg_priority_near_either_edge_and_clears_it_mid_screen() {
        let left_edge = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0x5E, 0x00);
        assert_eq!(left_edge.outcome, ApplyEnemyVelocityOutcome::BgPriority(0x20));
        let right_edge = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0x9F, 0x00);
        assert_eq!(right_edge.outcome, ApplyEnemyVelocityOutcome::BgPriority(0x20));
        let middle = apply_enemy_velocity_set_bg_priority(0x00, 0x00, 0x01, 0x7F, 0b0010_0000);
        assert_eq!(middle.outcome, ApplyEnemyVelocityOutcome::BgPriority(0x00));
    }

    #[test]
    fn init_sprite_from_frame_only_advances_on_the_4th_frame_and_wraps_at_3() {
        assert_eq!(init_sprite_from_frame(0x01, 0x01, 0x00, 0x00).enemy_frame, 0x01);
        assert_eq!(init_sprite_from_frame(0x04, 0x01, 0x00, 0x00).enemy_frame, 0x02);
        assert_eq!(init_sprite_from_frame(0x04, 0x02, 0x00, 0x00).enemy_frame, 0x00);
    }

    #[test]
    fn init_sprite_from_frame_sets_sprite_and_flip_bit_from_direction() {
        let right = init_sprite_from_frame(0x01, 0x01, 0b0100_0000, 0x01);
        assert_eq!(right.sprites, 0x94);
        assert_eq!(right.sprite_attr, 0x00);
        let left = init_sprite_from_frame(0x01, 0x01, 0x00, 0xFF);
        assert_eq!(left.sprite_attr, 0x40);
    }

    #[test]
    fn routine_00_composes_init_and_advances() {
        let r = indoor_soldier_routine_00(0x00, 5);
        assert_eq!(r.init, init_indoor_enemy_pos_and_vel(0, 0x00));
        assert_eq!(r.attack_delay, 0x08);
        assert_eq!(r.routine_update, advance_enemy_routine(5));
    }

    /// Shared property table (bullets, `ENEMY_TYPE=1 < $10`) plus level 0's
    /// per-level table with real-shaped records for rollers (`$11`) and
    /// grenades (`$12`) - same shape as `red_blue_soldier`'s own synthetic
    /// ROM fixture, needed since roller/grenade are both `>= $10`.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);

        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let bullet_record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 1 * 4;
        rom[bullet_record_off..bullet_record_off + 4].copy_from_slice(&[0x80, 0x00, 0x07, 0x00]);

        let level0_table_addr: u16 = 0xF000;
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level0_table_addr.to_le_bytes());
        let level0_off = 7 * 0x4000 + (level0_table_addr as usize - 0xC000);
        rom[level0_off + ENEMY_TYPE_ROLLER as usize * 4..level0_off + ENEMY_TYPE_ROLLER as usize * 4 + 4]
            .copy_from_slice(&[0x10, 0x20, 0x0A, 0x30]);
        rom[level0_off + ENEMY_TYPE_GRENADE as usize * 4..level0_off + ENEMY_TYPE_GRENADE as usize * 4 + 4]
            .copy_from_slice(&[0x11, 0x21, 0x0B, 0x31]);

        rom
    }

    #[test]
    fn create_indoor_bullet_rejects_x_positions_outside_the_indoor_screen() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        assert_eq!(create_indoor_bullet(&rom, &routine, 0, 1, 0xA0, 0x6D), None); // >= $a0
        assert_eq!(create_indoor_bullet(&rom, &routine, 0, 1, 0x5F, 0x6D), None); // < $60
    }

    #[test]
    fn create_indoor_bullet_respects_the_attack_flag_and_slot_availability() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        assert_eq!(create_indoor_bullet(&rom, &routine, 0, 0, 0x80, 0x6D), None); // attack flag off
        let full = [1u8; ENEMY_SLOT_COUNT];
        assert_eq!(create_indoor_bullet(&rom, &full, 0, 1, 0x80, 0x6D), None); // no free slot
    }

    #[test]
    fn create_indoor_bullet_success_sets_var_1_and_velocity_from_segment() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = create_indoor_bullet(&rom, &routine, 0, 1, 0x80, 0x6D).unwrap();
        assert_eq!(created.enemy_type, ENEMY_TYPE_BULLET);
        assert_eq!(created.hp, 0x07); // from the shared synthetic record
        assert_eq!(created.fields.var_1, 0x03);
        assert_eq!(created.fields.x_pos, 0x80);
        assert_eq!(created.fields.y_pos, 0x6D);
        assert_eq!(created.fields.y_velocity_fract, 0x40);
        assert_eq!(created.fields.y_velocity_fast, 0x01);
        let segment = find_far_segment_for_x_pos(0x80);
        assert_eq!((created.fields.x_velocity_fract, created.fields.x_velocity_fast), INDOOR_BULLET_VELOCITY_TBL[segment as usize]);
    }

    #[test]
    fn enemy_launch_grenade_has_no_range_check_only_the_attack_flag_gate() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        assert_eq!(enemy_launch_grenade(&rom, &routine, 0, 0, 0x00, 0x6D), None); // attack flag off
        // extreme X position, well outside create_indoor_bullet's own range - still fires.
        let created = enemy_launch_grenade(&rom, &routine, 0, 1, 0x00, 0x6D).unwrap();
        assert_eq!(created.enemy_type, ENEMY_TYPE_GRENADE);
        assert_eq!(created.hp, 0x0B);
        assert_eq!(created.fields.y_velocity_fract, 0x80);
        assert_eq!(created.fields.y_velocity_fast, 0x00);
    }

    #[test]
    fn create_roller_computes_its_own_segment_and_stamps_the_scratch_attributes() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = create_roller(&rom, &routine, 0, 1, 0x77, 0x75, 0xAB).unwrap();
        assert_eq!(created.enemy_type, ENEMY_TYPE_ROLLER);
        assert_eq!(created.hp, 0x0A);
        assert_eq!(created.fields.attributes, 0xAB); // the stale-$0a quirk, faithfully threaded through
        assert_eq!(created.fields.x_pos, 0x77);
        assert_eq!(created.fields.y_pos, 0x75);
        assert_eq!(created.fields.y_velocity_fract, 0x80);
        let segment = find_far_segment_for_x_pos(0x77);
        assert_eq!((created.fields.x_velocity_fract, created.fields.x_velocity_fast), ROLLER_VEL_CODE_TBL[segment as usize]);
        // create_roller_with_segment_a with the same pre-computed segment matches exactly.
        let via_segment_a = create_roller_with_segment_a(&rom, &routine, 0, 1, segment, 0x77, 0x75, 0xAB).unwrap();
        assert_eq!(created, via_segment_a);
    }

    #[test]
    fn routine_01_still_waits_while_attack_delay_has_not_elapsed() {
        let r = indoor_soldier_routine_01(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 0, 0, 0, 0, 0, 0x01, 0x80, 0x6D, 0x05, 0x00, 0x00, 1, 0x00);
        assert_eq!(r.attack_delay, 0x04);
        assert_eq!(r.attack, IndoorSoldierAttack::StillWaiting);
    }

    #[test]
    fn routine_01_resets_delay_but_stays_idle_when_x_is_out_of_attack_range() {
        // x_pos=0x50, x_vel_fast=0 -> updated x stays 0x50, outside $68..$98.
        let r = indoor_soldier_routine_01(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 0, 0, 0, 0, 0, 0x00, 0x50, 0x6D, 0x01, 0x00, 0x00, 1, 0x00);
        assert_eq!(r.attack_delay, 0x10);
        assert_eq!(r.attack, IndoorSoldierAttack::OutOfRange);
    }

    #[test]
    fn routine_01_weapon_type_0_fires_a_bullet() {
        // x_pos=0x80 (in range), x_vel_fast=0 -> stays 0x80. attributes=0x00 -> weapon_type=0.
        let r = indoor_soldier_routine_01(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 0, 0, 0, 0, 0, 0x00, 0x80, 0x6D, 0x01, 0x00, 0x00, 1, 0x00);
        match r.attack {
            IndoorSoldierAttack::Bullet(Some(b)) => assert_eq!(b.enemy_type, ENEMY_TYPE_BULLET),
            other => panic!("expected Bullet(Some(_)), got {other:?}"),
        }
    }

    #[test]
    fn routine_01_weapon_type_1_grenade_only_fires_every_other_call() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // attributes: bits 1-2 = 01 -> weapon_type=1. var_1 starts at 0 -> becomes 1 (odd) -> fires.
        let r1 = indoor_soldier_routine_01(&rom, &routine, 0, 0, 0, 0, 0, 0, 0x00, 0x80, 0x6D, 0x01, 0b0000_0010, 0x00, 1, 0x00);
        match r1.attack {
            IndoorSoldierAttack::Grenade { var_1: 1, grenade: Some(g) } => assert_eq!(g.enemy_type, ENEMY_TYPE_GRENADE),
            other => panic!("expected Grenade{{var_1:1, grenade:Some(_)}}, got {other:?}"),
        }
        // var_1 starts at 1 -> becomes 2 (even) -> skipped.
        let r2 = indoor_soldier_routine_01(&rom, &routine, 0, 0, 0, 0, 0, 0, 0x00, 0x80, 0x6D, 0x01, 0b0000_0010, 0x01, 1, 0x00);
        assert_eq!(r2.attack, IndoorSoldierAttack::GrenadeSkipped { var_1: 2 });
    }

    #[test]
    fn routine_01_weapon_types_2_and_3_both_create_a_roller() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // bits 1-2 = 10 -> weapon_type=2.
        let type_2 = indoor_soldier_routine_01(&rom, &routine, 0, 0, 0, 0, 0, 0, 0x00, 0x80, 0x6D, 0x01, 0b0000_0100, 0x00, 1, 0xCC);
        // bits 1-2 = 11 -> weapon_type=3, same roller path per this module's own doc comment.
        let type_3 = indoor_soldier_routine_01(&rom, &routine, 0, 0, 0, 0, 0, 0, 0x00, 0x80, 0x6D, 0x01, 0b0000_0110, 0x00, 1, 0xCC);
        match (type_2.attack, type_3.attack) {
            (IndoorSoldierAttack::Roller(Some(a)), IndoorSoldierAttack::Roller(Some(b))) => {
                assert_eq!(a.enemy_type, ENEMY_TYPE_ROLLER);
                assert_eq!(a, b); // identical spawn position/velocity/attributes either way
            }
            other => panic!("expected both Roller(Some(_)), got {other:?}"),
        }
        // roller spawn position is offset +8 Y / +0 X from the (post-movement) enemy position.
        if let IndoorSoldierAttack::Roller(Some(r)) = type_2.attack {
            assert_eq!(r.fields.x_pos, 0x80);
            assert_eq!(r.fields.y_pos, 0x6D + 0x08);
            assert_eq!(r.fields.attributes, 0xCC);
        }
    }
}
