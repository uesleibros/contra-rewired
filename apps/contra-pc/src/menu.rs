//! The in-game UI, built on [`egui`] - real widgets (checkboxes, sliders,
//! tabs, buttons) instead of a hand-rolled bitmap font and manual
//! hit-testing. `main.rs` owns the `egui::Context`/`egui-wgpu` renderer;
//! this module only builds the widget tree each frame from `Settings`/
//! `MenuState`/etc, mutating most of them directly (egui's immediate-mode
//! idiom - a checkbox bound to `&mut settings.widescreen` *is* the toggle,
//! no separate action/dispatch layer needed). The handful of things that
//! need state `menu.rs` doesn't own (the window handle, the live `Nes`,
//! `routine`) are reported back as a small [`MenuAction`] list for
//! `main.rs` to apply.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Settings,
    Mods,
    Debug,
}

pub const TABS: [(Tab, &str); 3] = [(Tab::Settings, "Settings"), (Tab::Mods, "Mods"), (Tab::Debug, "Debug")];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Player {
    P1,
    P2,
}

/// Side effects `menu.rs` can't apply itself - returned from [`pause_menu`]
/// / [`no_rom_screen`] for `main.rs` to handle against state this module
/// doesn't own (the window, the live `Nes`, mod list, routine).
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    Resume,
    ToggleMod(usize),
    SetWeapon(Player, u8),
    ToggleRapidFire(Player),
    LivesDelta(Player, i32),
    ContinuesDelta(i32),
    EnemyHpDelta(i32),
    LoadRom,
}

pub struct MenuState {
    pub tab: Tab,
}

impl MenuState {
    pub fn new() -> Self {
        Self { tab: Tab::Settings }
    }
}

#[derive(Debug, Clone)]
pub struct Settings {
    /// Renders wider than 256px, sampling nametable data the game already
    /// drew for scrolling (see `contra_nes::EXTENDED_WIDTH`). Always
    /// targets the safe max width the instant it's enabled - not tied to
    /// the current window size, so toggling it has an immediate, visible
    /// effect regardless of window shape (see docs/FIDELITY.md for why
    /// there's a cap at all).
    pub widescreen: bool,
    /// Lifts the real hardware's 8-sprites-per-scanline limit (the cause
    /// of "sprite flicker"). Off by default - `Original` mode stays
    /// hardware-accurate.
    pub unlimited_sprites: bool,
    /// On: integer-only scaling, crisp NES pixels, possible letterboxing.
    /// Off (default): fractional "fill the window" scaling that tracks
    /// any window size live.
    pub pixel_perfect: bool,
    pub zoom_percent: i32,
    pub fullscreen: bool,
    pub audio_muted: bool,
    /// Overlays each active sprite's bounding box (from OAM, scaled/offset
    /// to match wherever wide mode currently draws it - see
    /// `contra_nes::Nes::wide_x_offset`). This is the *visual* sprite
    /// bounding box, not necessarily Contra's exact collision box (which
    /// may be smaller or offset within it) - a useful, honest
    /// approximation without needing to reverse-engineer every entity's
    /// real hitbox from the disassembly.
    pub show_hitboxes: bool,
    /// Small on-screen overlay: frame count and both players' X/Y
    /// position, read live from RAM (`SPRITE_X_POS`/`SPRITE_Y_POS`). The
    /// "frame counter"/"coordinates" entries from `contra_core::config::
    /// PracticeConfig` finally have a renderer to draw into - see
    /// ROADMAP.md.
    pub show_stats: bool,
    /// Simulation speed, 25-200%: scales how much real time each
    /// simulated frame takes (`main.rs`'s `frame_duration`), *not* the PPU/
    /// APU themselves - the game still runs its own unmodified logic one
    /// real frame at a time, just paced slower or faster.
    pub sim_speed_percent: i32,
    /// Draws a faint dark line over every other scanline of the game
    /// image, egui-painter-side (no shader/render-pipeline change needed -
    /// see `game_image_rect`'s caller). Purely cosmetic, same spirit as
    /// `pixel_perfect`: an accuracy-flavored preference, not a gameplay
    /// setting.
    pub scanlines: bool,
}

