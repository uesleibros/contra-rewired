//! Native port of the red/blue soldier enemy family's shared spawn-init
//! state, `red_blue_soldier_routine_00` (`src/bank0.asm`, CPU `$a157`-
//! `$a17d`) - entry 0 of both `blue_soldier_routine_ptr_tbl` and `red_
//! soldier_routine_ptr_tbl` (a distinct enemy type from the plain
//! soldier in [`crate::enemy::soldier`], though both families share the
//! generic explosion/removal routines, [`crate::enemy::enemy_explosion`]).
//! Places the enemy at one of 4 fixed screen corners and gives it an
//! initial horizontal running velocity, both picked from `ENEMY_
//! ATTRIBUTES`, then advances to the next routine.
//!
//! Also carries the blue soldier's own 3 routines beyond that shared
//! init state ([`blue_soldier_routine_01`]/[`_02`]/[`_03`]: run across
//! the screen, jump-attack windup, then fall), the 2 small helpers both
//! blue *and* red soldiers share ([`red_blue_soldier_set_run_frame`]/
//! [`red_blue_soldier_set_bg_priority`]), the red soldier's own routines
//! ([`red_soldier_routine_01`]/[`_02`], reusing those same two helpers),
//! and the level 4 boss-screen generator that spawns both
//! ([`red_blue_soldier_gen_routine_00`]/[`_01`]).

use crate::enemy::create_enemy_bullet::{aim_and_create_enemy_bullet, CreatedBullet};
use crate::enemy::enemy_collision_flags::enable_enemy_collision;
use crate::enemy::enemy_position_utils::add_10_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{
    advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate,
};
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::{initialize_enemy, InitializedEnemy};
use crate::enemy::player_enemy_distance::player_enemy_x_dist;
use crate::enemy::update_enemy_pos::{remove_enemy, update_enemy_pos, RemovedEnemy, UpdatedEnemyPos};

/// `red_blue_soldier_init_pos_tbl` (`$a17e`, 8 bytes) - `(y_pos, x_pos)`
/// per spawn corner, indexed by `ENEMY_ATTRIBUTES` (real ASM doesn't mask
/// this before indexing - every real spawn placement for this enemy type
/// uses attributes `0`-`3` exactly, so this port masks defensively
/// rather than replicating an out-of-range table read no real caller can
/// trigger, the same reasoning `soldier::soldier_set_x_velocity` already
/// documents for an analogous case).
const RED_BLUE_SOLDIER_INIT_POS_TBL: [(u8, u8); 4] = [
    (0x9C, 0xF0), // lower right - negative (leftward) X velocity
    (0x9C, 0x10), // lower left - positive (rightward) X velocity
    (0x61, 0xF0), // upper right
    (0x61, 0x10), // upper left
];

/// `red_blue_soldier_init_vel_tbl` (`$a186`, 4 bytes) - `(x_vel_fract,
/// x_vel_fast)`, indexed by `ENEMY_ATTRIBUTES & 1` (real ASM does mask
/// this one explicitly).
const RED_BLUE_SOLDIER_INIT_VEL_TBL: [(u8, u8); 2] = [
    (0x00, 0xFF), // running from the left side of the screen
    (0x00, 0x01), // running from the right side
];

/// The full result of one [`red_blue_soldier_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedBlueSoldierRoutine00Result {
    pub y_pos: u8,
    pub x_pos: u8,
    pub x_velocity: (u8, u8),
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `red_blue_soldier_routine_00` (`$a157`).
pub fn red_blue_soldier_routine_00(enemy_attributes: u8, current_routine: u8) -> RedBlueSoldierRoutine00Result {
    let (y_pos, x_pos) = RED_BLUE_SOLDIER_INIT_POS_TBL[(enemy_attributes & 0x03) as usize];
    let x_velocity = RED_BLUE_SOLDIER_INIT_VEL_TBL[(enemy_attributes & 0x01) as usize];
    let routine_update = advance_enemy_routine(current_routine);
    RedBlueSoldierRoutine00Result { y_pos, x_pos, x_velocity, routine_update }
}

/// Native port of `red_blue_soldier_set_run_frame` (`$a1c5`) - cycles
/// `ENEMY_FRAME` through `0..3` once every 4th frame (`FRAME_COUNTER &
/// 3 == 0`), used by both blue and red soldiers' own "running" states
/// for their run-cycle animation.
pub fn red_blue_soldier_set_run_frame(frame_counter: u8, enemy_frame: u8) -> u8 {
    if frame_counter & 0x03 != 0 {
        return enemy_frame;
    }
    let new_frame = enemy_frame.wrapping_add(1);
    if new_frame < 0x03 {
        new_frame
    } else {
        0x00
    }
}

