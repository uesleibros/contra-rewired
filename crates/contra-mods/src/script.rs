//! `mlua`-backed script host. Only compiled with `--features lua`.
//!
//! Host API today - two layers, both real:
//!
//! **Low-level** (general NES/6502 concepts, work on any game in principle):
//! - `contra.on(eventName, fn)` / events fired: `frame_tick` (no payload),
//!   `stage_start(stage)` / `stage_clear(stage)` (0-based stage index,
//!   fired together whenever `apps/contra-pc` observes `CURRENT_LEVEL`
//!   change between frames), `player_hit({player, lives_remaining})`
//!   (fired when a player's lives count drops - a RAM-diff proxy for "got
//!   hit", so it's really "just lost a life"; see `fire_player_hit`'s doc
//!   comment), `enemy_spawn({slot, enemy_type, x, y, hp})` (fired from a
//!   real CPU instruction hook on `initialize_enemy` - the one routine
//!   every enemy type funnels through - so unlike the other three this one
//!   isn't a RAM-diff guess, it's the actual spawn moment; see
//!   `fire_enemy_spawn`'s doc comment and `contra-pc::main::
//!   INITIALIZE_ENEMY_PC`).
//! - `contra.log(msg)`.
//! - `contra.frame()` - the current frame counter, set by the host via
//!   [`LuaModHost::set_frame`] once per emulated frame. Drives time-based
//!   effects (`math.sin(contra.frame() / 10)`) without a script needing its
//!   own clock.
//! - `contra.write_ppu(addr, value)` - queues a raw PPU-address write
//!   (`$0000-$3FFF`: pattern tables/CHR-RAM, nametables, palette), applied
//!   after the current event finishes firing. Cosmetic only - can't affect
//!   game state, only what's already-drawn pixels look like.
//! - `contra.poke_ram(addr, value)` - queues a raw CPU work-RAM write
//!   (`$0000-$07FF`), applied the same way. Unlike `write_ppu`, this *does*
//!   change real game state - it's what a trainer/cheat mod uses.
//! - `contra.peek_ram(addr)` - reads work RAM as of the start of the
//!   current frame (see [`LuaModHost::set_ram_snapshot`]).
//! - `contra.draw_text(x, y, text[, {r=, g=, b=}])` - screen-space text
//!   overlay drawn by the host's own UI (`egui` in `contra-pc`), not the
//!   NES PPU - full Unicode, any position, doesn't touch a tile grid or
//!   fight the game's nametable for space. Purely presentational, same
//!   spirit as `write_ppu` but for text a mod wants to show that Contra's
//!   own font/HUD was never going to render (custom UI, debug readouts,
//!   messages) - see [`TextDraw`].
//! - `contra.draw_rect(x, y, w, h[, {r=, g=, b=}, filled])` - screen-space
//!   rectangle overlay, same coordinate system and reasoning as
//!   `draw_text`. `filled` defaults to `false` (outline, matching the
//!   built-in hitbox overlay's look) - see [`RectDraw`].
//!
//! **High-level** (`contra.player`, Contra-specific, built entirely on the
//! low-level primitives above - see the address constants at the top of
//! this file, sourced from the community disassembly's `ram.asm`):
//! - `contra.player.get_lives(idx)` / `.set_lives(idx, n)`
//! - `contra.player.get_weapon(idx)` / `.set_weapon(idx, id)`
//! - `contra.player.get_continues()` / `.set_continues(n)`
//!
//! `idx` is `0` for player 1, `1` for player 2. Weapon IDs and other
//! encodings match the raw byte values the game itself uses (see
//! docs/MODDING.md for what's documented so far).
//!
//! This is a real, working, if still not exhaustive, gameplay-hook surface
//! - expanding it (typed event payloads, more high-level helpers) is
//! tracked in ROADMAP.md, Phase 3.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, Table};
use thiserror::Error;

/// RAM addresses backing the high-level `contra.player.*` helpers, from the
/// community disassembly's `ram.asm`. `P1_*`/`P2_*` are 2 bytes apart
/// (indexed by player), `NUM_CONTINUES` is shared.
mod ram_addr {
    pub const P_NUM_LIVES: u16 = 0x32; // + player index
    pub const P_CURRENT_WEAPON: u16 = 0xAA; // + player index
    pub const NUM_CONTINUES: u16 = 0x3A;
    // Enemy slot arrays, one entry per slot (+ slot index, `0-15`) - the
    // same layout `apps/contra-pc`'s `INITIALIZE_ENEMY_PC` hook reads to
    // build `enemy_spawn`'s payload.
    pub const ENEMY_TYPE: u16 = 0x0528;
    pub const ENEMY_X_POS: u16 = 0x033E;
    pub const ENEMY_Y_POS: u16 = 0x0324;
    pub const ENEMY_HP: u16 = 0x0578;
}

