//! Native port of Contra's generic "enemy just died" explosion-init
//! state, `enemy_routine_init_explosion` (`src/bank7.asm`, CPU `$e74b`-
//! `$e75d`) - a real, *shared* entry in nearly every enemy type's
//! routine table (the plain soldier's own `soldier_routine_ptr_tbl`
//! entry 6, among dozens of others): marks the enemy as destroyed,
//! optionally triggers the destruction sound, re-palettes its sprite,
//! and either removes it immediately (nothing left to show) or hides it
//! for one frame before the actual explosion animation
//! ([`enemy_routine_explosion`]/[`show_explosion_a`]) takes over: cycles
//! through a per-explosion-type sprite sequence, one frame every `$0a`
//! game-frames, disabling collision just before the final frame, then
//! advances to the next routine once the sequence finishes.
//!
//! `play_sound` (`$c16b`) itself isn't ported here - it's a real bank-
//! switch wrapper around the sound engine (`jsr load_bank_1; jsr
//! init_sound_code_vars; jsr local_previous_1_bank`), not a pure RAM
//! transform like everything else in this crate. This port instead
//! returns *whether and which* sound code would be triggered
//! ([`EnemyRoutineInitExplosionResult::sound`]) as plain data, the same
//! way [`crate::enemy::create_enemy_bullet`] returns a bullet's fields rather
//! than performing the spawn itself - a caller integrating this into
//! live gameplay is responsible for actually invoking the sound engine.

use crate::enemy::add_scroll_to_enemy_pos::{add_scroll_to_enemy_pos, ScrolledEnemyPos};
use crate::enemy::enemy_clear::EnemyClearFields;
use crate::enemy::enemy_collision_flags::disable_enemy_collision;
use crate::enemy::enemy_routine_transition::{advance_enemy_routine, set_enemy_delay_adv_routine, DelayedRoutineUpdate, EnemyRoutineUpdate};
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::initialize_enemy;
use crate::enemy::update_enemy_pos::{remove_enemy, RemovedEnemy};

/// `enemy_routine_init_explosion`'s real destruction sound code
/// (`sound_19`).
const EXPLOSION_SOUND: u8 = 0x19;

/// [`enemy_routine_init_explosion`]'s result once it decided the enemy
/// still had a sprite to hide (real ASM's `@continue` path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRoutineInitExplosionHidden {
    /// Always `$ff` - the real ASM's own "no visible animation frame"
    /// sentinel.
    pub enemy_frame: u8,
    /// Always `$01` - an invisible placeholder sprite code, not `$00`
    /// (which would itself mean "no sprite," the very condition this
    /// path exists to avoid re-triggering next frame).
    pub enemy_sprites: u8,
    pub scroll: ScrolledEnemyPos,
    pub delayed_routine: DelayedRoutineUpdate,
}

/// The real branch [`enemy_routine_init_explosion`] takes: nothing left
/// to show (removed immediately) or hidden for one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyRoutineInitExplosionOutcome {
    Removed(RemovedEnemy),
    Hidden(EnemyRoutineInitExplosionHidden),
}

/// The full result of one [`enemy_routine_init_explosion`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnemyRoutineInitExplosionResult {
    /// `ENEMY_STATE_WIDTH` after unconditionally setting bits 0 and 7
    /// (real ASM comment: "set boss destroyed bits").
    pub state_width: u8,
    /// `Some($19)` if the *new* `state_width`'s bit 1 is set - real ASM
    /// tests the value it just stored, not the original input.
    pub sound: Option<u8>,
    /// `ENEMY_SPRITE_ATTR` after stripping the palette bits and forcing
    /// palette 2 (`& $fc | $06`).
    pub sprite_attr: u8,
    pub outcome: EnemyRoutineInitExplosionOutcome,
}