/// Native port of `red_blue_soldier_set_bg_priority` (`$a1db`) - forces
/// background priority (sprite drawn *behind* background tiles) whenever
/// the soldier is near either screen edge, where it'd otherwise draw
/// over the level's pillar/wall decorations it's meant to be behind;
/// clear in the middle of the screen. Real ASM's own branch comments are
/// misleading here (mislabeled "not behind pillar" on the branch that's
/// actually taken for the *behind-pillar* edges) - this port follows the
/// real control flow, not the comment text.
pub fn red_blue_soldier_set_bg_priority(enemy_x_pos: u8, enemy_sprite_attr: u8) -> u8 {
    let behind_pillar_flag = if enemy_x_pos >= 0xDC || enemy_x_pos < 0x24 { 0x20 } else { 0x00 };
    (enemy_sprite_attr & 0xDF) | behind_pillar_flag
}

/// The real, branchy result of one [`blue_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueSoldierRoutine01Outcome {
    /// Still outside the attack-trigger X range, or not yet close enough
    /// to a player within it.
    StillRunning,
    /// Close enough to a player: `ENEMY_FRAME` resets to `0`, advances to
    /// `blue_soldier_routine_02` after 1 frame.
    CloseToPlayer(DelayedRoutineUpdate),
}

/// The full result of one [`blue_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueSoldierRoutine01Result {
    /// Final `ENEMY_FRAME` - the freshly cycled run-frame normally, or
    /// `0` if [`BlueSoldierRoutine01Outcome::CloseToPlayer`] overrides it
    /// (a real, later `sta` that runs *after* `ENEMY_SPRITES` was already
    /// computed from the pre-override frame - see `sprites` below).
    pub enemy_frame: u8,
    /// `ENEMY_SPRITES` - always computed from the *pre-override* run
    /// frame (`+ $85`), even on the `CloseToPlayer` path.
    pub sprites: u8,
    pub sprite_attr: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: BlueSoldierRoutine01Outcome,
}

/// Native port of `blue_soldier_routine_01` (`$a18a`) - "run across
/// screen, once past trigger point, see if close to player, if so
/// advance routine to jump down": cycles the run animation, updates
/// position/scroll, and once inside the real `[$28, $d8)` X range,
/// checks real proximity to a player before deciding to start the
/// jump-attack.
#[allow(clippy::too_many_arguments)]
pub fn blue_soldier_routine_01(
    enemy_frame: u8,
    frame_counter: u8,
    enemy_attributes: u8,
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
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    current_routine: u8,
) -> BlueSoldierRoutine01Result {
    let new_frame = red_blue_soldier_set_run_frame(frame_counter, enemy_frame);
    let sprites = new_frame.wrapping_add(0x85);
    let sprite_attr_base = if enemy_attributes & 0x01 == 0 { 0x47 } else { 0x07 };
    let sprite_attr = red_blue_soldier_set_bg_priority(enemy_x_pos, sprite_attr_base);

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

    let updated_x_pos = position.x.pos;
    let outcome = if updated_x_pos >= 0xD8 || updated_x_pos < 0x28 {
        BlueSoldierRoutine01Outcome::StillRunning
    } else {
        let closest = player_enemy_x_dist(sprite_x_pos, updated_x_pos, player_state);
        if closest.distance >= 0x10 {
            BlueSoldierRoutine01Outcome::StillRunning
        } else {
            BlueSoldierRoutine01Outcome::CloseToPlayer(set_enemy_delay_adv_routine(0x01, current_routine))
        }
    };

    let enemy_frame = match outcome {
        BlueSoldierRoutine01Outcome::CloseToPlayer(_) => 0x00,
        BlueSoldierRoutine01Outcome::StillRunning => new_frame,
    };

    BlueSoldierRoutine01Result { enemy_frame, sprites, sprite_attr, position, outcome }
}

/// `blue_soldier_jmp_x_vel_tbl` (`$a241`, 4 bytes) - `(x_vel_fract,
/// x_vel_fast)` per direction, indexed by `ENEMY_ATTRIBUTES & 1`.
const BLUE_SOLDIER_JMP_X_VEL_TBL: [(u8, u8); 2] = [
    (0xC0, 0xFF), // coming from the left
    (0x40, 0x00), // coming from the right
];

/// The real, branchy result of one [`blue_soldier_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlueSoldierRoutine02Outcome {
    /// Attack windup delay hadn't elapsed.
    Waiting { animation_delay: u8 },
    /// Delay elapsed, windup animation isn't done: sprite advances,
    /// delay resets to `$08`.
    Animating { enemy_frame: u8, sprites: u8, animation_delay: u8 },
    /// Windup finished: forces foreground draw priority, enables
    /// collision, sets the jump velocity from `ENEMY_ATTRIBUTES`'
    /// direction bit, and advances to `blue_soldier_routine_03` after
    /// `$10` frames.
    JumpStart {
        sprites: u8,
        sprite_attr: u8,
        state_width: u8,
        x_velocity: (u8, u8),
        y_velocity: (u8, u8),
        delayed_routine: DelayedRoutineUpdate,
    },
}

