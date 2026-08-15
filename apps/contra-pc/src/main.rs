//! `contra-pc`: desktop shell around `contra-nes` (the real emulator core)
//! and `contra-core` (config/input/save-state plumbing).
//!
//! Pass your own legally-dumped ROM on the command line - or drop a
//! `baserom.nes` next to the executable - and this runs it for real: a
//! from-scratch 6502/2C02/mapper-2 core (see `crates/contra-nes`), not a
//! reimplementation of Contra's game logic. No ROM ships with this repo;
//! see docs/ASSETS.md. Without a ROM, it falls back to the same
//! placeholder physics demo from earlier in Phase 1, so the binary still
//! runs and the engine pipeline (config -> input -> save states ->
//! framebuffer -> window) stays demonstrable on its own.

mod audio;

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use clap::Parser;

use contra_core::config::{Config, ScalingMode};
use contra_core::input::{Action, ActionState, Bindings, PhysicalInput};
use contra_core::physics::{LevelLocation, PlayerPhysics};
use contra_core::savestate::{SaveState, SaveStateManager, SaveStateMeta, SlotId, SAVESTATE_FORMAT_VERSION};
use contra_core::state_machine::{GameEvent, GameRoutine};

use contra_nes::{Mirroring, Nes, NesSnapshot};

use gilrs::{Axis, Button, Gilrs};
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

const INTERNAL_W: u32 = 256;
const INTERNAL_H: u32 = 240;
const CONFIG_PATH: &str = "config.toml";
const GAMEPAD_STICK_DEADZONE: f32 = 0.35;

#[derive(Parser)]
#[command(author, version, about = "contra-rewired PC front-end. Pass your own legally-dumped Contra (NES) ROM to play it for real.")]
struct Args {
    /// Path to your own ROM dump. If omitted, looks for ./baserom.nes, then
    /// falls back to the engine-only placeholder demo.
    rom: Option<PathBuf>,
}

/// Tracks which one-shot inputs were already held last poll, so pause /
/// quick save / quick load / rewind fire once per press.
#[derive(Default)]
struct EdgeTracker {
    prev: HashSet<&'static str>,
}

impl EdgeTracker {
    fn just_pressed(&mut self, key: &'static str, is_down: bool) -> bool {
        let was_down = self.prev.contains(key);
        if is_down {
            self.prev.insert(key);
        } else {
            self.prev.remove(key);
        }
        is_down && !was_down
    }
}

fn meta(frame_count: u64) -> SaveStateMeta {
    SaveStateMeta {
        format_version: SAVESTATE_FORMAT_VERSION,
        slot: SlotId::Quick,
        stage: 0,
        checkpoint_index: 0,
        playtime_frames: frame_count,
        screenshot: None,
    }
}

/// Either a real, running game (loaded from the user's own ROM) or the
/// engine-only placeholder demo used when no ROM was found. Every method
/// here is the seam between the two - `contra-pc`'s main loop doesn't
/// otherwise care which one it's driving.
enum Session {
    Emulator {
        nes: Box<Nes>,
        save_mgr: SaveStateManager<NesSnapshot>,
        frame_count: u64,
    },
    Placeholder {
        player: PlayerPhysics,
        save_mgr: SaveStateManager<PlayerPhysics>,
        frame_count: u64,
    },
}

impl Session {
    fn step(&mut self, actions: &ActionState, rewind_enabled: bool) {
        match self {
            Session::Emulator { nes, save_mgr, frame_count } => {
                nes.set_controller(0, controller_byte(actions));
                nes.run_frame();
                *frame_count += 1;
                if rewind_enabled {
                    save_mgr.push_rewind_frame(nes.snapshot());
                }
            }
            Session::Placeholder { player, save_mgr, frame_count } => {
                if actions.is_held(Action::Jump) {
                    player.start_jump(LevelLocation::Outdoor);
                }
                let dir = match (actions.is_held(Action::Left), actions.is_held(Action::Right)) {
                    (true, false) => -1,
                    (false, true) => 1,
                    _ => 0,
                };
                player.step_horizontal(dir);
                player.x = player.x.clamp(8, INTERNAL_W as i16 - 16);
                player.step_vertical(200);
                *frame_count += 1;
                if rewind_enabled {
                    save_mgr.push_rewind_frame(player.clone());
                }
            }
        }
    }

