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
mod menu;

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
use menu::{MenuItem, MenuState, Settings};
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, WindowBuilder};

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

/// Scans `./mods/` and loads every mod that has a Lua entry script. Every
/// found mod is enabled automatically (no mod-management UI yet - see
/// ROADMAP.md); drop a mod in the folder and it runs.
#[cfg(feature = "mods")]
struct LoadedMod {
    id: String,
    host: contra_mods::script::LuaModHost,
}

#[cfg(feature = "mods")]
fn load_mods() -> Vec<LoadedMod> {
    let registry = contra_mods::ModRegistry::scan("mods");
    let mut loaded = Vec::new();
    for m in registry.all() {
        let Some(script_rel_path) = &m.manifest.entry_script else {
            continue;
        };
        let script_path = m.dir.join(script_rel_path);
        let source = match std::fs::read_to_string(&script_path) {
            Ok(s) => s,
            Err(e) => {
                log::error!("mod '{}': could not read {}: {e}", m.manifest.id, script_path.display());
                continue;
            }
        };
        let host = match contra_mods::script::LuaModHost::new() {
            Ok(h) => h,
            Err(e) => {
                log::error!("mod '{}': failed to create Lua host: {e}", m.manifest.id);
                continue;
            }
        };
        if let Err(e) = host.load_script(&source, &m.manifest.id) {
            log::error!("mod '{}': script error: {e}", m.manifest.id);
            continue;
        }
        log::info!("loaded mod: {} ({}) - {}", m.manifest.name, m.manifest.id, m.manifest.description);
        loaded.push(LoadedMod { id: m.manifest.id.clone(), host });
    }
    loaded
}

/// Fires `frame_tick` on every loaded mod and applies whatever PPU writes
/// it queued (see `contra_mods::script::LuaModHost`). No-op for the
/// placeholder demo (no `Nes` to poke) or if `mods` wasn't enabled at
/// build time.
#[cfg(feature = "mods")]
fn run_mods(mods: &[LoadedMod], session: &mut Session) {
    let Session::Emulator { nes, frame_count, .. } = session else {
        return;
    };
    for m in mods {
        m.host.set_frame(*frame_count);
        if let Err(e) = m.host.fire(contra_mods::script::ModEvent::FrameTick) {
            log::error!("mod '{}': runtime error: {e}", m.id);
            continue;
        }
        for (addr, value) in m.host.take_pending_writes() {
            nes.poke_ppu(addr, value);
        }
    }
}

#[cfg(not(feature = "mods"))]
fn run_mods(_mods: &[()], _session: &mut Session) {}

