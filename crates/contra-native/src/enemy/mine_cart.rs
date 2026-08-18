//! Native port of the hangar-zone mine cart family (`src/bank0.asm`,
//! `$b122`-`$b1fe`): `mine_cart_generator` (spawns a moving cart on a
//! delay, tracks it, respawns once destroyed), `immobile_cart_generator`
//! (a stationary cart that starts rolling once the player lands on it -
//! the actual "did the player land" check lives in player-side collision
//! code, not here), and `moving_cart_routine_00` itself (the shared
//! entry every real routine-table index for enemy type `$14` maps to -
//! a physics-driven object with no state-machine transitions of its own
//! except jumping straight to routine index `4` on an explosive impact).

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_position_utils::{add_a_to_enemy_y_fract_vel, reverse_enemy_x_direction};
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, set_enemy_routine_to_a, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::{initialize_enemy, InitializedEnemy};
use crate::enemy::update_enemy_pos::{update_enemy_pos, UpdatedEnemyPos};
use crate::physics::collision::{add_a_y_to_enemy_pos_get_bg_collision, bg_collision_with_nametable_xor, CollisionCode, BG_COLLISION_DATA_LEN};

/// Native port of `init_cart_vel_and_y_pos` (`$b1ed`) - shared by both
/// `immobile_cart_generator_routine_00` and a freshly-spawned moving
/// cart: sets the cart sprite and a fixed initial Y position, and (the
/// caller's own choice) an initial X velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InitCartVelAndYPosResult {
    pub x_vel_fract: u8,
    pub sprite: u8,
    pub y_pos: u8,
}

pub fn init_cart_vel_and_y_pos(x_vel_fract: u8) -> InitCartVelAndYPosResult {
    InitCartVelAndYPosResult { x_vel_fract, sprite: 0x2A, y_pos: 0xC8 }
}

/// The full result of one [`immobile_cart_generator_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImmobileCartGeneratorRoutine00Result {
    pub init: InitCartVelAndYPosResult,
    pub routine_update: EnemyRoutineUpdate,
}

/// Native port of `immobile_cart_generator_routine_00` (`$b1e5`) - real
/// ASM: `lda #$c0; jsr init_cart_vel_and_y_pos` (fixed initial X
/// velocity of `0xc0`), then falls into `cart_advance_enemy_routine`.
pub fn immobile_cart_generator_routine_00(current_routine: u8) -> ImmobileCartGeneratorRoutine00Result {
    ImmobileCartGeneratorRoutine00Result { init: init_cart_vel_and_y_pos(0xC0), routine_update: advance_enemy_routine(current_routine) }
}

/// One [`immobile_cart_generator_routine_01`] call's outcome - real ASM
/// has no `rts` of its own when the player hasn't landed on it yet
/// (`ENEMY_FRAME == 0`): it falls straight through into the *next*
/// routine's own code, `rising_spiked_wall_routine_03`'s `jmp add_
/// scroll_to_enemy_pos` (a real, deliberate ROM-space-saving reuse, not
/// a disassembly artifact - confirmed by that label's own real ASM
/// comment, "ensure scroll up to date", matching exactly what this
/// branch needs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImmobileCartGeneratorRoutine01Outcome {
    /// Player landed (`ENEMY_FRAME != 0`, set elsewhere by that
    /// collision code) - starts the cart rolling.
    Advanced(EnemyRoutineUpdate),
    /// Not yet landed on - just keeps its position in sync with scroll.
    ScrollOnly(ScrolledEnemyPos),
}

/// Native port of `immobile_cart_generator_routine_01` (`$b1fb`).
pub fn immobile_cart_generator_routine_01(
    enemy_frame: u8,
    current_routine: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
) -> ImmobileCartGeneratorRoutine01Outcome {
    if enemy_frame != 0 {
        ImmobileCartGeneratorRoutine01Outcome::Advanced(advance_enemy_routine(current_routine))
    } else {
        ImmobileCartGeneratorRoutine01Outcome::ScrollOnly(add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos))
    }
}

/// Native port of `mine_cart_generator_routine_00` (`$b122`) - real ASM:
/// `ENEMY_FRAME = $80` (the "no cart generated" sentinel), then `jmp
/// set_enemy_delay_adv_routine` with `a = $01`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MineCartGeneratorRoutine00Result {
    pub frame: u8,
    pub delayed_routine: DelayedRoutineUpdate,
}

