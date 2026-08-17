//! Native port of Contra's enemy-bullet creation chain, `src/bank7.asm`,
//! CPU `$f29e`-`$f333`: [`create_enemy_bullet`] (`@create_enemy_bullet`/
//! `set_bullet_velocities`/`bullet_gen_exit`) is the real "spawn an enemy
//! bullet object" routine, composing three already-ported building
//! blocks exactly the way the real ASM does:
//! [`crate::enemy::enemy_slots::find_next_enemy_slot`] (claim a slot),
//! [`crate::enemy::initialize_enemy::initialize_enemy`] (set up its baseline
//! fields), then [`crate::physics::bullet_physics::calc_bullet_velocities`]
//! (compute and write its velocity) - the first port in this crate
//! that's *purely* composition of prior, independently live-verified
//! routines, with no new 6502 control flow of its own beyond a couple of
//! small caller-side transforms (see below). [`create_enemy_bullet_if_attack_enabled`]
//! gates on `ENEMY_ATTACK_FLAG` (except one always-fires bullet type)
//! before delegating to [`create_enemy_bullet`] - shared by two real
//! callers: [`create_enemy_bullet_angle_a`] (computes the aim quadrant
//! from a raw angle byte via [`quadrant_from_angle`]) and
//! [`aim_and_create_enemy_bullet`] (resolves aim by targeting a player,
//! via [`crate::enemy::quadrant_aim_dir`], instead of using a fixed angle).
//!
//! ## `aim_and_create_enemy_bullet`'s live-verification gap, and an
//! apparently-unreachable branch inside it
//!
//! Unlike every other routine in this crate, `aim_and_create_enemy_
//! bullet` has exactly **one** real caller in the whole ROM:
//! `dragon_arm_orb_fire_projectile` (`bank0.asm`), the level 3 dragon
//! boss's arm-orb attack. That's not reachable by this project's current
//! scripted playthrough (a basic level-1 walk-and-shoot), so this
//! routine's own live-verification hook (`VERIFY_AIM_AND_CREATE_ENEMY_
//! BULLET=1`) has 0 real hits so far - confidence instead rests on this
//! module's own unit tests cross-checking its composition against
//! `get_quadrant_aim_dir`/`get_quadrant_aim_dir_for_player`'s own
//! (already independently live-verified) outputs, plus every building
//! block it calls already being separately live-verified in its own
//! right. Additionally: that one real caller always sets `$0a` (this
//! port's `aim_target`) via `sty $0a` from `player_enemy_x_dist`'s own
//! output, documented as "#$00 or #$01" - i.e. always bit-7-clear -
//! meaning `aim_and_create_enemy_bullet`'s "direct target" branch (bit 7
//! set, aim at an already-known fixed position) appears to be
//! **unreachable by any real code in the ROM**, the same category as
//! `adjust_bullet_velocity`'s speed code 8 or the dead-code top-level
//! `clear_enemy` label - ported faithfully regardless (it's still real,
//! valid 6502 control flow, just apparently never exercised), not
//! removed or "corrected".
//!
//! ## Two caller-side transforms, not delegated to the routines below
//!
//! - `bullet_type_and_angle` packs the bullet's type in bits 5-7
//!   (`>> 5`, stored to `ENEMY_VAR_1`) and its quadrant aim direction in
//!   bits 0-4 (`& 0x1f`, handed to `calc_bullet_velocities`).
//! - `speed_code` is **saturated** to a max of 7 here (`cmp #$07 / bcc /
//!   lda #$07`) - not masked. This is a *different, narrower* clamp than
//!   [`crate::physics::bullet_physics::adjust_bullet_velocity`]'s own internal
//!   `& 0x07`: a value that reaches `adjust_bullet_velocity` through
//!   *this* caller can never actually be 8 or higher, so it can never
//!   trigger that function's own wrap-to-0 masking behavior - only a
//!   caller that bypasses this saturation (there are others in the ROM;
//!   not ported here) could ever do that.
//!
//! ## What this port's `Option<CreatedBullet>` return doesn't literally
//! match about the real registers
//!
//! The real routine's own caller only gets a zero-flag success/failure
//! signal back (`a` = `$00`/`$01`) - on *both* paths it finishes with
//! `ldx ENEMY_CURRENT_SLOT`, discarding whichever slot `find_next_enemy_
//! slot` actually found before returning. This port's `Some(bullet).slot`
//! is real, useful information a Rust caller obviously wants (and this
//! module's own live-verification hook derives it independently, from
//! the same `ENEMY_ROUTINE` snapshot the real routine used, rather than
//! trying to read a register that's already been overwritten by the time
//! the real routine returns) - just not something the literal 6502
//! caller convention hands back.