/// Native port of `enemy_routine_init_explosion` (`$e74b`).
#[allow(clippy::too_many_arguments)]
pub fn enemy_routine_init_explosion(
    enemy_state_width: u8,
    enemy_sprite_attr: u8,
    enemy_sprites: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    current_routine: u8,
) -> EnemyRoutineInitExplosionResult {
    let state_width = enemy_state_width | 0x81;
    let sound = if state_width & 0x02 != 0 { Some(EXPLOSION_SOUND) } else { None };
    let sprite_attr = (enemy_sprite_attr & 0xFC) | 0x06;

    let outcome = if enemy_sprites == 0 {
        EnemyRoutineInitExplosionOutcome::Removed(remove_enemy())
    } else {
        let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);
        let delayed_routine = set_enemy_delay_adv_routine(0x01, current_routine);
        EnemyRoutineInitExplosionOutcome::Hidden(EnemyRoutineInitExplosionHidden {
            enemy_frame: 0xFF,
            enemy_sprites: 0x01,
            scroll,
            delayed_routine,
        })
    };

    EnemyRoutineInitExplosionResult { state_width, sound, sprite_attr, outcome }
}

/// `explosion_type_ptr_tbl` (`$e823`, 4 pointers) - the sprite-code
/// sequences an explosion can cycle through, each pointer's target
/// itself a small fixed-length real ROM table.
const EXPLOSION_TYPE_00: [u8; 3] = [0x38, 0x39, 0x3A]; // larger circular ring
const EXPLOSION_TYPE_01: [u8; 4] = [0x37, 0x35, 0x36, 0x37]; // cloudy
const EXPLOSION_TYPE_02: [u8; 3] = [0x9D, 0x9E, 0x9F]; // small ring (generated indoor soldiers)
const EXPLOSION_TYPE_03: [u8; 2] = [0x36, 0x37]; // short cloudy (rollers)

fn explosion_sprite(explosion_type: u8, enemy_frame: u8) -> u8 {
    match explosion_type {
        0 => EXPLOSION_TYPE_00[enemy_frame as usize],
        1 => EXPLOSION_TYPE_01[enemy_frame as usize],
        2 => EXPLOSION_TYPE_02[enemy_frame as usize],
        _ => EXPLOSION_TYPE_03[enemy_frame as usize],
    }
}

/// The real, branchy result of one [`show_explosion_a`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowExplosionAOutcome {
    /// `ENEMY_ROUTINE` was already `0` at entry - real ASM exits right
    /// after the scroll, touching nothing else.
    NoRoutine,
    /// Animation delay hadn't elapsed yet.
    Waiting { animation_delay: u8 },
    /// Delay elapsed and the sequence isn't done: sprite advances one
    /// frame, delay resets to `$0a`. `state_width` is `Some` only on the
    /// call that shows the *last* frame of the sequence (collision gets
    /// disabled pre-emptively right before it).
    Animating { enemy_frame: u8, animation_delay: u8, state_width: Option<u8>, enemy_sprites: u8 },
    /// Delay elapsed and the sequence just finished (this call's
    /// incremented frame reached the sequence length): advances to the
    /// next routine, nothing else in this call runs.
    Advanced(EnemyRoutineUpdate),
}

/// The full result of one [`show_explosion_a`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowExplosionAResult {
    pub scroll: ScrolledEnemyPos,
    pub outcome: ShowExplosionAOutcome,
}

