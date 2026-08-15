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

// Release builds on Windows shouldn't pop a console window alongside the
// game window - that's a `cargo run`/debugging artifact, not something a
// player who just downloaded the .exe should see. Kept in debug builds so
// `println!`/`log`/panic output during development still has somewhere to
// go without redirecting it elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod menu;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use clap::Parser;

use contra_core::config::{Config, ScalingMode};
use contra_core::input::{Action, ActionState, Bindings, PhysicalInput};
use contra_core::physics::{LevelLocation, PlayerPhysics};
use contra_core::savestate::{SaveState, SaveStateManager, SaveStateMeta, SlotId, SAVESTATE_FORMAT_VERSION};
use contra_core::state_machine::{GameEvent, GameRoutine};

use contra_nes::{Mirroring, Nes, NesSnapshot};

use gilrs::{Axis, Button, Gilrs};
use menu::{DebugInfo, MenuAction, MenuState, ModEntry, Settings};
use winit::event::{ElementState, Event, KeyEvent, WindowEvent};
use winit::event_loop::EventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Icon, WindowBuilder};

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
// SPRITE_X_POS/SPRITE_Y_POS: shared position arrays, index 0 = P1, 1 = P2
// (the same indexing `soldier_generation_01` reads player position with -
// see docs/FIDELITY.md). Backs the stats overlay's coordinates readout.
const RAM_SPRITE_X_POS: u16 = 0x0334;
const RAM_SPRITE_Y_POS: u16 = 0x031A;
// Jump-to-stage pokes this (+ LEVEL_ROUTINE_INDEX below, + the same RAM
// clear level_routine_05 itself does between levels) to fake a level
// transition. Got this wrong twice before it actually worked - see
// docs/FIDELITY.md for the full account - but the real root cause turned
// out to live in the PPU, not here: `Ppu::tile_cache` (the true-ultrawide
// memory of tiles a level has genuinely displayed) was only ever
// invalidated on a big single-frame scroll jump, which a jump landing back
// near scroll-0 (same as most levels' starting position) could dodge
// entirely, leaving the old level's tiles cached and shown alongside the
// new level's live-read ones - the "colliding/flickering tiles" that were
// reported. Fixed by also clearing the cache on every mask-off -> mask-on
// transition (`Ppu::write_register`, PPUMASK) - the same universal signal
// every NES game already uses to hide VRAM rewrites during a level/screen
// change, so it catches this without depending on scroll math at all.
// Re-verified with every-single-frame (not sampled) captures across the
// jump, at 700px widescreen (the case most likely to expose stale cache
// entries), on two different target stages - clean both times.
const RAM_CURRENT_LEVEL: u16 = 0x30;
// See `level_routine_05` in the disassembly: 0 restarts the new level at
// level_routine_00 (header/palette/graphics load), same as a real level
// completion does.
const RAM_LEVEL_ROUTINE_INDEX: u16 = 0x2C;

#[derive(Parser)]
#[command(author, version, about = "Contra: Rewired PC front-end. Pass your own legally-dumped Contra (NES) ROM to play it for real.")]
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
            window.set_title("Contra: Rewired");
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

/// Scans `./mods/` and loads every mod that has a Lua entry script. Mods
/// are opt-in: a mod starts *disabled* unless its id is in `enabled_ids`
/// (from `config.toml`'s `[mods] enabled_ids`, see `ModsConfig`) - dropping
/// a script into `./mods/` should never make it start running without the
/// player explicitly turning it on first, in the pause menu's Mods tab
/// (which updates that same list - `main`'s existing save-on-close
/// (`config.save`) already persists it, no separate save path needed).
#[cfg(feature = "mods")]
pub struct LoadedMod {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    host: contra_mods::script::LuaModHost,
}

/// Reorders `loaded` in place to match `order` (a list of mod IDs, top to
/// bottom - see `ModsConfig::order`'s doc comment): any ID in `order` that
/// `loaded` actually has moves to the front, in `order`'s sequence; every
/// other mod (new install, or simply never reordered) keeps its natural
/// registry-scan position relative to the others like it. `order` doesn't
/// need to be exhaustive or even valid - IDs it lists that no longer exist
/// are silently ignored.
#[cfg(feature = "mods")]
fn apply_mod_order(loaded: &mut Vec<LoadedMod>, order: &[String]) {
    let mut reordered = Vec::with_capacity(loaded.len());
    for id in order {
        if let Some(pos) = loaded.iter().position(|m| &m.id == id) {
            reordered.push(loaded.remove(pos));
        }
    }
    reordered.append(loaded);
    *loaded = reordered;
}