pub fn mine_cart_generator_routine_00(current_routine: u8) -> MineCartGeneratorRoutine00Result {
    MineCartGeneratorRoutine00Result { frame: 0x80, delayed_routine: set_enemy_delay_adv_routine(0x01, current_routine) }
}

/// A newly-spawned moving cart (enemy type `$14`) from [`mine_cart_generator_routine_01`]'s
/// own real spawn sequence: `ENEMY_X_POS = $f8`, `ENEMY_X_VELOCITY_FAST
/// = $ff` (1 unit left per frame), `ENEMY_VAR_4 = $02` (leftward
/// direction), `ENEMY_ATTRIBUTES = $80` (explodes on background
/// collision, unlike an immobile-generator cart), plus [`init_cart_vel_and_y_pos`]'s
/// own fields (`x_vel_fract = 0`, per the real `lda #$00; jsr init_cart_
/// vel_and_y_pos` right after).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedCartSpawn {
    pub slot: u8,
    pub initialized: InitializedEnemy,
    pub x_pos: u8,
    pub x_vel_fast: u8,
    pub var_4: u8,
    pub attributes: u8,
    pub init: InitCartVelAndYPosResult,
}

/// One [`mine_cart_generator_routine_01`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MineCartGeneratorRoutine01Outcome {
    /// No cart currently tracked, still counting down before trying to
    /// spawn one.
    Waiting { animation_delay: u8 },
    /// Delay elapsed, but no free enemy slot to spawn into.
    NoSlotAvailable { animation_delay: u8 },
    /// Delay elapsed - spawned a new moving cart.
    Spawned { animation_delay: u8, frame: u8, cart: GeneratedCartSpawn },
    /// Tracking a previously-spawned cart that's still alive.
    CartStillAlive,
    /// Tracking cart was destroyed - resets to the "no cart" state.
    CartDestroyed { frame: u8, animation_delay: u8 },
}

/// The full result of one [`mine_cart_generator_routine_01`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MineCartGeneratorRoutine01Result {
    pub scroll: ScrolledEnemyPos,
    pub outcome: MineCartGeneratorRoutine01Outcome,
}

/// Native port of `mine_cart_generator_routine_01` (`$b12c`).
/// `enemy_frame` doubles as a sentinel: `$80` means "no cart currently
/// tracked" (real ASM: `bpl` tests bit 7), any other value (`0`-`15`) is
/// the tracked cart's own enemy slot index.
#[allow(clippy::too_many_arguments)]
pub fn mine_cart_generator_routine_01(
    prg_rom: &[u8],
    enemy_routine_slots: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_frame: u8,
    animation_delay: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    x_pos: u8,
    y_pos: u8,
) -> MineCartGeneratorRoutine01Result {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, x_pos, y_pos);

    let outcome = if (enemy_frame as i8) >= 0 {
        let cart_routine = enemy_routine_slots[enemy_frame as usize];
        if cart_routine != 0 {
            MineCartGeneratorRoutine01Outcome::CartStillAlive
        } else {
            MineCartGeneratorRoutine01Outcome::CartDestroyed { frame: 0x80, animation_delay: 0x80 }
        }
    } else {
        let delay = animation_delay.wrapping_sub(1);
        if delay != 0 {
            MineCartGeneratorRoutine01Outcome::Waiting { animation_delay: delay }
        } else {
            match find_next_enemy_slot(enemy_routine_slots) {
                None => MineCartGeneratorRoutine01Outcome::NoSlotAvailable { animation_delay: 0x01 },
                Some(slot) => {
                    let initialized = initialize_enemy(prg_rom, 0x14, current_level);
                    let init = init_cart_vel_and_y_pos(0x00);
                    MineCartGeneratorRoutine01Outcome::Spawned {
                        animation_delay: 0x01,
                        frame: slot,
                        cart: GeneratedCartSpawn { slot, initialized, x_pos: 0xF8, x_vel_fast: 0xFF, var_4: 0x02, attributes: 0x80, init },
                    }
                }
            }
        }
    };

    MineCartGeneratorRoutine01Result { scroll, outcome }
}

/// `cart_collision_config_tbl` (`$b1d5`, 4 bytes) - `[nametable_xor(right), x_offset(right), nametable_xor(left), x_offset(left)]`,
/// indexed directly by `ENEMY_VAR_4` (`0` or `2`).
const CART_COLLISION_CONFIG_TBL: [u8; 4] = [0x00, 0x0F, 0xFF, 0xF1];

