//! Native port of Contra's enemy collision-flag toggles (`src/bank7.asm`,
//! CPU `$eb03`-`$eb1e`) - 5 tiny real routines that set or clear two
//! bits of `ENEMY_STATE_WIDTH`: bit 0 (player-enemy collision: `0` =
//! checked, `1` = skipped) and bit 7 (bullet-enemy collision: `1` =
//! bullets pass through, `0` = they collide, e.g. a weapon item bullets
//! shouldn't be able to destroy). Combined, **31 real call sites** across
//! `bank0.asm`/`bank7.asm` - small, but reused by nearly every enemy
//! type's spawn/death/invulnerability handling.

/// `disable_bullet_enemy_collision` (`$eb03`) - sets bit 7 (bullets pass
/// through this enemy).
pub fn disable_bullet_enemy_collision(state_width: u8) -> u8 {
    state_width | 0x80
}

/// `disable_enemy_collision` (`$eb07`) - sets bits 0 and 7 (neither
/// bullets nor the player collide with this enemy).
pub fn disable_enemy_collision(state_width: u8) -> u8 {
    state_width | 0x81
}

/// `enable_enemy_player_collision_check` (`$eb0e`) - clears bit 0
/// (player-enemy collision is checked again).
pub fn enable_enemy_player_collision_check(state_width: u8) -> u8 {
    state_width & 0xFE
}

/// `enable_bullet_enemy_collision` (`$eb12`) - clears bit 7 (bullets
/// collide with this enemy again).
pub fn enable_bullet_enemy_collision(state_width: u8) -> u8 {
    state_width & 0x7F
}

/// `enable_enemy_collision` (`$eb16`) - clears both bits 0 and 7 (full
/// collision restored, both player and bullets).
pub fn enable_enemy_collision(state_width: u8) -> u8 {
    state_width & 0x7E
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disable_bullet_enemy_collision_only_sets_bit_7() {
        assert_eq!(disable_bullet_enemy_collision(0x00), 0x80);
        assert_eq!(disable_bullet_enemy_collision(0x01), 0x81); // bit 0 untouched
        assert_eq!(disable_bullet_enemy_collision(0xFF), 0xFF);
    }

    #[test]
    fn disable_enemy_collision_sets_both_bits() {
        assert_eq!(disable_enemy_collision(0x00), 0x81);
        assert_eq!(disable_enemy_collision(0x7E), 0xFF);
    }

    #[test]
    fn enable_enemy_player_collision_check_only_clears_bit_0() {
        assert_eq!(enable_enemy_player_collision_check(0xFF), 0xFE);
        assert_eq!(enable_enemy_player_collision_check(0x00), 0x00);
    }

    #[test]
    fn enable_bullet_enemy_collision_only_clears_bit_7() {
        assert_eq!(enable_bullet_enemy_collision(0xFF), 0x7F);
        assert_eq!(enable_bullet_enemy_collision(0x00), 0x00);
    }

    #[test]
    fn enable_enemy_collision_clears_both_bits() {
        assert_eq!(enable_enemy_collision(0xFF), 0x7E);
        assert_eq!(enable_enemy_collision(0x00), 0x00);
    }

    #[test]
    fn disable_then_enable_round_trips_to_the_original_masked_bits() {
        // Non-flag bits (1-6) must survive an enable/disable round trip
        // untouched.
        let original = 0b0011_0110u8;
        let disabled = disable_enemy_collision(original);
        let re_enabled = enable_enemy_collision(disabled);
        assert_eq!(re_enabled, original);
    }
}