/// Native port of `show_explosion_a` (`$e7bc`) - the shared explosion-
/// animation driver [`enemy_routine_explosion`] and several other real
/// callers ([`shared_enemy_routine_03`], `roller_routine_04` - not
/// ported here) all fall into with their own `(explosion_type_override,
/// max_sprites)` pair.
#[allow(clippy::too_many_arguments)]
pub fn show_explosion_a(
    explosion_type_override: u8,
    max_sprites: u8,
    enemy_state_width: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
    enemy_animation_delay: u8,
    enemy_frame: u8,
) -> ShowExplosionAResult {
    let scroll = add_scroll_to_enemy_pos(level_scrolling_type, frame_scroll, enemy_x_pos, enemy_y_pos);

    if current_routine == 0 {
        return ShowExplosionAResult { scroll, outcome: ShowExplosionAOutcome::NoRoutine };
    }

    let delay = enemy_animation_delay.wrapping_sub(1);
    if delay != 0 {
        return ShowExplosionAResult { scroll, outcome: ShowExplosionAOutcome::Waiting { animation_delay: delay } };
    }

    let new_frame = enemy_frame.wrapping_add(1);
    if new_frame >= max_sprites {
        let routine_update = advance_enemy_routine(current_routine);
        return ShowExplosionAResult { scroll, outcome: ShowExplosionAOutcome::Advanced(routine_update) };
    }

    let state_width =
        if new_frame.wrapping_add(1) >= max_sprites { Some(disable_enemy_collision(enemy_state_width)) } else { None };

    let mut explosion_type = explosion_type_override;
    if explosion_type == 0 && enemy_state_width & 0x08 != 0 {
        explosion_type = 1;
    }
    let enemy_sprites = explosion_sprite(explosion_type, new_frame);

    ShowExplosionAResult {
        scroll,
        outcome: ShowExplosionAOutcome::Animating { enemy_frame: new_frame, animation_delay: 0x0A, state_width, enemy_sprites },
    }
}

/// Native port of `enemy_routine_explosion` (`$e7b0`) - another real,
/// shared enemy-routine-table entry (the soldier's own entry 7): picks
/// the explosion sequence length from `ENEMY_STATE_WIDTH` bit 3 (`4` if
/// set, else `3`) and always lets [`show_explosion_a`] derive the
/// explosion type from that same bit (`explosion_type_override = 0`).
#[allow(clippy::too_many_arguments)]
pub fn enemy_routine_explosion(
    enemy_state_width: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
    enemy_animation_delay: u8,
    enemy_frame: u8,
) -> ShowExplosionAResult {
    let max_sprites = if enemy_state_width & 0x08 != 0 { 4 } else { 3 };
    show_explosion_a(
        0,
        max_sprites,
        enemy_state_width,
        enemy_x_pos,
        enemy_y_pos,
        level_scrolling_type,
        frame_scroll,
        current_routine,
        enemy_animation_delay,
        enemy_frame,
    )
}

/// Native port of `shared_enemy_routine_03` (`$e7aa`) - "show explosion_
/// type_02": real ASM is just `lda #$02; ldy #$03; bne show_explosion_a`,
/// i.e. [`show_explosion_a`] with a fixed `(explosion_type_override=2,
/// max_sprites=3)` pair - explosion_type_02, the small ring used for
/// generated indoor soldiers. A real, shared enemy-routine-table entry
/// itself (the indoor soldier family's own routine index 5 - see
/// [`crate::enemy::indoor_soldier`]'s module doc comment), unlike
/// [`enemy_routine_explosion`] which is the *plain* soldier's own entry.
#[allow(clippy::too_many_arguments)]
pub fn shared_enemy_routine_03(
    enemy_state_width: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    level_scrolling_type: u8,
    frame_scroll: u8,
    current_routine: u8,
    enemy_animation_delay: u8,
    enemy_frame: u8,
) -> ShowExplosionAResult {
    show_explosion_a(2, 3, enemy_state_width, enemy_x_pos, enemy_y_pos, level_scrolling_type, frame_scroll, current_routine, enemy_animation_delay, enemy_frame)
}

/// `ENEMY_TYPE` code [`create_explosion_sequence`] spawns with (real
/// comment: "pill box sensor" / "weapon box" - not actually important,
/// it's just an enemy whose only job is to run the [`enemy_routine_
/// init_explosion`]/[`show_explosion_a`]/`enemy_routine_remove_enemy`
/// sequence at a fixed position).
pub const ENEMY_TYPE_EXPLOSION_SENSOR: u8 = 0x02;

/// A spawned explosion-sequence enemy's full real field set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedExplosionEnemy {
    pub slot: u8,
    pub enemy_type: u8,
    pub hp: u8,
    /// `ENEMY_ROUTINE` - `$06` (2 rounds of explosions) or `$09` (one
    /// round), overwritten *after* `initialize_enemy`'s own `routine=1`
    /// default, unlike every other spawn helper in this crate.
    pub routine: u8,
    pub fields: EnemyClearFields,
}