#[cfg(feature = "mods")]
fn load_mods(enabled_ids: &[String]) -> Vec<LoadedMod> {
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
        let enabled = enabled_ids.iter().any(|id| id == &m.manifest.id);
        log::info!(
            "loaded mod: {} ({}) - {} [{}]",
            m.manifest.name,
            m.manifest.id,
            m.manifest.description,
            if enabled { "enabled" } else { "disabled - enable it in the pause menu's Mods tab" }
        );
        loaded.push(LoadedMod { id: m.manifest.id.clone(), name: m.manifest.name.clone(), enabled, host });
    }
    loaded
}

/// Cross-frame state purely to detect the *moments* `run_mods` fires
/// `stage_start`/`stage_clear`/`player_hit` from - a single RAM read only
/// ever says "this is the value right now", never "this just changed", so
/// the previous frame's values have to be remembered somewhere. `None`
/// means "not observed yet" (the very first frame after a ROM loads),
/// which intentionally suppresses firing anything - there's no real
/// transition to report yet, just an initial value.
#[derive(Default)]
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
struct ModEventTracker {
    stage: Option<u8>,
    p1_lives: Option<u8>,
    p2_lives: Option<u8>,
}

/// Fires `frame_tick` on every loaded mod, plus `stage_start`/`stage_clear`
/// (when `RAM_CURRENT_LEVEL` changes between frames) and `player_hit`
/// (when either player's lives count drops), and applies whatever PPU/RAM
/// writes got queued in response (see `contra_mods::script::LuaModHost`).
/// No-op for the placeholder session (no `Nes` to poke) or if `mods` wasn't
/// enabled at build time.
#[cfg(feature = "mods")]
fn run_mods(mods: &[LoadedMod], session: &mut Session, tracker: &mut ModEventTracker) {
    let Session::Emulator { nes, frame_count, .. } = session else {
        return;
    };
    let stage = nes.peek_ram(RAM_CURRENT_LEVEL);
    let p1_lives = nes.peek_ram(RAM_P1_NUM_LIVES);
    let p2_lives = nes.peek_ram(RAM_P2_NUM_LIVES);
    let stage_changed = tracker.stage.is_some_and(|prev| prev != stage);
    let prev_stage = tracker.stage;
    let p1_hit = tracker.p1_lives.is_some_and(|prev| p1_lives < prev);
    let p2_hit = tracker.p2_lives.is_some_and(|prev| p2_lives < prev);
    tracker.stage = Some(stage);
    tracker.p1_lives = Some(p1_lives);
    tracker.p2_lives = Some(p2_lives);

    for m in mods {
        if !m.enabled {
            continue;
        }
        m.host.set_frame(*frame_count);
        m.host.set_ram_snapshot(nes.ram());
        let fire_result = (|| {
            m.host.fire(contra_mods::script::ModEvent::FrameTick)?;
            if stage_changed {
                // Both fire from the same "CURRENT_LEVEL changed"
                // observation - see `LuaModHost::fire_stage_clear`'s doc
                // comment for why there's no separate "you cleared it"
                // signal to tell them apart.
                m.host.fire_stage_clear(prev_stage.unwrap_or(stage))?;
                m.host.fire_stage_start(stage)?;
            }
            if p1_hit {
                m.host.fire_player_hit(0, p1_lives)?;
            }
            if p2_hit {
                m.host.fire_player_hit(1, p2_lives)?;
            }
            Ok::<(), contra_mods::script::ScriptError>(())
        })();
        if let Err(e) = fire_result {
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
fn run_mods(_mods: &[()], _session: &mut Session, _tracker: &mut ModEventTracker) {}

/// The inverse of [`apply_mod_order`] - captures the current execution
/// order as a list of IDs, for `config.mods.order` right before the
/// existing save-on-close call.
#[cfg(feature = "mods")]
fn mod_order(loaded: &[LoadedMod]) -> Vec<String> {
    loaded.iter().map(|m| m.id.clone()).collect()
}
#[cfg(not(feature = "mods"))]
fn mod_order(_loaded: &[()]) -> Vec<String> {
    Vec::new()
}

#[cfg(not(feature = "mods"))]
fn apply_mod_order(_loaded: &mut Vec<()>, _order: &[String]) {}

#[cfg(not(feature = "mods"))]
fn load_mods(_enabled_ids: &[String]) -> Vec<()> {
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
fn toggle_mod(mods: &mut [LoadedMod], idx: usize, enabled_ids: &mut Vec<String>) {
    if let Some(m) = mods.get_mut(idx) {
        m.enabled = !m.enabled;
        if m.enabled {
            if !enabled_ids.iter().any(|id| id == &m.id) {
                enabled_ids.push(m.id.clone());
            }
        } else {
            enabled_ids.retain(|id| id != &m.id);
        }
    }
}
#[cfg(not(feature = "mods"))]
fn toggle_mod(_mods: &mut [()], _idx: usize, _enabled_ids: &mut Vec<String>) {}

/// Swaps mod `idx` with its neighbor `idx + delta` - see
/// `MenuAction::MoveMod`'s doc comment. Only ever called with `delta` `-1`/
/// `1` from the Mods tab's up/down buttons, which already disable
/// themselves at the list's boundaries, but `checked_add`/bounds-check
/// anyway rather than trust the caller.
#[cfg(feature = "mods")]
fn move_mod(mods: &mut [LoadedMod], idx: usize, delta: i32) {
    let Some(other) = idx.checked_add_signed(delta as isize) else {
        return;
    };
    if other < mods.len() {
        mods.swap(idx, other);
    }
}
#[cfg(not(feature = "mods"))]
fn move_mod(_mods: &mut [()], _idx: usize, _delta: i32) {}

/// Steps exactly one simulated frame of `Session::Emulator` gameplay -
/// widescreen/sprite settings, the step itself, mods, and audio. Shared by
/// the normal per-tick loop and the frame-advance hotkey in `main`'s
/// `AboutToWait` handler, so freezing/advancing behaves identically to a
/// normal frame in every way except *when* it happens. No-op for
/// `Session::Placeholder` - nothing to step.
/// How wide widescreen should render to fill the *current* window - true
/// ultrawide (see `contra_nes::MAX_WIDE_WIDTH`) means this should actually
/// track the window's live aspect ratio again, unlike the fixed-cap
/// approach from when `EXTENDED_WIDTH` (380px) was both the target and the
/// hard ceiling: rendering 1024px-wide for a narrow window would just be
/// wasted work, scaled back down by `game_image_rect`'s letterboxing.
fn target_wide_width(window: &winit::window::Window) -> usize {
    let size = window.inner_size();
    if size.height == 0 {
        return contra_nes::SCREEN_W;
    }
    let aspect = size.width as f64 / size.height as f64;
    ((contra_nes::SCREEN_H as f64 * aspect).round() as usize).clamp(contra_nes::SCREEN_W, contra_nes::MAX_WIDE_WIDTH)
}

#[allow(clippy::too_many_arguments)]
fn step_gameplay_frame(
    session: &mut Session,
    action_state: &ActionState,
    rewind_enabled: bool,
    loaded_mods: &LoadedModsVec,
    mod_event_tracker: &mut ModEventTracker,
    settings: &Settings,
    audio_output: &Option<audio::AudioOutput>,
    target_wide_width: usize,
) {
    let Session::Emulator { nes, .. } = session else {
        return;
    };
    nes.set_wide_width(if settings.widescreen { target_wide_width } else { contra_nes::SCREEN_W });
    nes.set_unlimited_sprites(settings.unlimited_sprites);
    session.step(action_state, rewind_enabled);
    run_mods(loaded_mods, session, mod_event_tracker);
    let samples = session.drain_audio();
    if let Some(audio) = audio_output {
        if !settings.audio_muted {
            audio.push_samples(&samples);
        }
    }
}

/// Applies the handful of [`MenuAction`]s that need state `menu.rs` doesn't
/// own (the live `Nes`, the mod list). `Resume` and `LoadRom` are handled
/// inline where they're produced instead (they need the `GameRoutine`/
/// window handle respectively, and `LoadRom` blocks on a native dialog).
fn apply_menu_action(action: &MenuAction, session: &mut Session, loaded_mods: &mut LoadedModsVec, enabled_mod_ids: &mut Vec<String>) {
    match action {
        MenuAction::ToggleMod(idx) => toggle_mod(loaded_mods, *idx, enabled_mod_ids),
        MenuAction::MoveMod(idx, delta) => move_mod(loaded_mods, *idx, *delta),
        MenuAction::SetWeapon(player, id) => {
            if let Session::Emulator { nes, .. } = session {
                let addr = match player {
                    menu::Player::P1 => RAM_P1_CURRENT_WEAPON,
                    menu::Player::P2 => RAM_P2_CURRENT_WEAPON,
                };
                // Low nibble is weapon, bit 4 is the "R" rapid-fire flag
                // (same byte - see `ram.asm`) - preserve whatever rapid
                // fire is currently set to, only replace the weapon.
                let rapid_bit = nes.peek_ram(addr) & 0x10;
                nes.poke_ram(addr, (*id & 0x0F) | rapid_bit);
            }
        }
        MenuAction::ToggleRapidFire(player) => {
            if let Session::Emulator { nes, .. } = session {
                let addr = match player {
                    menu::Player::P1 => RAM_P1_CURRENT_WEAPON,
                    menu::Player::P2 => RAM_P2_CURRENT_WEAPON,
                };
                nes.poke_ram(addr, nes.peek_ram(addr) ^ 0x10);
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
        MenuAction::JumpToStage(stage) => {
            if let Session::Emulator { nes, .. } = session {
                // Mirrors what `level_routine_05` (level complete) itself
                // clears between levels, so a jump leaves RAM in the same
                // shape a real transition would.
                let pre_jump = nes.snapshot();
                for addr in 0x40..=0xF0u16 {
                    nes.poke_ram(addr, 0);
                }
                for addr in 0x300..0x600u16 {
                    nes.poke_ram(addr, 0);
                }
                // CPU_GRAPHICS_BUFFER ($0700, 80 bytes) + the 112 reserved
                // bytes after it, stopping short of PALETTE_CPU_BUFFER
                // ($07c0) and the high-score bytes past that (real
                // persistent state, not transition scratch). This is the
                // actual root cause of the Base 1/Base 2 hang, found via
                // `Nes::run_frame_with_pc_trace`: the graphics-buffer
                // flush loop (`$cc60-$cc7f` in the fixed bank) walks this
                // buffer via an 8-bit index until it reads a `#$00`
                // "no more data" byte; if that index wraps all the way
                // around $0700-$07ff without ever landing on a zero (which
                // happened reliably for these two levels' supertile data,
                // but not the other six), the flush - and with it the
                // entire level-routine dispatch, since nothing else runs
                // until it returns - never terminates. Zeroing the buffer
                // first guarantees a `#$00` is always reachable. See
                // docs/FIDELITY.md for the full account.
                for addr in 0x0700..0x07C0u16 {
                    nes.poke_ram(addr, 0);
                }
                nes.poke_ram(0x21, 0); // GRAPHICS_BUFFER_OFFSET
                nes.poke_ram(0x23, 0); // GRAPHICS_BUFFER_MODE
                nes.poke_ram(RAM_CURRENT_LEVEL, *stage);
                nes.poke_ram(RAM_LEVEL_ROUTINE_INDEX, 0);
                // The real level-load sequence this triggers is Contra's own
                // code, unmodified - it's genuinely ~30-60 real seconds of
                // score-flash/palette-load/supertile-render, same as a real
                // level-complete transition, and looks exactly as rough
                // mid-transition as the original game does (a blanked-
                // rendering loading screen isn't a bug here, it's just not
                // meant to be looked at). So: don't make the player sit
                // through it or look at it. Run it silently, as fast as the
                // host CPU can - no audio, no mod events, no presented
                // frames - until real gameplay resumes (`LEVEL_ROUTINE_INDEX
                // == 4`) or the cap is hit, then hand back control exactly
                // once it's actually ready. The jump itself reads as
                // instant; nothing rough about the transition is ever shown.
                nes.set_controller(0, 0);
                nes.set_controller(1, 0);
                let mut reached_gameplay = false;
                for _ in 0..3600 {
                    nes.run_frame();
                    if nes.peek_ram(RAM_LEVEL_ROUTINE_INDEX) == 4 {
                        reached_gameplay = true;
                        break;
                    }
                }
                let _ = nes.take_audio_samples(); // discard - see above
                if !reached_gameplay {
                    // Known to happen for Base 1/Base 2 (see
                    // docs/FIDELITY.md) - the Debug tab disables those two
                    // buttons, but this is the backstop for any other
                    // combination that turns out to hang too: undo the jump
                    // entirely rather than strand the player on a screen
                    // that's never coming back.
                    log::error!("jump to stage {}: never reached real gameplay within the frame cap, reverting", stage + 1);
                    nes.restore(&pre_jump);
                }
            }
        }
        // Handled by the caller, not here - see doc comment above.
        MenuAction::Resume | MenuAction::LoadRom => {}
    }
}

/// Decodes the baked-in app icon (see `apps/contra-pc/assets/icon-256.png`)
/// into a `winit::window::Icon` for the title bar/taskbar. `include_bytes!`
/// bakes it into the binary - no runtime file dependency, no missing-icon
/// case to handle at launch. Decode failure just means no icon (falls back
/// to winit's/the OS's default) rather than refusing to start.
fn load_app_icon() -> Option<Icon> {
    const ICON_PNG: &[u8] = include_bytes!("../assets/icon-256.png");
    let decoder = png::Decoder::new(ICON_PNG);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Icon::from_rgba(buf, info.width, info.height).ok()
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

/// Builds the pause menu's live [`Settings`] from what was last saved to
/// `config.toml` (`contra_core::config::PcSettings`) - see that struct's
/// doc comment for why it's a separate flat mirror instead of reusing
/// `VideoConfig`/etc directly.
fn settings_from_pc_config(pc: &contra_core::config::PcSettings) -> Settings {
    Settings {
        widescreen: pc.widescreen,
        unlimited_sprites: pc.unlimited_sprites,
        pixel_perfect: pc.pixel_perfect,
        zoom_percent: pc.zoom_percent,
        fullscreen: pc.fullscreen,
        audio_muted: pc.audio_muted,
        show_hitboxes: pc.show_hitboxes,
        show_stats: pc.show_stats,
        sim_speed_percent: pc.sim_speed_percent,
        scanlines: pc.scanlines,
        crt_filter: pc.crt_filter,
    }
}

/// The inverse of [`settings_from_pc_config`] - called just before
/// `config.save(CONFIG_PATH)` so whatever the player last had set (via
/// menu clicks or hotkeys, doesn't matter which) is what's there on the
/// next launch.
fn pc_config_from_settings(settings: &Settings) -> contra_core::config::PcSettings {
    contra_core::config::PcSettings {
        widescreen: settings.widescreen,
        unlimited_sprites: settings.unlimited_sprites,
        pixel_perfect: settings.pixel_perfect,
        zoom_percent: settings.zoom_percent,
        fullscreen: settings.fullscreen,
        audio_muted: settings.audio_muted,
        show_hitboxes: settings.show_hitboxes,
        show_stats: settings.show_stats,
        sim_speed_percent: settings.sim_speed_percent,
        scanlines: settings.scanlines,
        crt_filter: settings.crt_filter,
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let mut config = Config::load_or_default(CONFIG_PATH);
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
    let mut loaded_mods = load_mods(&config.mods.enabled_ids);
    apply_mod_order(&mut loaded_mods, &config.mods.order);
    let window_title = match &session {
        Session::Emulator { .. } => "Contra: Rewired",
        Session::Placeholder { .. } => "Contra: Rewired - load a ROM to play",
    };

    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        WindowBuilder::new()
            .with_title(window_title)
            .with_inner_size(winit::dpi::LogicalSize::new(
                INTERNAL_W * initial_scale,
                INTERNAL_H * initial_scale,
            ))
            .with_window_icon(load_app_icon())
            .build(&event_loop)?,
    );
    // Set again, explicitly, after the window exists - not redundant on
    // Windows specifically: the taskbar button's icon is applied via a
    // `WM_SETICON` message, and there are known cases where the icon
    // passed to the window *builder* doesn't reliably reach the taskbar
    // until something re-sends that message after the window is fully
    // created and shown (the builder-time icon can still apply to the
    // title bar/alt-tab even when this happens). This costs nothing if
    // the builder's icon already took.
    window.set_window_icon(load_app_icon());

    // `InstanceFlags::default()` auto-enables the Vulkan validation layer
    // in debug builds (`debug_assertions`) - real, measurable extra memory
    // and per-draw-call overhead from the validation layer itself, on top
    // of everything else a `cargo build` (vs `--release`) debug binary
    // already costs. Not something a player running a debug build needs;
    // `--release` (`cargo run -p contra-pc --release`) is the number worth
    // judging memory/perf against.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        flags: wgpu::InstanceFlags::empty(),
        ..Default::default()
    });
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
    let mut settings = settings_from_pc_config(&config.pc_settings);
    // Tracked outside `redraw` on purpose - see `apply_toggle_side_effects`.
    // Deliberately *not* seeded from `settings` itself: the window a fresh
    // launch actually creates below is windowed and narrow regardless of
    // what got loaded from config.toml, so starting these at the neutral
    // ("as if nothing's been toggled yet") state means a persisted
    // widescreen/fullscreen preference is naturally detected as a pending
    // change and applied for real on the first `apply_toggle_side_effects`
    // call - instead of the loaded setting silently disagreeing with the
    // window that's actually on screen until the player toggles it twice.
    let mut prev_widescreen = false;
    let mut prev_fullscreen = false;
    let mut menu_state = MenuState::new();
    let mut last_load_error: Option<String> = None;
    let mut rom_dialog_rx: Option<mpsc::Receiver<Option<PathBuf>>> = None;

    let mut gilrs = Gilrs::new().ok();
    if gilrs.is_none() {
        log::warn!("gilrs failed to initialize; gamepad input disabled for this session");
    }

    const BASE_FRAME_DURATION: Duration = Duration::from_nanos(1_000_000_000 / 60);
    let mut last_tick = Instant::now();
    let mut accumulator = Duration::ZERO;
    // Practice tooling: F12 freezes stepping without opening the pause
    // menu (rendering keeps happening, unlike `GameRoutine::Paused`), and
    // `.` steps exactly one simulated frame while frozen - real frame
    // advance, not just slow motion. `sim_speed_percent` (see
    // `menu::Settings`) scales `BASE_FRAME_DURATION` instead, for the
    // "keep playing, just slower/faster" case.
    let mut frozen = false;
    let mut advance_one_frame = false;
    let mut mod_event_tracker = ModEventTracker::default();

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
                        config.pc_settings = pc_config_from_settings(&settings);
                        config.mods.order = mod_order(&loaded_mods);
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
                                // Hotkeys for the toggleable Settings tab
                                // entries - work during gameplay too, not
                                // just while the menu's open, same as
                                // F5/F9/Backspace above. Each one just
                                // flips the same `Settings` field the
                                // matching checkbox is bound to; the
                                // widescreen-resize/fullscreen side effects
                                // (see `apply_toggle_side_effects`) are
                                // applied right after, same as they would
                                // be after a menu click.
                                if is_down {
                                    match key_code {
                                        KeyCode::F1 => settings.widescreen = !settings.widescreen,
                                        KeyCode::F2 => settings.unlimited_sprites = !settings.unlimited_sprites,
                                        KeyCode::F3 => settings.pixel_perfect = !settings.pixel_perfect,
                                        KeyCode::F4 => settings.show_hitboxes = !settings.show_hitboxes,
                                        KeyCode::F6 => settings.scanlines = !settings.scanlines,
                                        KeyCode::F10 => settings.crt_filter = !settings.crt_filter,
                                        KeyCode::F7 => settings.show_stats = !settings.show_stats,
                                        KeyCode::F8 => settings.audio_muted = !settings.audio_muted,
                                        KeyCode::F11 => settings.fullscreen = !settings.fullscreen,
                                        KeyCode::F12 => frozen = !frozen,
                                        KeyCode::Period if frozen => advance_one_frame = true,
                                        _ => {}
                                    }
                                    apply_toggle_side_effects(&settings, &mut prev_widescreen, &mut prev_fullscreen, &window);
                                    window.request_redraw();
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
                        // Defensive resync, not just reactive on `Resized`:
                        // `surface_config` only updates when a `Resized`
                        // event is processed, but `target_wide_width` (and
                        // egui's own layout) read `window.inner_size()`
                        // live every frame. Maximizing or entering
                        // fullscreen can deliver its `Resized` event a
                        // frame or more after the window's actual size has
                        // already changed (OS/compositor-dependent) - in
                        // that gap, wide-mode's target width and egui's
                        // layout would already reflect the new (larger)
                        // size while the wgpu swapchain is still
                        // configured for the old one, which is exactly the
                        // kind of mismatch that shows up as "not filling
                        // the screen" and flicker on the edge that's out
                        // of sync. Checking here, every redraw, means the
                        // swapchain can never be more than one redraw
                        // behind the window's real size.
                        let live_size = window.inner_size();
                        if live_size.width > 0 && live_size.height > 0 && (live_size.width != surface_config.width || live_size.height != surface_config.height) {
                            surface_config.width = live_size.width;
                            surface_config.height = live_size.height;
                            surface.configure(&device, &surface_config);
                        }
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
                            &mut config.mods.enabled_ids,
                            &mut routine,
                            &mut last_load_error,
                            &mut prev_widescreen,
                            &mut prev_fullscreen,
                            &mut rom_dialog_rx,
                        );
                    }
                    _ => {}
                }
            }
            Event::AboutToWait => {
                if let Some(rx) = &rom_dialog_rx {
                    match rx.try_recv() {
                        Ok(Some(path)) => {
                            load_rom_into_session(&path, rewind_capacity, audio_sample_rate, &mut session, &window, &mut last_load_error, &mut routine);
                            rom_dialog_rx = None;
                            window.request_redraw();
                        }
                        Ok(None) => {
                            rom_dialog_rx = None; // dialog closed with no file picked
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => rom_dialog_rx = None,
                    }
                }

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

                let speed = (settings.sim_speed_percent as f64 / 100.0).max(0.01);
                let frame_duration = Duration::from_secs_f64(BASE_FRAME_DURATION.as_secs_f64() / speed);
                let target_wide = target_wide_width(&window);

                let mut stepped = false;

                if advance_one_frame {
                    advance_one_frame = false;
                    let gp = poll_gamepad(gilrs.as_mut());
                    update_action_state(&mut action_state, &bindings, &held_keys, &gp);
                    step_gameplay_frame(&mut session, &action_state, rewind_enabled, &loaded_mods, &mut mod_event_tracker, &settings, &audio_output, target_wide);
                    // Frozen again immediately after - a backlog built up
                    // while frozen shouldn't turn into a burst of extra
                    // steps the instant `frozen` is cleared.
                    accumulator = Duration::ZERO;
                    stepped = true;
                } else if !frozen {
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
                            step_gameplay_frame(&mut session, &action_state, rewind_enabled, &loaded_mods, &mut mod_event_tracker, &settings, &audio_output, target_wide);
                        }
                        accumulator -= frame_duration;
                        stepped = true;
                    }
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

/// Applies the two `Settings` toggles that need more than a field flip -
/// called both right after a hotkey changes one of them (`WindowEvent::
/// KeyboardInput`) and after `redraw`'s `egui` pass, which is the only
/// other place they can change (a menu checkbox click). `prev_widescreen`/
/// `prev_fullscreen` are owned by `main`'s event loop, not `redraw`, for
/// exactly that reason: a diff captured fresh inside `redraw` can't see a
/// change the hotkey handler already applied to `settings` *before*
/// `redraw` ever ran for that frame.
fn apply_toggle_side_effects(settings: &Settings, prev_widescreen: &mut bool, prev_fullscreen: &mut bool, window: &winit::window::Window) {
    if settings.fullscreen != *prev_fullscreen {
        window.set_fullscreen(settings.fullscreen.then_some(Fullscreen::Borderless(None)));
        *prev_fullscreen = settings.fullscreen;
    }
    if settings.widescreen != *prev_widescreen {
        // Flipping the setting alone changes what `render_scanline` draws
        // (see `target_wide_width`, which tracks the window's *current*
        // aspect ratio every frame once widescreen is on), but if the
        // window is still sized narrow, there's nothing wide *to* track -
        // toggling on would barely look different. Resize the window
        // itself here, so there's real width for the live-tracking to
        // pick up on the very next frame: growing to the current
        // monitor's full width when turning on (true ultrawide, if the
        // monitor is one - see `contra_nes::MAX_WIDE_WIDTH`), or back to
        // the narrow 256px aspect at the current vertical scale when
        // turning off. No-op in fullscreen, where the compositor owns the
        // size, and no-op if the monitor size can't be read (rare, but
        // `request_inner_size` with a nonsense size is worse than doing
        // nothing).
        let current = window.inner_size();
        if current.height > 0 {
            let new_width = if settings.widescreen {
                window.current_monitor().map(|m| m.size().width)
            } else {
                Some((current.height as f64 * contra_nes::SCREEN_W as f64 / contra_nes::SCREEN_H as f64).round() as u32)
            };
            if let Some(new_width) = new_width {
                let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(new_width, current.height));
            }
        }
        *prev_widescreen = settings.widescreen;
    }
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
    enabled_mod_ids: &mut Vec<String>,
    routine: &mut GameRoutine,
    last_load_error: &mut Option<String>,
    prev_widescreen: &mut bool,
    prev_fullscreen: &mut bool,
    rom_dialog_rx: &mut Option<mpsc::Receiver<Option<PathBuf>>>,
) {
    let is_placeholder = matches!(session, Session::Placeholder { .. });
    let mut hitboxes: Vec<egui::Rect> = Vec::new();
    let mut stats_text: Option<String> = None;

    if let Session::Emulator { nes, frame_count, .. } = session {
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

        if settings.show_hitboxes {
            // OAM entries in *internal texture* pixel space (same space
            // `game_image_rect` maps to the screen) - the visual sprite
            // bounding box, not necessarily Contra's exact collision box
            // (see `menu::Settings::show_hitboxes`'s doc comment). `+1` on
            // Y and `+ wide_x_offset()` on X match the same placement math
            // `Ppu::render_sprites_line` actually draws with, so the boxes
            // line up in both narrow and wide mode.
            let x_offset = nes.wide_x_offset() as f32;
            let height = nes.sprite_height() as f32;
            let oam = &nes.bus.ppu.oam;
            for i in 0..64 {
                let oam_y = oam[i * 4];
                if oam_y >= 0xEF {
                    continue; // conventional "hidden" Y, not an active sprite
                }
                let x = oam[i * 4 + 3] as f32 + x_offset;
                let y = oam_y as f32 + 1.0;
                hitboxes.push(egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(8.0, height)));
            }
        }

        if settings.show_stats {
            stats_text = Some(format!(
                "FRAME {}\nP1  X:{:>3} Y:{:>3}\nP2  X:{:>3} Y:{:>3}",
                *frame_count,
                nes.peek_ram(RAM_SPRITE_X_POS),
                nes.peek_ram(RAM_SPRITE_Y_POS),
                nes.peek_ram(RAM_SPRITE_X_POS + 1),
                nes.peek_ram(RAM_SPRITE_Y_POS + 1),
            ));
        }
    }

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

                    let scale = rect.width() / internal_w;
                    for hb in &hitboxes {
                        let screen_hb = egui::Rect::from_min_size(
                            rect.min + hb.min.to_vec2() * scale,
                            hb.size() * scale,
                        );
                        ui.painter().rect_stroke(screen_hb, 0.0, egui::Stroke::new(1.5f32, egui::Color32::from_rgb(255, 64, 64)));
                    }

                    if let Some(text) = &stats_text {
                        let pos = rect.min + egui::vec2(6.0, 6.0);
                        ui.painter().text(
                            pos,
                            egui::Align2::LEFT_TOP,
                            text,
                            egui::FontId::monospace(13.0),
                            egui::Color32::from_rgb(255, 224, 128),
                        );
                    }

                    if settings.scanlines {
                        // A faint dark line over every other *NES* pixel
                        // row (not every other screen pixel - drawn at
                        // `scale`, same as everything else here, so it
                        // stays one line per emulated scanline regardless
                        // of zoom/window size). Purely a painter overlay,
                        // no shader or render-pipeline change - cheap
                        // enough that egui's own batching handles it
                        // without a measurable cost.
                        let line_color = egui::Color32::from_black_alpha(90);
                        let mut y = 1;
                        while (y as f32) < internal_h {
                            let line_rect = egui::Rect::from_min_size(
                                rect.min + egui::vec2(0.0, y as f32 * scale),
                                egui::vec2(rect.width(), (scale * 0.4).max(1.0)),
                            );
                            ui.painter().rect_filled(line_rect, 0.0, line_color);
                            y += 2;
                        }
                    }

                    if settings.crt_filter {
                        // Soft vignette: concentric border strokes fading
                        // from the edge inward, same painter-overlay
                        // technique as scanlines above (no shader/render-
                        // pipeline change). Approximates a curved-glass/
                        // off-axis-phosphor falloff well enough at typical
                        // window sizes without the cost or complexity of an
                        // actual radial-gradient shader.
                        let depth = (rect.width().min(rect.height()) * 0.08).max(4.0);
                        let steps = 12;
                        for i in 0..steps {
                            let t = i as f32 / steps as f32;
                            let alpha = (50.0 * (1.0 - t)) as u8;
                            if alpha == 0 {
                                continue;
                            }
                            let inset = depth * t;
                            if rect.width() - 2.0 * inset <= 0.0 || rect.height() - 2.0 * inset <= 0.0 {
                                break;
                            }
                            ui.painter().rect_stroke(
                                rect.shrink(inset),
                                0.0,
                                egui::Stroke::new(depth / steps as f32 + 0.75, egui::Color32::from_black_alpha(alpha)),
                            );
                        }
                    }
                });
            }
            if *routine == GameRoutine::Paused {
                let mods_view = mod_entries(loaded_mods);
                let debug_info = if let Session::Emulator { nes, .. } = session {
                    Some(DebugInfo {
                        p1_lives: nes.peek_ram(RAM_P1_NUM_LIVES),
                        p1_weapon: nes.peek_ram(RAM_P1_CURRENT_WEAPON) & 0x0F,
                        p1_rapid_fire: nes.peek_ram(RAM_P1_CURRENT_WEAPON) & 0x10 != 0,
                        p2_lives: nes.peek_ram(RAM_P2_NUM_LIVES),
                        p2_weapon: nes.peek_ram(RAM_P2_CURRENT_WEAPON) & 0x0F,
                        p2_rapid_fire: nes.peek_ram(RAM_P2_CURRENT_WEAPON) & 0x10 != 0,
                        continues: nes.peek_ram(RAM_NUM_CONTINUES),
                        current_stage: nes.peek_ram(RAM_CURRENT_LEVEL),
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
            other => apply_menu_action(other, session, loaded_mods, enabled_mod_ids),
        }
    }

    if pending_rom_pick && rom_dialog_rx.is_none() {
        // `rfd::FileDialog::pick_file()` blocks the calling thread until
        // the dialog closes. Calling it directly from here blocks the
        // *whole winit event loop* - on Windows specifically, this is a
        // known way to get the dialog itself stuck ("Working on it..."
        // with no files ever listed), because Explorer's shell namespace
        // enumeration wants the caller's thread to stay responsive while
        // it populates the list, and a thread that's inside `redraw`
        // isn't pumping any messages. Run it on its own thread instead and
        // poll the result (see `Event::AboutToWait` in `main`) - the
        // dialog gets a thread that's only ever doing this, and our event
        // loop keeps running normally while it's open.
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new().add_filter("NES ROM", &["nes"]).pick_file();
            let _ = tx.send(path);
        });
        *rom_dialog_rx = Some(rx);
    }

    apply_toggle_side_effects(settings, prev_widescreen, prev_fullscreen, window);

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