impl Default for Settings {
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

/// One row of the Mods tab's list - deliberately generic (just a name +
/// enabled flag) so `menu.rs` doesn't need to know about `contra_mods` or
/// depend on the `mods` Cargo feature.
pub struct ModEntry {
    pub name: String,
    pub enabled: bool,
}

/// Live values for the Debug tab, read fresh from the emulator's RAM each
/// frame by `main.rs` (see `contra_nes::Nes::peek_ram` and
/// `docs/FIDELITY.md`'s RAM address notes) - `None` when there's no ROM
/// loaded (nothing to poke).
pub struct DebugInfo {
    pub p1_lives: u8,
    pub p1_weapon: u8,
    pub p1_rapid_fire: bool,
    pub p2_lives: u8,
    pub p2_weapon: u8,
    pub p2_rapid_fire: bool,
    pub continues: u8,
    /// 0-based `CURRENT_LEVEL` from RAM - the Stage Select row shows
    /// `current_stage + 1` (Stage 1-8) to match how the game numbers them
    /// on screen.
    pub current_stage: u8,
    /// HP of whichever enemy slot currently has the most (a heuristic for
    /// "the boss" - see `RAM_ENEMY_HP_BASE`'s comment in `main.rs`).
    /// `None` when no enemy is active.
    pub enemy_hp: Option<u8>,
}

pub const WEAPON_NAMES: [(u8, &str); 5] = [(0, "Standard"), (1, "Machine Gun"), (2, "Fire"), (3, "Spread"), (4, "Laser")];

/// Contra (USA) has 8 stages (Jungle, Base 1, Waterfall, Base 2, Snowfield,
/// Energy Zone, Hangar, Alien's Lair) - confirmed against the reference
/// disassembly's per-level data tables, which consistently have 8 entries.
pub const STAGE_COUNT: u8 = 8;

fn weapon_name(id: u8) -> &'static str {
    WEAPON_NAMES.iter().find(|(wid, _)| *wid == id).map(|(_, n)| *n).unwrap_or("?")
}

/// Draws the pause menu (tabs + content + Resume). Most of `settings` is
/// mutated directly by the widgets bound to it; anything needing state
/// this module doesn't own comes back in the returned `Vec<MenuAction>`.
#[allow(clippy::too_many_arguments)]
pub fn pause_menu(
    ctx: &egui::Context,
    state: &mut MenuState,
    settings: &mut Settings,
    mods: &[ModEntry],
    debug: Option<&DebugInfo>,
) -> Vec<MenuAction> {
    let mut actions = Vec::new();

    egui::Window::new("contra-rewired")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .fixed_size(egui::vec2(380.0, 320.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (tab, label) in TABS {
                    if ui.selectable_label(state.tab == tab, label).clicked() {
                        state.tab = tab;
                    }
                }
            });
            ui.separator();

            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| match state.tab {
                Tab::Settings => settings_tab(ui, settings),
                Tab::Mods => mods_tab(ui, mods, &mut actions),
                Tab::Debug => debug_tab(ui, debug, &mut actions),
            });

            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Press Tab or Esc to resume, or click Resume");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Resume").clicked() {
                        actions.push(MenuAction::Resume);
                    }
                });
            });
        });

    actions
}

fn settings_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    ui.checkbox(&mut settings.widescreen, "Widescreen (F1)");
    ui.checkbox(&mut settings.unlimited_sprites, "No sprite flicker (F2)");
    ui.checkbox(&mut settings.pixel_perfect, "Pixel perfect (F3)");
    ui.checkbox(&mut settings.show_hitboxes, "Show hitboxes (F4)");
    ui.checkbox(&mut settings.show_stats, "Show stats overlay (F7)");
    ui.checkbox(&mut settings.fullscreen, "Fullscreen (F11)");
    ui.checkbox(&mut settings.scanlines, "Scanlines (F6)");
    ui.checkbox(&mut settings.audio_muted, "Mute audio (F8)");
    ui.horizontal(|ui| {
        ui.label("Zoom");
        ui.add(egui::Slider::new(&mut settings.zoom_percent, 50..=300).suffix("%"));
    });
    ui.horizontal(|ui| {
        ui.label("Speed");
        ui.add(egui::Slider::new(&mut settings.sim_speed_percent, 25..=200).suffix("%"));
    });
    ui.label(egui::RichText::new("F12 freeze, . to step one frame while frozen").weak());
}

fn mods_tab(ui: &mut egui::Ui, mods: &[ModEntry], actions: &mut Vec<MenuAction>) {
    if mods.is_empty() {
        ui.label("No mods found in ./mods");
        return;
    }
    for (i, m) in mods.iter().enumerate() {
        let mut enabled = m.enabled;
        if ui.checkbox(&mut enabled, &m.name).changed() {
            actions.push(MenuAction::ToggleMod(i));
        }
    }
}

