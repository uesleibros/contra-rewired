//! Mod support.
//!
//! A mod is a directory under `/mods/<id>/` containing a `mod.toml`
//! manifest plus any combination of: sprite/texture overrides, music
//! replacements, a `.contramap` level, and (with the `lua` feature enabled
//! at build time) a Lua entry script that hooks gameplay events.
//!
//! This crate works without Lua at all — asset-replacement-only mods (skins,
//! music packs, palettes) never need a script. The `lua` feature adds the
//! scripting host on top; it's off by default because `mlua`'s `vendored`
//! build needs a C toolchain (MSVC Build Tools on Windows, or a system
//! `cc`), which we don't assume every contributor has installed.
//! `apps/contra-pc` builds with `--features contra-mods/lua` in release
//! builds where that toolchain is expected to be present (see README).

pub mod manifest;
pub mod registry;

#[cfg(feature = "lua")]
pub mod script;

pub use manifest::ModManifest;
pub use registry::ModRegistry;
