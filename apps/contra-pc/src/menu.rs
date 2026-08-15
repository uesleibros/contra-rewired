//! A mouse-driven in-window menu: a built-in 5x7 bitmap font (no external
//! font/asset dependency - keeps with the "ships nothing but code" model),
//! three tabs (Settings, Mods, Debug), and real, working controls -
//! toggles, steppers, a mod enable/disable list, and live cheat-style
//! debug pokes (weapon/lives/continues). Every clickable element is
//! computed as a screen-space [`Rect`] alongside drawing it, returned in a
//! [`MenuLayout`] so `main.rs` can hit-test real mouse clicks against
//! exactly what's on screen - there's no separate "logical" layout that
//! could drift from the visual one.
//!
//! This is a deliberately real v1, not a finished options screen: keyboard/
//! gamepad Up/Down/X still work as a fallback (see `main.rs`), but the menu
//! is *designed* for the mouse - hover highlights, click targets sized for
//! a cursor, no forced directional navigation.

/// 5 columns x 7 rows per glyph, one `u8` per row (bits 4..0 = columns
/// left..right, 1 = pixel lit). Covers what the menu actually needs:
/// A-Z, 0-9, space, and a few punctuation marks.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b00100, 0b00100, 0b00100],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        '%' => [0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10001],
        '>' => [0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000],
        '<' => [0b00001, 0b00010, 0b00100, 0b01000, 0b00100, 0b00010, 0b00001],
        '[' => [0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110],
        ']' => [0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00000],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        _ => [0; 7],
    }
}

/// Blits `text` at `(x, y)` into `fb` (which is `fb_width` pixels wide),
/// `scale`x pixel size per glyph dot, each glyph 6 columns apart
/// (5 + 1 spacing) times `scale`.
pub fn draw_text(fb: &mut [u32], fb_width: usize, fb_height: usize, x: i32, y: i32, text: &str, color: u32, scale: i32) {
    for (i, ch) in text.chars().enumerate() {
        let glyph_x = x + i as i32 * 6 * scale;
        let rows = glyph(ch);
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                let px0 = glyph_x + col as i32 * scale;
                let py0 = y + row as i32 * scale;
                for sy in 0..scale {
                    for sx in 0..scale {
                        let px = px0 + sx;
                        let py = py0 + sy;
                        if px >= 0 && py >= 0 && (px as usize) < fb_width && (py as usize) < fb_height {
                            fb[py as usize * fb_width + px as usize] = color;
                        }
                    }
                }
            }
        }
    }
}

pub fn text_width(text: &str, scale: i32) -> i32 {
    (text.chars().count() as i32) * 6 * scale
}

/// Fills a solid rectangle.
pub fn fill_rect(fb: &mut [u32], fb_width: usize, fb_height: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    for py in y.max(0)..(y + h).min(fb_height as i32) {
        for px in x.max(0)..(x + w).min(fb_width as i32) {
            fb[py as usize * fb_width + px as usize] = color;
        }
    }
}

/// Draws a 1px rectangle outline.
pub fn stroke_rect(fb: &mut [u32], fb_width: usize, fb_height: usize, r: Rect, color: u32) {
    fill_rect(fb, fb_width, fb_height, r.x, r.y, r.w, 1, color);
    fill_rect(fb, fb_width, fb_height, r.x, r.y + r.h - 1, r.w, 1, color);
    fill_rect(fb, fb_width, fb_height, r.x, r.y, 1, r.h, color);
    fill_rect(fb, fb_width, fb_height, r.x + r.w - 1, r.y, 1, r.h, color);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Settings,
    Mods,
    Debug,
}

pub const TABS: [(Tab, &str); 3] = [(Tab::Settings, "SETTINGS"), (Tab::Mods, "MODS"), (Tab::Debug, "DEBUG")];