use crate::physics::bullet_physics::calc_bullet_velocities;
use crate::enemy::enemy_clear::EnemyClearFields;
use crate::enemy::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::enemy::initialize_enemy::initialize_enemy;
use crate::enemy::quadrant_aim_dir::{get_quadrant_aim_dir, get_quadrant_aim_dir_for_player, QUADRANT_AIM_DIR_01};

/// `ENEMY_TYPE` value real bullets are created with (`bank7.asm`'s own
/// `lda #$01 ; sta ENEMY_TYPE,x`).
pub const ENEMY_TYPE_BULLET: u8 = 1;

/// A successfully created enemy bullet's full real field set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedBullet {
    pub slot: u8,
    pub enemy_type: u8,
    pub hp: u8,
    pub fields: EnemyClearFields,
}

/// Native port of `@create_enemy_bullet` through `set_bullet_velocities`/
/// `bullet_gen_exit`. `None` if no enemy slot was free (real: `a=$01`,
/// nothing written); `Some` on success (real: `a=$00`).
#[allow(clippy::too_many_arguments)]
pub fn create_enemy_bullet(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    bullet_type_and_angle: u8,
    speed_code: u8,
    quadrant: u8,
    y_pos: u8,
    x_pos: u8,
) -> Option<CreatedBullet> {
    let slot = find_next_enemy_slot(enemy_routine)?;

    let init = initialize_enemy(prg_rom, ENEMY_TYPE_BULLET, current_level);
    let mut fields = init.fields;
    fields.var_1 = bullet_type_and_angle >> 5;

    let clamped_speed = speed_code.min(7);
    let aim_dir = bullet_type_and_angle & 0x1f;
    let v = calc_bullet_velocities(aim_dir, clamped_speed, quadrant);
    fields.y_pos = y_pos;
    fields.x_pos = x_pos;
    fields.y_velocity_fract = v.frac_y;
    fields.y_velocity_fast = v.fast_y;
    fields.x_velocity_fract = v.frac_x;
    fields.x_velocity_fast = v.fast_x;

    Some(CreatedBullet { slot, enemy_type: ENEMY_TYPE_BULLET, hp: init.hp, fields })
}

/// `bullet_type_and_angle`'s top-3-bits value (`& 0xe0`) that always
/// fires regardless of `ENEMY_ATTACK_FLAG` - the level 1 boss's large
/// cannonball (type `1`, i.e. `0x20` once shifted into bits 5-7).
const ALWAYS_FIRE_TYPE_BITS: u8 = 0x20;

/// Native port of `create_enemy_bullet_angle_a`'s own quadrant
/// computation (`bank7.asm`, `$f2bf`-`$f2d7`) - the one piece of new
/// logic in that routine [`create_enemy_bullet`] doesn't already cover
/// (which takes `quadrant` as a given input rather than computing it):
/// buckets the angle (bits 0-4 of `bullet_type_and_angle`) into one of
/// the 4 quadrant codes via two chained unsigned comparisons, exactly
/// mirroring the real `cmp #$07`/`cmp #$12`/`cmp #$0d` sequence rather
/// than a derived closed-form bucket list.
pub fn quadrant_from_angle(bullet_type_and_angle: u8) -> u8 {
    let angle = bullet_type_and_angle & 0x1f;
    let mut y = if (7..18).contains(&angle) { 2 } else { 0 };
    if angle >= 13 {
        y += 1;
    }
    y
}