#[derive(Debug, Error)]
pub enum ScriptError {
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}

/// Events a Lua mod can subscribe to via `contra.on(eventName, fn)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModEvent {
    EnemySpawn,
    PlayerHit,
    StageStart,
    StageClear,
    FrameTick,
}

impl ModEvent {
    fn lua_name(self) -> &'static str {
        match self {
            ModEvent::EnemySpawn => "enemy_spawn",
            ModEvent::PlayerHit => "player_hit",
            ModEvent::StageStart => "stage_start",
            ModEvent::StageClear => "stage_clear",
            ModEvent::FrameTick => "frame_tick",
        }
    }
}

/// One `contra.draw_text(...)` call queued by a mod - see
/// [`LuaModHost::take_pending_text_draws`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextDraw {
    /// Screen-space pixel position (top-left), *not* a tile/nametable
    /// coordinate - this is drawn by the host's own UI layer (`egui` in
    /// `contra-pc`), not the NES PPU, so it isn't bound to an 8x8 tile
    /// grid or the console's font. See this struct's doc comment on why.
    pub x: i32,
    pub y: i32,
    pub text: String,
    pub color: (u8, u8, u8),
}

/// One `contra.draw_rect(...)` call queued by a mod - see
/// [`LuaModHost::take_pending_rect_draws`]. Same screen-space coordinate
/// system as [`TextDraw`] (top-left origin, NES pixels).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RectDraw {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub color: (u8, u8, u8),
    pub filled: bool,
}

/// Reads an optional `{r=, g=, b=}` Lua table into an RGB triple, used by
/// both `contra.draw_text` and `contra.draw_rect` - a missing table, or any
/// channel missing from a present one, falls back to `default` rather than
/// erroring, so a mod that only cares about *some* of the color doesn't
/// need to spell out the rest.
fn default_rgb(color: &Option<Table>, default: (u8, u8, u8)) -> LuaResult<(u8, u8, u8)> {
    let channel = |key: &str, fallback: u8| -> LuaResult<u8> {
        match color {
            Some(t) => Ok(t.get::<_, Option<u8>>(key)?.unwrap_or(fallback)),
            None => Ok(fallback),
        }
    };
    Ok((channel("r", default.0)?, channel("g", default.1)?, channel("b", default.2)?))
}

/// One loaded mod's Lua VM. Each mod gets its own `Lua` instance so a
/// misbehaving mod can't reach into another mod's globals.
pub struct LuaModHost {
    lua: Lua,
    frame: Rc<RefCell<u64>>,
    pending_ppu_writes: Rc<RefCell<Vec<(u16, u8)>>>,
    pending_ram_writes: Rc<RefCell<Vec<(u16, u8)>>>,
    pending_text_draws: Rc<RefCell<Vec<TextDraw>>>,
    pending_rect_draws: Rc<RefCell<Vec<RectDraw>>>,
    ram_snapshot: Rc<RefCell<Vec<u8>>>,
}

impl LuaModHost {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        let frame = Rc::new(RefCell::new(0u64));
        let pending_ppu_writes: Rc<RefCell<Vec<(u16, u8)>>> = Rc::new(RefCell::new(Vec::new()));
        let pending_ram_writes: Rc<RefCell<Vec<(u16, u8)>>> = Rc::new(RefCell::new(Vec::new()));
        let pending_text_draws: Rc<RefCell<Vec<TextDraw>>> = Rc::new(RefCell::new(Vec::new()));
        let pending_rect_draws: Rc<RefCell<Vec<RectDraw>>> = Rc::new(RefCell::new(Vec::new()));
        let ram_snapshot: Rc<RefCell<Vec<u8>>> = Rc::new(RefCell::new(vec![0; 0x800]));

        let contra_table = lua.create_table()?;
        let handlers = lua.create_table()?;
        contra_table.set("_handlers", handlers)?;