/// Every clickable outcome the menu can produce. `main.rs` matches on
/// this to actually apply the effect - `menu.rs` never touches emulator
/// state directly.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    SwitchTab(Tab),
    ToggleWidescreen,
    ToggleNoSpriteLimit,
    TogglePixelPerfect,
    ZoomDelta(i32),
    ToggleFullscreen,
    ToggleAudioMuted,
    Resume,
    ToggleMod(usize),
    SetWeapon(u8),
    LivesDelta(i32),
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
/// loaded (the placeholder demo has no RAM worth poking).
pub struct DebugInfo {
    pub p1_lives: u8,
    pub p1_weapon: u8,
    pub continues: u8,
}

pub const WEAPON_NAMES: [(u8, &str); 5] = [(0, "STANDARD"), (1, "MACHINE GUN"), (2, "FIRE"), (3, "SPREAD"), (4, "LASER")];

/// Everything drawn this frame, in click-priority order (last drawn = last
/// in the list = checked first by `hit_test`, matching "topmost visually
/// wins" the way real UI stacks work).
pub struct MenuLayout {
    clickable: Vec<(Rect, MenuAction)>,
}

impl MenuLayout {
    pub fn hit_test(&self, x: i32, y: i32) -> Option<MenuAction> {
        self.clickable.iter().rev().find(|(r, _)| r.contains(x, y)).map(|(_, a)| a.clone())
    }
}

const PANEL_W: i32 = 420;
const ROW_H: i32 = 22;
const COLOR_PANEL_BG: u32 = 0x00101018;
const COLOR_PANEL_BORDER: u32 = 0x00404860;
const COLOR_ROW_BG: u32 = 0x00202838;
const COLOR_ROW_HOVER: u32 = 0x00304058;
const COLOR_TEXT: u32 = 0x00C0C8D0;
const COLOR_TEXT_DIM: u32 = 0x00707880;
const COLOR_ACCENT: u32 = 0x00FFE080;
const COLOR_ON: u32 = 0x0060E080;
const COLOR_OFF: u32 = 0x00707880;

#[allow(clippy::too_many_arguments)]
pub fn draw_menu(
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
    state: &MenuState,
    settings: &Settings,
    mods: &[ModEntry],
    debug: Option<&DebugInfo>,
    mouse: (i32, i32),
) -> MenuLayout {
    let mut layout = MenuLayout { clickable: Vec::new() };

    // Precise, not estimated: must match exactly what each draw_*_tab
    // function below actually draws, or content overlaps the Resume
    // button/panel border (this was a real bug - the previous estimate
    // undercounted the Debug tab's 8 rows and any Mods list under 3
    // entries, both of which visibly overflowed the panel).
    const CONTENT_TOP: i32 = 42; // panel_y -> first content row
    const GAP_BEFORE_FOOTER: i32 = 14;
    const RESUME_H: i32 = 24;
    const BOTTOM_MARGIN: i32 = 14;
    let content_h = match state.tab {
        Tab::Settings => 6 * ROW_H,
        Tab::Mods => mods.len().max(1) as i32 * ROW_H,
        Tab::Debug => {
            if debug.is_some() {
                2 * ROW_H + 4 + 18 + WEAPON_NAMES.len() as i32 * ROW_H
            } else {
                ROW_H
            }
        }
    };
    let panel_h = CONTENT_TOP + content_h + GAP_BEFORE_FOOTER + RESUME_H + BOTTOM_MARGIN;
    let panel_x = (fb_width as i32 - PANEL_W) / 2;
    let panel_y = (fb_height as i32 - panel_h) / 2;

    fill_rect(fb, fb_width, fb_height, panel_x, panel_y, PANEL_W, panel_h, COLOR_PANEL_BG);
    stroke_rect(fb, fb_width, fb_height, Rect { x: panel_x, y: panel_y, w: PANEL_W, h: panel_h }, COLOR_PANEL_BORDER);

    // Tab bar.
    let tab_y = panel_y + 10;
    let mut tab_x = panel_x + 16;
    for (tab, label) in TABS {
        let w = text_width(label, 2) + 16;
        let r = Rect { x: tab_x, y: tab_y, w, h: 20 };
        let is_active = tab == state.tab;
        let is_hover = r.contains(mouse.0, mouse.1);
        let bg = if is_active { COLOR_ACCENT } else if is_hover { COLOR_ROW_HOVER } else { COLOR_ROW_BG };
        fill_rect(fb, fb_width, fb_height, r.x, r.y, r.w, r.h, bg);
        let text_color = if is_active { 0x00101018 } else { COLOR_TEXT };
        draw_text(fb, fb_width, fb_height, r.x + 8, r.y + 3, label, text_color, 2);
        layout.clickable.push((r, MenuAction::SwitchTab(tab)));
        tab_x += w + 6;
    }

    let content_y = tab_y + 32;
    match state.tab {
        Tab::Settings => draw_settings_tab(fb, fb_width, fb_height, panel_x, content_y, settings, mouse, &mut layout),
        Tab::Mods => draw_mods_tab(fb, fb_width, fb_height, panel_x, content_y, mods, mouse, &mut layout),
        Tab::Debug => draw_debug_tab(fb, fb_width, fb_height, panel_x, content_y, debug, mouse, &mut layout),
    }

    // Footer: Resume button (bottom-right) + hint text (bottom-left), both
    // positioned from the same content_h this panel was actually sized
    // for, so they never collide with the tab content above them.
    let footer_y = panel_y + CONTENT_TOP + content_h + GAP_BEFORE_FOOTER;
    let resume_label = "RESUME";
    let resume_w = text_width(resume_label, 2) + 24;
    let resume_r = Rect { x: panel_x + PANEL_W - resume_w - 16, y: footer_y, w: resume_w, h: RESUME_H };
    let hover = resume_r.contains(mouse.0, mouse.1);
    fill_rect(fb, fb_width, fb_height, resume_r.x, resume_r.y, resume_r.w, resume_r.h, if hover { COLOR_ACCENT } else { COLOR_ROW_BG });
    draw_text(fb, fb_width, fb_height, resume_r.x + 12, resume_r.y + 4, resume_label, if hover { 0x00101018 } else { COLOR_TEXT }, 2);
    layout.clickable.push((resume_r, MenuAction::Resume));

    draw_text(fb, fb_width, fb_height, panel_x + 16, footer_y + 8, "CLICK TO INTERACT", COLOR_TEXT_DIM, 1);

    layout
}

