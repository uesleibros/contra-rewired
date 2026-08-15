# Modding

A mod is a directory: `/mods/<your-mod-id>/mod.toml` plus whatever assets or
scripts it needs. Drop it in `./mods/` next to the executable and it loads
automatically - there's no enable/disable UI yet (tracked in ROADMAP.md), so
every mod with a valid `mod.toml` in that folder runs.

## Scripting: Lua, real and working

Gameplay-affecting mods use Lua via [`mlua`](https://docs.rs/mlua)
(`crates/contra-mods/src/script.rs`), and it's genuinely wired end to end:
`contra-pc` scans `./mods/`, loads each mod's entry script into its own Lua
VM, and fires `frame_tick` once per emulated frame, applying whatever PPU
writes the script queued. This is not a stub - see `mods/rgb-character/` in
this repo for a complete, working example that cycles every sprite's colors
through the full NES palette in real time, using only the API below.

**Building with Lua support.** Off by default: `mlua`'s `vendored` build
compiles Lua from C source, which needs a C toolchain (MSVC Build Tools on
Windows - specifically the "Desktop development with C++" workload; `gcc`/
`clang` elsewhere) that not every contributor has installed. With it:

```sh
cargo build -p contra-pc --release --features mods
cargo run -p contra-pc --release --features mods -- path\to\your\baserom.nes
```

On Windows, if `cl.exe` isn't already on `PATH` (i.e. you're not running
from a "Developer Command Prompt"), import the MSVC environment first, e.g.
in PowerShell:

```powershell
$vcvars = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat"
cmd /c "`"$vcvars`" x64 && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') { [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
cargo build -p contra-pc --release --features mods
```

Without `--features mods`, `contra-pc` still scans `./mods/` and logs a
warning naming any script-carrying mods it found but can't run, rather than
silently ignoring them.

### The host API

```toml
# mods/rgb-character/mod.toml
id = "rgb-character"
name = "RGB Character"
version = "1.0.0"
author = "your name"
description = "Cycles every sprite palette through the full NES color range."
entry_script = "main.lua"
```

```lua
-- mods/rgb-character/main.lua
contra.on("frame_tick", function()
    local color = contra.frame() % 0x40   -- NES palette has 64 entries
    contra.write_ppu(0x3F11, color)        -- sprite sub-palette 0, color slot 1
end)
```

- `contra.on(eventName, fn)` - registers a handler. Events fired today:
  `frame_tick` (once per emulated frame, real). `enemy_spawn`, `player_hit`,
  `stage_start`, `stage_clear` are defined (see `ModEvent` in `script.rs`)
  but not wired to any real emulator signal yet - detecting them means
  reading known RAM addresses the way Custom Difficulty tooling will
  (tracked in ROADMAP.md).
- `contra.log(msg)` - writes to `contra-pc`'s log output.
- `contra.frame()` - the current frame count, set by the host once per
  frame. Use it as a clock for time-based effects
  (`math.sin(contra.frame() / 10)` and the like) without a script needing
  its own timer.
- `contra.write_ppu(addr, value)` - queues a raw write into PPU address
  space (`$0000-$3FFF`: pattern tables/CHR-RAM, nametables, palette),
  applied by the host right after `frame_tick` finishes firing. Addressed in
  real NES PPU terms, not anything Contra-specific, so a mod author working
  from this doc and the PPU section of docs/FIDELITY.md can reason about it
  the same way they would on real hardware. `$3F00-$3F1F` (palette RAM) is
  the most immediately useful range: `$3F01-$3F03`/`$3F05-$3F07`/
  `$3F09-$3F0B`/`$3F0D-$3F0F` are the 4 background sub-palettes' 3 real
  colors each, `$3F11-$3F13`/`$3F15-$3F17`/`$3F19-$3F1B`/`$3F1D-$3F1F` are
  the 4 sprite sub-palettes'. This never touches CPU/RAM/collision state -
  only what's already-drawn pixels are colored with, so it can't desync
  gameplay, only recolor it.

Each mod gets its own Lua VM (`LuaModHost::new()`), so a misbehaving script
can't reach into another mod's globals, and a script error is caught and
logged per-mod rather than taking down the whole session.

### What's not built yet

- **Asset overrides** (`sprite_overrides`/`music_overrides`/`level_files` in
  the manifest schema) aren't consumed by anything yet. Since `contra-pc`
  runs the real ROM through `contra-nes`, the game's own code decompresses
  its graphics into CHR-RAM and generates its own audio at run time - a mod
  wanting to replace *shapes*, not just colors, needs to patch CHR-RAM
  contents post-decompress or intercept APU register writes, not swap asset
  files. `contra.write_ppu` already reaches CHR-RAM (`$0000-$1FFF`), so a
  "replace this tile's pixels" mod is possible today with the existing API;
  a friendlier sprite-sheet-file-based workflow on top of that is future
  work.
- **Typed event payloads** (which enemy, how much damage, which stage) -
  `frame_tick` is the only event with real data behind it so far.
  `enemy_spawn`/`player_hit`/etc. need RAM-address-based detection wired in.
- **Mod management UI**, load order control, and `ModRegistry::
  unmet_dependencies` enforcement at load time (the check exists and is
  tested; `contra-pc` doesn't call it yet).
- **RAM peek/poke** (CPU-side memory, not just PPU) - would unlock gameplay
  mods (Custom Difficulty pokes, cheats, trainers), not just visual ones.

## `.contramap` (level editor format)

Not started. Tracked in ROADMAP.md, Phase 3, alongside the in-game level
editor itself. When it lands, this document will cover the format so
external tools can generate/consume it too.