/// Native port of `create_explosion_sequence` (`$eabd`) - the shared
/// core every real caller below falls into: claims a slot, initializes
/// it, then overwrites `ENEMY_ROUTINE`/`ENEMY_SPRITES`/`ENEMY_STATE_
/// WIDTH`/position with the given explosion parameters. `ENEMY_
/// ATTRIBUTES` is left at `initialize_enemy`'s own zeroed default - real
/// ASM never touches it here.
pub fn create_explosion_sequence(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    routine: u8,
    explosion_type: u8,
    x_pos: u8,
    y_pos: u8,
) -> Option<CreatedExplosionEnemy> {
    let slot = find_next_enemy_slot(enemy_routine)?;
    let init = initialize_enemy(prg_rom, ENEMY_TYPE_EXPLOSION_SENSOR, current_level);
    let mut fields = init.fields;
    fields.sprites = 1;
    fields.state_width = explosion_type;
    fields.y_pos = y_pos;
    fields.x_pos = x_pos;
    Some(CreatedExplosionEnemy { slot, enemy_type: ENEMY_TYPE_EXPLOSION_SENSOR, hp: init.hp, routine, fields })
}

/// Native port of `create_explosion_a` (`$eab9`) - `create_explosion_
/// sequence` with `ENEMY_ROUTINE` fixed to `$06` (2 rounds of
/// explosions).
pub fn create_explosion_a(prg_rom: &[u8], enemy_routine: &[u8; ENEMY_SLOT_COUNT], current_level: u8, explosion_type: u8, x_pos: u8, y_pos: u8) -> Option<CreatedExplosionEnemy> {
    create_explosion_sequence(prg_rom, enemy_routine, current_level, 0x06, explosion_type, x_pos, y_pos)
}

/// Native port of `create_enemy_for_explosion` (`$eab7`) - [`create_
/// explosion_a`] with `explosion_type` fixed to `$08`.
pub fn create_enemy_for_explosion(prg_rom: &[u8], enemy_routine: &[u8; ENEMY_SLOT_COUNT], current_level: u8, x_pos: u8, y_pos: u8) -> Option<CreatedExplosionEnemy> {
    create_explosion_a(prg_rom, enemy_routine, current_level, 0x08, x_pos, y_pos)
}

/// Native port of `create_two_explosion_89` (`$eab3`) - [`create_
/// explosion_a`] with `explosion_type` fixed to `$89`.
pub fn create_two_explosion_89(prg_rom: &[u8], enemy_routine: &[u8; ENEMY_SLOT_COUNT], current_level: u8, x_pos: u8, y_pos: u8) -> Option<CreatedExplosionEnemy> {
    create_explosion_a(prg_rom, enemy_routine, current_level, 0x89, x_pos, y_pos)
}

/// [`play_explosion_sound`]'s real destruction sound code (`sound_19`) -
/// same real sound code [`enemy_routine_init_explosion`] can also play,
/// coincidentally.
const PLAY_EXPLOSION_SOUND_CODE: u8 = 0x19;

/// The full result of one [`play_explosion_sound`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayExplosionSoundResult {
    /// Always `$19` - see this module's own doc comment on why `play_
    /// sound` itself isn't ported.
    pub sound: u8,
    pub explosion: Option<CreatedExplosionEnemy>,
    /// `ENEMY_ATTRIBUTES & 7` - the specific weapon item type to create,
    /// written back to the *calling* enemy's own slot (real ASM: `sta
    /// ENEMY_ATTRIBUTES,x`, not a new enemy).
    pub attributes: u8,
    /// [`crate::enemy::enemy_clear::clear_sprite_and_pt_3`]'s own output -
    /// the fields it zeroes, applied to the *calling* enemy's own slot.
    pub cleared: EnemyClearFields,
    /// Always `1` - `ENEMY_ROUTINE`, matching `weapon_item_routine_ptr_
    /// tbl`'s own off-by-one convention.
    pub routine: u8,
    /// Always `0` - `ENEMY_TYPE`, converting the calling enemy's own
    /// slot into a weapon item drop in place.
    pub enemy_type: u8,
}