/// Native port of `blue_soldier_routine_02` (`$a1f7`) - "go through jump
/// animation routine, then initialize jump velocities and advance
/// routine".
pub fn blue_soldier_routine_02(
    enemy_animation_delay: u8,
    enemy_frame: u8,
    enemy_attributes: u8,
    enemy_sprite_attr: u8,
    enemy_state_width: u8,
    current_routine: u8,
) -> BlueSoldierRoutine02Outcome {
    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return BlueSoldierRoutine02Outcome::Waiting { animation_delay: delay };
    }

    let sprites = enemy_frame.wrapping_add(0x88);
    let new_frame = enemy_frame.wrapping_add(1);
    if new_frame < 0x03 {
        return BlueSoldierRoutine02Outcome::Animating { enemy_frame: new_frame, sprites, animation_delay: 0x08 };
    }

    let sprite_attr = enemy_sprite_attr & 0xDF;
    let state_width = enable_enemy_collision(enemy_state_width);
    let x_velocity = BLUE_SOLDIER_JMP_X_VEL_TBL[(enemy_attributes & 0x01) as usize];
    let y_velocity = (0x00, 0xFF);
    let delayed_routine = set_enemy_delay_adv_routine(0x10, current_routine);
    BlueSoldierRoutine02Outcome::JumpStart { sprites, sprite_attr, state_width, x_velocity, y_velocity, delayed_routine }
}

/// The full result of one [`blue_soldier_routine_03`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlueSoldierRoutine03Result {
    pub sprites: u8,
    /// `0` if `ENEMY_ANIMATION_DELAY` was already `0` (real ASM doesn't
    /// decrement past zero here - it just keeps using `sprite_8b` and
    /// leaves the counter alone); otherwise decremented by one.
    pub animation_delay: u8,
    pub y_velocity: (u8, u8),
    pub position: UpdatedEnemyPos,
}

/// Native port of `blue_soldier_routine_03` (`$a245`) - "animate jumping
/// down frames based on time since jump, apply velocity": falls under
/// gravity, showing one of two sprites depending on whether the windup
/// delay (reused here as a brief falling-animation timer) has run out.
#[allow(clippy::too_many_arguments)]
pub fn blue_soldier_routine_03(
    enemy_animation_delay: u8,
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
) -> BlueSoldierRoutine03Result {
    let (sprites, animation_delay) =
        if enemy_animation_delay == 0 { (0x8B, 0x00) } else { (0x8A, enemy_animation_delay.wrapping_sub(1)) };
    let y_velocity = add_10_to_enemy_y_fract_vel(y_vel_fract, y_vel_fast);
    let position = update_enemy_pos(
        level_scrolling_type,
        frame_scroll,
        enemy_x_pos,
        x_vel_accum,
        x_vel_fract,
        x_vel_fast,
        enemy_y_pos,
        y_vel_accum,
        y_velocity.0,
        y_velocity.1,
    );
    BlueSoldierRoutine03Result { sprites, animation_delay, y_velocity, position }
}

/// The real, branchy result of one [`red_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedSoldierRoutine01Outcome {
    /// `ENEMY_VAR_2` already nonzero - already fired once, just keeps
    /// running off screen, no further checks this call.
    AlreadyFired,
    /// Outside the trigger X range, or too far from every player once
    /// inside it.
    StillRunning,
    /// Close enough to a player: commits to firing, advances to `red_
    /// soldier_routine_02`.
    Attack { var_1: u8, attack_delay: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`red_soldier_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedSoldierRoutine01Result {
    pub enemy_frame: u8,
    /// `ENEMY_SPRITES` - `$8f` (facing the player) on the `Attack`
    /// outcome, overriding the run-animation sprite that was already
    /// computed and stored earlier in the same call; the plain run
    /// sprite (`enemy_frame + $8c`) on every other outcome.
    pub sprites: u8,
    pub sprite_attr: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: RedSoldierRoutine01Outcome,
}

/// Native port of `red_soldier_routine_01` (`$a266`) - "run across
/// screen, once past trigger point, see if close to player, if so
/// advance routine to fire at player; if already fired from `red_
/// soldier_routine_02`, just continue running off screen". The real
/// minimum attack distance is itself picked from `ENEMY_ATTRIBUTES` bit
/// 1 (`$10` or `$30`), not a single fixed value.
#[allow(clippy::too_many_arguments)]
pub fn red_soldier_routine_01(
    enemy_frame: u8,
    frame_counter: u8,
    enemy_attributes: u8,
    enemy_var_2: u8,
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
    sprite_x_pos: [u8; 2],
    player_state: [u8; 2],
    current_routine: u8,
) -> RedSoldierRoutine01Result {
    let new_frame = red_blue_soldier_set_run_frame(frame_counter, enemy_frame);
    let run_sprite = new_frame.wrapping_add(0x8C);
    let sprite_attr_base = if enemy_attributes & 0x01 == 0 { 0x46 } else { 0x06 };
    let sprite_attr = red_blue_soldier_set_bg_priority(enemy_x_pos, sprite_attr_base);

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

    let outcome = if enemy_var_2 != 0 {
        RedSoldierRoutine01Outcome::AlreadyFired
    } else {
        let updated_x = position.x.pos;
        if updated_x >= 0xD8 || updated_x < 0x28 {
            RedSoldierRoutine01Outcome::StillRunning
        } else {
            let min_dist = if enemy_attributes & 0x02 == 0 { 0x10 } else { 0x30 };
            let closest = player_enemy_x_dist(sprite_x_pos, updated_x, player_state);
            if closest.distance >= min_dist {
                RedSoldierRoutine01Outcome::StillRunning
            } else {
                let routine_update = advance_enemy_routine(current_routine);
                RedSoldierRoutine01Outcome::Attack { var_1: 0x03, attack_delay: 0x10, routine_update }
            }
        }
    };

    let sprites = match outcome {
        RedSoldierRoutine01Outcome::Attack { .. } => 0x8F,
        _ => run_sprite,
    };

    RedSoldierRoutine01Result { enemy_frame: new_frame, sprites, sprite_attr, position, outcome }
}

/// The result of one [`red_soldier_routine_02`] call's `Fired` outcome -
/// a bullet spawn attempt via the already-verified [`aim_and_create_
/// enemy_bullet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedSoldierRoutine02FiredResult {
    pub sprites: u8,
    pub var_1: u8,
    pub attack_delay: u8,
    pub sprite_attr: u8,
    pub bullet: Option<CreatedBullet>,
}

