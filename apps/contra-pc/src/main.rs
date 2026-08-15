//! `contra-pc`: desktop shell around `contra-nes` (the real emulator core)
//! and `contra-core` (config/input/save-state plumbing).
//!
//! Pass your own legally-dumped ROM on the command line - or drop a
//! `baserom.nes` next to the executable - and this runs it for real: a
//! from-scratch 6502/2C02/mapper-2 core (see `crates/contra-nes`), not a
//! reimplementation of Contra's game logic. No ROM ships with this repo;
//! see docs/ASSETS.md. Without a ROM, it shows a real "Load ROM" screen
//! instead (native file picker or drag-and-drop) - see `menu::no_rom_screen`.
//!
//! Rendering and the in-game menu run on `wgpu` + `egui`: the NES
//! framebuffer is uploaded as a GPU texture and drawn as the background of
//! an `egui` frame, with the pause menu (when open) and the no-ROM screen
//! as real `egui` widgets on top - see `menu.rs`.

mod audio;
mod menu;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use clap::Parser;

use contra_core::config::{Config, ScalingMode};
use contra_core::input::{Action, ActionState, Bindings, PhysicalInput};
use contra_core::physics::{LevelLocation, PlayerPhysics};
use contra_core::savestate::{SaveState, SaveStateManager, SaveStateMeta, SlotId, SAVESTATE_FORMAT_VERSION};
use contra_core::state_machine::{GameEvent, GameRoutine};

use contra_nes::{Mirroring, Nes, NesSnapshot};

use gilrs::{Axis, Button, Gilrs};
use menu::{DebugInfo, MenuAction, MenuState, ModEntry, Settings, WEAPON_NAMES};
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, WindowBuilder};

const INTERNAL_W: u32 = 256;
const INTERNAL_H: u32 = 240;
const CONFIG_PATH: &str = "config.toml";
const GAMEPAD_STICK_DEADZONE: f32 = 0.35;

// RAM addresses backing the Debug tab's live weapon/lives/continues
// controls, from the community disassembly's `ram.asm` (see
// docs/MODDING.md and docs/FIDELITY.md). All are CPU work-RAM
// (`$0000-$07FF`), player-1-scoped (`+1` gets player 2's copy).
const RAM_P1_NUM_LIVES: u16 = 0x32;
const RAM_P2_NUM_LIVES: u16 = 0x33;
const RAM_P1_CURRENT_WEAPON: u16 = 0xAA;
const RAM_P2_CURRENT_WEAPON: u16 = 0xAB;
const RAM_NUM_CONTINUES: u16 = 0x3A;

#[derive(Parser)]
#[command(author, version, about = "contra-rewired PC front-end. Pass your own legally-dumped Contra (NES) ROM to play it for real.")]
struct Args {
    /// Path to your own ROM dump. If omitted, looks for ./baserom.nes, then
    /// shows the in-app Load ROM screen.
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
/// hand-ported placeholder physics session used when no ROM was found -
/// only ever stepped/saved through, never rendered as gameplay (see
/// `menu::no_rom_screen`, drawn instead while this variant is active).
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