/// Native port of `play_explosion_sound` (`$82d4`) - plays the
/// destruction sound, spawns a 2-round `$89`-type explosion at the
/// calling enemy's own position (`set_08_09_to_enemy_pos`'s zero-offset
/// case), then converts the calling enemy's own slot in place into a
/// weapon item drop. `enemy_attributes` here is whatever the real ASM's
/// `ENEMY_ATTRIBUTES,x` holds *at the point this is called* - real
/// callers may have already transformed it first (see [`crate::enemy::
/// jumping_soldier::jumping_soldier_routine_04`]'s own doc comment for
/// one that does).
pub fn play_explosion_sound(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    enemy_attributes: u8,
) -> PlayExplosionSoundResult {
    let explosion = create_two_explosion_89(prg_rom, enemy_routine, current_level, enemy_x_pos, enemy_y_pos);
    let attributes = enemy_attributes & 0x07;
    let mut cleared = EnemyClearFields::default();
    crate::enemy::enemy_clear::clear_sprite_and_pt_3(&mut cleared);
    PlayExplosionSoundResult { sound: PLAY_EXPLOSION_SOUND_CODE, explosion, attributes, cleared, routine: 1, enemy_type: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_the_destroyed_bits_regardless_of_input() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.state_width, 0x81);
    }

    #[test]
    fn plays_the_explosion_sound_when_bit_1_of_the_new_state_width_is_set() {
        // input state_width already has bit 1 set -> survives the |=0x81.
        let r = enemy_routine_init_explosion(0x02, 0x00, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.state_width, 0x83);
        assert_eq!(r.sound, Some(0x19));
    }

    #[test]
    fn no_sound_when_bit_1_is_clear() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.sound, None);
    }

    #[test]
    fn overrides_sprite_attr_palette_to_2_preserving_other_bits() {
        let r = enemy_routine_init_explosion(0x00, 0b0011_0101, 0x01, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.sprite_attr, 0b0011_0110);
    }

    #[test]
    fn removes_immediately_when_no_sprite_is_present() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x00, 0, 0x00, 0x50, 0x60, 3);
        assert_eq!(r.outcome, EnemyRoutineInitExplosionOutcome::Removed(remove_enemy()));
    }

    #[test]
    fn hides_and_schedules_the_next_frame_when_a_sprite_is_present() {
        let r = enemy_routine_init_explosion(0x00, 0x00, 0x05, 0, 0x02, 0x50, 0x60, 3);
        match r.outcome {
            EnemyRoutineInitExplosionOutcome::Hidden(h) => {
                assert_eq!(h.enemy_frame, 0xFF);
                assert_eq!(h.enemy_sprites, 0x01);
                assert_eq!(h.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
                assert_eq!(h.delayed_routine, set_enemy_delay_adv_routine(0x01, 3));
            }
            other => panic!("expected Hidden, got {other:?}"),
        }
    }

    #[test]
    fn no_routine_exits_after_the_scroll_with_nothing_else_touched() {
        let r = show_explosion_a(0, 3, 0x00, 0x50, 0x60, 0, 0x02, 0, 0x0A, 0x00);
        assert_eq!(r.scroll, add_scroll_to_enemy_pos(0, 0x02, 0x50, 0x60));
        assert_eq!(r.outcome, ShowExplosionAOutcome::NoRoutine);
    }

    #[test]
    fn waits_when_delay_has_not_elapsed() {
        let r = show_explosion_a(0, 3, 0x00, 0x50, 0x60, 0, 0x02, 3, 0x05, 0x00);
        assert_eq!(r.outcome, ShowExplosionAOutcome::Waiting { animation_delay: 0x04 });
    }

    #[test]
    fn advances_to_the_next_routine_once_the_sequence_is_fully_shown() {
        // max_sprites=3, enemy_frame=2 -> new_frame=3 >= 3, sequence done.
        let r = show_explosion_a(0, 3, 0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x02);
        assert_eq!(r.outcome, ShowExplosionAOutcome::Advanced(advance_enemy_routine(3)));
    }

    #[test]
    fn animates_the_middle_frame_without_disabling_collision_yet() {
        // max_sprites=3, enemy_frame=0 -> new_frame=1, new_frame+1=2 < 3: not the last frame.
        let r = show_explosion_a(0, 3, 0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        match r.outcome {
            ShowExplosionAOutcome::Animating { enemy_frame, animation_delay, state_width, enemy_sprites } => {
                assert_eq!(enemy_frame, 1);
                assert_eq!(animation_delay, 0x0A);
                assert_eq!(state_width, None);
                assert_eq!(enemy_sprites, EXPLOSION_TYPE_00[1]);
            }
            other => panic!("expected Animating, got {other:?}"),
        }
    }

    #[test]
    fn animates_the_last_frame_and_disables_collision_first() {
        // max_sprites=3, enemy_frame=0 -> new_frame=1, new_frame+1=2 >= max_sprites(2)? use max_sprites=2 to hit this.
        let r = show_explosion_a(0, 2, 0xFF, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        match r.outcome {
            ShowExplosionAOutcome::Animating { enemy_frame, state_width, .. } => {
                assert_eq!(enemy_frame, 1);
                assert_eq!(state_width, Some(disable_enemy_collision(0xFF)));
            }
            other => panic!("expected Animating, got {other:?}"),
        }
    }

    #[test]
    fn explosion_type_override_bypasses_the_state_width_check() {
        let r = show_explosion_a(3, 2, 0x08 /* bit 3 set, would normally mean type 1 */, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        match r.outcome {
            ShowExplosionAOutcome::Animating { enemy_sprites, .. } => {
                assert_eq!(enemy_sprites, EXPLOSION_TYPE_03[1]);
            }
            other => panic!("expected Animating, got {other:?}"),
        }
    }

    #[test]
    fn zero_override_derives_type_from_state_width_bit_3() {
        let without_bit3 = show_explosion_a(0, 3, 0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        let with_bit3 = show_explosion_a(0, 4, 0x08, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        match (without_bit3.outcome, with_bit3.outcome) {
            (ShowExplosionAOutcome::Animating { enemy_sprites: a, .. }, ShowExplosionAOutcome::Animating { enemy_sprites: b, .. }) => {
                assert_eq!(a, EXPLOSION_TYPE_00[1]);
                assert_eq!(b, EXPLOSION_TYPE_01[1]);
            }
            other => panic!("expected both Animating, got {other:?}"),
        }
    }

    #[test]
    fn shared_routine_03_matches_show_explosion_a_with_type_2_and_3_sprites() {
        let r = shared_enemy_routine_03(0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        let expected = show_explosion_a(2, 3, 0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        assert_eq!(r, expected);
        match r.outcome {
            ShowExplosionAOutcome::Animating { enemy_sprites, .. } => assert_eq!(enemy_sprites, EXPLOSION_TYPE_02[1]),
            other => panic!("expected Animating, got {other:?}"),
        }
    }

    #[test]
    fn shared_routine_03_ignores_state_width_bit_3_unlike_the_zero_override_case() {
        // explosion_type_override=2 is fixed, so bit 3 of state_width (which would
        // normally switch type 0 -> type 1) has no effect here.
        let without_bit3 = shared_enemy_routine_03(0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        let with_bit3 = shared_enemy_routine_03(0x08, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x00);
        match (without_bit3.outcome, with_bit3.outcome) {
            (ShowExplosionAOutcome::Animating { enemy_sprites: a, .. }, ShowExplosionAOutcome::Animating { enemy_sprites: b, .. }) => {
                assert_eq!(a, b);
            }
            other => panic!("expected both Animating, got {other:?}"),
        }
    }

    #[test]
    fn enemy_routine_explosion_picks_max_sprites_from_state_width_bit_3() {
        let r3 = enemy_routine_explosion(0x00, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x01);
        // frame 1 -> new_frame 2, 2 < 3 (max_sprites=3): still animating.
        assert!(matches!(r3.outcome, ShowExplosionAOutcome::Animating { .. }));
        let r4 = enemy_routine_explosion(0x08, 0x50, 0x60, 0, 0x02, 3, 0x01, 0x02);
        // frame 2 -> new_frame 3, 3 < 4 (max_sprites=4): still animating (would have
        // advanced already if max_sprites were still 3).
        assert!(matches!(r4.outcome, ShowExplosionAOutcome::Animating { .. }));
    }

    /// Shared property table record for `ENEMY_TYPE_EXPLOSION_SENSOR`
    /// (`$02 < $10`), same shape as `create_enemy_bullet`'s own fixture.
    fn synthetic_prg_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + ENEMY_TYPE_EXPLOSION_SENSOR as usize * 4;
        rom[record_off..record_off + 4].copy_from_slice(&[0x80, 0x00, 0x05, 0x00]);
        rom
    }

    #[test]
    fn create_explosion_sequence_sets_routine_sprites_and_position() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = create_explosion_sequence(&rom, &routine, 0, 0x09, 0x08, 0x50, 0x60).unwrap();
        assert_eq!(created.enemy_type, ENEMY_TYPE_EXPLOSION_SENSOR);
        assert_eq!(created.routine, 0x09);
        assert_eq!(created.hp, 0x05);
        assert_eq!(created.fields.sprites, 1);
        assert_eq!(created.fields.state_width, 0x08);
        assert_eq!(created.fields.x_pos, 0x50);
        assert_eq!(created.fields.y_pos, 0x60);
        assert_eq!(created.fields.attributes, 0); // never touched
    }

    #[test]
    fn create_explosion_sequence_returns_none_without_a_free_slot() {
        let rom = synthetic_prg_rom();
        let full = [1u8; ENEMY_SLOT_COUNT];
        assert_eq!(create_explosion_sequence(&rom, &full, 0, 0x09, 0x08, 0x50, 0x60), None);
    }

    #[test]
    fn create_explosion_a_fixes_routine_to_6() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = create_explosion_a(&rom, &routine, 0, 0x89, 0x50, 0x60).unwrap();
        assert_eq!(created.routine, 0x06);
        assert_eq!(created.fields.state_width, 0x89);
    }

    #[test]
    fn create_enemy_for_explosion_fixes_type_to_8() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = create_enemy_for_explosion(&rom, &routine, 0, 0x50, 0x60).unwrap();
        assert_eq!(created.fields.state_width, 0x08);
        assert_eq!(created.routine, 0x06);
    }

    #[test]
    fn create_two_explosion_89_fixes_type_to_0x89() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = create_two_explosion_89(&rom, &routine, 0, 0x50, 0x60).unwrap();
        assert_eq!(created.fields.state_width, 0x89);
        assert_eq!(created.routine, 0x06);
    }

    #[test]
    fn play_explosion_sound_composes_explosion_weapon_type_and_clear() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = play_explosion_sound(&rom, &routine, 0, 0x50, 0x60, 0b0000_0101);
        assert_eq!(r.sound, 0x19);
        assert_eq!(r.attributes, 0x05); // 0b101 & 7
        assert_eq!(r.routine, 1);
        assert_eq!(r.enemy_type, 0);
        let explosion = r.explosion.unwrap();
        assert_eq!(explosion.fields.state_width, 0x89);
        assert_eq!(explosion.fields.x_pos, 0x50);
        assert_eq!(explosion.fields.y_pos, 0x60);
        assert_eq!(explosion.routine, 0x06);
        // clear_sprite_and_pt_3's own output, cross-checked directly.
        let mut expected_cleared = EnemyClearFields::default();
        crate::enemy::enemy_clear::clear_sprite_and_pt_3(&mut expected_cleared);
        assert_eq!(r.cleared, expected_cleared);
    }

    #[test]
    fn play_explosion_sound_masks_attributes_to_the_low_3_bits() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let r = play_explosion_sound(&rom, &routine, 0, 0x50, 0x60, 0b1111_1111);
        assert_eq!(r.attributes, 0x07);
    }
}