/// The real, branchy result of one [`red_soldier_routine_02`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedSoldierRoutine02Outcome {
    /// Attack delay hadn't elapsed. `sprite_attr` is `Some` only on the
    /// real, specific case the delay lands on exactly `$2c` (a fixed
    /// point partway through the recoil animation) - strips the recoil
    /// sprite-attribute bit.
    Waiting { attack_delay: u8, sprite_attr: Option<u8> },
    /// Delay elapsed and bullets remain: fires one.
    Fired(RedSoldierRoutine02FiredResult),
    /// Delay elapsed and `ENEMY_VAR_1` just went negative (all bullets
    /// fired): marks the soldier as having fired and returns to `red_
    /// soldier_routine_01` to keep running off screen.
    AllFired { var_2: u8, routine_update: EnemyRoutineUpdate },
}

/// Native port of `red_soldier_routine_02` (`$a2bb`) - "fire `ENEMY_
/// VAR_1` times and then go back to `red_soldier_routine_01`".
#[allow(clippy::too_many_arguments)]
pub fn red_soldier_routine_02(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    enemy_attack_delay: u8,
    enemy_var_1: u8,
    enemy_var_2: u8,
    enemy_sprite_attr: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
    current_routine: u8,
) -> RedSoldierRoutine02Outcome {
    let delay = enemy_attack_delay.wrapping_sub(1);
    if delay != 0 {
        let sprite_attr = if delay == 0x2C { Some(enemy_sprite_attr & 0xF7) } else { None };
        return RedSoldierRoutine02Outcome::Waiting { attack_delay: delay, sprite_attr };
    }

    let sprites = 0x90;
    let var_1 = enemy_var_1.wrapping_sub(1);
    if (var_1 as i8) < 0 {
        let var_2 = enemy_var_2.wrapping_add(1);
        let routine_update = set_enemy_routine_to_a(current_routine, 0x02);
        return RedSoldierRoutine02Outcome::AllFired { var_2, routine_update };
    }

    let attack_delay = 0x30;
    let sprite_attr = enemy_sprite_attr | 0x08;
    let closest = player_enemy_x_dist(sprite_x_pos, enemy_x_pos, player_state);
    let bullet = aim_and_create_enemy_bullet(
        prg_rom,
        enemy_routine,
        current_level,
        enemy_attack_flag,
        0x00,
        0x04,
        enemy_y_pos,
        enemy_x_pos,
        closest.player_index,
        0,
        0,
        player_state,
        sprite_y_pos,
        sprite_x_pos,
        level_location_type,
    );
    RedSoldierRoutine02Outcome::Fired(RedSoldierRoutine02FiredResult { sprites, var_1, attack_delay, sprite_attr, bullet })
}

/// Native port of `red_blue_soldier_gen_routine_00` (`$a304`) - sets the
/// initial generation delay (`$80`) and advances.
pub fn red_blue_soldier_gen_routine_00(current_routine: u8) -> DelayedRoutineUpdate {
    set_enemy_delay_adv_routine(0x80, current_routine)
}

/// `red_blue_soldier_data_tbl` (`$a368`, 28 bytes) - a real, hand-authored
/// spawn script the level 4 boss-screen generator reads through
/// repeatedly (wrapping via the `$ff` terminator, not restarting the
/// enemy routine): positive bytes spawn a soldier (bits 0-1 = spawn
/// corner/`ENEMY_ATTRIBUTES`, bit 2 = red/blue - **not** "bits 0-2" and
/// "bit 3" as the real ASM's own comments claim; this port follows the
/// literal `and #$03`/`lsr;lsr` instructions, which the values below
/// confirm: `$00`-`$03` are commented "red" and `$04`-`$07` "blue",
/// exactly matching `byte & 3` for the corner and `byte >> 2` for the
/// color), negative bytes set the next delay (`byte << 1`, real ASM's
/// own `asl`) and stop the read for this call.
const RED_BLUE_SOLDIER_DATA_TBL: [u8; 28] = [
    0x00, 0x01, 0x02, 0x03, 0xD0, // red soldier x4, $a0 delay
    0x06, 0x07, 0xA0, // blue soldier x2, $40 delay
    0x04, 0x05, 0xC0, // blue soldier x2, $80 delay
    0x00, 0x01, 0xB0, // red soldier x2, $60 delay
    0x02, 0x03, 0xD0, // red soldier x2, $a0 delay
    0x04, 0x05, 0x06, 0x07, 0xD0, // blue soldier x4, $a0 delay
    0x00, 0x01, 0x02, 0x03, 0xFE, // red soldier x4, $fc delay
    0xFF, // wrap to the start
];

