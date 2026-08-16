//! Native port of Contra's enemy-slot field clearing: the `clear_enemy_*`
//! family (`src/bank7.asm`, CPU `$edf1`-`$ee46`) - zeroes a specific
//! subset of one enemy object slot's fields, real callers chaining into
//! progressively smaller tail shares (`pt_2` falls into `pt_3` falls into
//! `pt_4`). Every real call site loads `a = #$00` first, so unlike some
//! other ports in this crate there's no "value" parameter to carry -
//! these always clear to zero, matching every real caller.
//!
//! The routine's own top label, `clear_enemy` (`$edfc`, clears
//! `ENEMY_ROUTINE`/`ENEMY_HP`/`ENEMY_TYPE`/`ENEMY_SPRITES` in addition to
//! everything below), is **not ported** - its only caller,
//! `bank_7_unused_label_07`, is the disassembly's own documented dead
//! code ("never called !(UNUSED)"), so it's genuinely unreachable, per
//! this crate's standing policy.

/// One enemy object slot's fields touched by the real, reachable
/// `clear_enemy_*` entry points. Notably **not** included:
/// `ENEMY_ROUTINE`/`ENEMY_HP`/`ENEMY_TYPE` - only the dead-code
/// `clear_enemy` top label touches those, and `initialize_enemy` (the
/// only real caller of [`clear_enemy_pt_2`]) sets `ENEMY_ROUTINE`/
/// `ENEMY_SPRITES` itself beforehand and relies on `ENEMY_TYPE` already
/// being set by *its* caller - real, deliberate behavior, not an
/// oversight in this port.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnemyClearFields {
    pub attributes: u8,
    pub y_pos: u8,
    pub x_pos: u8,
    pub y_vel_accum: u8,
    pub x_vel_accum: u8,
    pub sprites: u8,
    pub sprite_attr: u8,
    pub y_velocity_fract: u8,
    pub x_velocity_fract: u8,
    pub y_velocity_fast: u8,
    pub x_velocity_fast: u8,
    pub animation_delay: u8,
    pub var_a: u8,
    pub attack_delay: u8,
    pub frame: u8,
    pub state_width: u8,
    pub score_collision: u8,
    pub var_1: u8,
    pub var_2: u8,
    pub var_3: u8,
    pub var_4: u8,
}

/// `clear_enemy_pt_4` (`$ee3a`): the innermost tail shared by every real
/// entry point - clears just the 4 general-purpose scratch vars.
pub fn clear_enemy_pt_4(f: &mut EnemyClearFields) {
    f.var_1 = 0;
    f.var_2 = 0;
    f.var_3 = 0;
    f.var_4 = 0;
}

/// `clear_enemy_custom_vars` (`$edf8`): real entry point that goes
/// straight to `clear_enemy_pt_4` - functionally identical to it, but
/// kept as its own named port since it's a distinct real `jsr` target
/// (4 real call sites in `bank0.asm`, all weapon-related).
pub fn clear_enemy_custom_vars(f: &mut EnemyClearFields) {
    clear_enemy_pt_4(f);
}

/// `clear_enemy_pt_3` (`$ee19`): clears the sprite/velocity/timer/AI-
/// scratch group, then falls into [`clear_enemy_pt_4`].
pub fn clear_enemy_pt_3(f: &mut EnemyClearFields) {
    f.sprite_attr = 0;
    f.y_velocity_fract = 0;
    f.x_velocity_fract = 0;
    f.y_velocity_fast = 0;
    f.x_velocity_fast = 0;
    f.animation_delay = 0;
    f.var_a = 0;
    f.attack_delay = 0;
    f.frame = 0;
    f.state_width = 0;
    f.score_collision = 0;
    clear_enemy_pt_4(f);
}