/// Native port of `create_enemy_bullet_if_attack_enabled` (`bank7.asm`,
/// `$f2d8`-`$f2e3`) - the shared gate both real upstream callers
/// ([`create_enemy_bullet_angle_a`] and [`aim_and_create_enemy_bullet`])
/// jump into: only actually spawns the bullet if `enemy_attack_flag` is
/// nonzero, *or* the bullet's type is the one that always fires
/// regardless (see [`ALWAYS_FIRE_TYPE_BITS`] - the level 1 boss's large
/// cannonball).
#[allow(clippy::too_many_arguments)]
pub fn create_enemy_bullet_if_attack_enabled(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    bullet_type_and_angle: u8,
    speed_code: u8,
    quadrant: u8,
    y_pos: u8,
    x_pos: u8,
) -> Option<CreatedBullet> {
    let always_fires = bullet_type_and_angle & 0xe0 == ALWAYS_FIRE_TYPE_BITS;
    if !always_fires && enemy_attack_flag == 0 {
        return None;
    }
    create_enemy_bullet(prg_rom, enemy_routine, current_level, bullet_type_and_angle, speed_code, quadrant, y_pos, x_pos)
}

/// Native port of `create_enemy_bullet_angle_a` (`bank7.asm`,
/// `$f2bf`-`$f2d7`) - computes the quadrant from the angle via
/// [`quadrant_from_angle`], then delegates to
/// [`create_enemy_bullet_if_attack_enabled`].
#[allow(clippy::too_many_arguments)]
pub fn create_enemy_bullet_angle_a(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    bullet_type_and_angle: u8,
    speed_code: u8,
    y_pos: u8,
    x_pos: u8,
) -> Option<CreatedBullet> {
    let quadrant = quadrant_from_angle(bullet_type_and_angle);
    create_enemy_bullet_if_attack_enabled(
        prg_rom,
        enemy_routine,
        current_level,
        enemy_attack_flag,
        bullet_type_and_angle,
        speed_code,
        quadrant,
        y_pos,
        x_pos,
    )
}