    fn quick_save(&mut self) {
        match self {
            Session::Emulator { nes, save_mgr, frame_count } => {
                save_mgr.save(SlotId::Quick, meta(*frame_count), nes.snapshot());
            }
            Session::Placeholder { player, save_mgr, frame_count } => {
                save_mgr.save(SlotId::Quick, meta(*frame_count), player.clone());
            }
        }
        log::info!("Quick saved");
    }

    fn quick_load(&mut self) {
        match self {
            Session::Emulator { nes, save_mgr, frame_count } => {
                let current = SaveState { meta: meta(*frame_count), payload: nes.snapshot() };
                if let Some(loaded) = save_mgr.load(SlotId::Quick, current).cloned() {
                    nes.restore(&loaded);
                    log::info!("Quick loaded");
                    return;
                }
            }
            Session::Placeholder { player, save_mgr, frame_count } => {
                let current = SaveState { meta: meta(*frame_count), payload: player.clone() };
                if let Some(loaded) = save_mgr.load(SlotId::Quick, current).cloned() {
                    *player = loaded;
                    log::info!("Quick loaded");
                    return;
                }
            }
        }
        log::info!("No quick save to load");
    }

    fn rewind(&mut self) {
        match self {
            Session::Emulator { nes, save_mgr, .. } => {
                if let Some(prev) = save_mgr.rewind_step() {
                    nes.restore(&prev);
                }
            }
            Session::Placeholder { player, save_mgr, .. } => {
                if let Some(prev) = save_mgr.rewind_step() {
                    *player = prev;
                }
            }
        }
    }

    /// Empty for the placeholder demo - it has no APU to drain.
    fn drain_audio(&mut self) -> Vec<f32> {
        match self {
            Session::Emulator { nes, .. } => nes.take_audio_samples(),
            Session::Placeholder { .. } => Vec::new(),
        }
    }
}

fn controller_byte(actions: &ActionState) -> u8 {
    use contra_nes::controller::*;
    let mut b = 0u8;
    if actions.is_held(Action::Up) {
        b |= BUTTON_UP;
    }
    if actions.is_held(Action::Down) {
        b |= BUTTON_DOWN;
    }
    if actions.is_held(Action::Left) {
        b |= BUTTON_LEFT;
    }
    if actions.is_held(Action::Right) {
        b |= BUTTON_RIGHT;
    }
    if actions.is_held(Action::Jump) {
        b |= BUTTON_A;
    }
    if actions.is_held(Action::Shoot) {
        b |= BUTTON_B;
    }
    if actions.is_held(Action::Start) {
        b |= BUTTON_START;
    }
    if actions.is_held(Action::Select) {
        b |= BUTTON_SELECT;
    }
    b
}

/// Resolves which ROM to load: an explicit CLI arg, else `./baserom.nes`
/// if present, else `None` (placeholder mode).
fn resolve_rom_path(args: &Args) -> Option<PathBuf> {
    if let Some(p) = &args.rom {
        return Some(p.clone());
    }
    let default = PathBuf::from("baserom.nes");
    default.exists().then_some(default)
}

