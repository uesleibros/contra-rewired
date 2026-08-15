//! Player physics ported from `bank7.asm`. Vertical integration mirrors
//! `apply_gravity` / `player_jumping_set_y_pos` exactly (see
//! [`crate::fixed`] for the byte-level operations). Horizontal movement and
//! jump takeoff velocity are now ported too — see [`JUMP_VELOCITY_OUTDOOR`],
//! [`JUMP_VELOCITY_INDOOR`], and [`WALK_SPEED`] below for the exact source
//! lines. Weapon-specific recoil and enemy-collision knockback are not
//! ported yet (ROADMAP.md, Phase 1).

use crate::fixed::{JumpAccumulator, Velocity16};

/// Player horizontal speed is not a fixed-point value on the NES — it's a
/// literal whole-pixel-per-frame constant, set directly from d-pad state
/// every frame rather than accumulated. From `set_player_positive_x_velocity`
/// / `set_player_negative_x_velocity` in `bank7.asm`:
///
/// ```text
/// set_player_positive_x_velocity:
///     lda #$01                  ; a = #$01
///     bne set_player_x_vel_to_a
/// set_player_negative_x_velocity:
///     lda #$ff                  ; a = #$ff (-1)
/// set_player_x_vel_to_a:
///     ...
///     sta PLAYER_X_VELOCITY,x
/// ```
pub const WALK_SPEED: i16 = 1;

/// Jump takeoff velocity on outdoor stages, from `set_jump_status_and_y_velocity`:
/// `PLAYER_Y_FAST_VELOCITY = $fb, PLAYER_Y_FRACT_VELOCITY = $f0`. Reproduced
/// as the exact raw register bytes rather than a decimal approximation, so
/// running it through [`Velocity16::apply_gravity`] frame-by-frame is
/// bit-exact by construction.
pub const JUMP_VELOCITY_OUTDOOR: Velocity16 = Velocity16 { fract: 0xf0, fast: -5 };

/// Jump takeoff velocity on indoor/base stages, same routine:
/// `PLAYER_Y_FAST_VELOCITY = $fc, PLAYER_Y_FRACT_VELOCITY = $90`.
pub const JUMP_VELOCITY_INDOOR: Velocity16 = Velocity16 { fract: 0x90, fast: -4 };

/// Initial upward velocity of the player-death "pop" animation, from
/// `kill_player`: `PLAYER_Y_FAST_VELOCITY = $fd, PLAYER_Y_FRACT_VELOCITY = $80`.
pub const DEATH_BOUNCE_VELOCITY: Velocity16 = Velocity16 { fract: 0x80, fast: -3 };

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LevelLocation {
    Outdoor,
    Indoor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VerticalState {
    Grounded,
    Jumping,
    Falling,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlayerPhysics {
    pub x: i16,
    pub y: u8,
    pub y_vel: Velocity16,
    pub jump_accum: JumpAccumulator,
    pub vertical_state: VerticalState,
    /// Multiplier applied to horizontal/vertical speed by Custom Difficulty
    /// / mutators (e.g. "Turbo", "Moon Gravity" presets). `1.0` reproduces
    /// NES behavior exactly; anything else is explicitly a modern, opt-in
    /// deviation (never applied in `Original` fidelity mode).
    pub speed_mult: f32,
}

impl PlayerPhysics {
    pub fn new(x: i16, y: u8) -> Self {
        Self {
            x,
            y,
            y_vel: Velocity16::zero(),
            jump_accum: JumpAccumulator::default(),
            vertical_state: VerticalState::Grounded,
            speed_mult: 1.0,
        }
    }

    /// Begins a jump with the NES's exact takeoff velocity for the given
    /// location type. No-op if the player isn't grounded (matches the
    /// original: jump input is only read while `PLAYER_STATE` is normal and
    /// `PLAYER_JUMP_STATUS` is clear).
    pub fn start_jump(&mut self, location: LevelLocation) {
        if self.vertical_state == VerticalState::Grounded {
            self.y_vel = match location {
                LevelLocation::Outdoor => JUMP_VELOCITY_OUTDOOR,
                LevelLocation::Indoor => JUMP_VELOCITY_INDOOR,
            };
            self.vertical_state = VerticalState::Jumping;
        }
    }

    /// One frame of vertical integration: gravity, then position update via
    /// the jump accumulator, exactly mirroring `apply_gravity_set_y_pos`.
    pub fn step_vertical(&mut self, ground_y: u8) {
        if self.vertical_state == VerticalState::Grounded {
            return;
        }
        self.y_vel.apply_gravity();
        self.y = self.jump_accum.integrate(self.y_vel, self.y);

        if self.y >= ground_y && self.y_vel.fast >= 0 {
            self.y = ground_y;
            self.y_vel = Velocity16::zero();
            self.jump_accum = JumpAccumulator::default();
            self.vertical_state = VerticalState::Grounded;
        } else if self.y_vel.fast >= 0 {
            self.vertical_state = VerticalState::Falling;
        }
    }

    /// One frame of horizontal movement: `dir` is -1, 0, or 1, matching
    /// d-pad state exactly the way `set_player_x_vel_to_a` does — this is a
    /// direct per-frame set of `WALK_SPEED`, not an accumulated velocity.
    /// `speed_mult` is where non-Original modes (Turbo mutator, Custom
    /// Difficulty) hook in; it's `1.0` for bit-exact behavior.
    pub fn step_horizontal(&mut self, dir: i8) {
        self.x += (dir as i16) * ((WALK_SPEED as f32) * self.speed_mult) as i16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_returns_to_ground_and_stops() {
        let mut p = PlayerPhysics::new(0, 200);
        p.start_jump(LevelLocation::Outdoor);
        assert_eq!(p.vertical_state, VerticalState::Jumping);
        for _ in 0..200 {
            p.step_vertical(200);
        }
        assert_eq!(p.vertical_state, VerticalState::Grounded);
        assert_eq!(p.y, 200);
    }

    #[test]
    fn indoor_jump_is_shorter_than_outdoor_jump() {
        let peak_height = |location| {
            let mut p = PlayerPhysics::new(0, 200);
            p.start_jump(location);
            let mut min_y = 200u8;
            for _ in 0..120 {
                p.step_vertical(200);
                min_y = min_y.min(p.y);
                if p.vertical_state == VerticalState::Grounded {
                    break;
                }
            }
            200 - min_y
        };
        // Outdoor takeoff velocity (~-4.06 combined) has a larger magnitude
        // than indoor (~-3.44), so it must jump higher.
        assert!(peak_height(LevelLocation::Outdoor) > peak_height(LevelLocation::Indoor));
    }

    #[test]
    fn walk_speed_is_exactly_one_pixel_per_frame() {
        let mut p = PlayerPhysics::new(100, 200);
        p.step_horizontal(1);
        assert_eq!(p.x, 101);
        p.step_horizontal(-1);
        p.step_horizontal(-1);
        assert_eq!(p.x, 99);
    }

    #[test]
    fn identical_inputs_produce_identical_trajectories() {
        let run = || {
            let mut p = PlayerPhysics::new(0, 200);
            p.start_jump(LevelLocation::Outdoor);
            let mut ys = Vec::new();
            for _ in 0..40 {
                p.step_vertical(200);
                ys.push(p.y);
            }
            ys
        };
        assert_eq!(run(), run());
    }
}