fn debug_tab(ui: &mut egui::Ui, debug: Option<&DebugInfo>, actions: &mut Vec<MenuAction>) {
    let Some(debug) = debug else {
        ui.label("No ROM loaded");
        return;
    };

    for (player, label, lives, weapon, rapid_fire) in [
        (Player::P1, "P1", debug.p1_lives, debug.p1_weapon, debug.p1_rapid_fire),
        (Player::P2, "P2", debug.p2_lives, debug.p2_weapon, debug.p2_rapid_fire),
    ] {
        ui.horizontal(|ui| {
            ui.label(format!("{label} lives: {lives}"));
            if ui.small_button("-").clicked() {
                actions.push(MenuAction::LivesDelta(player, -1));
            }
            if ui.small_button("+").clicked() {
                actions.push(MenuAction::LivesDelta(player, 1));
            }
        });
        ui.horizontal(|ui| {
            ui.label(format!("{label} weapon"));
            egui::ComboBox::from_id_source(("weapon-select", label))
                .selected_text(weapon_name(weapon))
                .show_ui(ui, |ui| {
                    for (id, name) in WEAPON_NAMES {
                        if ui.selectable_label(weapon == id, name).clicked() {
                            actions.push(MenuAction::SetWeapon(player, id));
                        }
                    }
                });
            // The "R" capsule powerup - stacks with whatever weapon is
            // equipped, just fires faster. Same RAM byte as the weapon
            // (`ram.asm`: low nibble weapon, bit 4 rapid fire), so this is
            // its own checkbox rather than part of the weapon dropdown.
            let mut rapid = rapid_fire;
            if ui.checkbox(&mut rapid, "Rapid fire (R)").changed() {
                actions.push(MenuAction::ToggleRapidFire(player));
            }
        });
    }

    ui.horizontal(|ui| {
        ui.label(format!("Continues: {}", debug.continues));
        if ui.small_button("-").clicked() {
            actions.push(MenuAction::ContinuesDelta(-1));
        }
        if ui.small_button("+").clicked() {
            actions.push(MenuAction::ContinuesDelta(1));
        }
    });

    if let Some(hp) = debug.enemy_hp {
        ui.horizontal(|ui| {
            ui.label(format!("Enemy/boss HP: {hp}"));
            if ui.small_button("-").clicked() {
                actions.push(MenuAction::EnemyHpDelta(-1));
            }
            if ui.small_button("+").clicked() {
                actions.push(MenuAction::EnemyHpDelta(1));
            }
        });
    }

    ui.separator();
    ui.label(format!("Current stage: {} / {STAGE_COUNT}", debug.current_stage + 1));
}

/// Drawn instead of gameplay when no ROM is loaded - a real "load your
/// ROM" screen (title + a big clickable button that opens a native file
/// picker), not an engine-only physics placeholder demo. `error`, if set,
/// is the reason the last load attempt (button click or drag-and-drop)
/// failed, shown under the button so a bad file doesn't just do nothing.
pub fn no_rom_screen(ctx: &egui::Context, error: Option<&str>) -> Vec<MenuAction> {
    let mut actions = Vec::new();
    egui::CentralPanel::default().frame(egui::Frame::none().fill(egui::Color32::BLACK)).show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() / 2.0 - 90.0);
            ui.label(egui::RichText::new("CONTRA").size(56.0).strong().color(egui::Color32::from_rgb(255, 224, 128)));
            ui.label(egui::RichText::new("PC PORT - NO ROM LOADED").size(16.0));
            ui.add_space(16.0);
            if ui.add(egui::Button::new(egui::RichText::new("Load ROM...").size(20.0)).min_size(egui::vec2(220.0, 44.0))).clicked() {
                actions.push(MenuAction::LoadRom);
            }
            ui.add_space(12.0);
            ui.label(egui::RichText::new("Or drag and drop a .nes file onto this window").weak());
            ui.label(egui::RichText::new("Bring your own legally-obtained ROM - none is included").weak());
            if let Some(err) = error {
                ui.add_space(8.0);
                ui.label(egui::RichText::new(err).color(egui::Color32::from_rgb(224, 96, 96)));
            }
        });
    });
    actions
}
