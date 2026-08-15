//! `mlua`-backed script host. Only compiled with `--features lua`.
//!
//! The API surface is intentionally small right now: enough to prove the
//! embedding works end-to-end (load script, call into it, it calls back
//! into host functions) rather than the full gameplay-hook surface a real
//! mod would want. Expanding `HostApi` is tracked in ROADMAP.md (Phase 3,
//! "Mod Support").

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
}

impl LuaModHost {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();
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

        lua.globals().set("contra", contra_table)?;
        Ok(Self { lua })
    }

    pub fn load_script(&self, source: &str, chunk_name: &str) -> Result<(), ScriptError> {
        self.lua.load(source).set_name(chunk_name).exec()?;
        Ok(())
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

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
        // mlua closures can't easily capture Rust state across the FFI
        // boundary in this minimal host, so we observe side effects via a
        // Lua-side counter table instead of a Rust Rc<RefCell<_>>.
        let _ = Rc::new(RefCell::new(0)); // sanity: Rc/RefCell available for future host state
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
}