/// One soldier spawned by [`red_blue_soldier_gen_routine_01`] - the enemy
/// type/`ENEMY_ATTRIBUTES` are set directly on top of what [`initialize_
/// enemy`] already wrote for the freshly-claimed slot (real ASM: `sta
/// ENEMY_TYPE,x` happens *before* `jsr initialize_enemy`, so the
/// property lookup inside it already sees the right type; `ENEMY_
/// ATTRIBUTES,x` is set *after*, since `initialize_enemy` doesn't touch
/// it at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RedBlueSoldierSpawn {
    pub slot: u8,
    pub enemy_type: u8,
    pub attributes: u8,
    pub initialized: InitializedEnemy,
}

/// The real, branchy result of one [`red_blue_soldier_gen_routine_01`]
/// call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedBlueSoldierGenRoutine01Outcome {
    /// All 3 level 4 boss-screen wall platings destroyed: stops
    /// generating.
    Removed(RemovedEnemy),
    /// Odd `FRAME_COUNTER` - real ASM exits before even touching `ENEMY_
    /// ANIMATION_DELAY` this call.
    OddFrame,
    /// Even frame, but the (now-decremented) delay hasn't reached zero.
    StillWaiting { animation_delay: u8 },
    /// Delay reached zero: read through the spawn script, creating zero
    /// or more soldiers (bounded by how many positive bytes precede the
    /// next negative/delay byte, real data never more than 4), then set
    /// the new delay from that byte.
    Spawned { var_1: u8, animation_delay: u8 },
}

/// The full result of one [`red_blue_soldier_gen_routine_01`] call - see
/// [`RedBlueSoldierGenRoutine01Outcome::Spawned`] for when `spawns` is
/// nonempty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedBlueSoldierGenRoutine01Result {
    pub spawns: Vec<RedBlueSoldierSpawn>,
    pub outcome: RedBlueSoldierGenRoutine01Outcome,
}

