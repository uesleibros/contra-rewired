//! `contra-pc`: minimal desktop shell around `contra-core`.
//!
//! This is the Phase 1 vertical slice: a real window, a real 256x240
//! internal framebuffer presented with integer scaling, real keyboard *and*
//! gamepad input routed through `contra-core`'s rebindable
//! [`contra_core::input`] system, working quick-save/quick-load/rewind
//! against `contra-core`'s save-state manager, and a placeholder scene
//! driven by the ported gravity/jump/walk physics — so the whole pipeline
//! (config -> input -> simulation -> save states -> framebuffer -> window)
//! is demonstrably working end to end. There is no real Contra gameplay
//! here yet — no ROM loading, no sprites, no levels. See ROADMAP.md.

use std::collections::HashSet;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use contra_core::config::{Config, ScalingMode};
use contra_core::input::{Action, ActionState, Bindings, PhysicalInput};
use contra_core::physics::{LevelLocation, PlayerPhysics};
use contra_core::savestate::{SaveStateManager, SaveStateMeta, SlotId, SAVESTATE_FORMAT_VERSION};
use contra_core::state_machine::{GameEvent, GameRoutine};

use gilrs::{Axis, Button, Gilrs};
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

const INTERNAL_W: u32 = 256;
const INTERNAL_H: u32 = 240;
const CONFIG_PATH: &str = "config.toml";
const GAMEPAD_STICK_DEADZONE: f32 = 0.35;

/// Tracks which buttons were held last poll, so one-shot actions (pause,
/// quick save/load, rewind) fire once per press instead of every frame
/// they're held.
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

fn quick_save_meta(frame_count: u64) -> SaveStateMeta {
    SaveStateMeta {
        format_version: SAVESTATE_FORMAT_VERSION,
        slot: SlotId::Quick,
        stage: 0,
        checkpoint_index: 0,
        playtime_frames: frame_count,
        screenshot: None,
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config = Config::load_or_default(CONFIG_PATH);
    let initial_scale = match config.video.scaling {
        ScalingMode::Integer(n) => n.max(1) as u32,
        _ => 3,
    };
    let rewind_capacity = (config.gameplay.rewind_buffer_seconds as usize) * 60;

    let event_loop = EventLoop::new()?;
    let window = Rc::new(
        WindowBuilder::new()
            .with_title("contra-rewired (Phase 1 preview — no ROM loaded)")
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

    let mut framebuffer = vec![0u32; (INTERNAL_W * INTERNAL_H) as usize];

    let bindings = config.input.player_bindings[0].clone();
    let mut action_state = ActionState::new();
    let mut routine = GameRoutine::Boot;
    routine = routine.transition(GameEvent::BootComplete);
    routine = routine.transition(GameEvent::StartPressed);
    routine = routine.transition(GameEvent::StageIntroFinished);

    let mut player = PlayerPhysics::new(120, 200);
    let mut held_keys: HashSet<String> = HashSet::new();
    let mut edges = EdgeTracker::default();
    let mut frame_count: u64 = 0;

    let mut save_mgr: SaveStateManager<PlayerPhysics> = SaveStateManager::new(rewind_capacity.max(1));
    let rewind_enabled = config.gameplay.rewind_enabled;

    let mut gilrs = Gilrs::new().ok();
    if gilrs.is_none() {
        log::warn!("gilrs failed to initialize; gamepad input disabled for this session");
    }

    let frame_duration = Duration::from_secs_f64(1.0 / 60.0);
    let mut last_tick = Instant::now();
    let mut accumulator = Duration::ZERO;

    log::info!("contra-pc starting. No ROM loaded — this build only demonstrates the engine pipeline.");

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
                    let code = format!("{physical_key:?}");
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
                            save_mgr.save(SlotId::Quick, quick_save_meta(frame_count), player.clone());
                            log::info!("Quick saved at frame {frame_count}");
                        }
                        if is_down && key_code == KeyCode::F9 {
                            let current = contra_core::savestate::SaveState {
                                meta: quick_save_meta(frame_count),
                                payload: player.clone(),
                            };
                            if let Some(loaded) = save_mgr.load(SlotId::Quick, current).cloned() {
                                player = loaded;
                                log::info!("Quick loaded");
                            } else {
                                log::info!("No quick save to load");
                            }
                        }
                        if is_down && key_code == KeyCode::Backspace && rewind_enabled {
                            if let Some(prev_state) = save_mgr.rewind_step() {
                                player = prev_state;
                            }
                        }
                    }
                }
                WindowEvent::Resized(size) => {
                    if let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) {
                        let _ = surface.resize(w, h);
                    }
                }
                WindowEvent::RedrawRequested => {
                    present(&mut surface, &window, &framebuffer);
                }
                _ => {}
            },
            Event::AboutToWait => {
                let now = Instant::now();
                accumulator += now.duration_since(last_tick);
                last_tick = now;

                let gp = poll_gamepad(gilrs.as_mut());
                let start_pressed = edges.just_pressed("gp_start", gp.start);
                if start_pressed {
                    routine = match routine {
                        GameRoutine::Playing => routine.transition(GameEvent::PausePressed),
                        GameRoutine::Paused => routine.transition(GameEvent::ResumePressed),
                        other => other,
                    };
                }

                update_action_state(&mut action_state, &bindings, &held_keys, &gp);

                while accumulator >= frame_duration {
                    if routine.accepts_gameplay_input() {
                        step(&mut player, &action_state);
                        frame_count += 1;
                        if rewind_enabled {
                            save_mgr.push_rewind_frame(player.clone());
                        }
                    }
                    accumulator -= frame_duration;
                }

                render(&mut framebuffer, &player, routine);
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
    ];
    for (action, held) in combos {
        let was_pressed = held && !action_state.is_held(action);
        action_state.update(action, held, was_pressed, bindings.fire_mode);
    }
}

fn step(player: &mut PlayerPhysics, actions: &ActionState) {
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
}

fn render(fb: &mut [u32], player: &PlayerPhysics, routine: GameRoutine) {
    let bg = match routine {
        GameRoutine::Paused => 0x00202020,
        _ => 0x00104010,
    };
    fb.fill(bg);

    // Draw an 8x8 placeholder "player" block so the physics pipeline is
    // visibly doing something even with no real sprites loaded.
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