fn load_session(args: &Args, rewind_capacity: usize, audio_sample_rate: f64) -> Session {
    let Some(path) = resolve_rom_path(args) else {
        log::info!("No ROM specified and no ./baserom.nes found - running the engine-only placeholder demo. Pass a ROM path: contra-pc <path-to-your-rom.nes>");
        return Session::Placeholder {
            player: PlayerPhysics::new(120, 200),
            save_mgr: SaveStateManager::new(rewind_capacity.max(1)),
            frame_count: 0,
        };
    };

    match contra_assets::NesRom::load(&path) {
        Ok(rom) if rom.mapper == 2 => {
            log::info!(
                "Loaded {} (mapper {}, {} KiB PRG, MD5 {})",
                path.display(),
                rom.mapper,
                rom.prg_rom.len() / 1024,
                rom.md5_hex
            );
            let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
            let nes = Nes::new_with_audio(rom.prg_rom, mirroring, audio_sample_rate);
            Session::Emulator {
                nes: Box::new(nes),
                save_mgr: SaveStateManager::new(rewind_capacity.max(1)),
                frame_count: 0,
            }
        }
        Ok(rom) => {
            log::error!(
                "{} uses mapper {}, but contra-nes only supports mapper 2 (UxROM) right now - falling back to the placeholder demo.",
                path.display(),
                rom.mapper
            );
            Session::Placeholder {
                player: PlayerPhysics::new(120, 200),
                save_mgr: SaveStateManager::new(rewind_capacity.max(1)),
                frame_count: 0,
            }
        }
        Err(e) => {
            log::error!("Could not load {}: {e} - falling back to the placeholder demo.", path.display());
            Session::Placeholder {
                player: PlayerPhysics::new(120, 200),
                save_mgr: SaveStateManager::new(rewind_capacity.max(1)),
                frame_count: 0,
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let config = Config::load_or_default(CONFIG_PATH);
    let initial_scale = match config.video.scaling {
        ScalingMode::Integer(n) => n.max(1) as u32,
        _ => 3,
    };
    let rewind_capacity = (config.gameplay.rewind_buffer_seconds as usize) * 60;
    let rewind_enabled = config.gameplay.rewind_enabled;

    let audio_output = audio::AudioOutput::new();
    if audio_output.is_none() {
        log::warn!("no audio output available; running silently");
    }
    let audio_sample_rate = audio_output.as_ref().map(|a| a.sample_rate).unwrap_or(44_100.0);

    let mut session = load_session(&args, rewind_capacity, audio_sample_rate);
    let window_title = match &session {
        Session::Emulator { .. } => "contra-rewired",
        Session::Placeholder { .. } => "contra-rewired (no ROM loaded - engine placeholder demo)",
    };

    let event_loop = EventLoop::new()?;
    let window = Rc::new(
        WindowBuilder::new()
            .with_title(window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                INTERNAL_W * initial_scale,
                INTERNAL_H * initial_scale,
            ))
            .build(&event_loop)?,
    );

    let context = softbuffer::Context::new(window.clone())
        .map_err(|e| anyhow::anyhow!("softbuffer context: {e}"))?;
    let mut surface = softbuffer::Surface::new(&context, window.clone())
        .map_err(|e| anyhow::anyhow!("softbuffer surface: {e}"))?;

    let mut placeholder_framebuffer = vec![0u32; (INTERNAL_W * INTERNAL_H) as usize];

    let bindings = config.input.player_bindings[0].clone();
    let mut action_state = ActionState::new();
    let mut routine = GameRoutine::Boot;
    routine = routine.transition(GameEvent::BootComplete);
    routine = routine.transition(GameEvent::StartPressed);
    routine = routine.transition(GameEvent::StageIntroFinished);

    let mut held_keys: HashSet<String> = HashSet::new();
    let mut edges = EdgeTracker::default();

    let mut gilrs = Gilrs::new().ok();
    if gilrs.is_none() {
        log::warn!("gilrs failed to initialize; gamepad input disabled for this session");
    }

    let frame_duration = Duration::from_secs_f64(1.0 / 60.0);
    let mut last_tick = Instant::now();
    let mut accumulator = Duration::ZERO;

    event_loop.run(move |event, elwt| {
        elwt.set_control_flow(winit::event_loop::ControlFlow::Poll);

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    let _ = config.save(CONFIG_PATH);
                    elwt.exit();
                }
                WindowEvent::KeyboardInput {
                    event: KeyEvent { physical_key, state, repeat, .. },
                    ..
                } => {
                    if repeat {
                        return;
                    }
                    let code = key_code_name(physical_key);
                    let is_down = state == ElementState::Pressed;
                    if is_down {
                        held_keys.insert(code);
                    } else {
                        held_keys.remove(&code);
                    }

                    if let PhysicalKey::Code(key_code) = physical_key {
                        if is_down && key_code == KeyCode::Escape {
                            routine = match routine {
                                GameRoutine::Playing => routine.transition(GameEvent::PausePressed),
                                GameRoutine::Paused => routine.transition(GameEvent::ResumePressed),
                                other => other,
                            };
                        }
                        if is_down && key_code == KeyCode::F5 {
                            session.quick_save();
                        }
                        if is_down && key_code == KeyCode::F9 {
                            session.quick_load();
                        }
                        if is_down && key_code == KeyCode::Backspace && rewind_enabled {
                            session.rewind();
                        }
                    }
                }
                WindowEvent::Resized(size) => {
                    if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                        let _ = surface.resize(w, h);
                    }
                }
                WindowEvent::RedrawRequested => {
                    let fb = match &session {
                        Session::Emulator { nes, .. } => nes.framebuffer(),
                        Session::Placeholder { .. } => &placeholder_framebuffer[..],
                    };
                    present(&mut surface, &window, fb);
                }
                _ => {}
            },
            Event::AboutToWait => {
                let now = Instant::now();
                accumulator += now.duration_since(last_tick);
                last_tick = now;

                let gp = poll_gamepad(gilrs.as_mut());
                if edges.just_pressed("gp_start", gp.start) {
                    routine = match routine {
                        GameRoutine::Playing => routine.transition(GameEvent::PausePressed),
                        GameRoutine::Paused => routine.transition(GameEvent::ResumePressed),
                        other => other,
                    };
                }

                update_action_state(&mut action_state, &bindings, &held_keys, &gp);

                while accumulator >= frame_duration {
                    if routine.accepts_gameplay_input() {
                        session.step(&action_state, rewind_enabled);
                        if let Some(audio) = &audio_output {
                            audio.push_samples(&session.drain_audio());
                        }
                    }
                    accumulator -= frame_duration;
                }

                if let Session::Placeholder { player, .. } = &session {
                    render_placeholder(&mut placeholder_framebuffer, player, routine);
                }
                window.request_redraw();
            }
            _ => {}
        }
    })?;

    Ok(())
}