/// Native port of `red_blue_soldier_gen_routine_01` (`$a309`) - the level
/// 4 boss-screen red/blue soldier generator's main loop.
pub fn red_blue_soldier_gen_routine_01(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    wall_plating_destroyed_count: u8,
    frame_counter: u8,
    enemy_animation_delay: u8,
    enemy_var_1: u8,
) -> RedBlueSoldierGenRoutine01Result {
    if wall_plating_destroyed_count >= 0x03 {
        return RedBlueSoldierGenRoutine01Result { spawns: Vec::new(), outcome: RedBlueSoldierGenRoutine01Outcome::Removed(remove_enemy()) };
    }
    if frame_counter & 0x01 != 0 {
        return RedBlueSoldierGenRoutine01Result { spawns: Vec::new(), outcome: RedBlueSoldierGenRoutine01Outcome::OddFrame };
    }
    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return RedBlueSoldierGenRoutine01Result {
            spawns: Vec::new(),
            outcome: RedBlueSoldierGenRoutine01Outcome::StillWaiting { animation_delay: delay },
        };
    }

    let mut live_routine = *enemy_routine;
    let mut spawns = Vec::new();
    let mut read_offset = enemy_var_1;
    let mut new_delay = 0u8;

    // Generous safety cap - real data always terminates within a
    // handful of bytes; this only guards against pathological/corrupted
    // ENEMY_VAR_1 input, matching this crate's other bounded-loop ports.
    for _ in 0..64 {
        let byte = RED_BLUE_SOLDIER_DATA_TBL[read_offset as usize];
        read_offset = read_offset.wrapping_add(1);

        if byte == 0xFF {
            read_offset = 0;
            continue;
        }
        if (byte as i8) < 0 {
            new_delay = byte.wrapping_shl(1);
            break;
        }

        let attributes = byte & 0x03;
        let is_blue = (byte >> 2) & 0x01 != 0;
        if let Some(slot) = find_next_enemy_slot(&live_routine) {
            let enemy_type = if is_blue { 0x1E } else { 0x1F };
            let initialized = initialize_enemy(prg_rom, enemy_type, current_level);
            live_routine[slot as usize] = initialized.routine;
            spawns.push(RedBlueSoldierSpawn { slot, enemy_type, attributes, initialized });
        }
    }

    RedBlueSoldierGenRoutine01Result {
        spawns,
        outcome: RedBlueSoldierGenRoutine01Outcome::Spawned { var_1: read_offset, animation_delay: new_delay },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_attribute_value_selects_the_right_corner_and_direction() {
        let cases = [
            (0x00u8, (0x9C, 0xF0), (0x00, 0xFF)),
            (0x01, (0x9C, 0x10), (0x00, 0x01)),
            (0x02, (0x61, 0xF0), (0x00, 0xFF)),
            (0x03, (0x61, 0x10), (0x00, 0x01)),
        ];
        for (attrs, (y, x), vel) in cases {
            let r = red_blue_soldier_routine_00(attrs, 5);
            assert_eq!((r.y_pos, r.x_pos), (y, x), "attrs={attrs:02X}");
            assert_eq!(r.x_velocity, vel, "attrs={attrs:02X}");
        }
    }

    #[test]
    fn advances_the_routine_guarded_the_usual_way() {
        let r = red_blue_soldier_routine_00(0x00, 5);
        assert_eq!(r.routine_update, advance_enemy_routine(5));

        let guarded = red_blue_soldier_routine_00(0x00, 0);
        assert_eq!(guarded.routine_update, advance_enemy_routine(0));
        assert_eq!(guarded.routine_update.routine, 0);
        assert_eq!(guarded.routine_update.sprites, Some(0));
    }

    #[test]
    fn set_run_frame_only_advances_on_the_4th_frame() {
        assert_eq!(red_blue_soldier_set_run_frame(0x01, 0x01), 0x01); // not 4th frame, unchanged
        assert_eq!(red_blue_soldier_set_run_frame(0x04, 0x01), 0x02); // 4th frame, increments
    }

    #[test]
    fn set_run_frame_wraps_at_3() {
        assert_eq!(red_blue_soldier_set_run_frame(0x00, 0x02), 0x00);
    }

    #[test]
    fn set_bg_priority_forces_background_near_either_edge() {
        assert_eq!(red_blue_soldier_set_bg_priority(0x10, 0x00) & 0x20, 0x20); // left edge
        assert_eq!(red_blue_soldier_set_bg_priority(0xE0, 0x00) & 0x20, 0x20); // right edge
        assert_eq!(red_blue_soldier_set_bg_priority(0x50, 0x00) & 0x20, 0x00); // middle
    }

    #[test]
    fn set_bg_priority_preserves_bits_outside_the_priority_flag() {
        let r = red_blue_soldier_set_bg_priority(0x50, 0b0011_0111);
        assert_eq!(r, 0b0001_0111); // bit 5 stripped, no priority flag added (middle of screen)
    }

    #[test]
    fn routine_01_still_running_when_outside_the_trigger_x_range() {
        // x=0x50, running left (fract=0,fast=0xff -> -1/frame): stays > 0x28.
        let r = blue_soldier_routine_01(0x00, 0x04, 0x00, 0, 0x00, 0x50, 0, 0, 0xFF, 0x60, 0, 0, 0, [0, 0], [0, 0], 5);
        assert_eq!(r.outcome, BlueSoldierRoutine01Outcome::StillRunning);
        assert_eq!(r.enemy_frame, red_blue_soldier_set_run_frame(0x04, 0x00));
    }

    #[test]
    fn routine_01_still_running_when_in_range_but_far_from_every_player() {
        let r = blue_soldier_routine_01(0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x00, 0x00], [1, 1], 5);
        assert_eq!(r.outcome, BlueSoldierRoutine01Outcome::StillRunning);
    }

    #[test]
    fn routine_01_close_to_player_resets_frame_and_advances() {
        let r = blue_soldier_routine_01(0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x55, 0x00], [1, 0], 5);
        match r.outcome {
            BlueSoldierRoutine01Outcome::CloseToPlayer(delayed) => {
                assert_eq!(delayed, set_enemy_delay_adv_routine(0x01, 5));
                assert_eq!(r.enemy_frame, 0x00);
            }
            other => panic!("expected CloseToPlayer, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_sprites_always_use_the_pre_override_frame() {
        let r = blue_soldier_routine_01(0x02, 0x04, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x55, 0x00], [1, 0], 5);
        let new_frame = red_blue_soldier_set_run_frame(0x04, 0x02);
        assert_eq!(r.sprites, new_frame.wrapping_add(0x85));
    }

    #[test]
    fn routine_02_waits_when_delay_has_not_elapsed() {
        let r = blue_soldier_routine_02(0x03, 0x00, 0x00, 0x00, 0x00, 5);
        assert_eq!(r, BlueSoldierRoutine02Outcome::Waiting { animation_delay: 0x02 });
    }

    #[test]
    fn routine_02_animates_when_windup_is_not_done() {
        let r = blue_soldier_routine_02(0x01, 0x00, 0x00, 0x00, 0x00, 5);
        assert_eq!(r, BlueSoldierRoutine02Outcome::Animating { enemy_frame: 0x01, sprites: 0x88, animation_delay: 0x08 });
    }

    #[test]
    fn routine_02_starts_the_jump_once_windup_finishes() {
        let r = blue_soldier_routine_02(0x01, 0x02, 0x00, 0b0010_0000, 0x00, 5);
        match r {
            BlueSoldierRoutine02Outcome::JumpStart { sprites, sprite_attr, state_width, x_velocity, y_velocity, delayed_routine } => {
                assert_eq!(sprites, 0x8A);
                assert_eq!(sprite_attr, 0x00); // bg priority bit stripped
                assert_eq!(state_width, enable_enemy_collision(0x00));
                assert_eq!(x_velocity, BLUE_SOLDIER_JMP_X_VEL_TBL[0]);
                assert_eq!(y_velocity, (0x00, 0xFF));
                assert_eq!(delayed_routine, set_enemy_delay_adv_routine(0x10, 5));
            }
            other => panic!("expected JumpStart, got {other:?}"),
        }
    }

    #[test]
    fn routine_03_uses_8b_and_leaves_delay_alone_when_already_zero() {
        let r = blue_soldier_routine_03(0x00, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0x00, 0x10);
        assert_eq!(r.sprites, 0x8B);
        assert_eq!(r.animation_delay, 0x00);
        assert_eq!(r.y_velocity, add_10_to_enemy_y_fract_vel(0x00, 0x10));
    }

    #[test]
    fn routine_03_uses_8a_and_decrements_when_delay_is_nonzero() {
        let r = blue_soldier_routine_03(0x05, 0, 0x00, 0x50, 0, 0, 0, 0x60, 0, 0x00, 0x10);
        assert_eq!(r.sprites, 0x8A);
        assert_eq!(r.animation_delay, 0x04);
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

    /// Like [`synthetic_prg_rom`], but also sets up level 0's per-level
    /// property table with real-shaped records for enemy types `$1e`
    /// (blue soldier) and `$1f` (red soldier) - `red_blue_soldier_gen_
    /// routine_01` spawns types `>= $10`, which route through the *per-
    /// level* pointer (`y = CURRENT_LEVEL*2`), not the shared one.
    fn synthetic_prg_rom_with_level_soldiers() -> Vec<u8> {
        let mut rom = synthetic_prg_rom();
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let level_table_addr: u16 = 0xF000;
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level_table_addr.to_le_bytes()); // level 0 -> y=0
        let table_off = 7 * 0x4000 + (level_table_addr as usize - 0xC000);
        rom[table_off + 0x1E * 4..table_off + 0x1E * 4 + 4].copy_from_slice(&[0x10, 0x20, 0x05, 0x30]); // blue
        rom[table_off + 0x1F * 4..table_off + 0x1F * 4 + 4].copy_from_slice(&[0x11, 0x21, 0x06, 0x31]); // red
        rom
    }

    #[test]
    fn routine_01_already_fired_skips_every_other_check() {
        let r = red_soldier_routine_01(0x00, 0x00, 0x00, 0x01, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0, 0], [0, 0], 5);
        assert_eq!(r.outcome, RedSoldierRoutine01Outcome::AlreadyFired);
    }

    #[test]
    fn routine_01_still_running_outside_trigger_range() {
        let r = red_soldier_routine_01(0x00, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0xFF, 0x60, 0, 0, 0, [0, 0], [0, 0], 5);
        assert_eq!(r.outcome, RedSoldierRoutine01Outcome::StillRunning);
    }

    #[test]
    fn routine_01_attacks_when_close_enough_and_overrides_the_sprite() {
        // attrs bit1=0 -> min_dist=0x10; player at 0x55, enemy ends up at 0x50 -> dist=5 < 0x10.
        let r = red_soldier_routine_01(0x00, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x55, 0x00], [1, 0], 5);
        assert_eq!(r.sprites, 0x8F);
        match r.outcome {
            RedSoldierRoutine01Outcome::Attack { var_1, attack_delay, routine_update } => {
                assert_eq!(var_1, 0x03);
                assert_eq!(attack_delay, 0x10);
                assert_eq!(routine_update, advance_enemy_routine(5));
            }
            other => panic!("expected Attack, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_min_dist_widens_when_attribute_bit_1_is_set() {
        // player at distance 0x20: within 0x30 (bit1 set) but not within 0x10 (bit1 clear).
        let narrow = red_soldier_routine_01(0x00, 0x00, 0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x70, 0x00], [1, 0], 5);
        assert_eq!(narrow.outcome, RedSoldierRoutine01Outcome::StillRunning);
        let wide = red_soldier_routine_01(0x00, 0x00, 0x02, 0x00, 0, 0x00, 0x50, 0, 0, 0x00, 0x60, 0, 0, 0, [0x70, 0x00], [1, 0], 5);
        assert!(matches!(wide.outcome, RedSoldierRoutine01Outcome::Attack { .. }));
    }

    #[test]
    fn routine_02_waits_and_strips_recoil_exactly_at_0x2c() {
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x2D, 0x02, 0x00, 0b0000_1000, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r, RedSoldierRoutine02Outcome::Waiting { attack_delay: 0x2C, sprite_attr: Some(0x00) });
    }

    #[test]
    fn routine_02_waits_without_touching_sprite_attr_otherwise() {
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x05, 0x02, 0x00, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r, RedSoldierRoutine02Outcome::Waiting { attack_delay: 0x04, sprite_attr: None });
    }

    #[test]
    fn routine_02_fires_a_bullet_when_bullets_remain() {
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[5] = 0; // free slot
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &routine, 0, 1, 0x01, 0x02, 0x00, 0x00, 0x50, 0x60, [1, 0], [0, 0], [0x60, 0], 0, 5);
        match r {
            RedSoldierRoutine02Outcome::Fired(f) => {
                assert_eq!(f.sprites, 0x90);
                assert_eq!(f.var_1, 0x01);
                assert_eq!(f.attack_delay, 0x30);
                assert_eq!(f.sprite_attr, 0x08);
                assert!(f.bullet.is_some());
            }
            other => panic!("expected Fired, got {other:?}"),
        }
    }

    #[test]
    fn routine_02_all_fired_marks_var_2_and_returns_to_routine_01() {
        let r = red_soldier_routine_02(&synthetic_prg_rom(), &[0u8; ENEMY_SLOT_COUNT], 0, 1, 0x01, 0x00, 0x00, 0x00, 0x50, 0x60, [0, 0], [0, 0], [0, 0], 0, 5);
        assert_eq!(r, RedSoldierRoutine02Outcome::AllFired { var_2: 0x01, routine_update: set_enemy_routine_to_a(5, 0x02) });
    }

    #[test]
    fn gen_routine_00_sets_the_initial_generation_delay() {
        assert_eq!(red_blue_soldier_gen_routine_00(5), set_enemy_delay_adv_routine(0x80, 5));
    }

    #[test]
    fn gen_routine_01_removed_once_all_wall_platings_are_destroyed() {
        let r = red_blue_soldier_gen_routine_01(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 0x03, 0x00, 0x01, 0x00);
        assert!(r.spawns.is_empty());
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::Removed(remove_enemy()));
    }

    #[test]
    fn gen_routine_01_odd_frame_touches_nothing() {
        let r = red_blue_soldier_gen_routine_01(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 0x00, 0x01, 0x05, 0x00);
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::OddFrame);
    }

    #[test]
    fn gen_routine_01_even_frame_decrements_and_waits() {
        let r = red_blue_soldier_gen_routine_01(&[], &[0u8; ENEMY_SLOT_COUNT], 0, 0x00, 0x00, 0x05, 0x00);
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::StillWaiting { animation_delay: 0x04 });
    }

    #[test]
    fn gen_routine_01_spawns_a_full_run_and_sets_the_next_delay() {
        let rom = synthetic_prg_rom_with_level_soldiers();
        let r = red_blue_soldier_gen_routine_01(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 0x00, 0x00, 0x01, 0x00);
        // table[0..4] = 0x00,0x01,0x02,0x03 (4 red soldiers), table[4] = 0xD0 (delay byte).
        assert_eq!(r.spawns.len(), 4);
        for (i, spawn) in r.spawns.iter().enumerate() {
            assert_eq!(spawn.attributes, i as u8);
            assert_eq!(spawn.enemy_type, 0x1F); // all red (byte>>2 == 0 for 0x00-0x03)
            assert_eq!(spawn.initialized.routine, 1);
        }
        // slots claimed in order 15, 14, 13, 12 (find_next_enemy_slot scans top-down).
        assert_eq!(r.spawns.iter().map(|s| s.slot).collect::<Vec<_>>(), vec![15, 14, 13, 12]);
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::Spawned { var_1: 5, animation_delay: 0xA0 });
    }

    #[test]
    fn gen_routine_01_skips_spawns_once_slots_run_out_but_still_advances_the_read_offset() {
        let rom = synthetic_prg_rom_with_level_soldiers();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[15] = 0;
        routine[14] = 0; // only 2 free slots, run wants 4
        let r = red_blue_soldier_gen_routine_01(&rom, &routine, 0, 0x00, 0x00, 0x01, 0x00);
        assert_eq!(r.spawns.len(), 2);
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::Spawned { var_1: 5, animation_delay: 0xA0 });
    }

    #[test]
    fn gen_routine_01_wraps_the_read_offset_at_the_ff_terminator() {
        let rom = synthetic_prg_rom_with_level_soldiers();
        // var_1=27 -> table[27]=0xFF (last byte) -> wraps to 0 -> reads table[0]=0x00 (red, attrs 0).
        let r = red_blue_soldier_gen_routine_01(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 0x00, 0x00, 0x01, 27);
        assert_eq!(r.spawns.len(), 4);
        assert_eq!(r.spawns[0].attributes, 0x00);
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::Spawned { var_1: 5, animation_delay: 0xA0 });
    }

    #[test]
    fn gen_routine_01_blue_soldiers_decode_from_the_high_bits() {
        let rom = synthetic_prg_rom_with_level_soldiers();
        // var_1=5 -> table[5..8] = 0x06,0x07,0xA0 (2 blue soldiers, then delay).
        let r = red_blue_soldier_gen_routine_01(&rom, &[0u8; ENEMY_SLOT_COUNT], 0, 0x00, 0x00, 0x01, 5);
        assert_eq!(r.spawns.len(), 2);
        for spawn in &r.spawns {
            assert_eq!(spawn.enemy_type, 0x1E); // blue
        }
        assert_eq!(r.spawns[0].attributes, 0x06 & 0x03);
        assert_eq!(r.spawns[1].attributes, 0x07 & 0x03);
        // table[7] = 0xA0 (delay byte) -> new_delay = 0xA0 << 1 = 0x40.
        assert_eq!(r.outcome, RedBlueSoldierGenRoutine01Outcome::Spawned { var_1: 8, animation_delay: 0x40 });
    }
}