        let on_fn = lua.create_function(|lua, (event, callback): (String, mlua::Function)| {
            let contra: Table = lua.globals().get("contra")?;
            let handlers: Table = contra.get("_handlers")?;
            let bucket: Table = match handlers.get::<_, Option<Table>>(event.clone())? {
                Some(t) => t,
                None => {
                    let t = lua.create_table()?;
                    handlers.set(event, t.clone())?;
                    t
                }
            };
            bucket.set(bucket.raw_len() + 1, callback)?;
            Ok(())
        })?;
        contra_table.set("on", on_fn)?;

        let log_fn = lua.create_function(|_, msg: String| {
            log::info!(target: "contra-mods", "{msg}");
            Ok(())
        })?;
        contra_table.set("log", log_fn)?;

        let frame_for_read = frame.clone();
        let frame_fn = lua.create_function(move |_, ()| Ok(*frame_for_read.borrow()))?;
        contra_table.set("frame", frame_fn)?;

        let ppu_writes_for_queue = pending_ppu_writes.clone();
        let write_ppu_fn = lua.create_function(move |_, (addr, value): (u16, u8)| {
            ppu_writes_for_queue.borrow_mut().push((addr, value));
            Ok(())
        })?;
        contra_table.set("write_ppu", write_ppu_fn)?;

        // Screen-space text overlay - the host's own UI font (`egui`'s in
        // `contra-pc`), not the NES's, and not bound to an 8x8 tile grid or
        // a nametable address. Deliberately *not* built on `write_ppu`:
        // drawing real text into the PPU would mean either patching CHR-RAM
        // with a custom font (destructive - overwrites real game tiles) or
        // learning Contra's own font's tile-index mapping and fighting the
        // nametable for space the game is already using. An overlay avoids
        // both and gives mods full Unicode, not just whatever glyphs
        // happen to be in this one game's character set. `color` is an
        // optional `{r=, g=, b=}` table (each `0-255`); omitted or any
        // missing channel defaults to a warm off-white matching the stats
        // overlay's own text color, so a mod that doesn't care about color
        // still gets something readable.
        let text_draws_for_queue = pending_text_draws.clone();
        let draw_text_fn = lua.create_function(move |_, (x, y, text, color): (i32, i32, String, Option<Table>)| {
            let rgb = default_rgb(&color, (255, 224, 128))?;
            text_draws_for_queue.borrow_mut().push(TextDraw { x, y, text, color: rgb });
            Ok(())
        })?;
        contra_table.set("draw_text", draw_text_fn)?;

        // Screen-space rectangle overlay - same coordinate system and
        // reasoning as `draw_text` above (host UI, not the PPU): a mod
        // visualizing hitboxes, spawn zones, or any other rectangular
        // region of interest doesn't need to fight the nametable or patch
        // CHR-RAM for it. `filled` defaults to `false` (outline only,
        // matching the built-in hitbox overlay's own look) if omitted.
        let rect_draws_for_queue = pending_rect_draws.clone();
        let draw_rect_fn = lua.create_function(move |_, (x, y, w, h, color, filled): (i32, i32, i32, i32, Option<Table>, Option<bool>)| {
            let rgb = default_rgb(&color, (255, 64, 64))?;
            rect_draws_for_queue.borrow_mut().push(RectDraw { x, y, w, h, color: rgb, filled: filled.unwrap_or(false) });
            Ok(())
        })?;
        contra_table.set("draw_rect", draw_rect_fn)?;

        let ram_writes_for_queue = pending_ram_writes.clone();
        let poke_ram_fn = lua.create_function(move |_, (addr, value): (u16, u8)| {
            ram_writes_for_queue.borrow_mut().push((addr, value));
            Ok(())
        })?;
        contra_table.set("poke_ram", poke_ram_fn)?;

        let ram_for_read = ram_snapshot.clone();
        let peek_ram_fn = lua.create_function(move |_, addr: u16| {
            let ram = ram_for_read.borrow();
            Ok(ram.get((addr & 0x07FF) as usize).copied().unwrap_or(0))
        })?;
        contra_table.set("peek_ram", peek_ram_fn)?;

        // High-level `contra.player.*`, built entirely on poke_ram/peek_ram
        // above via the same pending-write queue - see `ram_addr`.
        let player_table = lua.create_table()?;