/// Drawn instead of gameplay when no ROM is loaded - a real "load your
/// ROM" screen (title + a big clickable button that opens a native file
/// picker), not the old engine-only physics placeholder demo. `error`, if
/// set, is the reason the last load attempt (button click or drag-and-drop)
/// failed, shown under the button so a bad file doesn't just silently do
/// nothing.
pub fn draw_no_rom_screen(fb: &mut [u32], fb_width: usize, fb_height: usize, mouse: (i32, i32), error: Option<&str>) -> MenuLayout {
    let mut layout = MenuLayout { clickable: Vec::new() };
    fb.fill(0x00000000);

    let title = "CONTRA";
    let title_scale = 6;
    let title_w = text_width(title, title_scale);
    let cx = fb_width as i32 / 2;
    let title_y = (fb_height as i32 / 2) - 90;
    draw_text(fb, fb_width, fb_height, cx - title_w / 2, title_y, title, COLOR_ACCENT, title_scale);

    let subtitle = "PC PORT - NO ROM LOADED";
    let sub_w = text_width(subtitle, 2);
    draw_text(fb, fb_width, fb_height, cx - sub_w / 2, title_y + title_scale * 8 + 10, subtitle, COLOR_TEXT, 2);

    let btn_label = "LOAD ROM...";
    let btn_w = text_width(btn_label, 3) + 40;
    let btn_h = 44;
    let btn_r = Rect { x: cx - btn_w / 2, y: title_y + title_scale * 8 + 46, w: btn_w, h: btn_h };
    let hover = btn_r.contains(mouse.0, mouse.1);
    fill_rect(fb, fb_width, fb_height, btn_r.x, btn_r.y, btn_r.w, btn_r.h, if hover { COLOR_ACCENT } else { COLOR_ROW_BG });
    stroke_rect(fb, fb_width, fb_height, btn_r, COLOR_PANEL_BORDER);
    draw_text(
        fb,
        fb_width,
        fb_height,
        btn_r.x + 20,
        btn_r.y + 14,
        btn_label,
        if hover { 0x00101018 } else { COLOR_TEXT },
        3,
    );
    layout.clickable.push((btn_r, MenuAction::LoadRom));

    let hint = "OR DRAG AND DROP A .NES FILE ONTO THIS WINDOW";
    let hint_w = text_width(hint, 1);
    draw_text(fb, fb_width, fb_height, cx - hint_w / 2, btn_r.y + btn_h + 16, hint, COLOR_TEXT_DIM, 1);
    let hint2 = "BRING YOUR OWN LEGALLY-OBTAINED ROM - NONE IS INCLUDED";
    let hint2_w = text_width(hint2, 1);
    draw_text(fb, fb_width, fb_height, cx - hint2_w / 2, btn_r.y + btn_h + 30, hint2, COLOR_TEXT_DIM, 1);

    if let Some(err) = error {
        let err_w = text_width(err, 1);
        draw_text(fb, fb_width, fb_height, cx - err_w / 2, btn_r.y + btn_h + 52, err, 0x00E06060, 1);
    }

    layout
}

