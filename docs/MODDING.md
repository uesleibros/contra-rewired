# Modding

A mod is a directory: `/mods/<your-mod-id>/mod.toml` plus whatever assets or
scripts it needs. Drop it in `./mods/` next to the executable and it's
picked up - but *disabled* by default: a mod is opt-in, so dropping a
`.lua` file into `./mods/` never silently starts running code the player
hasn't agreed to. Toggle individual mods on/off, and reorder them (top runs
first - matters if two mods touch the same thing), from the pause menu's
Mods tab; both which mods are enabled and their order persist to
`config.toml` (`contra_core::config::ModsConfig`), so it's a one-time
choice per install, not per launch.

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
  - `frame_tick(fn())` - once per emulated frame, no payload.
  - `stage_start(fn(stage))` / `stage_clear(fn(stage))` - `stage` is the
    0-based level index, fired together whenever `contra-pc` observes
    `CURRENT_LEVEL` change between frames (a real level completion, or the
    Debug tab's stage-select jump - both look the same from here).
  - `player_hit(fn({player, lives_remaining}))` - `player` is `0`/`1` for
    P1/P2. Fires when that player's lives count drops between frames - the
    closest honest signal available without the reference disassembly
    exposing an exact "just got hit" flag, so this is really "just lost a
    life" (won't fire for a hit survived on temporary invincibility, if
    Contra even has that).
  - `enemy_spawn(fn({slot, enemy_type, x, y, hp}))` - `slot` is `0-15`, the
    enemy-slot index every `contra.enemy.*` getter below also uses. Unlike
    the other three events, this one isn't a RAM-diff guess: it fires from
    a real CPU instruction hook on `initialize_enemy` (`bank7.asm`'s
    single, universal enemy-slot-init routine - every enemy type funnels
    through it, from the random soldier generator to a level's scripted
    placements to bosses), found by searching the ROM's raw bytes for that
    routine's known opening instructions and converting the match to a CPU
    address (`apps/contra-pc`'s `INITIALIZE_ENEMY_PC`, `$ee47` - always in
    UxROM's fixed bank, so no bank-scoping needed to hook it). This is the
    first real use of `contra_nes::Nes::run_frame_with_hook`, the general
    "bank-and-PC-scoped instruction hook" infrastructure tracked in
    ROADMAP.md as a prerequisite for widescreen-aware enemy behavior too
    (see docs/FIDELITY.md's "Enemies/bullets/collision" entry for why that
    part is a much bigger, still-open problem than firing an event).
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
- `contra.draw_text(x, y, text[, {r=, g=, b=}])` - draws `text` at
  screen-space position `(x, y)` (same NES-pixel coordinate space as the
  game image itself - `(0, 0)` is the top-left of the 256x240 playfield,
  regardless of window size/zoom/widescreen). This is the host's own UI
  font (`egui`'s, in `contra-pc`), *not* the NES's - full Unicode, any
  position, and it can't fail to render because a tile it needed was
  already something else. Not built on `write_ppu`: writing real text into
  the PPU would mean either patching CHR-RAM with a custom font
  (destructive - overwrites real game tiles) or learning Contra's own
  font's tile-index mapping and fighting the nametable for space the game
  is already using. Purely presentational, cleared and requeued every
  frame (so redraw it every `frame_tick` if you want it to persist -
  nothing lingers on screen from a frame you didn't draw it in). `color`
  is optional, defaults to a warm off-white if omitted entirely, and any
  channel left out of the table defaults the same way (`{g = 255}` gives
  you `(255, 255, 128)`, not black-and-green):
  ```lua
  contra.on("frame_tick", function()
      contra.draw_text(4, 4, "mod loaded", {r = 128, g = 255, b = 128})
  end)
  ```
- `contra.draw_rect(x, y, w, h[, {r=, g=, b=}, filled])` - same coordinate
  system and "cleared and requeued every frame" behavior as `draw_text`,
  for a rectangle instead. `filled` defaults to `false` (outline only,
  matching the built-in hitbox overlay's look):
  ```lua
  contra.on("frame_tick", function()
      -- outline every enemy slot that's actually populated
      for slot = 0, 15 do
          if contra.enemy.get_hp(slot) > 0 then
              contra.draw_rect(contra.enemy.get_x(slot), contra.enemy.get_y(slot), 16, 16, {r = 255, g = 0, b = 0})
          end
      end
  end)
  ```
- `contra.poke_ram(addr, value)` / `contra.peek_ram(addr)` - **low-level,
  gameplay-affecting.** `addr` is a CPU work-RAM offset (`$0000-$07FF`, the
  NES's 2KB of real RAM), the same address space the reference disassembly's
  `ram.asm` documents. Unlike `write_ppu`, this *does* change real game
  state - it's the same RAM the running game itself reads and writes, so a
  poke here is indistinguishable from the game doing it. `peek_ram` reads
  the RAM snapshot taken at the start of the current frame (before this
  frame's queued pokes are applied), so reading back a value you just poked
  in the same tick won't yet reflect it - queue the write, then read it back
  next frame.
- `contra.player` - a high-level convenience layer built entirely on
  `poke_ram`/`peek_ram`, for mod authors who'd rather not memorize RAM
  addresses:
  - `contra.player.get_lives(idx)` / `.set_lives(idx, n)` - `idx` is 0 for
    P1, 1 for P2
  - `contra.player.get_weapon(idx)` / `.set_weapon(idx, id)` - `id` is
    0=Standard, 1=Machine Gun, 2=Fire, 3=Spread, 4=Laser (matches the
    Debug tab's weapon list and `bank6.asm`'s weapon IDs)
  - `contra.player.get_continues()` / `.set_continues(n)` - no `idx`, this
    one's a single counter shared between both players (matches the
    arcade-style continue system - see `ram.asm`'s `NUM_CONTINUES`)
- `contra.enemy` - same idea as `contra.player`, for the 16-slot enemy
  array (`slot` is `0-15` throughout, same index `enemy_spawn`'s payload
  uses). **Read-only** - no `set_*`: an enemy slot's fields only mean
  something together and change every frame under the game's own logic, so
  poking one in isolation is far more likely to desync or crash that
  enemy's state machine than do what a mod author actually wants.
  `contra.poke_ram` is still there directly if you really need it.
  - `contra.enemy.get_type(slot)` - the enemy type code (matches
    `enemy_spawn`'s `enemy_type`, and `ENEMY_TYPE` in `ram.asm`)
  - `contra.enemy.get_x(slot)` / `.get_y(slot)` - screen-space position,
    same coordinate space `draw_text`/`draw_rect` use
  - `contra.enemy.get_hp(slot)` - `0` for an inactive/unused slot, so
    `contra.enemy.get_hp(slot) > 0` is the usual "is anything here" check

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
- **Mod load order control** and `ModRegistry::unmet_dependencies`
  enforcement at load time (the check exists and is tested; `contra-pc`
  doesn't call it yet). Enable/disable is built (see above); reordering
  isn't.

## `.contramap` (level editor format)

Not started. Tracked in ROADMAP.md, Phase 3, alongside the in-game level
editor itself. When it lands, this document will cover the format so
external tools can generate/consume it too.
