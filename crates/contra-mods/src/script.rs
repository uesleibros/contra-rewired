//! `mlua`-backed script host. Only compiled with `--features lua`.
//!
//! Host API today - two layers, both real:
//!
//! **Low-level** (general NES/6502 concepts, work on any game in principle):
//! - `contra.on(eventName, fn)` / events fired: `frame_tick`, `stage_start`,
//!   `stage_clear`, `player_hit`, `enemy_spawn` (the last three not wired to
//!   any real emulator signal yet - see ROADMAP.md).
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

/// One loaded mod's Lua VM. Each mod gets its own `Lua` instance so a
/// misbehaving mod can't reach into another mod's globals.
pub struct LuaModHost {
    lua: Lua,
    frame: Rc<RefCell<u64>>,
    pending_ppu_writes: Rc<RefCell<Vec<(u16, u8)>>>,
    pending_ram_writes: Rc<RefCell<Vec<(u16, u8)>>>,
    ram_snapshot: Rc<RefCell<Vec<u8>>>,
}

impl LuaModHost {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        let frame = Rc::new(RefCell::new(0u64));
        let pending_ppu_writes: Rc<RefCell<Vec<(u16, u8)>>> = Rc::new(RefCell::new(Vec::new()));
        let pending_ram_writes: Rc<RefCell<Vec<(u16, u8)>>> = Rc::new(RefCell::new(Vec::new()));
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

        lua.globals().set("contra", contra_table)?;
        Ok(Self { lua, frame, pending_ppu_writes, pending_ram_writes, ram_snapshot })
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

    /// Fires an event with no arguments; extend as real gameplay hooks
    /// need typed payloads (enemy id, damage amount, etc).
    pub fn fire(&self, event: ModEvent) -> Result<(), ScriptError> {
        let contra: Table = self.lua.globals().get("contra")?;
        let handlers: Table = contra.get("_handlers")?;
        if let Some(bucket) = handlers.get::<_, Option<Table>>(event.lua_name())? {
            for pair in bucket.sequence_values::<mlua::Function>() {
                pair?.call::<_, ()>(())?;
            }
        }
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
