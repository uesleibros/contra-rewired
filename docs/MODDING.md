# Modding

A mod is a directory: `/mods/<your-mod-id>/mod.toml` plus whatever assets or
scripts it needs.

```toml
# mods/hd-sprites/mod.toml
id = "hd-sprites"
name = "HD Sprites"
version = "1.0.0"
author = "your name"
description = "Higher-resolution player/enemy sprites."

sprite_overrides = ["sprites/"]
music_overrides = []
level_files = []
requires = []
```

Asset-only mods (skins, palettes, music packs) need nothing else - no Lua,
no C toolchain, no recompiling the engine. `contra_mods::ModRegistry::scan`
finds every `mods/*/mod.toml`, and the (not-yet-built, see ROADMAP.md) in-game
Mods menu lets you enable/reorder/disable them:

```
Mods
☑ HD Sprites
☑ Random Enemies
☐ Cursed Contra
```

Load order matters for overrides: mods enabled later in the list win
conflicts. `ModRegistry::unmet_dependencies` checks each enabled mod's
`requires` list so a script that depends on another mod's data fails loudly
instead of silently misbehaving.

**A note on how sprite/music overrides will actually work.** Since
`contra-pc` runs the real ROM through `contra-nes` (see docs/ASSETS.md), the
game's own code decompresses its graphics into the emulated PPU's CHR-RAM
and generates its own audio at run time - there are no loose sprite/music
files to replace. A `sprite_overrides`/`music_overrides` mod will need to
hook the emulator after that point: patching CHR-RAM contents post-decompress
(for sprites) or intercepting APU register writes (for music/SFX), not
swapping asset files. That hook doesn't exist yet - tracked in ROADMAP.md.
`level_files` (`.contramap`, below) are a different, more tractable story
since they'd be consumed by original engine code, not the emulated ROM.

## Scripting: Lua, behind a feature flag

Gameplay-affecting mods (new enemy patterns, weapon rules, event hooks) use
Lua via [`mlua`](https://docs.rs/mlua) (`crates/contra-mods/src/script.rs`).
This is **off by default** - `mlua`'s `vendored` build compiles Lua from C
source, which needs a C toolchain (MSVC Build Tools on Windows, `gcc`/`clang`
elsewhere) that not every contributor has installed. Build with it enabled:

```
cargo build --features contra-mods/lua
```

Minimal working example (this exact shape is covered by
`crates/contra-mods/src/script.rs`'s tests):

```lua
contra.on("stage_start", function()
    contra.log("stage started from lua")
end)

contra.on("enemy_spawn", function()
    -- not yet passed typed event data - see below
end)
```

Host API today: `contra.on(eventName, fn)`, `contra.log(msg)`. Events fired
today: `enemy_spawn`, `player_hit`, `stage_start`, `stage_clear`,
`frame_tick` (see `ModEvent` in `script.rs`) - none of them are wired to
`contra-nes` yet, even though a real emulator now runs behind `contra-pc`
(see docs/ASSETS.md). Wiring them means detecting these events from emulator
state (e.g. a `frame_tick` firing once per `Nes::run_frame`, gameplay events
inferred from known RAM addresses the same way Custom Difficulty tooling
will) rather than from a hand-ported simulation. Expanding this into a real
gameplay-hook API (typed event payloads, reading/writing emulator RAM,
registering new weapons or enemy behaviors) is tracked in ROADMAP.md,
Phase 3.

## `.contramap` (level editor format)

Not started. Tracked in ROADMAP.md, Phase 3, alongside the in-game level
editor itself. When it lands, this document will cover the format so
external tools can generate/consume it too.