/// What happened when [`moving_cart_routine_00`] found a background
/// collision in its direction of travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovingCartCollisionOutcome {
    /// `ENEMY_ATTRIBUTES` bit 7 set (a mine-cart-generator spawn) -
    /// explodes on impact.
    Explodes(EnemyRoutineUpdate),
    /// Otherwise (an immobile-generator cart) - reverses direction.
    ReversesDirection { var_4: u8, x_vel_fract: u8, x_vel_fast: u8 },
}

/// One [`moving_cart_routine_00`] call's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovingCartRoutine00Outcome {
    CollisionAhead(MovingCartCollisionOutcome),
    /// No collision ahead; solid ground below - stays on track, no
    /// gravity added this frame.
    OnTrack,
    /// No collision ahead, nothing below - gravity applied.
    Falling { y_vel_fract: u8, y_vel_fast: u8 },
}

/// The full result of one [`moving_cart_routine_00`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovingCartRoutine00Result {
    pub sprite: u8,
    pub position: UpdatedEnemyPos,
    pub outcome: MovingCartRoutine00Outcome,
}

/// Native port of `moving_cart_routine_00` (`$b186`) - the single real
/// routine every one of enemy type `$14`'s own table indices `0`-`2`
/// maps to (no state-machine transitions of its own except jumping
/// straight to routine index `4` on an explosive impact). Real ASM
/// calls `update_enemy_pos` via a plain `jsr`, so its own possible
/// internal removal is a side effect that doesn't stop the rest of this
/// routine from running - if it happened, the `Explodes` outcome's own
/// routine-index update must see the already-zeroed routine, not the
/// stale `current_routine` input (same real quirk this crate already
/// caught in `enemy_bullet_routine_01`).
#[allow(clippy::too_many_arguments)]
pub fn moving_cart_routine_00(
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
    var_4: u8,
    attributes: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
    current_routine: u8,
) -> MovingCartRoutine00Result {
    let sprite = 0x2A + ((frame_counter >> 2) & 0x01);
    let position = update_enemy_pos(level_scrolling_type, frame_scroll, x_pos, x_vel_accum, x_vel_fract, x_vel_fast, y_pos, y_vel_accum, y_vel_fract, y_vel_fast);
    let effective_routine = if position.removed.is_some() { 0 } else { current_routine };

    let dir = var_4 as usize;
    let offset = CART_COLLISION_CONFIG_TBL[dir + 1];
    let (x_collision, carry) = position.x.pos.overflowing_add(offset);
    let nametable_xor = CART_COLLISION_CONFIG_TBL[dir].wrapping_add(carry as u8);

    let ahead = bg_collision_with_nametable_xor(x_collision, position.y.pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, nametable_xor, bg_collision_data);

    let outcome = if ahead != CollisionCode::Empty {
        if (attributes as i8) < 0 {
            MovingCartRoutine00Outcome::CollisionAhead(MovingCartCollisionOutcome::Explodes(set_enemy_routine_to_a(effective_routine, 0x04)))
        } else {
            let new_var_4 = var_4 ^ 0x02;
            let (new_x_vel_fract, new_x_vel_fast) = reverse_enemy_x_direction(x_vel_fract, x_vel_fast);
            MovingCartRoutine00Outcome::CollisionAhead(MovingCartCollisionOutcome::ReversesDirection { var_4: new_var_4, x_vel_fract: new_x_vel_fract, x_vel_fast: new_x_vel_fast })
        }
    } else {
        let below = add_a_y_to_enemy_pos_get_bg_collision(0x00, 0x09, position.x.pos, position.y.pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
        if below != CollisionCode::Empty {
            MovingCartRoutine00Outcome::OnTrack
        } else {
            let (new_y_vel_fract, new_y_vel_fast) = add_a_to_enemy_y_fract_vel(0x20, y_vel_fract, y_vel_fast);
            MovingCartRoutine00Outcome::Falling { y_vel_fract: new_y_vel_fract, y_vel_fast: new_y_vel_fast }
        }
    };

    MovingCartRoutine00Result { sprite, position, outcome }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_scroll_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0u8; BG_COLLISION_DATA_LEN]
    }

    fn solid_bg_collision_data() -> [u8; BG_COLLISION_DATA_LEN] {
        [0xFFu8; BG_COLLISION_DATA_LEN]
    }

    fn synthetic_prg_rom() -> Vec<u8> {
        // ENEMY_TYPE 0x14 (moving cart) is >= 0x10, so `initialize_enemy`
        // uses the per-level pointer (level 0's own slot, offset 0), not
        // the shared 0x10 pointer.
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let level0_table_addr: u16 = 0xEF10;
        rom[ptr_tbl_off..ptr_tbl_off + 2].copy_from_slice(&level0_table_addr.to_le_bytes());
        let level0_off = 7 * 0x4000 + (level0_table_addr as usize - 0xC000) + 0x14 * 4;
        rom[level0_off..level0_off + 4].copy_from_slice(&[0x81, 0x0d, 0x01, 0x00]);
        rom
    }

    #[test]
    fn init_cart_vel_and_y_pos_sets_fixed_sprite_and_y() {
        let r = init_cart_vel_and_y_pos(0xC0);
        assert_eq!(r, InitCartVelAndYPosResult { x_vel_fract: 0xC0, sprite: 0x2A, y_pos: 0xC8 });
    }

    #[test]
    fn immobile_generator_routine_00_composes_init_and_advance() {
        let r = immobile_cart_generator_routine_00(3);
        assert_eq!(r.init.x_vel_fract, 0xC0);
        assert_eq!(r.routine_update, advance_enemy_routine(3));
    }

    #[test]
    fn immobile_generator_routine_01_advances_once_landed_on() {
        let r = immobile_cart_generator_routine_01(0x01, 3, 0, 0x02, 0x50, 0x60);
        assert_eq!(r, ImmobileCartGeneratorRoutine01Outcome::Advanced(advance_enemy_routine(3)));
    }

    #[test]
    fn immobile_generator_routine_01_scrolls_only_while_waiting() {
        let r = immobile_cart_generator_routine_01(0x00, 3, 0, 0x02, 0x50, 0x60);
        assert_eq!(r, ImmobileCartGeneratorRoutine01Outcome::ScrollOnly(add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60)));
    }

    #[test]
    fn mine_cart_generator_routine_00_seeds_the_sentinel_frame() {
        let r = mine_cart_generator_routine_00(3);
        assert_eq!(r.frame, 0x80);
        assert_eq!(r, MineCartGeneratorRoutine00Result { frame: 0x80, delayed_routine: set_enemy_delay_adv_routine(0x01, 3) });
    }

    #[test]
    fn generator_01_waits_while_no_cart_and_delay_pending() {
        let rom = synthetic_prg_rom();
        let slots = [0u8; ENEMY_SLOT_COUNT];
        let r = mine_cart_generator_routine_01(&rom, &slots, 0, 0x80, 0x05, 0, 0x02, 0x50, 0x60);
        assert_eq!(r.outcome, MineCartGeneratorRoutine01Outcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn generator_01_spawns_when_delay_elapses_and_a_slot_is_free() {
        let rom = synthetic_prg_rom();
        let slots = [0u8; ENEMY_SLOT_COUNT];
        let r = mine_cart_generator_routine_01(&rom, &slots, 0, 0x80, 0x01, 0, 0x02, 0x50, 0x60);
        match r.outcome {
            MineCartGeneratorRoutine01Outcome::Spawned { animation_delay, frame, cart } => {
                assert_eq!(animation_delay, 0x01);
                assert_eq!(frame, 15); // highest free slot
                assert_eq!(cart.slot, 15);
                assert_eq!(cart.x_pos, 0xF8);
                assert_eq!(cart.x_vel_fast, 0xFF);
                assert_eq!(cart.var_4, 0x02);
                assert_eq!(cart.attributes, 0x80);
                assert_eq!(cart.init.x_vel_fract, 0x00);
                assert_eq!(cart.initialized.routine, 1);
            }
            other => panic!("expected Spawned, got {other:?}"),
        }
    }

    #[test]
    fn generator_01_reports_no_slot_but_still_resets_the_delay() {
        let rom = synthetic_prg_rom();
        let slots = [1u8; ENEMY_SLOT_COUNT];
        let r = mine_cart_generator_routine_01(&rom, &slots, 0, 0x80, 0x01, 0, 0x02, 0x50, 0x60);
        assert_eq!(r.outcome, MineCartGeneratorRoutine01Outcome::NoSlotAvailable { animation_delay: 0x01 });
    }

    #[test]
    fn generator_01_tracks_a_still_alive_cart() {
        let rom = synthetic_prg_rom();
        let mut slots = [0u8; ENEMY_SLOT_COUNT];
        slots[5] = 3; // tracked cart's own routine index, nonzero = alive
        let r = mine_cart_generator_routine_01(&rom, &slots, 0, 5, 0x00, 0, 0x02, 0x50, 0x60);
        assert_eq!(r.outcome, MineCartGeneratorRoutine01Outcome::CartStillAlive);
    }

    #[test]
    fn generator_01_resets_once_the_tracked_cart_is_destroyed() {
        let rom = synthetic_prg_rom();
        let mut slots = [0u8; ENEMY_SLOT_COUNT];
        slots[5] = 0; // destroyed (routine zeroed)
        let r = mine_cart_generator_routine_01(&rom, &slots, 0, 5, 0x00, 0, 0x02, 0x50, 0x60);
        assert_eq!(r.outcome, MineCartGeneratorRoutine01Outcome::CartDestroyed { frame: 0x80, animation_delay: 0x80 });
    }

    #[test]
    fn moving_cart_falls_when_nothing_ahead_or_below() {
        let r = moving_cart_routine_00(0x00, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0x10, 0x00, 0x00, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(r.outcome, MovingCartRoutine00Outcome::Falling { y_vel_fract: 0x20, y_vel_fast: 0x10 });
    }

    #[test]
    fn moving_cart_stays_on_track_when_supported() {
        // x_pos=0xb5, y_pos=0x10, dir=0 (offset +0x0f ahead): the "ahead"
        // check (x=0xc4, y=0x10) and the "below" check (x=0xb5, y=0x19)
        // land on different (offset, column) pairs in BG_COLLISION_DATA
        // at these specific inputs - set only the "below" one solid.
        let mut data = [0u8; BG_COLLISION_DATA_LEN];
        data[6] = 0b0000_0011; // column 3 (shift 0) -> code 3 (Solid), read by the "below" check
        let r = moving_cart_routine_00(0x00, 0, 0x00, 0xB5, 0, 0, 0, 0x10, 0, 0, 0, 0x00, 0x00, 0, 0, 0, &data, 3);
        assert_eq!(r.outcome, MovingCartRoutine00Outcome::OnTrack);
    }

    #[test]
    fn moving_cart_reverses_on_collision_when_not_explosive() {
        // solid data everywhere means both the "ahead" and "below" checks hit solid.
        let r = moving_cart_routine_00(0x00, 0, 0x00, 0x50, 0, 0, 0x01, 0x50, 0, 0, 0x00, 0x00, 0x00, 0, 0, 0, &solid_bg_collision_data(), 3);
        match r.outcome {
            MovingCartRoutine00Outcome::CollisionAhead(MovingCartCollisionOutcome::ReversesDirection { var_4, .. }) => {
                assert_eq!(var_4, 0x02); // 0x00 ^ 0x02
            }
            other => panic!("expected ReversesDirection, got {other:?}"),
        }
    }

    #[test]
    fn moving_cart_explodes_on_collision_when_attributes_bit_7_set() {
        let r = moving_cart_routine_00(0x00, 0, 0x00, 0x50, 0, 0, 0x01, 0x50, 0, 0, 0x00, 0x00, 0x80, 0, 0, 0, &solid_bg_collision_data(), 3);
        match r.outcome {
            MovingCartRoutine00Outcome::CollisionAhead(MovingCartCollisionOutcome::Explodes(update)) => {
                assert_eq!(update, set_enemy_routine_to_a(3, 0x04));
            }
            other => panic!("expected Explodes, got {other:?}"),
        }
    }

    #[test]
    fn moving_cart_sprite_alternates_on_frame_counter_bit_2() {
        let a = moving_cart_routine_00(0x00, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x00, 0x00, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        let b = moving_cart_routine_00(0x04, 0, 0x00, 0x50, 0, 0, 0, 0x50, 0, 0, 0, 0x00, 0x00, 0, 0, 0, &no_scroll_bg_collision_data(), 3);
        assert_eq!(a.sprite, 0x2A);
        assert_eq!(b.sprite, 0x2B);
    }
}
