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
    WeaponDelta(Player, i32),
    LivesDelta(Player, i32),
    ContinuesDelta(i32),
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
    pub p2_lives: u8,
    pub p2_weapon: u8,
    pub continues: u8,
}

pub const WEAPON_NAMES: [(u8, &str); 5] = [(0, "Standard"), (1, "Machine Gun"), (2, "Fire"), (3, "Spread"), (4, "Laser")];

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
    ui.checkbox(&mut settings.widescreen, "Widescreen");
    ui.checkbox(&mut settings.unlimited_sprites, "No sprite flicker");
    ui.checkbox(&mut settings.pixel_perfect, "Pixel perfect");
    ui.checkbox(&mut settings.fullscreen, "Fullscreen");
    ui.checkbox(&mut settings.audio_muted, "Mute audio");
    ui.horizontal(|ui| {
        ui.label("Zoom");
        ui.add(egui::Slider::new(&mut settings.zoom_percent, 50..=300).suffix("%"));
    });
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

    for (player, label, lives, weapon) in [
        (Player::P1, "P1", debug.p1_lives, debug.p1_weapon),
        (Player::P2, "P2", debug.p2_lives, debug.p2_weapon),
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
            ui.label(format!("{label} weapon: {}", weapon_name(weapon)));
            if ui.small_button("<").clicked() {
                actions.push(MenuAction::WeaponDelta(player, -1));
            }
            if ui.small_button(">").clicked() {
                actions.push(MenuAction::WeaponDelta(player, 1));
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
