//! Native port of the level-5 (Snow Field/ice) grenade and pipe-joint
//! family (`src/bank0.asm`, `$a384`-`$a3fb`, `$a985`-`$a99b`):
//! `ice_grenade_generator` (waits for the player to scroll close, then
//! periodically lobs ice grenades via `generate_enemy_a`), `ice_grenade`
//! itself (a lobbed projectile that falls, explodes on the first real
//! ground collision, and shares `mortar_shot_routine_03` for its own
//! explosion-hide entry - no new port needed for that state), and
//! `ice_separator` (the "pipe joint" sprite between tank body segments -
//! purely cosmetic, follows the tank's own scripted scroll illusion via
//! a global flag rather than real physics). `tank_routine` itself
//! (`$a41a`) is not ported here - a stationary *nametable* object (real
//! ASM comment: "tank is actually in nametable, not a sprite"), a
//! genuinely different rendering path this crate hasn't touched yet.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_position_utils::add_a_to_enemy_y_fract_vel;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::ENEMY_SLOT_COUNT;
use crate::enemy::generate_enemy_at_pos::{generate_enemy_at_pos, GeneratedEnemy};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};
use crate::physics::collision::{add_y_to_y_pos_get_bg_collision, CollisionCode, BG_COLLISION_DATA_LEN};

/// One [`ice_grenade_generator_routine_00`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceGrenadeGeneratorRoutine00Outcome {
    /// Not yet scrolled close enough (`ENEMY_X_POS >= $c8`, real ASM
    /// comment: "78% of screen").
    Waiting,
    Activated { delayed_routine: DelayedRoutineUpdate },
}

/// The full result of one [`ice_grenade_generator_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IceGrenadeGeneratorRoutine00Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: IceGrenadeGeneratorRoutine00Outcome,
}

/// Native port of `ice_grenade_generator_routine_00` (`$a38a`).
pub fn ice_grenade_generator_routine_00(level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8, current_routine: u8) -> IceGrenadeGeneratorRoutine00Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let outcome = if scroll.x_pos >= 0xC8 {
        IceGrenadeGeneratorRoutine00Outcome::Waiting
    } else {
        IceGrenadeGeneratorRoutine00Outcome::Activated { delayed_routine: set_enemy_delay_adv_routine(0x01, current_routine) }
    };
    IceGrenadeGeneratorRoutine00Result { scroll, outcome }
}

/// One [`ice_grenade_generator_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceGrenadeGeneratorRoutine01Outcome {
    Waiting { animation_delay: u8 },
    /// Delay elapsed - spawns an ice grenade (enemy type `$11`) at the
    /// generator's own (already scroll-updated) position, no offset.
    Spawned { animation_delay: u8, grenade: Option<GeneratedEnemy> },
}

/// The full result of one [`ice_grenade_generator_routine_01`] call.
/// Real ASM's own tail is `jmp generate_enemy_a`, so (like `rock_cave_
/// routine_02`) this routine never advances its own routine index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IceGrenadeGeneratorRoutine01Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: IceGrenadeGeneratorRoutine01Outcome,
}

/// Native port of `ice_grenade_generator_routine_01` (`$a399`).
pub fn ice_grenade_generator_routine_01(
    prg_rom: &[u8],
    enemy_routine_slots: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    animation_delay: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
) -> IceGrenadeGeneratorRoutine01Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);
    let delay = animation_delay.wrapping_sub(1);
    let outcome = if delay != 0 {
        IceGrenadeGeneratorRoutine01Outcome::Waiting { animation_delay: delay }
    } else {
        let grenade = generate_enemy_at_pos(prg_rom, enemy_routine_slots, 0x11, current_level, scroll.x_pos, scroll.y_pos, 0x00, 0x00);
        IceGrenadeGeneratorRoutine01Outcome::Spawned { animation_delay: 0x80, grenade }
    };
    IceGrenadeGeneratorRoutine01Result { scroll, outcome }
}

/// The full result of one [`ice_grenade_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IceGrenadeRoutine00Result {
    pub sound: u8,
    pub sprite_attr: u8,
    pub x_vel_fract: u8,
    pub x_vel_fast: u8,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `ice_grenade_routine_00` (`$a3b5`).
pub fn ice_grenade_routine_00(current_routine: u8) -> IceGrenadeRoutine00Result {
    IceGrenadeRoutine00Result {
        sound: 0x1A,
        sprite_attr: 0x20,
        x_vel_fract: 0x80,
        x_vel_fast: 0x00,
        y_vel_fract: 0x00,
        y_vel_fast: 0xFE,
        routine_update: advance_enemy_routine(current_routine),
    }
}

