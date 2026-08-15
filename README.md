# contra-rewired

A from-scratch Rust engine for a PC (and eventually Android) port of
*Contra* (NES, 1988) — built to the "definitive port" brief: **the original
game, unmodified by default, with every modern convenience available as an
opt-in layer instead of a replacement.**

> **Status: Phase 1 foundation.** This is a real, tested, compiling Rust
> workspace — fixed-point physics and RNG mechanism ported from the
> community disassembly, a full config/input/save-state/replay system, a
> legal ROM pipeline, and a Lua modding scaffold. It is **not** a playable
> Contra yet: no ROM decoder, no levels, no sprites. See
> [Project status](#project-status) and [ROADMAP.md](ROADMAP.md) for exactly
> what's real today versus planned.

## Design principle

> Don't touch what makes Contra *Contra*. Put everything else behind a flag.

Concretely: an `Original` mode that boots straight into the unmodified game
with no menus in the way, and every other system — widescreen, save states,
randomizers, roguelike mode, online co-op, a level editor — built as opt-in
layers that never change how `Original` plays. `contra_core::config::Config`
enforces this today: `hardcore_mode` and `original_nes_mode` are hard
overrides that force every convenience feature off, not just defaults.

## Why Rust, and why this architecture

- **Determinism you can unit test.** The simulation core
  (`crates/contra-core`) has no rendering, audio, or windowing dependencies,
  so physics, RNG, replays, and save states are tested headlessly — see
  `cargo test --workspace` (31 tests, all green, no window required).
- **One core, two platforms.** PC (`apps/contra-pc`, winit) and a future
  Android front-end share `contra-core`/`contra-assets` unchanged; only the
  windowing/input/rendering layer differs. See
  [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).
- **Modding without recompiling the engine.** Asset-replacement mods need
  nothing but a `mod.toml`; gameplay mods get a Lua host
  (`crates/contra-mods`, feature-gated). See [docs/MODDING.md](docs/MODDING.md).

## The legal model: bring your own ROM

**This repository contains zero Konami-owned assets** — no graphics, no
audio, no level data, no ROM. It's built and verified against the public,
source-only [vermiceli/nes-contra-us](https://github.com/vermiceli/nes-contra-us)
disassembly, and follows the exact same rule that project does: you supply
your own legally-dumped copy of the game (`baserom.nes`), and tooling in
this repo (`apps/contra-extract`) reads *your* file at build/run time on
*your* machine. See [docs/ASSETS.md](docs/ASSETS.md) for details and current
limitations (the graphics/audio decoder isn't written yet).

## Project status

| Area | Status |
|---|---|
| Fixed-point vertical physics (gravity, jump integration) | Ported from the disassembly, unit tested — see [docs/FIDELITY.md](docs/FIDELITY.md) |
| Horizontal walk speed, jump takeoff velocity (outdoor/indoor), death-bounce velocity | Ported exactly (raw register bytes, not approximations) — see FIDELITY.md |
| RNG mechanism | Modeled (idle-loop accumulator), not yet cycle-exact |
| Weapon recoil, hitboxes, enemy AI, spawn tables | Not ported yet — honest placeholders, see FIDELITY.md |
| Config (video/audio/input/gameplay/accessibility/practice) | Full schema implemented, round-trips through `config.toml` |
| Rebindable input, hold/toggle fire | Implemented; keyboard **and gamepad** (`gilrs`: d-pad/stick + face buttons) both wired in `contra-pc` |
| Save states (manual/quick/autosave/suspend), undo-load, rewind buffer | Implemented; quick save/load (F5/F9) and rewind (Backspace) wired end-to-end in `contra-pc` |
| Difficulty presets + full Custom Difficulty + shareable codes | Implemented, tested |
| Checkpoint modes (Original/Casual/Modern/Practice) | Implemented |
| Replay format (input log, "Take Control" handoff) | Format implemented; no recording/playback loop yet |
| ROM loading + identity check | Implemented; **asset decompression not implemented** |
| Mod manifest, registry, load order, dependency checks | Implemented, tested |
| Lua mod scripting | Working host behind `--features contra-mods/lua` (needs a C toolchain), minimal event API |
| Actual playable game (levels, enemies, bosses, sprites, audio) | **Not implemented** |

See [ROADMAP.md](ROADMAP.md) for the full three-phase plan (fidelity/PC/Android
→ online/replays/speedrun tooling → editor/mods/roguelike/everything else),
with every item tagged by what's actually done versus planned.

## Building

Requires the Rust toolchain (`rustup`, stable channel).

```sh
git clone https://github.com/uesleibros/contra-rewired.git
cd contra-rewired
cargo build --workspace
cargo test --workspace
```

Run the Phase 1 preview (opens a window, moves a placeholder block with the
ported walk/jump physics via keyboard or gamepad — **not the real game**):

```sh
cargo run -p contra-pc
```

Validate your own ROM (see [docs/ASSETS.md](docs/ASSETS.md); does not yet
extract playable assets):

```sh
cargo run -p contra-extract -- path\to\your\baserom.nes
```

Lua modding support requires a C toolchain (MSVC Build Tools on Windows,
`gcc`/`clang` elsewhere) because `mlua`'s vendored build compiles Lua from
source:

```sh
cargo build --features contra-mods/lua
```

## Controls (Phase 1 preview)

| Action | Keyboard | Gamepad |
|---|---|---|
| Move | Arrow keys | D-pad or left stick |
| Jump | X | East face button (B/Circle) |
| Shoot | Z | South face button (A/Cross) |
| Pause | Escape | Start |
| Quick save / load | F5 / F9 | — |
| Rewind | Backspace | — |

Gamepad support is via [`gilrs`](https://docs.rs/gilrs) and works
alongside keyboard input; only the first connected controller is read today
(per-player device assignment is tracked in ROADMAP.md). Rewind only does
anything if `gameplay.rewind_enabled = true` in `config.toml` (off by
default, matching `Original` fidelity).

Fully rebindable via `contra_core::input::Bindings` — an in-game rebinding
UI isn't built yet; edit `config.toml` after first run, or see
`crates/contra-core/src/input.rs`.

## Repository layout

```
crates/contra-core/     simulation: physics, RNG, config, input, save states,
                         replays, difficulty, checkpoints, state machine
crates/contra-assets/   legal ROM loading + (future) asset extraction
crates/contra-mods/     mod manifest/registry + optional Lua host
apps/contra-pc/         desktop window/input/presentation shell
apps/contra-extract/    ROM validation/extraction CLI
docs/                   architecture, fidelity notes, asset pipeline, modding
mods/                   drop community mods here (gitignored)
```

## Contributing

The most valuable thing right now is porting more of the disassembly
faithfully — horizontal movement tables, hitboxes, one enemy type, the
graphics decompressor — each as a small, cited, unit-tested PR against
[docs/FIDELITY.md](docs/FIDELITY.md)'s format: quote the exact routine,
port it operation-for-operation, add a determinism test. See
[ROADMAP.md](ROADMAP.md) for what's next in Phase 1.

## Credits

- Original game: Konami, 1988. This project is an independent, unofficial
  fan engine; it is not affiliated with or endorsed by Konami.
- Physics/RNG/ROM-structure facts verified against
  [vermiceli/nes-contra-us](https://github.com/vermiceli/nes-contra-us), an
  annotated disassembly by the credited author(s), itself built on Trax's
  original disassembly for the *Revenge of the Red Falcon* project. No code
  or prose from that repository is reproduced here.

## License

MIT — see [LICENSE](LICENSE). Applies to the original code in this
repository only; it does not grant any rights to Konami's *Contra* itself,
and does not cover any ROM, asset, or mod file you supply or install
yourself.