fn toggle_row(
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
    x: i32,
    y: i32,
    label: &str,
    on: bool,
    action: MenuAction,
    mouse: (i32, i32),
    layout: &mut MenuLayout,
) {
    let r = Rect { x, y, w: PANEL_W - 32, h: ROW_H - 4 };
    let hover = r.contains(mouse.0, mouse.1);
    fill_rect(fb, fb_width, fb_height, r.x, r.y, r.w, r.h, if hover { COLOR_ROW_HOVER } else { COLOR_ROW_BG });
    draw_text(fb, fb_width, fb_height, r.x + 8, r.y + 5, label, COLOR_TEXT, 2);
    let badge = if on { "ON" } else { "OFF" };
    let badge_color = if on { COLOR_ON } else { COLOR_OFF };
    let badge_x = r.x + r.w - text_width(badge, 2) - 12;
    draw_text(fb, fb_width, fb_height, badge_x, r.y + 5, badge, badge_color, 2);
    layout.clickable.push((r, action));
}

fn stepper_row(
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
    x: i32,
    y: i32,
    label: &str,
    value: &str,
    minus: MenuAction,
    plus: MenuAction,
    mouse: (i32, i32),
    layout: &mut MenuLayout,
) {
    let row_w = PANEL_W - 32;
    let r = Rect { x, y, w: row_w, h: ROW_H - 4 };
    fill_rect(fb, fb_width, fb_height, r.x, r.y, r.w, r.h, COLOR_ROW_BG);
    draw_text(fb, fb_width, fb_height, r.x + 8, r.y + 5, label, COLOR_TEXT, 2);

    let btn_w = 20;
    // 76px (not 60) between the minus button and the right edge: leaves
    // room for up to a 4-character value ("300%") between minus and plus
    // without the text running into the plus button - it did at 60px.
    let minus_r = Rect { x: r.x + r.w - btn_w * 2 - 76, y: r.y, w: btn_w, h: r.h };
    let plus_r = Rect { x: r.x + r.w - btn_w - 12, y: r.y, w: btn_w, h: r.h };
    let value_x = minus_r.x + btn_w + 8;

    for (br, glyph_str) in [(minus_r, "-"), (plus_r, "+")] {
        let hover = br.contains(mouse.0, mouse.1);
        fill_rect(fb, fb_width, fb_height, br.x, br.y, br.w, br.h, if hover { COLOR_ACCENT } else { COLOR_ROW_HOVER });
        draw_text(fb, fb_width, fb_height, br.x + 6, br.y + 5, glyph_str, if hover { 0x00101018 } else { COLOR_TEXT }, 2);
    }
    draw_text(fb, fb_width, fb_height, value_x, r.y + 5, value, COLOR_ACCENT, 2);

    layout.clickable.push((minus_r, minus));
    layout.clickable.push((plus_r, plus));
}