/// `ice_grenade_sprite_tbl` (`$a3fb`, 4 bytes) - the tumbling grenade's
/// own animation frames.
const ICE_GRENADE_SPRITE_TBL: [u8; 4] = [0x74, 0x75, 0x76, 0x77];

/// One [`ice_grenade_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceGrenadeRoutine01Outcome {
    /// Still rising/falling (post-gravity Y velocity still negative).
    StillFalling,
    /// Falling, gravity settled non-negative, but no real ground
    /// collision found yet.
    NoGroundYet { sprite_attr: u8 },
    /// Found a real ground collision - explodes (advances to the
    /// already-ported `mortar_shot_routine_03`, this family's own
    /// routine index `2`).
    Exploding { sprite_attr: u8, sound: u8, routine_update: EnemyRoutineUpdate },
}

/// The full result of one [`ice_grenade_routine_01`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IceGrenadeRoutine01Result {
    pub frame: u8,
    pub sprite: u8,
    pub position: UpdatedEnemyPos,
    pub y_vel_fract: u8,
    pub y_vel_fast: u8,
    pub outcome: IceGrenadeRoutine01Outcome,
}

/// Native port of `ice_grenade_routine_01` (`$a3d7`) - real ASM applies
/// gravity *after* `update_enemy_pos` (so this frame's position uses the
/// pre-gravity velocity, matching `mortar_shot_routine_01`'s own
/// ordering), and its `jsr update_enemy_pos` call means a possible
/// internal removal there must still be accounted for before the
/// `Exploding` outcome's own `advance_enemy_routine` call (same real
/// quirk this crate already caught in `enemy_bullet_routine_01`).
#[allow(clippy::too_many_arguments)]
pub fn ice_grenade_routine_01(
    frame_counter: u8,
    enemy_frame: u8,
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
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    current_routine: u8,
) -> IceGrenadeRoutine01Result {
    let frame = if frame_counter & 0x07 == 0 { enemy_frame.wrapping_add(1) } else { enemy_frame };
    let sprite = ICE_GRENADE_SPRITE_TBL[(frame & 0x03) as usize];

    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);
    let effective_routine = if position.removed.is_some() { 0 } else { current_routine };

    let (y_vel_fract, y_vel_fast) = add_a_to_enemy_y_fract_vel(0x0A, y_vel_fract, y_vel_fast);

    let outcome = if (y_vel_fast as i8) < 0 {
        IceGrenadeRoutine01Outcome::StillFalling
    } else {
        let collision = add_y_to_y_pos_get_bg_collision(0x04, position.x.pos, position.y.pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
        if collision == CollisionCode::Empty {
            IceGrenadeRoutine01Outcome::NoGroundYet { sprite_attr: 0x00 }
        } else {
            IceGrenadeRoutine01Outcome::Exploding { sprite_attr: 0x00, sound: 0x24, routine_update: advance_enemy_routine(effective_routine) }
        }
    };

    IceGrenadeRoutine01Result { frame, sprite, position, y_vel_fract, y_vel_fast, outcome }
}

/// One [`ice_separator_routine_00`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceSeparatorRoutine00Outcome {
    /// `TANK_ICE_JOINT_SCROLL_FLAG` clear - normal scroll.
    Scrolled(ScrolledEnemyPos),
    /// Flag set, but no scroll this frame - untouched.
    NoScrollThisFrame,
    /// Flag set and scrolling - nudged 1 pixel left instead of using the
    /// real scroll delta, to sell the "tank driving forward" illusion
    /// (real ASM comment: "tank is actually in nametable... auto scroll
    /// makes it look like the tank is approaching the player").
    Nudged { x_pos: u8 },
}

/// The full result of one [`ice_separator_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IceSeparatorRoutine00Result {
    pub sprite: u8,
    pub outcome: IceSeparatorRoutine00Outcome,
}

