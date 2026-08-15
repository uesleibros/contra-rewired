//! The full user-facing options surface, serialized to `config.toml`.
//!
//! This is deliberately one big serde-friendly tree instead of scattered
//! globals: every toggle in the README's feature list has a field here (even
//! where the underlying system is still a Phase 2/3 stub - see ROADMAP.md),
//! so the config format is stable from day one and front-ends (PC, Android)
//! just render whichever sections their platform supports.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::checkpoint::CheckpointMode;
use crate::difficulty::Difficulty;
use crate::input::Bindings;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub fidelity: FidelityConfig,
    pub video: VideoConfig,
    pub audio: AudioConfig,
    pub input: InputConfig,
    pub gameplay: GameplayConfig,
    pub accessibility: AccessibilityConfig,
    pub practice: PracticeConfig,
    // `serde(default)` specifically: without it, loading an existing
    // config.toml saved before this field existed would fail to parse
    // entirely (`toml`'s derived Deserialize errors on a missing field by
    // default), and `load_or_default` silently discards the *whole*
    // config and resets every other section to defaults too when that
    // happens - a real regression for anyone with an already-customized
    // config.toml, not just a "mods list resets" one.
    #[serde(default)]
    pub mods: ModsConfig,
    // Same `serde(default)` reasoning as `mods` above - added after
    // `mods`, so an existing config.toml missing this section must not
    // fail to parse and silently reset everything else.
    #[serde(default)]
    pub pc_settings: PcSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fidelity: FidelityConfig::default(),
            video: VideoConfig::default(),
            audio: AudioConfig::default(),
            input: InputConfig::default(),
            gameplay: GameplayConfig::default(),
            accessibility: AccessibilityConfig::default(),
            practice: PracticeConfig::default(),
            mods: ModsConfig::default(),
            pc_settings: PcSettings::default(),
        }
    }
}

impl Config {
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let s = toml::to_string_pretty(self)?;
        std::fs::write(path, s)?;
        Ok(())
    }
}

/// Everything that governs "does this behave like a real NES", separate
/// from difficulty/accessibility so "Original" can stay a single flag flip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FidelityConfig {
    /// When true, forces real region timing/RNG-idle-tick behavior and
    /// disables every convenience feature that isn't period-accurate
    /// (rewind, save states outside of Suspend, HUD overlays, etc).
    pub original_nes_mode: bool,
    pub region: Region,
    pub emulate_slowdown: bool,
    pub replicate_known_bugs: bool,
    pub target_fps: TargetFps,
}

