//! A 2C02 PPU core, driven at **scanline granularity** rather than
//! per-dot. For each of the 240 visible scanlines, the background is
//! rendered from the current scroll registers (derived directly from the
//! `v` "loopy" register - see NESdev's "PPU scrolling" article, this is
//! the same algorithm real hardware uses, just evaluated once per line
//! instead of once per dot), then `v` is advanced exactly the way the
//! hardware advances it at dot 256 (Y increment) and dot 257 (horizontal
//! bits copied from `t`). This reproduces per-scanline scroll splits (the
//! common "HUD status bar" technique) correctly; it does **not** reproduce
//! effects that change PPU registers mid-scanline (rare outside of a
//! handful of trick effects). See docs/FIDELITY.md.
//!
//! Assumes CHR-RAM (mapper 2 / UxROM's usual configuration, which is what
//! Contra (USA) uses) rather than bank-switched CHR-ROM.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

pub const SCREEN_W: usize = 256;
pub const SCREEN_H: usize = 240;

/// How far past the normal 256px view is safe to sample *live* nametable/
/// pattern data from, symmetrically on both sides. This is a real,
/// hardware-imposed radius, not an arbitrary choice - tuned empirically
/// against the real US retail ROM: the NES only has 2 physical
/// nametables, and Contra's engine only pre-draws the direction it
/// auto-scrolls *toward*, so live-sampling too far past the live window
/// on the trailing edge shows solid black (undrawn tiles) well before the
/// leading edge does. 380px total (62px extra on each side) rendered
/// clean at every scroll position tested; 420px already showed black
/// creeping into the trailing edge's sky row, and 480px was consistently
/// broken on the trailing side.
///
/// This used to also be the hard cap on [`Ppu::wide_width`] - true
/// ultrawide beyond it wasn't reachable without either accepting that
/// trailing-edge garbage or touching game state to force it to pre-draw
/// further. It no longer is: everything past this radius is served from
/// [`Ppu::tile_cache`] (tiles genuinely displayed at some earlier point
/// this level, remembered rather than re-guessed) or left blank if never
/// visited, instead of being read live - see [`Ppu::render_background_line`].
pub const EXTENDED_WIDTH: usize = 380;

/// Hard ceiling on [`Ppu::wide_width`] now that going past
/// [`EXTENDED_WIDTH`] is served from [`Ppu::tile_cache`]/blank rather than
/// a live (and potentially wrong) VRAM read - this is just "wide enough
/// for any real ultrawide monitor at any reasonable zoom," not a
/// correctness boundary. 1024px covers 32:9 (needs ~854px at native NES
/// height) with headroom.
pub const MAX_WIDE_WIDTH: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mirroring {
    Horizontal,
    Vertical,
}

const CTRL_NT_MASK: u8 = 0x03;
const CTRL_VRAM_INC32: u8 = 1 << 2;
const CTRL_SPRITE_PT: u8 = 1 << 3;
const CTRL_BG_PT: u8 = 1 << 4;
const CTRL_SPRITE_16: u8 = 1 << 5;
const CTRL_NMI: u8 = 1 << 7;

const MASK_SHOW_BG: u8 = 1 << 3;
const MASK_SHOW_SPRITES: u8 = 1 << 4;