        let w = pending_ram_writes.clone();
        player_table.set(
            "set_lives",
            lua.create_function(move |_, (idx, n): (u16, u8)| {
                w.borrow_mut().push((ram_addr::P_NUM_LIVES + idx, n));
                Ok(())
            })?,
        )?;
        let r = ram_snapshot.clone();
        player_table.set(
            "get_lives",
            lua.create_function(move |_, idx: u16| {
                let ram = r.borrow();
                Ok(ram.get(((ram_addr::P_NUM_LIVES + idx) & 0x07FF) as usize).copied().unwrap_or(0))
            })?,
        )?;

        let w = pending_ram_writes.clone();
        player_table.set(
            "set_weapon",
            lua.create_function(move |_, (idx, id): (u16, u8)| {
                w.borrow_mut().push((ram_addr::P_CURRENT_WEAPON + idx, id));
                Ok(())
            })?,
        )?;
        let r = ram_snapshot.clone();
        player_table.set(
            "get_weapon",
            lua.create_function(move |_, idx: u16| {
                let ram = r.borrow();
                Ok(ram.get(((ram_addr::P_CURRENT_WEAPON + idx) & 0x07FF) as usize).copied().unwrap_or(0))
            })?,
        )?;

        let w = pending_ram_writes.clone();
        player_table.set(
            "set_continues",
            lua.create_function(move |_, n: u8| {
                w.borrow_mut().push((ram_addr::NUM_CONTINUES, n));
                Ok(())
            })?,
        )?;
        let r = ram_snapshot.clone();
        player_table.set(
            "get_continues",
            lua.create_function(move |_, ()| {
                let ram = r.borrow();
                Ok(ram.get(ram_addr::NUM_CONTINUES as usize).copied().unwrap_or(0))
            })?,
        )?;

        contra_table.set("player", player_table)?;

        // `contra.enemy.*` - read-only (no `set_*`, unlike `player`: an
        // enemy slot's fields are only meaningful together and change
        // constantly as the game's own logic runs, so poking one in
        // isolation is far more likely to desync/crash the enemy's own
        // state machine than do something a mod author actually wants -
        // `contra.poke_ram` is still there directly for anyone who
        // genuinely needs it). `slot` is `0-15`, the same index
        // `enemy_spawn`'s payload uses.
        let enemy_table = lua.create_table()?;
        macro_rules! enemy_getter {
            ($name:literal, $addr:expr) => {
                let r = ram_snapshot.clone();
                enemy_table.set(
                    $name,
                    lua.create_function(move |_, slot: u16| {
                        let ram = r.borrow();
                        Ok(ram.get((($addr + slot) & 0x07FF) as usize).copied().unwrap_or(0))
                    })?,
                )?;
            };
        }
        enemy_getter!("get_type", ram_addr::ENEMY_TYPE);
        enemy_getter!("get_x", ram_addr::ENEMY_X_POS);
        enemy_getter!("get_y", ram_addr::ENEMY_Y_POS);
        enemy_getter!("get_hp", ram_addr::ENEMY_HP);
        contra_table.set("enemy", enemy_table)?;