fn draw_settings_tab(
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
    panel_x: i32,
    start_y: i32,
    settings: &Settings,
    mouse: (i32, i32),
    layout: &mut MenuLayout,
) {
    let x = panel_x + 16;
    let mut y = start_y;
    toggle_row(fb, fb_width, fb_height, x, y, "WIDESCREEN", settings.widescreen, MenuAction::ToggleWidescreen, mouse, layout);
    y += ROW_H;
    toggle_row(fb, fb_width, fb_height, x, y, "NO SPRITE FLICKER", settings.unlimited_sprites, MenuAction::ToggleNoSpriteLimit, mouse, layout);
    y += ROW_H;
    toggle_row(fb, fb_width, fb_height, x, y, "PIXEL PERFECT", settings.pixel_perfect, MenuAction::TogglePixelPerfect, mouse, layout);
    y += ROW_H;
    stepper_row(fb, fb_width, fb_height, x, y, "ZOOM", &format!("{}%", settings.zoom_percent), MenuAction::ZoomDelta(-10), MenuAction::ZoomDelta(10), mouse, layout);
    y += ROW_H;
    toggle_row(fb, fb_width, fb_height, x, y, "FULLSCREEN", settings.fullscreen, MenuAction::ToggleFullscreen, mouse, layout);
    y += ROW_H;
    toggle_row(fb, fb_width, fb_height, x, y, "AUDIO", !settings.audio_muted, MenuAction::ToggleAudioMuted, mouse, layout);
}

fn draw_mods_tab(
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
    panel_x: i32,
    start_y: i32,
    mods: &[ModEntry],
    mouse: (i32, i32),
    layout: &mut MenuLayout,
) {
    let x = panel_x + 16;
    let mut y = start_y;
    if mods.is_empty() {
        draw_text(fb, fb_width, fb_height, x, y + 5, "NO MODS FOUND IN ./MODS", COLOR_TEXT_DIM, 2);
        return;
    }
    for (i, m) in mods.iter().enumerate() {
        toggle_row(fb, fb_width, fb_height, x, y, &m.name, m.enabled, MenuAction::ToggleMod(i), mouse, layout);
        y += ROW_H;
    }
}

fn draw_debug_tab(
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
    panel_x: i32,
    start_y: i32,
    debug: Option<&DebugInfo>,
    mouse: (i32, i32),
    layout: &mut MenuLayout,
) {
    let x = panel_x + 16;
    let mut y = start_y;
    let Some(debug) = debug else {
        draw_text(fb, fb_width, fb_height, x, y + 5, "NO ROM LOADED", COLOR_TEXT_DIM, 2);
        return;
    };

    stepper_row(fb, fb_width, fb_height, x, y, "P1 LIVES", &debug.p1_lives.to_string(), MenuAction::LivesDelta(-1), MenuAction::LivesDelta(1), mouse, layout);
    y += ROW_H;
    stepper_row(fb, fb_width, fb_height, x, y, "CONTINUES", &debug.continues.to_string(), MenuAction::ContinuesDelta(-1), MenuAction::ContinuesDelta(1), mouse, layout);
    y += ROW_H + 4;

    draw_text(fb, fb_width, fb_height, x, y, "P1 WEAPON", COLOR_TEXT, 2);
    y += 18;
    for (id, name) in WEAPON_NAMES {
        let selected = debug.p1_weapon == id;
        let r = Rect { x, y, w: PANEL_W - 32, h: ROW_H - 4 };
        let hover = r.contains(mouse.0, mouse.1);
        let bg = if selected { COLOR_ACCENT } else if hover { COLOR_ROW_HOVER } else { COLOR_ROW_BG };
        fill_rect(fb, fb_width, fb_height, r.x, r.y, r.w, r.h, bg);
        let text_color = if selected { 0x00101018 } else { COLOR_TEXT };
        draw_text(fb, fb_width, fb_height, r.x + 8, r.y + 5, name, text_color, 2);
        layout.clickable.push((r, MenuAction::SetWeapon(id)));
        y += ROW_H;
    }
}