const STATUS_SPRITE_OVERFLOW: u8 = 1 << 5;
const STATUS_SPRITE0_HIT: u8 = 1 << 6;
const STATUS_VBLANK: u8 = 1 << 7;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ppu {
    #[serde(with = "crate::serde_arrays::arr_0x800")]
    pub vram: [u8; 0x800],
    pub palette: [u8; 32],
    #[serde(with = "crate::serde_arrays::arr_256")]
    pub oam: [u8; 256],
    #[serde(with = "crate::serde_arrays::arr_0x2000")]
    pub chr_ram: [u8; 0x2000],
    pub oam_addr: u8,
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub v: u16,
    pub t: u16,
    pub fine_x: u8,
    pub write_toggle: bool,
    pub data_buffer: u8,
    pub mirroring: Mirroring,
    pub nmi_requested: bool,
    #[serde(skip)]
    pub framebuffer: Vec<u32>,
    /// Presentation-only "Extended" widescreen width: `SCREEN_W` (256)
    /// means disabled/normal; anything greater (up to [`EXTENDED_WIDTH`])
    /// renders that many pixels wide instead, and can change every frame
    /// (e.g. to track a resizable window's live aspect ratio) without any
    /// cost when left at `SCREEN_W`. Skipped in save states: it's a
    /// display setting the front-end owns, not part of the machine's
    /// state.
    #[serde(skip)]
    pub wide_width: usize,
    #[serde(skip)]
    pub wide_framebuffer: Vec<u32>,
    /// Presentation-only: when true, the 8-sprites-per-scanline limit
    /// (real NES hardware's cause of "sprite flicker") is lifted, up to
    /// all 64 OAM sprites per line. Off by default so `Original` mode
    /// stays hardware-accurate; purely a rendering choice, same as
    /// `wide_width` - never touches game state.
    #[serde(skip)]
    pub unlimited_sprites: bool,
    /// Presentation-only "true ultrawide" memory: every (tile, palette)
    /// this level has actually displayed at a given absolute horizontal
    /// tile column and screen-relative tile row, keyed
    /// `(abs_tile_col, coarse_y)`. Populated as a side effect of the
    /// normal live-sampled render (see [`Self::render_background_line`]);
    /// consulted for wide-mode columns beyond [`EXTENDED_WIDTH`]'s safe
    /// live-read radius, where sampling live VRAM directly risks showing
    /// undrawn/wrong data. This only ever remembers what the *unmodified*
    /// game genuinely drew - it can't introduce wrong data, only whether
    /// there's real data available for a given far-off column yet.
    /// Cleared on a detected screen/level transition (see
    /// [`Self::update_absolute_scroll`] and the mask-off/on handling in
    /// [`Self::write_register`]).
    #[serde(skip)]
    tile_cache: HashMap<(i32, u8), (u8, u8)>,
    /// This level's cumulative horizontal scroll in pixels, unwrapped
    /// (unlike the PPU's own 0..511 wrapping scroll) so it can key
    /// [`Self::tile_cache`] by an ever-increasing absolute position rather
    /// than one that wraps every 512px. Reset (along with the cache) on a
    /// detected level/screen transition.
    #[serde(skip)]
    absolute_scroll_x: i64,
    /// Raw 0..511 scroll position sampled at the end of the previous
    /// frame, purely to compute this frame's delta into
    /// [`Self::absolute_scroll_x`] - see [`Self::update_absolute_scroll`].
    #[serde(skip)]
    prev_frame_scroll_x: Option<u16>,
}

impl Ppu {
    pub fn new(mirroring: Mirroring) -> Self {
        Self {
            vram: [0; 0x800],
            palette: [0; 32],
            oam: [0; 256],
            chr_ram: [0; 0x2000],
            oam_addr: 0,
            ctrl: 0,
            mask: 0,
            status: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            write_toggle: false,
            data_buffer: 0,
            mirroring,
            nmi_requested: false,
            framebuffer: vec![0; SCREEN_W * SCREEN_H],
            wide_width: SCREEN_W,
            wide_framebuffer: Vec::new(),
            unlimited_sprites: false,
            tile_cache: HashMap::new(),
            absolute_scroll_x: 0,
            prev_frame_scroll_x: None,
        }
    }

    fn nmi_enabled(&self) -> bool {
        self.ctrl & CTRL_NMI != 0
    }

    fn bg_enabled(&self) -> bool {
        self.mask & MASK_SHOW_BG != 0
    }

    fn sprites_enabled(&self) -> bool {
        self.mask & MASK_SHOW_SPRITES != 0
    }

    // ---- CPU-facing register interface ($2000-$2007, mirrored to $3FFF) ----

    pub fn read_register(&mut self, reg: u16) -> u8 {
        match reg & 7 {
            2 => {
                let v = self.status | (self.data_buffer & 0x1F);
                self.status &= !STATUS_VBLANK;
                self.write_toggle = false;
                v
            }
            4 => self.oam[self.oam_addr as usize],
            7 => {
                let addr = self.v & 0x3FFF;
                let value = if addr >= 0x3F00 {
                    self.data_buffer = self.read_vram(addr - 0x1000);
                    self.read_palette(addr)
                } else {
                    let buffered = self.data_buffer;
                    self.data_buffer = self.read_vram(addr);
                    buffered
                };
                self.v = self.v.wrapping_add(if self.ctrl & CTRL_VRAM_INC32 != 0 { 32 } else { 1 });
                value
            }
            _ => 0,
        }
    }

    pub fn write_register(&mut self, reg: u16, value: u8) {
        match reg & 7 {
            0 => {
                self.ctrl = value;
                self.t = (self.t & !0x0C00) | (((value & CTRL_NT_MASK) as u16) << 10);
            }
            1 => {
                let was_bg_enabled = self.bg_enabled();
                self.mask = value;
                if self.bg_enabled() && !was_bg_enabled {
                    // Background rendering just flipped back on - the
                    // standard NES tell for "a new screen is ready": title
                    // screens, game-overs, and Contra's own level-load fade
                    // all mask rendering off while they rewrite VRAM over
                    // several frames, then flip it back on once done. Tiles
                    // cached under the old screen's absolute coordinate
                    // space are stale here even when the scroll position
                    // happens to land close to where it used to be (e.g.
                    // both levels starting at scroll 0) - the previous
                    // clear-on-big-scroll-delta heuristic alone missed
                    // exactly that case, which is what let a jumped-to
                    // stage show persistent colliding/flickering tiles:
                    // stale cache entries from the old level never got
                    // invalidated and were never re-read since a cache hit
                    // is trusted forever. Clearing here doesn't depend on
                    // scroll math at all, so it also catches every organic
                    // level transition, not just the debug stage-jump.
                    self.tile_cache.clear();
                    self.absolute_scroll_x = 0;
                    self.prev_frame_scroll_x = None;
                }
            }
            3 => self.oam_addr = value,
            4 => {
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if !self.write_toggle {
                    self.fine_x = value & 0x07;
                    self.t = (self.t & !0x001F) | ((value >> 3) as u16);
                } else {
                    self.t = (self.t & !0x73E0) | (((value & 0x07) as u16) << 12) | (((value >> 3) as u16) << 5);
                }
                self.write_toggle = !self.write_toggle;
            }
            6 => {
                if !self.write_toggle {
                    self.t = (self.t & 0x00FF) | (((value & 0x3F) as u16) << 8);
                } else {
                    self.t = (self.t & 0xFF00) | value as u16;
                    self.v = self.t;
                }
                self.write_toggle = !self.write_toggle;
            }
            7 => {
                let addr = self.v & 0x3FFF;
                self.write_vram(addr, value);
                self.v = self.v.wrapping_add(if self.ctrl & CTRL_VRAM_INC32 != 0 { 32 } else { 1 });
            }
            _ => {}
        }
    }