/// Raw, pre-`Bindings` gamepad state for the first connected controller.
/// Movement reads both d-pad and left-stick (with deadzone); shoot/jump map
/// to the south/east face buttons (A/Cross, B/Circle).
#[derive(Default, Clone, Copy)]
struct GamepadFrame {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
    shoot: bool,
    jump: bool,
    start: bool,
    select: bool,
}

fn poll_gamepad(gilrs: Option<&mut Gilrs>) -> GamepadFrame {
    let Some(gilrs) = gilrs else {
        return GamepadFrame::default();
    };
    while gilrs.next_event().is_some() {}

    let Some((_, gamepad)) = gilrs.gamepads().next() else {
        return GamepadFrame::default();
    };
    let stick_x = gamepad.value(Axis::LeftStickX);
    let stick_y = gamepad.value(Axis::LeftStickY);

    GamepadFrame {
        left: gamepad.is_pressed(Button::DPadLeft) || stick_x < -GAMEPAD_STICK_DEADZONE,
        right: gamepad.is_pressed(Button::DPadRight) || stick_x > GAMEPAD_STICK_DEADZONE,
        up: gamepad.is_pressed(Button::DPadUp) || stick_y > GAMEPAD_STICK_DEADZONE,
        down: gamepad.is_pressed(Button::DPadDown) || stick_y < -GAMEPAD_STICK_DEADZONE,
        shoot: gamepad.is_pressed(Button::South),
        jump: gamepad.is_pressed(Button::East),
        start: gamepad.is_pressed(Button::Start),
        select: gamepad.is_pressed(Button::Select),
    }
}

/// Turns a winit [`PhysicalKey`] into the plain key-name string
/// [`contra_core::input::Bindings`] uses (e.g. `"Enter"`, `"ArrowUp"`).
///
/// This must format the *inner* [`KeyCode`], not [`PhysicalKey`] itself:
/// `PhysicalKey` is `enum { Code(KeyCode), Unidentified(NativeKeyCode) }`,
/// so `format!("{physical_key:?}")` on a known key produces `"Code(Enter)"`,
/// which never matches a binding stored as `"Enter"`. That mismatch
/// silently broke every keyboard action bound through `Bindings` (Start,
/// Select, and anything else routed through `keyboard_held`) even though
/// the input event itself was received correctly.
fn key_code_name(physical_key: PhysicalKey) -> String {
    match physical_key {
        PhysicalKey::Code(key_code) => format!("{key_code:?}"),
        PhysicalKey::Unidentified(native) => format!("Unidentified({native:?})"),
    }
}

fn keyboard_held(bindings: &Bindings, held_keys: &HashSet<String>, action: Action) -> bool {
    bindings
        .map
        .get(&action)
        .map(|inputs| {
            inputs.iter().any(|input| matches!(input, PhysicalInput::Keyboard(k) if held_keys.contains(k)))
        })
        .unwrap_or(false)
}

fn update_action_state(action_state: &mut ActionState, bindings: &Bindings, held_keys: &HashSet<String>, gp: &GamepadFrame) {
    let combos = [
        (Action::Up, keyboard_held(bindings, held_keys, Action::Up) || gp.up),
        (Action::Down, keyboard_held(bindings, held_keys, Action::Down) || gp.down),
        (Action::Left, keyboard_held(bindings, held_keys, Action::Left) || gp.left),
        (Action::Right, keyboard_held(bindings, held_keys, Action::Right) || gp.right),
        (Action::Jump, keyboard_held(bindings, held_keys, Action::Jump) || gp.jump),
        (Action::Shoot, keyboard_held(bindings, held_keys, Action::Shoot) || gp.shoot),
        (Action::Start, keyboard_held(bindings, held_keys, Action::Start) || gp.start),
        (Action::Select, keyboard_held(bindings, held_keys, Action::Select) || gp.select),
    ];
    for (action, held) in combos {
        let was_pressed = held && !action_state.is_held(action);
        action_state.update(action, held, was_pressed, bindings.fire_mode);
    }
}