/// Native port of `aim_and_create_enemy_bullet` (`bank7.asm`,
/// `$f29e`-`$f2b3`) - the real top-level "figure out where to aim and
/// spawn the bullet" entry point enemy AI calls. Always aims using
/// `quadrant_aim_dir_01` (the real routine hardcodes the table selector
/// to `1` before either aiming call below - a genuine, deliberate
/// restriction of this specific caller, not a limitation of the aiming
/// routines themselves).
///
/// `aim_target` packs a dual-purpose real 6502 value, exactly matching
/// the real ASM's own reuse of `$0a` for two different things depending
/// on a sign check: bit 7 **clear** means "resolve via player state" -
/// `aim_target & 1` is the player index, handed straight to
/// [`crate::enemy::quadrant_aim_dir::get_quadrant_aim_dir_for_player`]; bit 7
/// **set** means "aim at an already-known fixed target position"
/// instead, bypassing player-state resolution entirely and using
/// `direct_target_y`/`direct_target_x` (the real ASM's own `$0c`/`$0b`)
/// with [`crate::enemy::quadrant_aim_dir::get_quadrant_aim_dir`] directly - the
/// path the level 3 dragon boss's arm-orb projectiles take.
#[allow(clippy::too_many_arguments)]
pub fn aim_and_create_enemy_bullet(
    prg_rom: &[u8],
    enemy_routine: &[u8; ENEMY_SLOT_COUNT],
    current_level: u8,
    enemy_attack_flag: u8,
    bullet_type: u8,
    speed_code: u8,
    source_y: u8,
    source_x: u8,
    aim_target: u8,
    direct_target_x: u8,
    direct_target_y: u8,
    player_state: [u8; 2],
    sprite_y_pos: [u8; 2],
    sprite_x_pos: [u8; 2],
    level_location_type: u8,
) -> Option<CreatedBullet> {
    let aimed = if aim_target & 0x80 != 0 {
        get_quadrant_aim_dir(source_y, source_x, direct_target_y, direct_target_x, &QUADRANT_AIM_DIR_01)
    } else {
        get_quadrant_aim_dir_for_player(
            source_y,
            source_x,
            aim_target,
            player_state,
            sprite_y_pos,
            sprite_x_pos,
            level_location_type,
            &QUADRANT_AIM_DIR_01,
        )
    };

    let bullet_type_and_angle = bullet_type | aimed.aim_dir;
    create_enemy_bullet_if_attack_enabled(
        prg_rom,
        enemy_routine,
        current_level,
        enemy_attack_flag,
        bullet_type_and_angle,
        speed_code,
        aimed.quadrant,
        source_y,
        source_x,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_prg_rom() -> Vec<u8> {
        // Mirrors `initialize_enemy`'s own synthetic-ROM test: a shared
        // property-table pointer (bullets are ENEMY_TYPE=1, always <
        // $10, so always the shared path) with a recognizable record at
        // enemy_type=1's offset (1*4=4).
        let mut rom = vec![0u8; 8 * 0x4000];
        let ptr_tbl_off = 7 * 0x4000 + (0xEE8D_usize - 0xC000);
        let shared_table_addr: u16 = 0xEF00;
        rom[ptr_tbl_off + 0x10..ptr_tbl_off + 0x12].copy_from_slice(&shared_table_addr.to_le_bytes());
        let record_off = 7 * 0x4000 + (shared_table_addr as usize - 0xC000) + 4;
        rom[record_off..record_off + 4].copy_from_slice(&[0x80, 0x00, 0x01, 0x00]);
        rom
    }

    #[test]
    fn no_free_slot_returns_none() {
        let rom = synthetic_prg_rom();
        let full = [1u8; ENEMY_SLOT_COUNT];
        assert_eq!(create_enemy_bullet(&rom, &full, 0, 0x00, 3, 0, 10, 20), None);
    }

    #[test]
    fn creates_a_bullet_in_the_first_free_slot_with_the_right_fields() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[9] = 0; // only slot 9 free
        // bullet_type_and_angle = $23: type = $23>>5 = 1, angle = $23&$1f = 3
        let created = create_enemy_bullet(&rom, &routine, 0, 0x23, 3, 0, 0x40, 0x60).unwrap();
        assert_eq!(created.slot, 9);
        assert_eq!(created.enemy_type, ENEMY_TYPE_BULLET);
        assert_eq!(created.hp, 0x01); // from the synthetic property record
        assert_eq!(created.fields.var_1, 1);
        assert_eq!(created.fields.y_pos, 0x40);
        assert_eq!(created.fields.x_pos, 0x60);
        assert_eq!(created.fields.sprites, 1); // initialize_enemy's own doing
        // velocity fields match calc_bullet_velocities(3, 3, 0) directly
        let expected_v = calc_bullet_velocities(3, 3, 0);
        assert_eq!(created.fields.y_velocity_fract, expected_v.frac_y);
        assert_eq!(created.fields.x_velocity_fract, expected_v.frac_x);
    }

    #[test]
    fn speed_code_saturates_at_7_rather_than_wrapping() {
        let rom = synthetic_prg_rom();
        let mut routine = [1u8; ENEMY_SLOT_COUNT];
        routine[0] = 0;
        // speed_code=8 here must behave like 7 (saturation), NOT like 0
        // (which is what `adjust_bullet_velocity`'s own internal `&0x07`
        // masking alone would give it).
        let with_8 = create_enemy_bullet(&rom, &routine, 0, 0x00, 8, 0, 0, 0).unwrap();
        let with_7 = create_enemy_bullet(&rom, &routine, 0, 0x00, 7, 0, 0, 0).unwrap();
        let with_0 = create_enemy_bullet(&rom, &routine, 0, 0x00, 0, 0, 0, 0).unwrap();
        assert_eq!(with_8.fields.x_velocity_fract, with_7.fields.x_velocity_fract);
        assert_eq!(with_8.fields.x_velocity_fast, with_7.fields.x_velocity_fast);
        // aim_dir=0's X base ($ff) is nonzero, so speed 0 (halved) vs. 7
        // actually differ - a meaningful check, unlike Y's base ($00).
        assert_ne!(with_8.fields.x_velocity_fract, with_0.fields.x_velocity_fract);
    }

    #[test]
    fn quadrant_from_angle_matches_the_real_boundaries() {
        // Boundaries per the real cmp #$07/#$12/#$0d sequence: [0,7)->0,
        // [7,13)->2, [13,18)->3, [18,32)->1 (angle masked to 5 bits, so
        // this covers the full domain including values past the table's
        // real 0-23 range).
        for angle in 0..7u8 {
            assert_eq!(quadrant_from_angle(angle), 0, "angle={angle}");
        }
        for angle in 7..13u8 {
            assert_eq!(quadrant_from_angle(angle), 2, "angle={angle}");
        }
        for angle in 13..18u8 {
            assert_eq!(quadrant_from_angle(angle), 3, "angle={angle}");
        }
        for angle in 18..32u8 {
            assert_eq!(quadrant_from_angle(angle), 1, "angle={angle}");
        }
    }

    #[test]
    fn quadrant_from_angle_only_looks_at_the_low_5_bits() {
        // A bullet-type-and-angle byte with type bits set (e.g. $23 =
        // type 1, angle 3) must compute the same quadrant as angle 3
        // alone.
        assert_eq!(quadrant_from_angle(0x23), quadrant_from_angle(0x03));
    }

    #[test]
    fn angle_a_does_not_fire_when_attack_flag_clear_and_not_always_fire_type() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT]; // plenty of free slots
        // type bits = 0x00 (regular bullet, not the always-fire cannonball)
        assert_eq!(create_enemy_bullet_angle_a(&rom, &routine, 0, 0, 0x00, 3, 10, 20), None);
    }

    #[test]
    fn angle_a_fires_when_attack_flag_set() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let result = create_enemy_bullet_angle_a(&rom, &routine, 0, 1, 0x00, 3, 10, 20);
        assert!(result.is_some());
    }

    #[test]
    fn angle_a_always_fires_for_the_level_1_boss_cannonball_type_regardless_of_attack_flag() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // type bits = 0x20 (level 1 boss large cannonball), attack flag clear
        let result = create_enemy_bullet_angle_a(&rom, &routine, 0, 0, 0x20, 3, 10, 20);
        assert!(result.is_some());
    }

    #[test]
    fn aim_and_create_direct_target_branch_matches_get_quadrant_aim_dir_directly() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        // aim_target bit7 set -> direct-target branch, ignores player_state.
        let created = aim_and_create_enemy_bullet(
            &rom,
            &routine,
            0,
            1, // attack flag on
            0x00,
            3,
            0x50, // source_y
            0x50, // source_x
            0x80, // aim_target: bit7 set -> direct target sentinel
            0x90, // direct_target_x
            0x30, // direct_target_y
            [0, 0],
            [0, 0],
            [0, 0],
            0,
        )
        .unwrap();
        let direct = get_quadrant_aim_dir(0x50, 0x50, 0x30, 0x90, &QUADRANT_AIM_DIR_01);
        assert_eq!(created.fields.var_1, 0); // bullet_type=0 >> 5 (angle merged separately below)
        // the merged bullet_type_and_angle's low nibble must equal the aim dir
        // (recoverable since type bits/aim bits don't overlap): re-derive via
        // a fresh angle_a call using the same aim dir and confirm identical
        // velocity output as an indirect cross-check.
        let via_angle_a = create_enemy_bullet_angle_a(&rom, &routine, 0, 1, direct.aim_dir, 3, 0x50, 0x50).unwrap();
        assert_eq!(created.fields.x_velocity_fract, via_angle_a.fields.x_velocity_fract);
    }

    #[test]
    fn aim_and_create_player_index_branch_uses_get_quadrant_aim_dir_for_player() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let created = aim_and_create_enemy_bullet(
            &rom,
            &routine,
            0,
            1,
            0x00,
            3,
            0x50,
            0x50,
            0x00, // aim_target: bit7 clear -> player index 0
            0x00,
            0x00,
            [1, 0],       // player 1 normal
            [0x30, 0x00], // player 1 Y
            [0x90, 0x00], // player 1 X
            0,
        )
        .unwrap();
        let via_for_player =
            get_quadrant_aim_dir_for_player(0x50, 0x50, 0x00, [1, 0], [0x30, 0x00], [0x90, 0x00], 0, &QUADRANT_AIM_DIR_01);
        let via_angle_a = create_enemy_bullet_angle_a(&rom, &routine, 0, 1, via_for_player.aim_dir, 3, 0x50, 0x50).unwrap();
        assert_eq!(created.fields.x_velocity_fract, via_angle_a.fields.x_velocity_fract);
    }

    #[test]
    fn aim_and_create_respects_the_attack_flag_gate() {
        let rom = synthetic_prg_rom();
        let routine = [0u8; ENEMY_SLOT_COUNT];
        let result =
            aim_and_create_enemy_bullet(&rom, &routine, 0, 0, 0x00, 3, 0x50, 0x50, 0x00, 0, 0, [0, 0], [0, 0], [0, 0], 0);
        assert_eq!(result, None);
    }
}
