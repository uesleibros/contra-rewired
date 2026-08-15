//! Device-agnostic input: logical actions, rebindable bindings, and the
//! per-frame button state (with optional turbo/toggle-fire) that the
//! simulation actually consumes. `contra-pc` is responsible for turning
//! winit key events / gilrs gamepad events into [`PhysicalInput`] values and
//! feeding them through a [`Bindings`] table to produce [`ActionState`].

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Every action the simulation understands, independent of which key or
/// button triggers it. Matches the NES 8-button pad plus the modern
/// additions (dual-stick aim, pause/menu, UI/TAS helpers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Shoot,
    Jump,
    Pause,
    Start,
    Select,
    /// Dual-Stick Contra: aim stick as a 2D vector is handled separately
    /// (see [`ActionState::aim_vector`]); this action is the "fire" trigger
    /// in that mode, kept distinct from `Shoot` so both schemes can be
    /// bound independently.
    AimFire,
    QuickSave,
    QuickLoad,
    Rewind,
    FrameAdvance,
    ToggleTurbo,
}

/// A physical input source, deliberately string/code based rather than
/// tied to a specific windowing crate's enum, so `contra-core` doesn't
/// depend on winit/gilrs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PhysicalInput {
    Keyboard(String),
    /// XInput/DirectInput/generic-HID button index, scoped by controller id.
    GamepadButton(u32),
    GamepadAxisPositive(u32),
    GamepadAxisNegative(u32),
}

/// One player's full rebinding table plus per-action fire mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bindings {
    pub name: String,
    pub map: HashMap<Action, Vec<PhysicalInput>>,
    pub fire_mode: FireMode,
    pub turbo_rate_hz: f32,
    pub deadzone: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FireMode {
    Hold,
    Toggle,
}

impl Bindings {
    /// A reasonable default keyboard layout for Player 1.
    pub fn default_keyboard_p1() -> Self {
        let mut map = HashMap::new();
        map.insert(Action::Up, vec![PhysicalInput::Keyboard("ArrowUp".into())]);
        map.insert(Action::Down, vec![PhysicalInput::Keyboard("ArrowDown".into())]);
        map.insert(Action::Left, vec![PhysicalInput::Keyboard("ArrowLeft".into())]);
        map.insert(Action::Right, vec![PhysicalInput::Keyboard("ArrowRight".into())]);
        map.insert(Action::Shoot, vec![PhysicalInput::Keyboard("KeyZ".into())]);
        map.insert(Action::Jump, vec![PhysicalInput::Keyboard("KeyX".into())]);
        map.insert(Action::Start, vec![PhysicalInput::Keyboard("Enter".into())]);
        map.insert(Action::Select, vec![PhysicalInput::Keyboard("ShiftRight".into())]);
        map.insert(Action::Pause, vec![PhysicalInput::Keyboard("Escape".into())]);
        map.insert(Action::QuickSave, vec![PhysicalInput::Keyboard("F5".into())]);
        map.insert(Action::QuickLoad, vec![PhysicalInput::Keyboard("F9".into())]);
        map.insert(Action::Rewind, vec![PhysicalInput::Keyboard("Backspace".into())]);
        map.insert(Action::FrameAdvance, vec![PhysicalInput::Keyboard("F6".into())]);
        Self {
            name: "Keyboard (P1)".into(),
            map,
            fire_mode: FireMode::Hold,
            turbo_rate_hz: 10.0,
            deadzone: 0.2,
        }
    }

    /// A default gamepad layout, also usable as the base for Dual-Stick
    /// Contra (left stick = move, right stick = aim, trigger = `AimFire`).
    pub fn default_gamepad(name: impl Into<String>) -> Self {
        let mut map = HashMap::new();
        map.insert(Action::Shoot, vec![PhysicalInput::GamepadButton(0)]); // A / Cross
        map.insert(Action::Jump, vec![PhysicalInput::GamepadButton(1)]); // B / Circle
        map.insert(Action::Start, vec![PhysicalInput::GamepadButton(9)]);
        map.insert(Action::Select, vec![PhysicalInput::GamepadButton(8)]);
        map.insert(Action::Pause, vec![PhysicalInput::GamepadButton(9)]);
        map.insert(Action::AimFire, vec![PhysicalInput::GamepadButton(7)]); // right trigger
        Self {
            name: name.into(),
            map,
            fire_mode: FireMode::Hold,
            turbo_rate_hz: 10.0,
            deadzone: 0.2,
        }
    }

    pub fn rebind(&mut self, action: Action, inputs: Vec<PhysicalInput>) {
        self.map.insert(action, inputs);
    }

    pub fn is_bound(&self, action: Action, input: &PhysicalInput) -> bool {
        self.map
            .get(&action)
            .map(|inputs| inputs.contains(input))
            .unwrap_or(false)
    }
}

/// Resolved, per-frame state for one player: which actions are held, which
/// were just pressed/released this frame (for toggle-fire / TAS input
/// display), and the analog aim vector for Dual-Stick Contra.
#[derive(Debug, Clone, Default)]
pub struct ActionState {
    held: HashMap<Action, bool>,
    pressed_this_frame: HashMap<Action, bool>,
    toggle_latches: HashMap<Action, bool>,
    pub aim_vector: (f32, f32),
}

impl ActionState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Call once per frame with the raw held/pressed state and the active
    /// [`Bindings`], to resolve toggle-fire semantics.
    pub fn update(&mut self, action: Action, raw_held: bool, was_pressed: bool, fire_mode: FireMode) {
        let effective = match fire_mode {
            FireMode::Hold => raw_held,
            FireMode::Toggle => {
                if was_pressed {
                    let latch = self.toggle_latches.entry(action).or_insert(false);
                    *latch = !*latch;
                }
                *self.toggle_latches.get(&action).unwrap_or(&false)
            }
        };
        self.held.insert(action, effective);
        self.pressed_this_frame.insert(action, was_pressed);
    }

    pub fn is_held(&self, action: Action) -> bool {
        *self.held.get(&action).unwrap_or(&false)
    }

    pub fn just_pressed(&self, action: Action) -> bool {
        *self.pressed_this_frame.get(&action).unwrap_or(&false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_fire_flips_on_each_press() {
        let mut state = ActionState::new();
        state.update(Action::Shoot, true, true, FireMode::Toggle);
        assert!(state.is_held(Action::Shoot));
        state.update(Action::Shoot, false, false, FireMode::Toggle);
        assert!(state.is_held(Action::Shoot), "toggle stays on until pressed again");
        state.update(Action::Shoot, false, true, FireMode::Toggle);
        assert!(!state.is_held(Action::Shoot));
    }

    #[test]
    fn hold_fire_tracks_raw_state() {
        let mut state = ActionState::new();
        state.update(Action::Jump, true, true, FireMode::Hold);
        assert!(state.is_held(Action::Jump));
        state.update(Action::Jump, false, false, FireMode::Hold);
        assert!(!state.is_held(Action::Jump));
    }
}
