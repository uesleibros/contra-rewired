//! A minimal in-window pause menu: a built-in 5x7 bitmap font (no external
//! font/asset dependency - keeps with the "ships nothing but code" model)
//! and a handful of real, working toggles. This is a deliberately small v1
//! - the goal is a genuine, functional settings surface to build on, not a
//! finished options screen.

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
        '%' => [0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b10001],
        '>' => [0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000],
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

/// Fills a solid rectangle - used for the menu's background panel.
pub fn fill_rect(fb: &mut [u32], fb_width: usize, fb_height: usize, x: i32, y: i32, w: i32, h: i32, color: u32) {
    for py in y.max(0)..(y + h).min(fb_height as i32) {
        for px in x.max(0)..(x + w).min(fb_width as i32) {
            fb[py as usize * fb_width + px as usize] = color;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuItem {
    Widescreen,
    NoSpriteLimit,
    PixelPerfect,
    Zoom,
    Fullscreen,
    AudioMuted,
    Resume,
}

pub const MENU_ITEMS: [MenuItem; 7] = [
    MenuItem::Widescreen,
    MenuItem::NoSpriteLimit,
    MenuItem::PixelPerfect,
    MenuItem::Zoom,
    MenuItem::Fullscreen,
    MenuItem::AudioMuted,
    MenuItem::Resume,
];

pub struct MenuState {
    pub selected: usize,
}

impl MenuState {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn move_up(&mut self) {
        self.selected = (self.selected + MENU_ITEMS.len() - 1) % MENU_ITEMS.len();
    }

    pub fn move_down(&mut self) {
        self.selected = (self.selected + 1) % MENU_ITEMS.len();
    }

    pub fn current(&self) -> MenuItem {
        MENU_ITEMS[self.selected]
    }
}

/// Settings the pause menu actually toggles. Kept as one small struct so
/// `main.rs` has one place to read from when deciding how to render/present
/// each frame.
#[derive(Debug, Clone, Copy)]
pub struct Settings {
    /// Renders wider than 256px, sampling nametable data the game already
    /// drew for scrolling (see `contra_nes::EXTENDED_WIDTH`). When on, the
    /// actual width used each frame tracks the live window's aspect ratio
    /// (see `main.rs`'s `compute_wide_width`), up to the safe cap.
    pub widescreen: bool,
    /// Lifts the real hardware's 8-sprites-per-scanline limit (the cause
    /// of "sprite flicker"). Off by default - `Original` mode stays
    /// hardware-accurate.
    pub unlimited_sprites: bool,
    /// On: integer-only scaling, crisp NES pixels, possible letterboxing.
    /// Off (default): fractional "fill the window" scaling that tracks
    /// any window size live, matching how e.g. the Switch Pokemon/Link's
    /// Awakening ports handle a resizable/dockable display.
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

pub fn draw_pause_menu(fb: &mut [u32], fb_width: usize, fb_height: usize, menu: &MenuState, settings: &Settings) {
    let item_count = MENU_ITEMS.len() as i32;
    let panel_w = 26 * 6 * 2 + 20;
    let panel_h = item_count * 20 + 40 + 36; // + header space + 2 help-text lines
    let panel_x = (fb_width as i32 - panel_w) / 2;
    let panel_y = (fb_height as i32 - panel_h) / 2;

    fill_rect(fb, fb_width, fb_height, panel_x, panel_y, panel_w, panel_h, 0x00101018);
    fill_rect(fb, fb_width, fb_height, panel_x + 4, panel_y + 4, panel_w - 8, panel_h - 8, 0x00202838);

    let text_x = panel_x + 20;
    let mut text_y = panel_y + 16;
    draw_text(fb, fb_width, fb_height, text_x, text_y, "PAUSED", 0x00FFE080, 2);
    text_y += 28;

    let on_off = |b: bool| if b { "ON" } else { "OFF" };
    let lines = [
        (MenuItem::Widescreen, format!("WIDESCREEN: {}", on_off(settings.widescreen))),
        (MenuItem::NoSpriteLimit, format!("NO SPRITE FLICKER: {}", on_off(settings.unlimited_sprites))),
        (MenuItem::PixelPerfect, format!("PIXEL PERFECT: {}", on_off(settings.pixel_perfect))),
        (MenuItem::Zoom, format!("ZOOM: {}%", settings.zoom_percent)),
        (MenuItem::Fullscreen, format!("FULLSCREEN: {}", on_off(settings.fullscreen))),
        (MenuItem::AudioMuted, format!("AUDIO: {}", on_off(!settings.audio_muted))),
        (MenuItem::Resume, "RESUME".to_string()),
    ];

    for (item, label) in lines {
        let is_selected = item == menu.current();
        let color = if is_selected { 0x00FFFFFF } else { 0x00A0A8B0 };
        let cursor = if is_selected { ">" } else { " " };
        draw_text(fb, fb_width, fb_height, text_x, text_y, &format!("{cursor} {label}"), color, 2);
        text_y += 20;
    }

    text_y += 8;
    draw_text(fb, fb_width, fb_height, text_x, text_y, "UP-DOWN NAVIGATE  X TOGGLE", 0x00707880, 1);
    text_y += 10;
    draw_text(fb, fb_width, fb_height, text_x, text_y, "LEFT-RIGHT ADJUST ZOOM", 0x00707880, 1);
}