    pub fn oam_dma_write(&mut self, offset: u8, value: u8) {
        self.oam[offset as usize] = value;
    }

    // ---- Memory ----

    fn nametable_index(&self, addr: u16) -> usize {
        let nt = ((addr - 0x2000) / 0x400) as usize;
        let offset = (addr as usize - 0x2000) % 0x400;
        let physical = match self.mirroring {
            Mirroring::Horizontal => nt >> 1,
            Mirroring::Vertical => nt & 1,
        };
        physical * 0x400 + offset
    }

    fn read_vram(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x2FFF => self.vram[self.nametable_index(addr)],
            0x3000..=0x3EFF => self.vram[self.nametable_index(addr - 0x1000)],
            _ => 0,
        }
    }

    /// Direct external write into PPU address space (`$0000-$3FFF`:
    /// pattern tables/CHR-RAM, nametables, palette), bypassing the
    /// `$2006`/`$2007` register sequence real hardware/game code would use.
    /// For tooling that pokes memory directly - mods, trainers, debug UIs -
    /// not part of normal CPU-driven emulation.
    pub fn poke(&mut self, addr: u16, value: u8) {
        self.write_vram(addr, value);
    }

    /// Direct external read of PPU address space (`$0000-$3FFF`), the read
    /// counterpart to [`Self::poke`] - for tooling (debug UIs, the asset
    /// extraction verification examples) that wants to inspect live
    /// nametable/attribute/palette state without going through the
    /// `$2006`/`$2007` register sequence real game code uses.
    pub fn peek(&self, addr: u16) -> u8 {
        let masked = addr & 0x3FFF;
        match masked {
            0x3F00..=0x3FFF => self.read_palette(masked),
            _ => self.read_vram(masked),
        }
    }

    fn write_vram(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x2FFF => {
                let i = self.nametable_index(addr);
                self.vram[i] = value;
            }
            0x3000..=0x3EFF => {
                let i = self.nametable_index(addr - 0x1000);
                self.vram[i] = value;
            }
            0x3F00..=0x3FFF => self.write_palette(addr, value),
            _ => {}
        }
    }

    fn palette_index(addr: u16) -> usize {
        let mut i = (addr & 0x1F) as usize;
        if i >= 16 && i % 4 == 0 {
            i -= 16; // $3F10/$14/$18/$1C mirror $3F00/$04/$08/$0C
        }
        i
    }
    fn read_palette(&self, addr: u16) -> u8 {
        self.palette[Self::palette_index(addr)]
    }
    fn write_palette(&mut self, addr: u16, value: u8) {
        self.palette[Self::palette_index(addr)] = value;
    }

    // ---- Rendering ----

    /// Called once per visible scanline (0..SCREEN_H). Renders background
    /// + sprites for `y` using the current `v`/`t`/`fine_x`, then advances
    /// `v` for the next line exactly as hardware does at dots 256/257.
    /// When [`Self::wide_width`] is greater than `SCREEN_W`, renders into
    /// [`Self::wide_framebuffer`] at that width instead of
    /// [`Self::framebuffer`] at [`SCREEN_W`] - see [`EXTENDED_WIDTH`]'s
    /// docs for why this can't affect gameplay.
    pub fn render_scanline(&mut self, y: usize) {
        let width = self.wide_width.clamp(SCREEN_W, MAX_WIDE_WIDTH);
        let wide = width > SCREEN_W;
        let extra = width as i32 - SCREEN_W as i32;
        // Always centered: the normal 256px view sits at a fixed position
        // in the wide frame regardless of scroll direction, so the
        // player's on-screen position relative to the window never shifts
        // - matching real Contra's camera framing exactly, wide or not.
        // (A direction-biased offset was tried here - putting most of the
        // extra width on the scrolling-toward edge, to dodge stale tiles
        // on the trailing edge - but it moved the player's apparent
        // on-screen position as the bias tracked scroll direction, which
        // read as the camera itself moving differently than normal. Fixed
        // centering doesn't have that problem, and `EXTENDED_WIDTH` (see
        // its docs) was already tuned empirically *with* fixed centering
        // to be clean at 380px within its safe live-read radius, so this
        // isn't reopening the trailing-edge issue there - and past that
        // radius, [`Self::tile_cache`] takes over rather than a live read.)
        let x_offset = if wide { extra / 2 } else { 0 };

        let mut line = [0u32; MAX_WIDE_WIDTH];
        let mut bg_opaque = [false; MAX_WIDE_WIDTH];

        if self.bg_enabled() {
            self.render_background_line(width, x_offset, &mut bg_opaque[..width], &mut line[..width]);
        } else {
            let backdrop = NES_PALETTE[(self.palette[0] & 0x3F) as usize];
            line[..width].fill(backdrop);
        }
        if self.sprites_enabled() {
            self.render_sprites_line(y, width, x_offset, &bg_opaque[..width], &mut line[..width]);
        }

        if wide {
            // Sized (and re-sized, if `wide_width` changed since the last
            // scanline - e.g. a live window resize) to exactly `width` per
            // row, not the max cap, so the buffer's actual stride always
            // matches what the caller asked for this frame.
            if self.wide_framebuffer.len() != width * SCREEN_H {
                self.wide_framebuffer = vec![0; width * SCREEN_H];
            }
            self.wide_framebuffer[y * width..(y + 1) * width].copy_from_slice(&line[..width]);
        } else {
            self.framebuffer[y * SCREEN_W..(y + 1) * SCREEN_W].copy_from_slice(&line[..SCREEN_W]);
        }

        if y == SCREEN_H - 1 {
            self.update_absolute_scroll();
        }

        self.advance_v_for_next_line();
    }

    /// How many pixels the wide-mode background/sprites are currently
    /// shifted right of their normal (narrow-mode) position - always
    /// `(width - SCREEN_W) / 2` (fixed centering, not direction-biased -
    /// see the comment in [`Self::render_scanline`] for why), `0` when not
    /// wide. Public so a front-end overlaying its own screen-space
    /// annotations (e.g. a hitbox viewer) on top of the wide framebuffer
    /// can line them up with where sprites actually got drawn.
    pub fn wide_x_offset(&self) -> i32 {
        let width = self.wide_width.clamp(SCREEN_W, MAX_WIDE_WIDTH);
        if width <= SCREEN_W {
            0
        } else {
            (width - SCREEN_W) as i32 / 2
        }
    }

    /// Current sprite height in pixels (8 or 16), from PPUCTRL bit 5 - the
    /// same value [`Self::render_sprites_line`] uses, exposed for a
    /// front-end drawing sprite-bounds overlays (see [`Self::wide_x_offset`]).
    pub fn sprite_height(&self) -> i32 {
        if self.ctrl & CTRL_SPRITE_16 != 0 {
            16
        } else {
            8
        }
    }

    /// `width` pixels of background starting `x_offset` pixels to the left
    /// of the normal 256px view (0 for the normal, non-widescreen case).
    ///
    /// Always fills every column - there's no blank/backdrop fallback for
    /// "don't know yet" columns, by design: a wide window with black gaps
    /// reads as broken, and Contra was never going to be pixel-perfect
    /// everywhere in a mode it wasn't built for anyway. Priority order per
    /// column: (1) [`Self::tile_cache`] - a tile this level has *actually*
    /// displayed before at this absolute position, the reliable case; (2) a
    /// live VRAM read using the *current* nametable/attribute data
    /// wrapped to whatever real tile happens to land there - not
    /// guaranteed correct that far from the live camera window, but always
    /// draws *something* real (an actual NES tile, just possibly the wrong
    /// one) rather than nothing. Only case (1)'s reads get cached; a
    /// wrapped guess from case (2) is deliberately not remembered, so once
    /// the level actually shows that position for real, the cache
    /// overwrites the guess with the correct tile instead of being stuck
    /// with a wrong cached value.
    fn render_background_line(&mut self, width: usize, x_offset: i32, bg_opaque: &mut [bool], out: &mut [u32]) {
        const SAFE_LIVE_MARGIN: i32 = ((EXTENDED_WIDTH - SCREEN_W) / 2) as i32;

        let base_nt = (self.v >> 10) & 0x03;
        let coarse_x0 = (self.v & 0x1F) as i32;
        let coarse_y = (self.v >> 5) & 0x1F;
        let fine_y = (self.v >> 12) & 0x07;
        let pattern_base: u16 = if self.ctrl & CTRL_BG_PT != 0 { 0x1000 } else { 0x0000 };
        let backdrop = NES_PALETTE[(self.palette[0] & 0x3F) as usize];

        for x in 0..width {
            let total_fine_x = self.fine_x as i32 + x as i32 - x_offset;
            let px_in_tile = total_fine_x.rem_euclid(8) as u8;
            let in_safe_margin = total_fine_x >= -SAFE_LIVE_MARGIN && total_fine_x < SCREEN_W as i32 + SAFE_LIVE_MARGIN;
            let abs_tile_col = (self.absolute_scroll_x + total_fine_x as i64).div_euclid(8) as i32;
            let cached = self.tile_cache.get(&(abs_tile_col, coarse_y as u8)).copied();

            let (tile_index, palette_select) = if let Some(cached) = cached {
                cached
            } else {
                let tile_offset = total_fine_x.div_euclid(8);
                let tile_col_raw = coarse_x0 + tile_offset;
                let tile_col = tile_col_raw.rem_euclid(32) as u16;
                let nt_h_flip = tile_col_raw.div_euclid(32).rem_euclid(2) == 1;
                let nt = base_nt ^ (nt_h_flip as u16);

                let nt_addr = 0x2000 + nt * 0x400 + coarse_y * 32 + tile_col;
                let tile_index = self.read_vram(nt_addr);

                let attr_addr = 0x2000 + nt * 0x400 + 0x3C0 + (coarse_y / 4) * 8 + tile_col / 4;
                let attr_byte = self.read_vram(attr_addr);
                let quadrant = (((coarse_y % 4) / 2) * 2 + (tile_col % 4) / 2) as u8;
                let palette_select = (attr_byte >> (quadrant * 2)) & 0x03;

                if in_safe_margin {
                    self.tile_cache.insert((abs_tile_col, coarse_y as u8), (tile_index, palette_select));
                }
                (tile_index, palette_select)
            };

            let plane0 = self.read_vram(pattern_base + tile_index as u16 * 16 + fine_y);
            let plane1 = self.read_vram(pattern_base + tile_index as u16 * 16 + fine_y + 8);
            let bit = 7 - px_in_tile;
            let color_index = (((plane1 >> bit) & 1) << 1) | ((plane0 >> bit) & 1);

            let color = if color_index == 0 {
                backdrop
            } else {
                bg_opaque[x] = true;
                let pal = self.read_palette(0x3F00 + (palette_select as u16) * 4 + color_index as u16);
                NES_PALETTE[(pal & 0x3F) as usize]
            };
            out[x] = color;
        }
    }

    /// Updates [`Self::absolute_scroll_x`] from how far the playfield
    /// scrolled since the same point last frame - sampled at the last
    /// visible scanline, below any split-scroll status bar (which
    /// typically occupies only the first several scanlines and has its
    /// own scroll, unrelated to camera movement). An unusually large
    /// single-frame delta (much more than any real scrolling speed) means
    /// a screen/level transition just happened - a checkpoint, respawn, or
    /// warp - at which point the absolute coordinate space [`Self::
    /// tile_cache`] is keyed against no longer means anything consistent,
    /// so the cache is cleared instead of being polluted with entries
    /// under the wrong key.
    fn update_absolute_scroll(&mut self) {
        let raw_x = (((self.v >> 10) & 1) as i32) * 256 + ((self.v & 0x1F) as i32) * 8 + self.fine_x as i32;
        if let Some(prev) = self.prev_frame_scroll_x {
            let mut delta = raw_x - prev as i32;
            if delta > 256 {
                delta -= 512;
            } else if delta < -256 {
                delta += 512;
            }
            if delta.abs() > 64 {
                self.tile_cache.clear();
            } else {
                self.absolute_scroll_x += delta as i64;
            }
        }
        self.prev_frame_scroll_x = Some(raw_x as u16);
    }

    /// `width`/`x_offset` as in [`Self::render_background_line`]. Sprite
    /// positions are shifted by `x_offset` so they stay visually aligned
    /// with the (possibly widened) background. Note: in wide mode, sprite
    /// 0 hit is evaluated against the widened background, which can differ
    /// in timing from real hardware (which never had that extra background
    /// to hit) - a known, accepted tradeoff of an opt-in presentation mode
    /// that never touches actual game state.
    fn render_sprites_line(&mut self, y: usize, width: usize, x_offset: i32, bg_opaque: &[bool], out: &mut [u32]) {
        let sprite_height: i32 = if self.ctrl & CTRL_SPRITE_16 != 0 { 16 } else { 8 };
        let mut rendered_on_line = 0u8;
        let mut sprite0_this_line = false;
        // Sprite 0 has the highest display priority, sprite 63 the lowest:
        // among the (up to 8) sprites on this scanline, a lower OAM index
        // must win any overlap. Iterating 0..64 and writing unconditionally
        // would let a *later* (lower-priority) sprite overwrite an earlier
        // one wherever they overlap, which is backwards from hardware and
        // shows up as flicker/wrong-part-on-top on any multi-sprite
        // character or overlapping-sprite effect. Track which pixels a
        // higher-priority sprite already claimed this line and skip them.
        let mut claimed = [false; MAX_WIDE_WIDTH];

        for i in 0..64 {
            let sprite_y = self.oam[i * 4] as i32 + 1;
            let row = y as i32 - sprite_y;
            if row < 0 || row >= sprite_height {
                continue;
            }
            if rendered_on_line >= 8 {
                self.status |= STATUS_SPRITE_OVERFLOW;
                if !self.unlimited_sprites {
                    break;
                }
                // `unlimited_sprites`: still report overflow (a script/mod
                // reading $2002 should see the same flag a real cartridge
                // would), just don't stop drawing at the hardware's 8-per-
                // line cap - this is the actual "no sprite flicker" effect,
                // an opt-in accuracy break, never on in Original mode.
            }
            rendered_on_line += 1;

            let tile = self.oam[i * 4 + 1];
            let attr = self.oam[i * 4 + 2];
            let sx = self.oam[i * 4 + 3] as i32;
            let flip_h = attr & 0x40 != 0;
            let flip_v = attr & 0x80 != 0;
            let behind_bg = attr & 0x20 != 0;
            let palette_select = attr & 0x03;

            let mut row_in_sprite = row;
            if flip_v {
                row_in_sprite = sprite_height - 1 - row_in_sprite;
            }

            let (pattern_table, tile_index, fine_row) = if sprite_height == 16 {
                let table = if tile & 1 != 0 { 0x1000 } else { 0x0000 };
                let base_tile = (tile & 0xFE) as u16 + if row_in_sprite >= 8 { 1 } else { 0 };
                (table, base_tile, (row_in_sprite % 8) as u16)
            } else {
                let table: u16 = if self.ctrl & CTRL_SPRITE_PT != 0 { 0x1000 } else { 0x0000 };
                (table, tile as u16, row_in_sprite as u16)
            };

            let plane0 = self.read_vram(pattern_table + tile_index * 16 + fine_row);
            let plane1 = self.read_vram(pattern_table + tile_index * 16 + fine_row + 8);

            for col in 0..8i32 {
                let px = if flip_h { col } else { 7 - col };
                let bit = px as u8;
                let color_index = (((plane1 >> bit) & 1) << 1) | ((plane0 >> bit) & 1);
                if color_index == 0 {
                    continue;
                }
                let screen_x = sx + col + x_offset;
                if !(0..width as i32).contains(&screen_x) {
                    continue;
                }
                if i == 0 && bg_opaque[screen_x as usize] {
                    sprite0_this_line = true;
                }
                if claimed[screen_x as usize] {
                    continue;
                }
                if behind_bg && bg_opaque[screen_x as usize] {
                    // A lower-priority-than-background pixel is still
                    // "claimed" for sprite-vs-sprite priority purposes even
                    // though it doesn't get drawn over the background.
                    claimed[screen_x as usize] = true;
                    continue;
                }
                claimed[screen_x as usize] = true;
                let pal = self.read_palette(0x3F10 + (palette_select as u16) * 4 + color_index as u16);
                out[screen_x as usize] = NES_PALETTE[(pal & 0x3F) as usize];
            }
        }

        if sprite0_this_line {
            self.status |= STATUS_SPRITE0_HIT;
        }
    }

    /// Mirrors the hardware's dot-256 Y increment and dot-257 horizontal-bits
    /// copy, so scroll registers set mid-frame take effect on the correct
    /// scanline boundary.
    fn advance_v_for_next_line(&mut self) {
        if self.bg_enabled() || self.sprites_enabled() {
            let mut v = self.v;
            if v & 0x7000 != 0x7000 {
                v += 0x1000;
            } else {
                v &= !0x7000;
                let mut coarse_y = (v & 0x03E0) >> 5;
                if coarse_y == 29 {
                    coarse_y = 0;
                    v ^= 0x0800;
                } else if coarse_y == 31 {
                    coarse_y = 0;
                } else {
                    coarse_y += 1;
                }
                v = (v & !0x03E0) | (coarse_y << 5);
            }
            v = (v & !0x041F) | (self.t & 0x041F);
            self.v = v;
        }
    }

    /// Called once, at the start of vblank (scanline 241). Sets the vblank
    /// flag and returns whether NMI should fire.
    pub fn start_vblank(&mut self) -> bool {
        self.status |= STATUS_VBLANK;
        self.nmi_enabled()
    }

    /// Called once, at the pre-render line: clears status flags and copies
    /// vertical scroll bits from `t` into `v` (approximating the real
    /// per-dot 280-304 copy).
    pub fn start_prerender(&mut self) {
        self.status &= !(STATUS_VBLANK | STATUS_SPRITE0_HIT | STATUS_SPRITE_OVERFLOW);
        if self.bg_enabled() || self.sprites_enabled() {
            self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
        }
    }
}