        lua.globals().set("contra", contra_table)?;
        Ok(Self { lua, frame, pending_ppu_writes, pending_ram_writes, pending_text_draws, pending_rect_draws, ram_snapshot })
    }

    pub fn load_script(&self, source: &str, chunk_name: &str) -> Result<(), ScriptError> {
        self.lua.load(source).set_name(chunk_name).exec()?;
        Ok(())
    }

    /// Sets the value `contra.frame()` returns until the next call.
    pub fn set_frame(&self, frame: u64) {
        *self.frame.borrow_mut() = frame;
    }

    /// Updates what `contra.peek_ram()` reads. Call once per frame with the
    /// emulator's live RAM *before* firing events, so scripts see
    /// this-frame-fresh state rather than last frame's.
    pub fn set_ram_snapshot(&self, ram: &[u8]) {
        let mut snap = self.ram_snapshot.borrow_mut();
        snap.clear();
        snap.extend_from_slice(ram);
    }

    /// Calls every handler registered for `event` with `args` (`()` for no
    /// payload). Shared by [`Self::fire`] and the typed `fire_*` helpers
    /// below - the only difference between them is what they pass here.
    fn fire_with<'lua, A>(&'lua self, event: ModEvent, args: A) -> Result<(), ScriptError>
    where
        A: mlua::IntoLuaMulti<'lua> + Clone,
    {
        let contra: Table = self.lua.globals().get("contra")?;
        let handlers: Table = contra.get("_handlers")?;
        if let Some(bucket) = handlers.get::<_, Option<Table>>(event.lua_name())? {
            for pair in bucket.sequence_values::<mlua::Function>() {
                pair?.call::<_, ()>(args.clone())?;
            }
        }
        Ok(())
    }

    /// Fires an event with no payload - only [`ModEvent::FrameTick`] uses
    /// this; the others carry real data via the typed helpers below.
    pub fn fire(&self, event: ModEvent) -> Result<(), ScriptError> {
        self.fire_with(event, ())
    }

    /// Fires [`ModEvent::StageStart`] with the 0-based stage index a script
    /// receives as its handler's first argument
    /// (`contra.on("stage_start", function(stage) ... end)`).
    pub fn fire_stage_start(&self, stage: u8) -> Result<(), ScriptError> {
        self.fire_with(ModEvent::StageStart, stage)
    }

    /// Fires [`ModEvent::StageClear`] with the 0-based stage index that was
    /// just cleared - the host calls this immediately before
    /// [`Self::fire_stage_start`] with the new one, since both fire from
    /// the same "`CURRENT_LEVEL` changed" observation (see
    /// `apps/contra-pc/src/main.rs`) with no separate "you cleared it"
    /// signal to tell them apart otherwise.
    pub fn fire_stage_clear(&self, stage: u8) -> Result<(), ScriptError> {
        self.fire_with(ModEvent::StageClear, stage)
    }

    /// Fires [`ModEvent::PlayerHit`] with a `{player, lives_remaining}`
    /// table (`player` is `0`/`1` for P1/P2, matching every other
    /// `contra.player.*` index). Triggered by the host observing that
    /// player's lives count go down between frames - the closest signal
    /// this project can read without the actual disassembly to find a
    /// precise "just got hit" flag, so it's really "just lost a life",
    /// which won't fire for a hit survived on a shield/invincibility frame.
    /// Documented as such rather than claimed more precise than it is.
    pub fn fire_player_hit(&self, player_idx: u8, lives_remaining: u8) -> Result<(), ScriptError> {
        let payload = self.lua.create_table()?;
        payload.set("player", player_idx)?;
        payload.set("lives_remaining", lives_remaining)?;
        self.fire_with(ModEvent::PlayerHit, payload)
    }

    /// Fires [`ModEvent::EnemySpawn`] with a `{slot, enemy_type, x, y, hp}`
    /// table. `slot` is `0-15`, the same enemy-slot index every
    /// `ENEMY_*,x`-indexed RAM array (`ENEMY_TYPE`, `ENEMY_HP`, ...) uses -
    /// unlike [`Self::fire_player_hit`], this one *is* precise: the host
    /// triggers it from a real CPU instruction hook on `initialize_enemy`
    /// (the single routine every enemy type funnels through to populate a
    /// freshly-claimed slot - see `INITIALIZE_ENEMY_PC` in `contra-pc`'s
    /// `main.rs`), not a RAM-diff guessing at what probably just happened.
    pub fn fire_enemy_spawn(&self, slot: u8, enemy_type: u8, x: u8, y: u8, hp: u8) -> Result<(), ScriptError> {
        let payload = self.lua.create_table()?;
        payload.set("slot", slot)?;
        payload.set("enemy_type", enemy_type)?;
        payload.set("x", x)?;
        payload.set("y", y)?;
        payload.set("hp", hp)?;
        self.fire_with(ModEvent::EnemySpawn, payload)
    }

    /// Drains every `contra.write_ppu(addr, value)` call queued since the
    /// last drain, for the host to actually apply to the running emulator.
    pub fn take_pending_ppu_writes(&self) -> Vec<(u16, u8)> {
        std::mem::take(&mut *self.pending_ppu_writes.borrow_mut())
    }

    /// Drains every `contra.poke_ram(...)` / `contra.player.set_*(...)`
    /// call queued since the last drain.
    pub fn take_pending_ram_writes(&self) -> Vec<(u16, u8)> {
        std::mem::take(&mut *self.pending_ram_writes.borrow_mut())
    }

    /// Drains every `contra.draw_text(...)` call queued since the last
    /// drain, for the host to render as a screen-space overlay this frame
    /// (see [`TextDraw`]'s doc comment).
    pub fn take_pending_text_draws(&self) -> Vec<TextDraw> {
        std::mem::take(&mut *self.pending_text_draws.borrow_mut())
    }

    /// Drains every `contra.draw_rect(...)` call queued since the last
    /// drain, for the host to render as a screen-space overlay this frame
    /// (see [`RectDraw`]'s doc comment).
    pub fn take_pending_rect_draws(&self) -> Vec<RectDraw> {
        std::mem::take(&mut *self.pending_rect_draws.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enemy_getters_read_the_shared_snapshot_by_slot() {
        let host = LuaModHost::new().unwrap();
        let mut ram = vec![0u8; 0x800];
        ram[(ram_addr::ENEMY_TYPE + 3) as usize] = 0x0a;
        ram[(ram_addr::ENEMY_X_POS + 3) as usize] = 200;
        ram[(ram_addr::ENEMY_Y_POS + 3) as usize] = 100;
        ram[(ram_addr::ENEMY_HP + 3) as usize] = 2;
        host.set_ram_snapshot(&ram);
        host.load_script(
            r#"
                seen = {}
                contra.on("frame_tick", function()
                    seen.type = contra.enemy.get_type(3)
                    seen.x = contra.enemy.get_x(3)
                    seen.y = contra.enemy.get_y(3)
                    seen.hp = contra.enemy.get_hp(3)
                end)
            "#,
            "enemy_reader",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let seen: Table = host.lua.globals().get("seen").unwrap();
        assert_eq!(seen.get::<_, u8>("type").unwrap(), 0x0a);
        assert_eq!(seen.get::<_, u8>("x").unwrap(), 200);
        assert_eq!(seen.get::<_, u8>("y").unwrap(), 100);
        assert_eq!(seen.get::<_, u8>("hp").unwrap(), 2);
    }

    #[test]
    fn fire_enemy_spawn_delivers_full_payload_to_handlers() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                seen = nil
                contra.on("enemy_spawn", function(e)
                    seen = e
                end)
            "#,
            "enemy_tracker",
        )
        .unwrap();
        host.fire_enemy_spawn(3, 0x0a, 100, 50, 4).unwrap();
        let seen: Table = host.lua.globals().get("seen").unwrap();
        assert_eq!(seen.get::<_, u8>("slot").unwrap(), 3);
        assert_eq!(seen.get::<_, u8>("enemy_type").unwrap(), 0x0a);
        assert_eq!(seen.get::<_, u8>("x").unwrap(), 100);
        assert_eq!(seen.get::<_, u8>("y").unwrap(), 50);
        assert_eq!(seen.get::<_, u8>("hp").unwrap(), 4);
    }

    #[test]
    fn draw_rect_queues_with_defaulted_color_and_fill_and_drains_once() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                contra.on("frame_tick", function()
                    contra.draw_rect(10, 20, 16, 16)
                    contra.draw_rect(1, 2, 8, 8, {r = 0, g = 255, b = 0}, true)
                end)
            "#,
            "rect_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let draws = host.take_pending_rect_draws();
        assert_eq!(
            draws,
            vec![
                RectDraw { x: 10, y: 20, w: 16, h: 16, color: (255, 64, 64), filled: false },
                RectDraw { x: 1, y: 2, w: 8, h: 8, color: (0, 255, 0), filled: true },
            ]
        );
        assert!(host.take_pending_rect_draws().is_empty());
    }

    #[test]
    fn draw_text_queues_with_defaulted_color_and_drains_once() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                contra.on("frame_tick", function()
                    contra.draw_text(10, 20, "hello")
                    contra.draw_text(1, 2, "colored", {r = 0, g = 255, b = 0})
                    contra.draw_text(3, 4, "partial", {g = 100})
                end)
            "#,
            "text_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let draws = host.take_pending_text_draws();
        assert_eq!(
            draws,
            vec![
                TextDraw { x: 10, y: 20, text: "hello".into(), color: (255, 224, 128) },
                TextDraw { x: 1, y: 2, text: "colored".into(), color: (0, 255, 0) },
                TextDraw { x: 3, y: 4, text: "partial".into(), color: (255, 100, 128) },
            ]
        );
        assert!(host.take_pending_text_draws().is_empty());
    }

    #[test]
    fn script_can_register_and_receive_events() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                contra.on("stage_start", function()
                    contra.log("stage started from lua")
                end)
            "#,
            "test_mod",
        )
        .unwrap();
        host.fire(ModEvent::StageStart).unwrap();
    }

    #[test]
    fn host_functions_are_callable_and_observable() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                counter = 0
                contra.on("frame_tick", function() counter = counter + 1 end)
            "#,
            "counter_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let counter: i64 = host.lua.globals().get("counter").unwrap();
        assert_eq!(counter, 2);
    }

    #[test]
    fn scripts_can_read_the_host_supplied_frame_counter() {
        let host = LuaModHost::new().unwrap();
        host.set_frame(42);
        host.load_script(
            r#"
                seen_frame = 0
                contra.on("frame_tick", function() seen_frame = contra.frame() end)
            "#,
            "clock_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let seen: i64 = host.lua.globals().get("seen_frame").unwrap();
        assert_eq!(seen, 42);
    }

    #[test]
    fn write_ppu_calls_are_queued_and_drained_in_order() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                contra.on("frame_tick", function()
                    contra.write_ppu(0x3F11, 0x01)
                    contra.write_ppu(0x3F12, 0x16)
                end)
            "#,
            "rgb_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let writes = host.take_pending_ppu_writes();
        assert_eq!(writes, vec![(0x3F11, 0x01), (0x3F12, 0x16)]);
        // A second drain with nothing new queued must come back empty, not
        // repeat the same writes forever.
        assert!(host.take_pending_ppu_writes().is_empty());
    }

    #[test]
    fn a_full_rgb_cycling_mod_produces_varied_colors_over_simulated_time() {
        // This is the actual "RGB character" mod shape: cycle a sprite
        // palette entry through the full NES color range using the frame
        // counter as a clock, entirely from Lua.
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                contra.on("frame_tick", function()
                    local color = contra.frame() % 64
                    contra.write_ppu(0x3F11, color)
                end)
            "#,
            "rgb_character",
        )
        .unwrap();

        let mut colors_seen = std::collections::HashSet::new();
        for frame in 0..200u64 {
            host.set_frame(frame);
            host.fire(ModEvent::FrameTick).unwrap();
            for (addr, value) in host.take_pending_ppu_writes() {
                assert_eq!(addr, 0x3F11);
                colors_seen.insert(value);
            }
        }
        assert!(colors_seen.len() > 10, "expected many distinct colors over 200 frames, got {}", colors_seen.len());
    }

    #[test]
    fn low_level_ram_peek_and_poke_round_trip() {
        let host = LuaModHost::new().unwrap();
        host.set_ram_snapshot(&{
            let mut ram = vec![0u8; 0x800];
            ram[0x32] = 3; // P1 lives
            ram
        });
        host.load_script(
            r#"
                seen_lives = 0
                contra.on("frame_tick", function()
                    seen_lives = contra.peek_ram(0x32)
                    contra.poke_ram(0x32, seen_lives + 1)
                end)
            "#,
            "ram_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let seen: i64 = host.lua.globals().get("seen_lives").unwrap();
        assert_eq!(seen, 3);
        assert_eq!(host.take_pending_ram_writes(), vec![(0x32, 4)]);
    }

    #[test]
    fn high_level_player_helpers_write_the_documented_addresses() {
        let host = LuaModHost::new().unwrap();
        host.load_script(
            r#"
                contra.on("frame_tick", function()
                    contra.player.set_lives(0, 9)   -- P1
                    contra.player.set_lives(1, 5)   -- P2
                    contra.player.set_weapon(0, 3)  -- P1 weapon id 3
                    contra.player.set_continues(2)
                end)
            "#,
            "cheat_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let writes = host.take_pending_ram_writes();
        assert_eq!(
            writes,
            vec![
                (ram_addr::P_NUM_LIVES, 9),
                (ram_addr::P_NUM_LIVES + 1, 5),
                (ram_addr::P_CURRENT_WEAPON, 3),
                (ram_addr::NUM_CONTINUES, 2),
            ]
        );
    }

    #[test]
    fn high_level_player_helpers_read_back_via_the_shared_snapshot() {
        let host = LuaModHost::new().unwrap();
        let mut ram = vec![0u8; 0x800];
        ram[(ram_addr::P_NUM_LIVES + 1) as usize] = 7; // P2 lives
        host.set_ram_snapshot(&ram);
        host.load_script(
            r#"
                p2_lives = 0
                contra.on("frame_tick", function() p2_lives = contra.player.get_lives(1) end)
            "#,
            "read_mod",
        )
        .unwrap();
        host.fire(ModEvent::FrameTick).unwrap();
        let seen: i64 = host.lua.globals().get("p2_lives").unwrap();
        assert_eq!(seen, 7);
    }
}