/// Native port of `ice_separator_routine_00` (`$a985`) - the only real
/// table entry for enemy type `$13` (level 5's own remap) - stays here
/// permanently, purely cosmetic.
pub fn ice_separator_routine_00(tank_ice_joint_scroll_flag: u8, level_scrolling_type: u8, frame_scroll: u8, x_pos: u8, y_pos: u8) -> IceSeparatorRoutine00Result {
    let outcome = if tank_ice_joint_scroll_flag == 0 {
        IceSeparatorRoutine00Outcome::Scrolled(add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos))
    } else if frame_scroll == 0 {
        IceSeparatorRoutine00Outcome::NoScrollThisFrame
    } else {
        IceSeparatorRoutine00Outcome::Nudged { x_pos: x_pos.wrapping_sub(1) }
    };
    IceSeparatorRoutine00Result { sprite: 0xC4, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        // ENEMY_TYPE 0x11 (ice grenade) is >= 0x10, so `initialize_enemy`
        // uses the per-level pointer (level 0's own slot), not the
        // shared 0x10 pointer.
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let level0_table_addr: u16 = 0xEF10;
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level0_table_addr.to_le_bytes());
        let level0_off = 7 * 0x4000 + (level0_table_addr as usize - 0xC000) + 0x11 * 4;
        rom[level0_off..level0_off + 4].copy_from_slice(&[0x81, 0x0d, 0x01, 0x00]);
        rom
    }

    fn no_scroll_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0u8; BG_COLLISION_DATA_LEN]
    }

    fn solid_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0xFFu8; BG_COLLISION_DATA_LEN]
    }

    #[test]
    fn generator_00_waits_before_the_trigger_point() {
        let r = ice_grenade_generator_routine_00(0, 0x00, 0xD0, 0x50, 3);
        assert_eq!(r.outcome, IceGrenadeGeneratorRoutine00Outcome::Waiting);
    }

    #[test]
    fn generator_00_activates_past_the_trigger_point() {
        let r = ice_grenade_generator_routine_00(0, 0x00, 0xC0, 0x50, 3);
        assert_eq!(r.outcome, IceGrenadeGeneratorRoutine00Outcome::Activated { delayed_routine: set_enemy_delay_adv_routine(0x01, 3) });
    }

    #[test]
    fn generator_01_spawns_at_its_own_updated_position() {
        let rom = synthetic_prg_rom();
        let slots = [0u8; ENEMY_SLOT_COUNT];
        let r = ice_grenade_generator_routine_01(&rom, &slots, 0, 0x01, 0, 0x00, 0x50, 0x60);
        match r.outcome {
            IceGrenadeGeneratorRoutine01Outcome::Spawned { animation_delay, grenade } => {
                assert_eq!(animation_delay, 0x80);
                let g = grenade.unwrap();
                assert_eq!(g.x_pos, r.scroll.x_pos);
                assert_eq!(g.y_pos, r.scroll.y_pos);
            }
            other => panic!("expected Spawned, got {other:?}"),
        }
    }

    #[test]
    fn routine_00_sets_the_fixed_lob_velocity() {
        let r = ice_grenade_routine_00(3);
        assert_eq!(r.x_vel_fract, 0x80);
        assert_eq!(r.y_vel_fast, 0xFE);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn routine_01_still_rising_exits_without_a_ground_check() {
        // y_vel_fast starts at 0xFE (-2); +0x0a gravity -> 0xFE+0x0A=0x08? wait need still-negative result.
        let r = ice_grenade_routine_01(0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0xC0, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert!((r.y_vel_fast as i8) < 0);
        assert_eq!(r.outcome, IceGrenadeRoutine01Outcome::StillFalling);
    }

    #[test]
    fn routine_01_no_ground_yet_when_falling_but_no_collision() {
        let r = ice_grenade_routine_01(0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0x00, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert!((r.y_vel_fast as i8) >= 0);
        assert_eq!(r.outcome, IceGrenadeRoutine01Outcome::NoGroundYet { sprite_attr: 0x00 });
    }

    #[test]
    fn routine_01_explodes_on_real_ground_collision() {
        let r = ice_grenade_routine_01(0x00, 0x00, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0x00, 0, 0, 0, &solid_bg_collision_data(), 3);
        match r.outcome {
            IceGrenadeRoutine01Outcome::Exploding { sound, routine_update, .. } => {
                assert_eq!(sound, 0x24);
                assert_eq!(routine_update, advance_enemy_routine(3));
            }
            other => panic!("expected Exploding, got {other:?}"),
        }
    }

    #[test]
    fn routine_01_frame_advances_every_8th_counter_tick() {
        let a = ice_grenade_routine_01(0x00, 0x01, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0x00, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(a.frame, 0x02);
        let b = ice_grenade_routine_01(0x01, 0x01, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0x00, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(b.frame, 0x01);
    }

    #[test]
    fn separator_scrolls_normally_when_flag_clear() {
        let r = ice_separator_routine_00(0x00, 0, 0x02, 0x50, 0x60);
        assert_eq!(r.sprite, 0xC4);
        assert_eq!(r.outcome, IceSeparatorRoutine00Outcome::Scrolled(add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60)));
    }

    #[test]
    fn separator_untouched_when_flag_set_and_no_scroll() {
        let r = ice_separator_routine_00(0x01, 0, 0x00, 0x50, 0x60);
        assert_eq!(r.outcome, IceSeparatorRoutine00Outcome::NoScrollThisFrame);
    }

    #[test]
    fn separator_nudges_left_when_flag_set_and_scrolling() {
        let r = ice_separator_routine_00(0x01, 0, 0x02, 0x50, 0x60);
        assert_eq!(r.outcome, IceSeparatorRoutine00Outcome::Nudged { x_pos: 0x4F });
    }
}