/// `clear_sprite_clear_enemy_pt_3` (`$edf1`): real entry point that
/// clears `sprites` (the one field between here and the dead-code
/// `clear_enemy` top label's own extra clears) then falls into
/// [`clear_enemy_pt_3`]. Real caller: `bank0.asm`'s weapon-item creation.
pub fn clear_sprite_and_pt_3(f: &mut EnemyClearFields) {
    f.sprites = 0;
    clear_enemy_pt_3(f);
}

/// `clear_enemy_pt_2` (`$ee0a`): clears position/attribute/velocity-
/// accumulator fields, then falls into [`clear_enemy_pt_3`]. Real
/// caller: `initialize_enemy` (every enemy spawn path in the game).
pub fn clear_enemy_pt_2(f: &mut EnemyClearFields) {
    f.attributes = 0;
    f.y_pos = 0;
    f.x_pos = 0;
    f.y_vel_accum = 0;
    f.x_vel_accum = 0;
    clear_enemy_pt_3(f);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentinel() -> EnemyClearFields {
        EnemyClearFields {
            attributes: 1,
            y_pos: 2,
            x_pos: 3,
            y_vel_accum: 4,
            x_vel_accum: 5,
            sprites: 6,
            sprite_attr: 7,
            y_velocity_fract: 8,
            x_velocity_fract: 9,
            y_velocity_fast: 10,
            x_velocity_fast: 11,
            animation_delay: 12,
            var_a: 13,
            attack_delay: 14,
            frame: 15,
            state_width: 16,
            score_collision: 17,
            var_1: 18,
            var_2: 19,
            var_3: 20,
            var_4: 21,
        }
    }

    #[test]
    fn pt4_only_clears_the_4_custom_vars() {
        let mut f = sentinel();
        clear_enemy_pt_4(&mut f);
        assert_eq!((f.var_1, f.var_2, f.var_3, f.var_4), (0, 0, 0, 0));
        // everything else untouched
        assert_eq!(f.attributes, 1);
        assert_eq!(f.sprites, 6);
        assert_eq!(f.score_collision, 17);
    }

    #[test]
    fn clear_enemy_custom_vars_matches_pt4_exactly() {
        let mut a = sentinel();
        let mut b = sentinel();
        clear_enemy_custom_vars(&mut a);
        clear_enemy_pt_4(&mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn pt3_clears_its_own_group_plus_pt4_but_not_pt2_or_sprites() {
        let mut f = sentinel();
        clear_enemy_pt_3(&mut f);
        assert_eq!(f.sprite_attr, 0);
        assert_eq!(f.y_velocity_fract, 0);
        assert_eq!(f.x_velocity_fract, 0);
        assert_eq!(f.y_velocity_fast, 0);
        assert_eq!(f.x_velocity_fast, 0);
        assert_eq!(f.animation_delay, 0);
        assert_eq!(f.var_a, 0);
        assert_eq!(f.attack_delay, 0);
        assert_eq!(f.frame, 0);
        assert_eq!(f.state_width, 0);
        assert_eq!(f.score_collision, 0);
        assert_eq!((f.var_1, f.var_2, f.var_3, f.var_4), (0, 0, 0, 0));
        // pt_2's group and `sprites` are NOT this entry point's job
        assert_eq!(f.attributes, 1);
        assert_eq!(f.y_pos, 2);
        assert_eq!(f.sprites, 6);
    }

    #[test]
    fn clear_sprite_and_pt3_additionally_clears_sprites() {
        let mut f = sentinel();
        clear_sprite_and_pt_3(&mut f);
        assert_eq!(f.sprites, 0);
        assert_eq!(f.sprite_attr, 0); // pt_3's group still runs
        assert_eq!(f.attributes, 1); // pt_2's group still not this entry's job
    }

    #[test]
    fn pt2_clears_everything_except_sprites() {
        let mut f = sentinel();
        clear_enemy_pt_2(&mut f);
        assert_eq!(f, EnemyClearFields { sprites: 6, ..Default::default() });
    }
}