fn render_placeholder(fb: &mut [u32], player: &PlayerPhysics, routine: GameRoutine) {
    let bg = match routine {
        GameRoutine::Paused => 0x00202020,
        _ => 0x00104010,
    };
    fb.fill(bg);

    let px = player.x.clamp(0, INTERNAL_W as i16 - 8) as u32;
    let py = (player.y as u32).min(INTERNAL_H - 8);
    for y in 0..8u32 {
        for x in 0..8u32 {
            let idx = ((py + y) * INTERNAL_W + (px + x)) as usize;
            if idx < fb.len() {
                fb[idx] = 0x00E0E040;
            }
        }
    }
}

fn present(surface: &mut softbuffer::Surface<Rc<winit::window::Window>, Rc<winit::window::Window>>, window: &winit::window::Window, framebuffer: &[u32]) {
    if framebuffer.len() < (INTERNAL_W * INTERNAL_H) as usize {
        return;
    }
    let size = window.inner_size();
    let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else {
        return;
    };
    if surface.resize(w, h).is_err() {
        return;
    }
    let Ok(mut buffer) = surface.buffer_mut() else {
        return;
    };

    // Integer-scale + letterbox the internal framebuffer into the window.
    let scale = (size.width / INTERNAL_W).min(size.height / INTERNAL_H).max(1);
    let out_w = INTERNAL_W * scale;
    let out_h = INTERNAL_H * scale;
    let off_x = (size.width - out_w) / 2;
    let off_y = (size.height - out_h) / 2;

    buffer.fill(0);
    for sy in 0..out_h {
        let src_y = sy / scale;
        for sx in 0..out_w {
            let src_x = sx / scale;
            let src_idx = (src_y * INTERNAL_W + src_x) as usize;
            let dst_x = off_x + sx;
            let dst_y = off_y + sy;
            if dst_x < size.width && dst_y < size.height {
                let dst_idx = (dst_y * size.width + dst_x) as usize;
                if src_idx < framebuffer.len() && dst_idx < buffer.len() {
                    buffer[dst_idx] = framebuffer[src_idx];
                }
            }
        }
    }
    let _ = buffer.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_code_name_matches_bindings_string_not_the_enum_debug_wrapper() {
        // This is the exact bug: PhysicalKey debug-formats as "Code(Enter)",
        // not "Enter". key_code_name must unwrap to the bare KeyCode name so
        // it matches what Bindings::default_keyboard_p1 stores.
        assert_eq!(key_code_name(PhysicalKey::Code(KeyCode::Enter)), "Enter");
        assert_eq!(key_code_name(PhysicalKey::Code(KeyCode::ShiftRight)), "ShiftRight");
        assert_eq!(key_code_name(PhysicalKey::Code(KeyCode::ArrowUp)), "ArrowUp");
        assert_eq!(key_code_name(PhysicalKey::Code(KeyCode::KeyZ)), "KeyZ");
    }

    #[test]
    fn default_keyboard_bindings_are_reachable_via_key_code_name() {
        // Every default P1 binding must be producible by key_code_name for
        // some real KeyCode - otherwise the binding can never match a live
        // key press, regardless of what the player presses.
        let bindings = Bindings::default_keyboard_p1();
        let live_codes = [
            KeyCode::ArrowUp,
            KeyCode::ArrowDown,
            KeyCode::ArrowLeft,
            KeyCode::ArrowRight,
            KeyCode::KeyZ,
            KeyCode::KeyX,
            KeyCode::Enter,
            KeyCode::ShiftRight,
            KeyCode::Escape,
            KeyCode::F5,
            KeyCode::F9,
            KeyCode::Backspace,
            KeyCode::F6,
        ];
        let reachable: std::collections::HashSet<String> =
            live_codes.into_iter().map(|kc| key_code_name(PhysicalKey::Code(kc))).collect();

        for inputs in bindings.map.values() {
            for input in inputs {
                if let PhysicalInput::Keyboard(name) = input {
                    assert!(reachable.contains(name), "binding {name:?} is unreachable from any live KeyCode");
                }
            }
        }
    }
}