    /// Empty for the placeholder session - it has no APU to drain.
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
/// if present, else `None` (shows the Load ROM screen).
fn resolve_rom_path(args: &Args) -> Option<PathBuf> {
    if let Some(p) = &args.rom {
        return Some(p.clone());
    }
    let default = PathBuf::from("baserom.nes");
    default.exists().then_some(default)
}

fn placeholder_session(rewind_capacity: usize) -> Session {
    Session::Placeholder {
        player: PlayerPhysics::new(120, 200),
        save_mgr: SaveStateManager::new(rewind_capacity.max(1)),
        frame_count: 0,
    }
}

/// Tries to load `path` as a Contra ROM and build a real `Emulator`
/// session. Used both at startup (CLI arg / `./baserom.nes`) and at
/// runtime (the no-ROM screen's "Load ROM..." button and drag-and-drop) -
/// one code path for "does this file work", so a ROM picked at runtime is
/// validated exactly the same way as one passed on the command line.
/// `Err` carries a short, user-facing reason suitable for the no-ROM
/// screen's error line.
fn try_load_rom(path: &Path, rewind_capacity: usize, audio_sample_rate: f64) -> Result<Session, String> {
    let rom = contra_assets::NesRom::load(path).map_err(|e| format!("could not load: {e}"))?;
    if rom.mapper != 2 {
        return Err(format!("mapper {} not supported (only mapper 2 / UxROM)", rom.mapper));
    }
    log::info!(
        "Loaded {} (mapper {}, {} KiB PRG, MD5 {})",
        path.display(),
        rom.mapper,
        rom.prg_rom.len() / 1024,
        rom.md5_hex
    );
    let mirroring = if rom.vertical_mirroring { Mirroring::Vertical } else { Mirroring::Horizontal };
    let nes = Nes::new_with_audio(rom.prg_rom, mirroring, audio_sample_rate);
    Ok(Session::Emulator {
        nes: Box::new(nes),
        save_mgr: SaveStateManager::new(rewind_capacity.max(1)),
        frame_count: 0,
    })
}

/// Shared by the no-ROM screen's "Load ROM..." button and drag-and-drop:
/// tries `path`, and on success replaces `session` and updates the window
/// title in place; on failure records a short reason in `last_load_error`
/// so the no-ROM screen can show it instead of failing silently.
fn load_rom_into_session(
    path: &Path,
    rewind_capacity: usize,
    audio_sample_rate: f64,
    session: &mut Session,
    window: &winit::window::Window,
    last_load_error: &mut Option<String>,
    routine: &mut GameRoutine,
) {
    match try_load_rom(path, rewind_capacity, audio_sample_rate) {
        Ok(loaded) => {
            *session = loaded;
            *last_load_error = None;
            window.set_title("contra-rewired");
            // In case Escape/Tab was pressed while staring at the no-ROM
            // screen (which has no menu of its own to open) - start the
            // freshly loaded game unpaused rather than inheriting a
            // dangling Paused state from before any ROM existed to pause.
            *routine = GameRoutine::Playing;
        }
        Err(e) => {
            log::error!("{}: {e}", path.display());
            *last_load_error = Some(e);
        }
    }
}

fn load_session(args: &Args, rewind_capacity: usize, audio_sample_rate: f64) -> Session {
    let Some(path) = resolve_rom_path(args) else {
        log::info!("No ROM specified and no ./baserom.nes found - showing the Load ROM screen. Pass a ROM path, drop a ./baserom.nes next to the executable, or use the in-app file picker.");
        return placeholder_session(rewind_capacity);
    };

    match try_load_rom(&path, rewind_capacity, audio_sample_rate) {
        Ok(session) => session,
        Err(e) => {
            log::error!("{}: {e} - showing the Load ROM screen.", path.display());
            placeholder_session(rewind_capacity)
        }
    }
}

/// Scans `./mods/` and loads every mod that has a Lua entry script. Every
/// found mod starts enabled; toggle them from the pause menu's Mods tab
/// (session-only for now - not yet persisted across launches, see
/// ROADMAP.md).
#[cfg(feature = "mods")]
pub struct LoadedMod {
    pub id: String,
    pub name: String,
    pub enabled: bool,
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
        loaded.push(LoadedMod { id: m.manifest.id.clone(), name: m.manifest.name.clone(), enabled: true, host });
    }
    loaded
}

