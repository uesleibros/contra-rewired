//! `mlua`-backed script host. Only compiled with `--features lua`.
//!
//! Host API today:
//! - `contra.on(eventName, fn)` / events fired: `frame_tick`, `stage_start`,
//!   `stage_clear`, `player_hit`, `enemy_spawn` (the last three not wired to
//!   any real emulator signal yet - see ROADMAP.md).
//! - `contra.log(msg)`.
//! - `contra.frame()` - returns the current frame counter, set by the host
//!   via [`LuaModHost::set_frame`] once per emulated frame. Lets a script
//!   drive time-based effects (`math.sin(contra.frame() / 10)` and the
//!   like) without needing its own clock.
//! - `contra.write_ppu(addr, value)` - queues a raw PPU-address write
//!   (nametable/pattern/palette space, `$0000-$3FFF`) to be applied by the
//!   host after the current event finishes firing (see
//!   [`LuaModHost::take_pending_writes`]). This is deliberately addressed
//!   in real NES PPU terms, not anything Contra-specific: a mod that
//!   cycles `$3F11` (sprite palette 0, color 1) each frame is doing exactly
//!   what a "give the player character a shifting RGB palette" mod looks
//!   like, using only general NES concepts this crate already understands.
//!
//! This is a real, working, if still small, gameplay-hook surface -
//! expanding it (typed event payloads, RAM peek/poke, registering new
//! behavior) is tracked in ROADMAP.md, Phase 3.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{Lua, Result as LuaResult, Table};
use thiserror::Error;

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
    pending_writes: Rc<RefCell<Vec<(u16, u8)>>>,
}

impl LuaModHost {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
        let frame = Rc::new(RefCell::new(0u64));
        let pending_writes: Rc<RefCell<Vec<(u16, u8)>>> = Rc::new(RefCell::new(Vec::new()));

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

        let writes_for_queue = pending_writes.clone();
        let write_ppu_fn = lua.create_function(move |_, (addr, value): (u16, u8)| {
            writes_for_queue.borrow_mut().push((addr, value));
            Ok(())
        })?;
        contra_table.set("write_ppu", write_ppu_fn)?;

        lua.globals().set("contra", contra_table)?;
        Ok(Self { lua, frame, pending_writes })
    }

    pub fn load_script(&self, source: &str, chunk_name: &str) -> Result<(), ScriptError> {
        self.lua.load(source).set_name(chunk_name).exec()?;
        Ok(())
    }

    /// Sets the value `contra.frame()` returns until the next call.
    pub fn set_frame(&self, frame: u64) {
        *self.frame.borrow_mut() = frame;
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
    pub fn take_pending_writes(&self) -> Vec<(u16, u8)> {
        std::mem::take(&mut *self.pending_writes.borrow_mut())
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
        let writes = host.take_pending_writes();
        assert_eq!(writes, vec![(0x3F11, 0x01), (0x3F12, 0x16)]);
        // A second drain with nothing new queued must come back empty, not
        // repeat the same writes forever.
        assert!(host.take_pending_writes().is_empty());
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
            for (addr, value) in host.take_pending_writes() {
                assert_eq!(addr, 0x3F11);
                colors_seen.insert(value);
            }
        }
        assert!(colors_seen.len() > 10, "expected many distinct colors over 200 frames, got {}", colors_seen.len());
    }
}