#[cfg(not(feature = "mods"))]
fn load_mods() -> Vec<()> {
    let registry = contra_mods::ModRegistry::scan("mods");
    let scriptable = registry.all().iter().filter(|m| m.manifest.entry_script.is_some()).count();
    if scriptable > 0 {
        log::warn!(
            "{scriptable} mod(s) in ./mods have a Lua script, but this build doesn't have scripting enabled - \
             rebuild with `cargo build --features mods` (requires a C toolchain; see docs/MODDING.md)"
        );
    }
    Vec::new()
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
    let loaded_mods = load_mods();
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
    // Tracks the widescreen width that matches the *current* window's live
    // aspect ratio (recomputed on every resize), so an ultrawide monitor
    // fills edge to edge instead of pillarboxing at a fixed extension.
    let mut target_wide_width = compute_wide_width(window.inner_size().width, window.inner_size().height);

    let bindings = config.input.player_bindings[0].clone();
    let mut action_state = ActionState::new();
    let mut routine = GameRoutine::Boot;
    routine = routine.transition(GameEvent::BootComplete);
    routine = routine.transition(GameEvent::StartPressed);
    routine = routine.transition(GameEvent::StageIntroFinished);

    let mut held_keys: HashSet<String> = HashSet::new();
    let mut edges = EdgeTracker::default();
    let mut settings = Settings::default();
    let mut menu_state = MenuState::new();

    let mut gilrs = Gilrs::new().ok();
    if gilrs.is_none() {
        log::warn!("gilrs failed to initialize; gamepad input disabled for this session");
    }

    let frame_duration = Duration::from_secs_f64(1.0 / 60.0);
    let mut last_tick = Instant::now();
    let mut accumulator = Duration::ZERO;

    window.request_redraw();

    event_loop.run(move |event, elwt| {
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
                        if is_down && (key_code == KeyCode::Escape || key_code == KeyCode::Tab) {
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
                    target_wide_width = compute_wide_width(size.width, size.height);
                }
                WindowEvent::RedrawRequested => {
                    let (fb_source, fb_width): (&[u32], u32) = match &session {
                        Session::Emulator { nes, .. } if nes.wide_width() > contra_nes::SCREEN_W && !nes.wide_framebuffer().is_empty() => {
                            (nes.wide_framebuffer(), nes.wide_width() as u32)
                        }
                        Session::Emulator { nes, .. } => (nes.framebuffer(), contra_nes::SCREEN_W as u32),
                        Session::Placeholder { .. } => (&placeholder_framebuffer[..], INTERNAL_W),
                    };

                    let menu_overlay = (routine == GameRoutine::Paused).then_some(&menu_state);
                    present(&mut surface, &window, fb_source, fb_width, INTERNAL_H, &settings, menu_overlay);
                }
                _ => {}
            },
            Event::AboutToWait => {
                // WaitUntil (not Poll) so this only wakes up when a frame is
                // actually due, instead of spinning as fast as the OS will
                // allow. Poll was calling request_redraw() - a full
                // framebuffer scale-copy - potentially hundreds of thousands
                // of times per second, which is real CPU/GPU load that can
                // starve the audio callback thread and make everything feel
                // sluggish even though the emulation itself is nowhere near
                // the bottleneck (it runs at ~30x real-time; see
                // crates/contra-nes/examples/perf_test.rs).
                let now = Instant::now();
                accumulator += now.duration_since(last_tick);
                last_tick = now;

                let mut stepped = false;
                while accumulator >= frame_duration {
                    let gp = poll_gamepad(gilrs.as_mut());
                    if edges.just_pressed("gp_start", gp.start) {
                        routine = match routine {
                            GameRoutine::Playing => routine.transition(GameEvent::PausePressed),
                            GameRoutine::Paused => routine.transition(GameEvent::ResumePressed),
                            other => other,
                        };
                    }
                    update_action_state(&mut action_state, &bindings, &held_keys, &gp);

                    if routine == GameRoutine::Paused {
                        if action_state.just_pressed(Action::Up) {
                            menu_state.move_up();
                        }
                        if action_state.just_pressed(Action::Down) {
                            menu_state.move_down();
                        }
                        if menu_state.current() == MenuItem::Zoom {
                            if action_state.just_pressed(Action::Left) {
                                settings.zoom_percent = (settings.zoom_percent - 10).max(50);
                            }
                            if action_state.just_pressed(Action::Right) {
                                settings.zoom_percent = (settings.zoom_percent + 10).min(300);
                            }
                        }
                        if action_state.just_pressed(Action::Jump) {
                            match menu_state.current() {
                                MenuItem::Widescreen => settings.widescreen = !settings.widescreen,
                                MenuItem::NoSpriteLimit => settings.unlimited_sprites = !settings.unlimited_sprites,
                                MenuItem::PixelPerfect => settings.pixel_perfect = !settings.pixel_perfect,
                                MenuItem::Zoom => {}
                                MenuItem::Fullscreen => {
                                    settings.fullscreen = !settings.fullscreen;
                                    window.set_fullscreen(settings.fullscreen.then_some(Fullscreen::Borderless(None)));
                                }
                                MenuItem::AudioMuted => settings.audio_muted = !settings.audio_muted,
                                MenuItem::Resume => routine = routine.transition(GameEvent::ResumePressed),
                            }
                        }
                    } else if routine.accepts_gameplay_input() {
                        if let Session::Emulator { nes, .. } = &mut session {
                            nes.set_wide_width(if settings.widescreen { target_wide_width } else { contra_nes::SCREEN_W });
                            nes.set_unlimited_sprites(settings.unlimited_sprites);
                        }
                        session.step(&action_state, rewind_enabled);
                        run_mods(&loaded_mods, &mut session);
                        let samples = session.drain_audio();
                        if let Some(audio) = &audio_output {
                            if !settings.audio_muted {
                                audio.push_samples(&samples);
                            }
                        }
                    }
                    accumulator -= frame_duration;
                    stepped = true;
                }

                if stepped {
                    if let Session::Placeholder { player, .. } = &session {
                        render_placeholder(&mut placeholder_framebuffer, player, routine);
                    }
                    window.request_redraw();
                }

                elwt.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                    last_tick + frame_duration.saturating_sub(accumulator),
                ));
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

/// Picks a widescreen render width that matches the window's *current*
/// live aspect ratio, clamped to the hardware-safe cap
/// (`contra_nes::EXTENDED_WIDTH`) - so resizing the window (dragging it
/// wider, maximizing onto an ultrawide monitor, etc.) is reflected on the
/// very next frame instead of needing a mode toggle. See
/// `contra_nes::EXTENDED_WIDTH`'s docs for why there's a cap at all: the
/// NES doesn't have infinite off-screen world to reveal.
fn compute_wide_width(window_w: u32, window_h: u32) -> usize {
    if window_h == 0 {
        return contra_nes::SCREEN_W;
    }
    let aspect = window_w as f32 / window_h as f32;
    let desired = (contra_nes::SCREEN_H as f32 * aspect).round() as usize;
    desired.clamp(contra_nes::SCREEN_W, contra_nes::EXTENDED_WIDTH)
}

/// Blits `framebuffer` (`internal_w` x `internal_h`) into the window.
///
/// Two scaling philosophies, both real (see `menu::Settings::pixel_perfect`):
/// - **Pixel perfect** (opt-in): scale is floored to a whole number, so
///   every NES pixel is an exact NxN block on screen - crisp, but leaves
///   letterbox bars unless the window happens to be an exact multiple.
/// - **Dynamic fill** (default): scale is fractional, chosen to cover as
///   much of the window as possible while preserving aspect ratio - the
///   "whatever size the window is, it fills it" behavior of e.g. the
///   Switch Pokemon/Link's Awakening ports. Combined with live widescreen
///   width tracking, this reaches zero letterboxing on most window shapes.
///
/// `zoom_percent` (50-300) is an extra multiplier on top of either mode,
/// letting content be cropped-in past a perfect fit if desired.
///
/// The pause menu, if `menu_overlay` is `Some`, is drawn *after* scaling,
/// directly at the window's native resolution - crisp text that sits
/// visually "outside" the low-res emulated picture, not blocky upscaled
/// NES-style pixels.
fn present(
    surface: &mut softbuffer::Surface<Rc<winit::window::Window>, Rc<winit::window::Window>>,
    window: &winit::window::Window,
    framebuffer: &[u32],
    internal_w: u32,
    internal_h: u32,
    settings: &menu::Settings,
    menu_state_if_paused: Option<&MenuState>,
) {
    if framebuffer.len() < (internal_w * internal_h) as usize {
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

    let zoom = (settings.zoom_percent as f32 / 100.0).max(0.01);
    let fit_scale = (size.width as f32 / internal_w as f32).min(size.height as f32 / internal_h as f32);
    let scale = if settings.pixel_perfect { fit_scale.floor().max(1.0) } else { fit_scale } * zoom;

    let out_w = (internal_w as f32 * scale).round() as i32;
    let out_h = (internal_h as f32 * scale).round() as i32;
    let off_x = (size.width as i32 - out_w) / 2;
    let off_y = (size.height as i32 - out_h) / 2;

    buffer.fill(0);
    for dy in 0..size.height as i32 {
        let src_y = ((dy - off_y) as f32 / scale) as i32;
        if src_y < 0 || src_y >= internal_h as i32 {
            continue;
        }
        for dx in 0..size.width as i32 {
            let src_x = ((dx - off_x) as f32 / scale) as i32;
            if src_x < 0 || src_x >= internal_w as i32 {
                continue;
            }
            let src_idx = (src_y as u32 * internal_w + src_x as u32) as usize;
            let dst_idx = (dy as u32 * size.width + dx as u32) as usize;
            if src_idx < framebuffer.len() && dst_idx < buffer.len() {
                buffer[dst_idx] = framebuffer[src_idx];
            }
        }
    }

    if let Some(menu_state) = menu_state_if_paused {
        menu::draw_pause_menu(&mut buffer, size.width as usize, size.height as usize, menu_state, settings);
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