/// Fires `frame_tick` on every loaded mod and applies whatever PPU writes
/// it queued (see `contra_mods::script::LuaModHost`). No-op for the
/// placeholder session (no `Nes` to poke) or if `mods` wasn't enabled at
/// build time.
#[cfg(feature = "mods")]
fn run_mods(mods: &[LoadedMod], session: &mut Session) {
    let Session::Emulator { nes, frame_count, .. } = session else {
        return;
    };
    for m in mods {
        if !m.enabled {
            continue;
        }
        m.host.set_frame(*frame_count);
        m.host.set_ram_snapshot(nes.ram());
        if let Err(e) = m.host.fire(contra_mods::script::ModEvent::FrameTick) {
            log::error!("mod '{}': runtime error: {e}", m.id);
            continue;
        }
        for (addr, value) in m.host.take_pending_ppu_writes() {
            nes.poke_ppu(addr, value);
        }
        for (addr, value) in m.host.take_pending_ram_writes() {
            nes.poke_ram(addr, value);
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

#[cfg(feature = "mods")]
type LoadedModsVec = Vec<LoadedMod>;
#[cfg(not(feature = "mods"))]
type LoadedModsVec = Vec<()>;

/// Read-only view of the loaded mods for `menu.rs`'s Mods tab - decoupled
/// from the `mods` Cargo feature so `menu.rs` never needs to know it exists.
#[cfg(feature = "mods")]
fn mod_entries(mods: &[LoadedMod]) -> Vec<ModEntry> {
    mods.iter().map(|m| ModEntry { name: m.name.clone(), enabled: m.enabled }).collect()
}
#[cfg(not(feature = "mods"))]
fn mod_entries(_mods: &[()]) -> Vec<ModEntry> {
    Vec::new()
}

#[cfg(feature = "mods")]
fn toggle_mod(mods: &mut [LoadedMod], idx: usize) {
    if let Some(m) = mods.get_mut(idx) {
        m.enabled = !m.enabled;
    }
}
#[cfg(not(feature = "mods"))]
fn toggle_mod(_mods: &mut [()], _idx: usize) {}

/// Applies the handful of [`MenuAction`]s that need state `menu.rs` doesn't
/// own (the live `Nes`, the mod list). `Resume` and `LoadRom` are handled
/// inline where they're produced instead (they need the `GameRoutine`/
/// window handle respectively, and `LoadRom` blocks on a native dialog).
fn apply_menu_action(action: &MenuAction, session: &mut Session, loaded_mods: &mut LoadedModsVec) {
    match action {
        MenuAction::ToggleMod(idx) => toggle_mod(loaded_mods, *idx),
        MenuAction::WeaponDelta(player, delta) => {
            if let Session::Emulator { nes, .. } = session {
                let addr = match player {
                    menu::Player::P1 => RAM_P1_CURRENT_WEAPON,
                    menu::Player::P2 => RAM_P2_CURRENT_WEAPON,
                };
                let current = (nes.peek_ram(addr) & 0x0F) as i32;
                let count = WEAPON_NAMES.len() as i32;
                let next = (current + delta).rem_euclid(count) as u8;
                nes.poke_ram(addr, next);
            }
        }
        MenuAction::LivesDelta(player, delta) => {
            if let Session::Emulator { nes, .. } = session {
                let addr = match player {
                    menu::Player::P1 => RAM_P1_NUM_LIVES,
                    menu::Player::P2 => RAM_P2_NUM_LIVES,
                };
                let current = nes.peek_ram(addr) as i32;
                nes.poke_ram(addr, (current + delta).clamp(0, 99) as u8);
            }
        }
        MenuAction::ContinuesDelta(delta) => {
            if let Session::Emulator { nes, .. } = session {
                let current = nes.peek_ram(RAM_NUM_CONTINUES) as i32;
                nes.poke_ram(RAM_NUM_CONTINUES, (current + delta).clamp(0, 9) as u8);
            }
        }
        // Handled by the caller, not here - see doc comment above.
        MenuAction::Resume | MenuAction::LoadRom => {}
    }
}

/// Converts the NES framebuffer (`0x00RRGGBB` per pixel) into an
/// `egui::ColorImage` for upload as a GPU texture.
fn framebuffer_to_color_image(fb: &[u32], w: usize, h: usize) -> egui::ColorImage {
    let mut pixels = Vec::with_capacity(w * h);
    for &px in &fb[..w * h] {
        let r = ((px >> 16) & 0xFF) as u8;
        let g = ((px >> 8) & 0xFF) as u8;
        let b = (px & 0xFF) as u8;
        pixels.push(egui::Color32::from_rgb(r, g, b));
    }
    egui::ColorImage { size: [w, h], pixels }
}

/// Same fill-vs-pixel-perfect-vs-zoom sizing `blit_scaled` used with
/// `softbuffer`, just computed in `egui` points instead of physical
/// pixels so it drives an `egui::Painter` image draw instead of a raw CPU
/// pixel loop:
/// - **Dynamic fill** (default): scale is fractional, chosen to cover as
///   much of the window as possible while preserving aspect ratio.
/// - **Pixel perfect** (opt-in): scale is floored to a whole number, crisp
///   NES pixels, possible letterbox bars.
///
/// `zoom_percent` (50-300) is an extra multiplier on top of either mode.
fn game_image_rect(screen: egui::Rect, internal_w: f32, internal_h: f32, settings: &Settings) -> egui::Rect {
    let zoom = (settings.zoom_percent as f32 / 100.0).max(0.01);
    let fit_scale = (screen.width() / internal_w).min(screen.height() / internal_h);
    let scale = if settings.pixel_perfect { fit_scale.floor().max(1.0) } else { fit_scale } * zoom;
    let size = egui::vec2(internal_w * scale, internal_h * scale);
    egui::Rect::from_center_size(screen.center(), size)
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
    let mut loaded_mods = load_mods();
    let window_title = match &session {
        Session::Emulator { .. } => "contra-rewired",
        Session::Placeholder { .. } => "contra-rewired - load a ROM to play",
    };

    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                INTERNAL_W * initial_scale,
                INTERNAL_H * initial_scale,
            ))
            .build(&event_loop)?,
    );

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
    let surface = instance.create_surface(window.clone())?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .ok_or_else(|| anyhow::anyhow!("no compatible GPU adapter found"))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))?;

    let surface_caps = surface.get_capabilities(&adapter);
    // egui-wgpu wants a non-sRGB target format (it does its own sRGB-aware
    // blending in the shader) - an sRGB swapchain format double-applies
    // gamma correction, which egui itself warns about at startup.
    let surface_format = surface_caps.formats.iter().find(|f| !f.is_srgb()).copied().unwrap_or(surface_caps.formats[0]);
    let initial_size = window.inner_size();
    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: initial_size.width.max(1),
        height: initial_size.height.max(1),
        present_mode: wgpu::PresentMode::AutoVsync,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &surface_config);

    let egui_ctx = egui::Context::default();
    let mut egui_state = egui_winit::State::new(egui_ctx.clone(), egui::ViewportId::ROOT, &window, None, None);
    let mut egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1);
    let mut fb_texture: Option<egui::TextureHandle> = None;

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
    let mut last_load_error: Option<String> = None;

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
            Event::WindowEvent { event, .. } => {
                let menu_active = routine == GameRoutine::Paused || matches!(session, Session::Placeholder { .. });
                let egui_consumed = if menu_active {
                    let response = egui_state.on_window_event(&window, &event);
                    if response.repaint {
                        window.request_redraw();
                    }
                    response.consumed
                } else {
                    false
                };

                match event {
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

                        if !egui_consumed {
                            if let PhysicalKey::Code(key_code) = physical_key {
                                if is_down && (key_code == KeyCode::Escape || key_code == KeyCode::Tab) {
                                    routine = match routine {
                                        GameRoutine::Playing => routine.transition(GameEvent::PausePressed),
                                        GameRoutine::Paused => routine.transition(GameEvent::ResumePressed),
                                        other => other,
                                    };
                                    window.request_redraw();
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
                    }
                    WindowEvent::Resized(size) => {
                        if size.width > 0 && size.height > 0 {
                            surface_config.width = size.width;
                            surface_config.height = size.height;
                            surface.configure(&device, &surface_config);
                        }
                    }
                    WindowEvent::DroppedFile(path) => {
                        if matches!(session, Session::Placeholder { .. }) {
                            load_rom_into_session(&path, rewind_capacity, audio_sample_rate, &mut session, &window, &mut last_load_error, &mut routine);
                            window.request_redraw();
                        }
                    }
                    WindowEvent::RedrawRequested => {
                        redraw(
                            &window,
                            &device,
                            &queue,
                            &surface,
                            &surface_config,
                            &egui_ctx,
                            &mut egui_state,
                            &mut egui_renderer,
                            &mut fb_texture,
                            &mut session,
                            &mut menu_state,
                            &mut settings,
                            &mut loaded_mods,
                            &mut routine,
                            &mut last_load_error,
                            rewind_capacity,
                            audio_sample_rate,
                        );
                    }
                    _ => {}
                }
            }
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
                        // The menu is egui-driven (see WindowEvent above) -
                        // nothing to poll here every simulated frame.
                        // Gameplay input/stepping is simply suspended while
                        // paused.
                    } else if routine.accepts_gameplay_input() {
                        if let Session::Emulator { nes, .. } = &mut session {
                            // Widescreen ON always targets the max safe
                            // width immediately, regardless of the current
                            // window shape - decoupled from window size so
                            // toggling it has an instant, visible effect.
                            nes.set_wide_width(if settings.widescreen { contra_nes::EXTENDED_WIDTH } else { contra_nes::SCREEN_W });
                            nes.set_unlimited_sprites(settings.unlimited_sprites);
                            session.step(&action_state, rewind_enabled);
                            run_mods(&loaded_mods, &mut session);
                            let samples = session.drain_audio();
                            if let Some(audio) = &audio_output {
                                if !settings.audio_muted {
                                    audio.push_samples(&samples);
                                }
                            }
                        }
                        // Session::Placeholder: no ROM loaded, nothing to
                        // step - the no-ROM screen is static aside from
                        // widget hover, which egui redraws on its own.
                    }
                    accumulator -= frame_duration;
                    stepped = true;
                }

                if stepped && matches!(session, Session::Emulator { .. }) {
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

/// Runs one `egui` frame: builds the widget tree (game background image +
/// pause menu / no-ROM screen as applicable), applies any resulting
/// [`MenuAction`]s, and paints via `egui-wgpu`. Split out of the event
/// loop closure purely for readability - it owns no state of its own.
#[allow(clippy::too_many_arguments)]
fn redraw(
    window: &winit::window::Window,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    surface: &wgpu::Surface,
    surface_config: &wgpu::SurfaceConfiguration,
    egui_ctx: &egui::Context,
    egui_state: &mut egui_winit::State,
    egui_renderer: &mut egui_wgpu::Renderer,
    fb_texture: &mut Option<egui::TextureHandle>,
    session: &mut Session,
    menu_state: &mut MenuState,
    settings: &mut Settings,
    loaded_mods: &mut LoadedModsVec,
    routine: &mut GameRoutine,
    last_load_error: &mut Option<String>,
    rewind_capacity: usize,
    audio_sample_rate: f64,
) {
    let is_placeholder = matches!(session, Session::Placeholder { .. });

    if let Session::Emulator { nes, .. } = session {
        let (fb, w) = if nes.wide_width() > contra_nes::SCREEN_W && !nes.wide_framebuffer().is_empty() {
            (nes.wide_framebuffer(), nes.wide_width())
        } else {
            (nes.framebuffer(), contra_nes::SCREEN_W)
        };
        let image = framebuffer_to_color_image(fb, w, contra_nes::SCREEN_H);
        match fb_texture {
            Some(handle) => handle.set(image, egui::TextureOptions::NEAREST),
            None => *fb_texture = Some(egui_ctx.load_texture("nes-fb", image, egui::TextureOptions::NEAREST)),
        }
    }

    let prev_widescreen = settings.widescreen;
    let prev_fullscreen = settings.fullscreen;
    let mut actions: Vec<MenuAction> = Vec::new();
    let mut pending_rom_pick = false;

    let raw_input = egui_state.take_egui_input(window);
    let full_output = egui_ctx.run(raw_input, |ctx| {
        if is_placeholder {
            actions = menu::no_rom_screen(ctx, last_load_error.as_deref());
        } else {
            if let Some(tex) = fb_texture {
                egui::CentralPanel::default().frame(egui::Frame::none().fill(egui::Color32::BLACK)).show(ctx, |ui| {
                    let screen = ui.max_rect();
                    let internal_w = tex.size()[0] as f32;
                    let internal_h = tex.size()[1] as f32;
                    let rect = game_image_rect(screen, internal_w, internal_h, settings);
                    ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                });
            }
            if *routine == GameRoutine::Paused {
                let mods_view = mod_entries(loaded_mods);
                let debug_info = if let Session::Emulator { nes, .. } = session {
                    Some(DebugInfo {
                        p1_lives: nes.peek_ram(RAM_P1_NUM_LIVES),
                        p1_weapon: nes.peek_ram(RAM_P1_CURRENT_WEAPON) & 0x0F,
                        p2_lives: nes.peek_ram(RAM_P2_NUM_LIVES),
                        p2_weapon: nes.peek_ram(RAM_P2_CURRENT_WEAPON) & 0x0F,
                        continues: nes.peek_ram(RAM_NUM_CONTINUES),
                    })
                } else {
                    None
                };
                actions = menu::pause_menu(ctx, menu_state, settings, &mods_view, debug_info.as_ref());
            }
        }
    });

    for action in &actions {
        match action {
            MenuAction::Resume => *routine = routine.transition(GameEvent::ResumePressed),
            MenuAction::LoadRom => pending_rom_pick = true,
            other => apply_menu_action(other, session, loaded_mods),
        }
    }

    if pending_rom_pick {
        if let Some(path) = rfd::FileDialog::new().add_filter("NES ROM", &["nes"]).pick_file() {
            load_rom_into_session(&path, rewind_capacity, audio_sample_rate, session, window, last_load_error, routine);
        }
    }

    if settings.fullscreen != prev_fullscreen {
        window.set_fullscreen(settings.fullscreen.then_some(Fullscreen::Borderless(None)));
    }
    if settings.widescreen != prev_widescreen {
        // Flipping the setting alone changes what `render_scanline` draws,
        // but if the window is still sized for the narrow 256px view, the
        // fill-scaling in `game_image_rect` just draws the wider content
        // smaller to fit - visually a much smaller change than intended.
        // Resize the window's width to match, at whatever per-pixel scale
        // is already in effect (so a window the user has already resized/
        // zoomed keeps that scale, it just gets proportionally wider or
        // narrower) - the way other NES PC ports grow their window when
        // widescreen is turned on. No-op in fullscreen, where the
        // compositor owns the size.
        let (old_internal_w, new_internal_w) = if settings.widescreen {
            (contra_nes::SCREEN_W, contra_nes::EXTENDED_WIDTH)
        } else {
            (contra_nes::EXTENDED_WIDTH, contra_nes::SCREEN_W)
        };
        let current = window.inner_size();
        if current.width > 0 {
            let scale = current.width as f64 / old_internal_w as f64;
            let new_width = (new_internal_w as f64 * scale).round() as u32;
            let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(new_width, current.height));
        }
    }

    egui_state.handle_platform_output(window, full_output.platform_output);
    let clipped_primitives = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
    let screen_descriptor = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [surface_config.width, surface_config.height],
        pixels_per_point: full_output.pixels_per_point,
    };

    for (id, delta) in &full_output.textures_delta.set {
        egui_renderer.update_texture(device, queue, *id, delta);
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui-encoder") });
    egui_renderer.update_buffers(device, queue, &mut encoder, &clipped_primitives, &screen_descriptor);

    let frame = match surface.get_current_texture() {
        Ok(f) => f,
        Err(_) => {
            surface.configure(device, surface_config);
            return;
        }
    };
    let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
    {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("main"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        egui_renderer.render(&mut rpass, &clipped_primitives, &screen_descriptor);
    }
    for id in &full_output.textures_delta.free {
        egui_renderer.free_texture(id);
    }

    queue.submit(std::iter::once(encoder.finish()));
    frame.present();
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
            // Pause/QuickSave/QuickLoad/Rewind/FrameAdvance aren't routed
            // through `update_action_state`'s `Action` combos (Pause and
            // FrameAdvance's KeyCodes are handled directly in the event
            // loop instead, and FrameAdvance isn't wired to anything yet),
            // but `default_keyboard_p1()` still binds all of them, so they
            // must be producible here too or this test's own premise -
            // "every default binding is reachable" - is broken.
            KeyCode::Escape,
            KeyCode::F5,
            KeyCode::F9,
            KeyCode::Backspace,
            KeyCode::F6,
        ];
        let reachable: HashSet<String> = live_codes.into_iter().map(|k| key_code_name(PhysicalKey::Code(k))).collect();
        for inputs in bindings.map.values() {
            for input in inputs {
                if let PhysicalInput::Keyboard(k) = input {
                    assert!(reachable.contains(k), "binding {k:?} is not producible by any live KeyCode via key_code_name");
                }
            }
        }
    }
}
