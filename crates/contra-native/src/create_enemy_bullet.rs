//! Native port of `@create_enemy_bullet`/`set_bullet_velocities`/
//! `bullet_gen_exit` (`src/bank7.asm`, CPU `$f2e4`-`$f333`) - the real
//! "spawn an enemy bullet object" routine, composing three already-
//! ported building blocks exactly the way the real ASM does:
//! [`crate::enemy_slots::find_next_enemy_slot`] (claim a slot),
//! [`crate::initialize_enemy::initialize_enemy`] (set up its baseline
//! fields), then [`crate::bullet_physics::calc_bullet_velocities`]
//! (compute and write its velocity). A capstone of sorts - the first
//! port in this crate that's *purely* composition of prior, independently
//! live-verified routines, with no new 6502 control flow of its own
//! beyond a couple of small caller-side transforms (see below).
//!
//! ## Two caller-side transforms, not delegated to the routines below
//!
//! - `bullet_type_and_angle` packs the bullet's type in bits 5-7
//!   (`>> 5`, stored to `ENEMY_VAR_1`) and its quadrant aim direction in
//!   bits 0-4 (`& 0x1f`, handed to `calc_bullet_velocities`).
//! - `speed_code` is **saturated** to a max of 7 here (`cmp #$07 / bcc /
//!   lda #$07`) - not masked. This is a *different, narrower* clamp than
//!   [`crate::bullet_physics::adjust_bullet_velocity`]'s own internal
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

use crate::bullet_physics::calc_bullet_velocities;
use crate::enemy_clear::EnemyClearFields;
use crate::enemy_slots::{find_next_enemy_slot, ENEMY_SLOT_COUNT};
use crate::initialize_enemy::initialize_enemy;

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
}