impl Default for FidelityConfig {
    fn default() -> Self {
        Self {
            original_nes_mode: false,
            region: Region::Ntsc,
            emulate_slowdown: false,
            replicate_known_bugs: false,
            target_fps: TargetFps::Uncapped60Logic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Region {
    Ntsc,
    Pal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetFps {
    /// Simulation always steps at 60Hz logic; present as fast as vsync
    /// allows (used for real 120/144Hz+ displays without changing gameplay
    /// speed).
    Uncapped60Logic,
    /// Locked to 60 FPS exactly, no real slowdown even under load.
    Locked60,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub window_mode: WindowMode,
    pub scaling: ScalingMode,
    pub aspect_ratio: AspectRatio,
    pub overscan_px: u8,
    pub crt_filter: bool,
    pub scanlines: bool,
    pub composite_artifact_sim: bool,
    pub tv_ghosting: bool,
    pub palette: PaletteChoice,
    pub widescreen: WidescreenMode,
    pub vsync: bool,
    pub frame_limiter_fps: Option<u32>,
    pub graphics_style: GraphicsStyle,
    pub particle_intensity: f32,
    pub screen_shake_intensity: f32,
    pub flashes_enabled: bool,
    pub gore_enabled: bool,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            window_mode: WindowMode::Windowed,
            scaling: ScalingMode::Integer(3),
            aspect_ratio: AspectRatio::Nes4x3,
            overscan_px: 0,
            crt_filter: false,
            scanlines: false,
            composite_artifact_sim: false,
            tv_ghosting: false,
            palette: PaletteChoice::Nes,
            widescreen: WidescreenMode::Classic,
            vsync: true,
            frame_limiter_fps: None,
            graphics_style: GraphicsStyle::Nes,
            particle_intensity: 1.0,
            screen_shake_intensity: 1.0,
            flashes_enabled: true,
            gore_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowMode {
    Windowed,
    Borderless,
    Fullscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScalingMode {
    Integer(u8),
    PixelPerfectFit,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectRatio {
    Nes4x3,
    EightBySeven,
    Native256x240,
    Ultrawide21x9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaletteChoice {
    Nes,
    Famicom,
    GameBoy,
    GameBoyPocket,
    VirtualBoy,
    Cga,
    Monochrome,
    AmberMonitor,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidescreenMode {
    /// 256x240 internal resolution, letterboxed/pillarboxed with themed
    /// borders. Gameplay-identical to NES.
    Classic,
    /// Renders extra world space on the sides, but spawn/camera logic must
    /// be adapted so nothing spawns/activates earlier than on real
    /// hardware. See docs/FIDELITY.md - tracked as a Phase 2 item.
    Extended,
    Ultrawide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphicsStyle {
    Nes,
    NesEnhanced,
    SixteenBit,
    ArcadeInspired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub soundtrack: Soundtrack,
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub master_volume: f32,
    pub stereo_pan: f32,
    pub mono_output: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            soundtrack: Soundtrack::Nes,
            music_volume: 0.8,
            sfx_volume: 0.8,
            master_volume: 1.0,
            stereo_pan: 0.0,
            mono_output: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Soundtrack {
    Nes,
    Famicom,
    Arcade,
    Orchestral,
    Metal,
    ChiptuneRemix,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    pub player_bindings: Vec<Bindings>,
    pub dual_stick_mode: bool,
    pub vibration_enabled: bool,
    pub input_display_enabled: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            player_bindings: vec![Bindings::default_keyboard_p1(), Bindings::default_gamepad("Gamepad (P2)")],
            dual_stick_mode: false,
            vibration_enabled: true,
            input_display_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameplayConfig {
    pub difficulty: Difficulty,
    pub checkpoint_mode: CheckpointMode,
    pub save_states_enabled: bool,
    pub rewind_enabled: bool,
    pub rewind_buffer_seconds: u32,
    pub autosave_on_stage_entry: bool,
    pub hardcore_mode: bool,
}

impl Default for GameplayConfig {
    fn default() -> Self {
        Self {
            difficulty: Difficulty::Normal,
            checkpoint_mode: CheckpointMode::Original,
            save_states_enabled: true,
            rewind_enabled: false,
            rewind_buffer_seconds: 30,
            autosave_on_stage_entry: true,
            hardcore_mode: false,
        }
    }
}

impl GameplayConfig {
    /// Hardcore mode is a hard override: no save states, no rewind, no
    /// practice checkpoint jumping, full "Original" checkpoint behavior.
    pub fn apply_hardcore_override(&mut self) {
        if self.hardcore_mode {
            self.save_states_enabled = false;
            self.rewind_enabled = false;
            self.checkpoint_mode = CheckpointMode::Original;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilityConfig {
    pub game_speed_pct: u8,
    pub projectile_high_contrast: bool,
    pub enemy_outlines: bool,
    pub screen_shake_disabled: bool,
    pub flashes_disabled: bool,
    pub colorblind_mode: ColorblindMode,
    pub ui_scale_pct: u16,
    pub text_size_pct: u16,
    pub aim_assist: bool,
    pub invincibility_assist: bool,
}

impl Default for AccessibilityConfig {
    fn default() -> Self {
        Self {
            game_speed_pct: 100,
            projectile_high_contrast: false,
            enemy_outlines: false,
            screen_shake_disabled: false,
            flashes_disabled: false,
            colorblind_mode: ColorblindMode::Off,
            ui_scale_pct: 100,
            text_size_pct: 100,
            aim_assist: false,
            invincibility_assist: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorblindMode {
    Off,
    Protanopia,
    Deuteranopia,
    Tritanopia,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PracticeConfig {
    pub show_hitboxes: bool,
    pub show_spawn_markers: bool,
    pub show_frame_counter: bool,
    pub show_coordinates: bool,
    pub show_boss_hp: bool,
    pub input_display: bool,
    pub fixed_rng_seed: Option<u32>,
}

impl Default for PracticeConfig {
    fn default() -> Self {
        Self {
            show_hitboxes: false,
            show_spawn_markers: false,
            show_frame_counter: false,
            show_coordinates: false,
            show_boss_hp: false,
            input_display: false,
            fixed_rng_seed: None,
        }
    }
}

/// Which mods are enabled, persisted across launches. Stores IDs to
/// *enable* rather than IDs to *disable*: a mod is opt-in, so a newly-added
/// mod (one this list has never heard of) defaults to *disabled* until the
/// player explicitly turns it on in the Mods tab - dropping a `.lua` file
/// into `./mods/` should never silently start running code without the
/// player having said yes to it first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModsConfig {
    pub enabled_ids: Vec<String>,
}

/// A flat mirror of `apps/contra-pc`'s `menu::Settings` - everything the
/// pause menu's Settings tab controls, persisted across launches the same
/// way [`ModsConfig`] is. Deliberately its own small struct rather than
/// shoehorned into [`VideoConfig`]/[`AccessibilityConfig`] above: those are
/// this crate's aspirational full options schema (see this module's doc
/// comment) for a config surface no front-end fully implements yet, and
/// their richer enums (`ScalingMode`, `WidescreenMode`, ...) don't map
/// cleanly onto what `contra-pc` actually has today (a `widescreen: bool`
/// that resizes to the monitor, not a 4-way mode enum). This mirrors
/// `contra-pc`'s *real*, current settings 1:1 instead of forcing a mapping
/// that would either lose information or silently drift out of sync with
/// what the front-end actually does. `contra-pc` owns the conversion to/
/// from `menu::Settings` (see `main.rs`) since `contra-core` can't depend
/// on `contra-pc`'s `menu` module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PcSettings {
    pub widescreen: bool,
    pub unlimited_sprites: bool,
    pub pixel_perfect: bool,
    pub zoom_percent: i32,
    pub fullscreen: bool,
    pub audio_muted: bool,
    pub show_hitboxes: bool,
    pub show_stats: bool,
    pub sim_speed_percent: i32,
    pub scanlines: bool,
}

impl Default for PcSettings {
    fn default() -> Self {
        Self {
            widescreen: false,
            unlimited_sprites: false,
            pixel_perfect: false,
            zoom_percent: 100,
            fullscreen: false,
            audio_muted: false,
            show_hitboxes: false,
            show_stats: false,
            sim_speed_percent: 100,
            scanlines: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_round_trips_through_toml() {
        let cfg = Config::default();
        let s = toml::to_string_pretty(&cfg).expect("serialize");
        let parsed: Config = toml::from_str(&s).expect("deserialize");
        assert_eq!(cfg.video.scaling, parsed.video.scaling);
        assert_eq!(cfg.fidelity.region, parsed.fidelity.region);
    }

    #[test]
    fn hardcore_mode_forces_original_rules() {
        let mut gp = GameplayConfig {
            hardcore_mode: true,
            save_states_enabled: true,
            rewind_enabled: true,
            checkpoint_mode: CheckpointMode::Practice,
            ..GameplayConfig::default()
        };
        gp.apply_hardcore_override();
        assert!(!gp.save_states_enabled);
        assert!(!gp.rewind_enabled);
        assert_eq!(gp.checkpoint_mode, CheckpointMode::Original);
    }
}