/// The standard NTSC 2C02 palette (64 entries, RGB packed as 0x00RRGGBB).
/// These are measured/derived hardware output colors, not Konami content -
/// the same table (give or take small revisions) ships in essentially
/// every NES emulator.
#[rustfmt::skip]
pub const NES_PALETTE: [u32; 64] = [
    0x00666666, 0x00002A88, 0x001412A7, 0x003B00A4, 0x005C007E, 0x006E0040, 0x006C0600, 0x00561D00,
    0x00333500, 0x000B4800, 0x00005200, 0x00004F08, 0x0000404D, 0x00000000, 0x00000000, 0x00000000,
    0x00ADADAD, 0x00155FD9, 0x004240FF, 0x007527FE, 0x00A01ACC, 0x00B71E7B, 0x00B53120, 0x00994E00,
    0x006B6D00, 0x00388700, 0x000C9300, 0x00008F32, 0x00007C8D, 0x00000000, 0x00000000, 0x00000000,
    0x00FFFEFF, 0x0064B0FF, 0x009290FF, 0x00C676FF, 0x00F36AFF, 0x00FE6ECC, 0x00FE8170, 0x00EA9E22,
    0x00BCBE00, 0x0088D800, 0x005CE430, 0x0045E082, 0x0048CDDE, 0x004F4F4F, 0x00000000, 0x00000000,
    0x00FFFEFF, 0x00C0DFFF, 0x00D3D2FF, 0x00E8C8FF, 0x00FBC2FF, 0x00FEC4EA, 0x00FECCC5, 0x00F7D8A5,
    0x00E4E594, 0x00CFEF96, 0x00BDF4AB, 0x00B3F3CC, 0x00B5EBF2, 0x00B8B8B8, 0x00000000, 0x00000000,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn ppu_with_test_pattern() -> Ppu {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.mask = MASK_SHOW_BG;
        // Fill every nametable byte across both logical nametables with a
        // repeating, non-trivial tile pattern so widening the view actually
        // samples something other than tile 0 everywhere.
        for addr in 0x2000u16..0x2800 {
            let i = (addr - 0x2000) as u8;
            ppu.write_vram(addr, i.wrapping_mul(7).wrapping_add(1));
        }
        for tile in 0u16..256 {
            for row in 0..8u16 {
                ppu.chr_ram[(tile * 16 + row) as usize] = 0xAA;
                ppu.chr_ram[(tile * 16 + row + 8) as usize] = 0x00;
            }
        }
        for i in 0..32u16 {
            ppu.write_palette(0x3F00 + i, (i + 1) as u8 & 0x3F);
        }
        ppu
    }

    #[test]
    fn wide_mode_center_matches_narrow_mode_exactly() {
        let mut narrow = ppu_with_test_pattern();
        let mut wide = ppu_with_test_pattern();
        wide.wide_width = EXTENDED_WIDTH;

        narrow.render_scanline(5);
        wide.render_scanline(5);

        let x_offset = (EXTENDED_WIDTH - SCREEN_W) / 2;
        let wide_row = &wide.wide_framebuffer[5 * EXTENDED_WIDTH..(5 + 1) * EXTENDED_WIDTH];
        let narrow_row = &narrow.framebuffer[5 * SCREEN_W..(5 + 1) * SCREEN_W];

        assert_eq!(
            &wide_row[x_offset..x_offset + SCREEN_W],
            narrow_row,
            "the center EXTENDED_WIDTH-minus-256 columns of the wide render must exactly match the normal render"
        );
        // And there must be *varied* (non-placeholder) content on both
        // extended sides, not just one flat color - otherwise this
        // "widescreen" mode would just be letterboxing with extra steps.
        // (Checked as "not every pixel identical" rather than "not zero":
        // a single specific pixel can legitimately land on one of the
        // NES's reserved black palette slots ($0D/$0E/$0F per group) by
        // coincidence, which isn't a rendering bug.)
        let left_ext = &wide_row[..x_offset];
        let right_ext = &wide_row[x_offset + SCREEN_W..];
        assert!(left_ext.iter().any(|&p| p != left_ext[0]), "left extension should show varied background data");
        assert!(right_ext.iter().any(|&p| p != right_ext[0]), "right extension should show varied background data");
    }

    #[test]
    fn arbitrary_wide_width_below_the_cap_produces_a_correctly_strided_buffer() {
        let mut ppu = ppu_with_test_pattern();
        ppu.wide_width = 320; // an in-between size, e.g. matching some window's aspect ratio
        ppu.render_scanline(3);
        assert_eq!(ppu.wide_framebuffer.len(), 320 * SCREEN_H);
        let row = &ppu.wide_framebuffer[3 * 320..4 * 320];
        assert!(row.iter().any(|&p| p != row[0]), "should show varied content, not a flat fill");
    }

    #[test]
    fn unlimited_sprites_draws_past_the_hardware_eight_per_line_cap() {
        let mut limited = Ppu::new(Mirroring::Horizontal);
        let mut unlimited = Ppu::new(Mirroring::Horizontal);
        for ppu in [&mut limited, &mut unlimited] {
            ppu.mask = MASK_SHOW_SPRITES;
            for row in 0..8usize {
                ppu.chr_ram[row] = 0xFF;
            }
            // $0D is one of the NES's reserved-black palette entries, so
            // the (sprite-less) backdrop renders as literal 0 and this
            // test's "count non-zero pixels" check only counts sprites.
            ppu.write_palette(0x3F00, 0x0D);
            ppu.write_palette(0x3F11, 0x01);
            // 10 sprites, all on the same scanline, spread out horizontally
            // so none overlap - only OAM-index-order + the 8-sprite cap
            // determines how many actually draw.
            for i in 0..10 {
                ppu.oam[i * 4] = 9;
                ppu.oam[i * 4 + 1] = 0;
                ppu.oam[i * 4 + 2] = 0;
                ppu.oam[i * 4 + 3] = (i * 10) as u8;
            }
        }
        unlimited.unlimited_sprites = true;

        limited.render_scanline(10);
        unlimited.render_scanline(10);

        let count_lit = |fb: &[u32]| fb[10 * SCREEN_W..11 * SCREEN_W].iter().filter(|&&p| p != 0).count();
        assert_eq!(count_lit(&limited.framebuffer), 8 * 8, "hardware cap: exactly 8 sprites x 8px wide");
        assert_eq!(count_lit(&unlimited.framebuffer), 10 * 8, "unlimited: all 10 sprites draw");
        assert_ne!(limited.status & STATUS_SPRITE_OVERFLOW, 0);
        assert_ne!(unlimited.status & STATUS_SPRITE_OVERFLOW, 0, "overflow flag should still be reported even when not enforced");
    }

    #[test]
    fn narrow_mode_is_unaffected_when_wide_framebuffer_never_allocated() {
        let mut ppu = ppu_with_test_pattern();
        assert_eq!(ppu.wide_width, SCREEN_W);
        ppu.render_scanline(0);
        assert!(ppu.wide_framebuffer.is_empty(), "narrow mode must not allocate the wide buffer at all");
    }

    #[test]
    fn lower_oam_index_sprite_wins_overlap_priority() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.mask = MASK_SHOW_SPRITES; // sprites only, background left blank

        // Tile 0 and tile 1: both fully opaque (color index 1) 8x8 tiles.
        for row in 0..8usize {
            ppu.chr_ram[row] = 0xFF; // tile 0 plane 0
            ppu.chr_ram[8 + row] = 0x00; // tile 0 plane 1
            ppu.chr_ram[16 + row] = 0xFF; // tile 1 plane 0
            ppu.chr_ram[24 + row] = 0x00; // tile 1 plane 1
        }
        ppu.write_palette(0x3F11, 0x01); // sprite palette 0, color 1
        ppu.write_palette(0x3F15, 0x02); // sprite palette 1, color 1

        // Sprite 0 (highest priority) and sprite 1 fully overlap at (50, 10).
        ppu.oam[0] = 9; // y+1 = 10
        ppu.oam[1] = 0; // tile 0
        ppu.oam[2] = 0; // palette 0, in front
        ppu.oam[3] = 50; // x

        ppu.oam[4] = 9;
        ppu.oam[5] = 1; // tile 1
        ppu.oam[6] = 1; // palette 1, in front
        ppu.oam[7] = 50;

        ppu.render_scanline(10);

        let expected = NES_PALETTE[0x01];
        let got = ppu.framebuffer[10 * SCREEN_W + 50];
        assert_eq!(got, expected, "sprite 0 must win the overlap, not sprite 1");
    }

    #[test]
    fn palette_mirroring_maps_sprite_backdrops_to_bg() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.write_palette(0x3F00, 0x0F);
        assert_eq!(ppu.read_palette(0x3F10), 0x0F);
    }

    #[test]
    fn horizontal_mirroring_maps_top_nametables_together() {
        let ppu = Ppu::new(Mirroring::Horizontal);
        assert_eq!(ppu.nametable_index(0x2000), ppu.nametable_index(0x23FF) - 0x3FF);
        // $2000 and $2400 share the same physical table under horizontal mirroring.
        assert_eq!(ppu.nametable_index(0x2005), ppu.nametable_index(0x2405));
    }

    #[test]
    fn vertical_mirroring_maps_left_nametables_together() {
        let ppu = Ppu::new(Mirroring::Vertical);
        assert_eq!(ppu.nametable_index(0x2005), ppu.nametable_index(0x2805));
    }

    #[test]
    fn ppuaddr_write_sequence_sets_v() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.write_register(6, 0x21); // high byte
        ppu.write_register(6, 0x08); // low byte
        assert_eq!(ppu.v, 0x2108);
    }

    #[test]
    fn status_read_clears_vblank_and_write_toggle() {
        let mut ppu = Ppu::new(Mirroring::Horizontal);
        ppu.status |= STATUS_VBLANK;
        ppu.write_toggle = true;
        let v = ppu.read_register(2);
        assert!(v & STATUS_VBLANK != 0);
        assert!(!ppu.write_toggle);
        assert_eq!(ppu.status & STATUS_VBLANK, 0);
    }
}
