# Architecture

## Workspace layout

```
contra-rewired/
├── crates/
│   ├── contra-nes/       from-scratch NES emulation core: 6502/2A03 CPU, 2C02
│   │                     PPU, mapper 2 (UxROM), real APU (pulse/triangle/
│   │                     noise, DMC not yet implemented), controller. Runs
│   │                     the user's own ROM directly - see docs/FIDELITY.md.
│   │                     No game-specific code; doesn't know it's Contra.
│   │
│   ├── contra-core/      hand-ported simulation: fixed-point physics, RNG,
│   │                     config schema, input mapping, save states, replays,
│   │                     difficulty, checkpoints, the top-level state machine.
│   │                     No rendering, audio, windowing, or filesystem UI code.
│   │
│   ├── contra-native/    real decompilation-based port: routines and asset
│   │                     formats translated instruction-for-instruction from
│   │                     a verified byte-matching disassembly, each checked
│   │                     against real gameplay captured through `contra-nes`
│   │                     before being trusted. Two tracks at very different
│   │                     stages - asset extraction (graphics, palettes,
│   │                     levels, enemy spawns, audio bytecode + playback
│   │                     engine) is substantially along; CPU game logic
│   │                     (collision, physics, AI, ...) has 2 routines done
│   │                     out of realistically hundreds. Not yet wired into
│   │                     `contra-pc` at runtime - see docs/NATIVE_PORT.md.
│   │
│   ├── contra-assets/    legal ROM loading/validation.
│   │                     Ships zero Konami-owned bytes - see docs/ASSETS.md.
│   │
│   └── contra-mods/      mod manifest/registry + optional Lua host (`lua`
│                          feature, requires a C toolchain - see docs/MODDING.md).
│
├── apps/
│   ├── contra-pc/        desktop shell: loads a ROM into `contra-nes` (or
│   │                     falls back to the `contra-core` placeholder demo),
│   │                     window, framebuffer presentation, keyboard/gamepad
│   │                     input, config load/save, save states.
│   │
│   └── contra-extract/   CLI that validates a user-supplied ROM.
│
└── docs/                 this folder.
```

## Two different kinds of "fidelity", and why both crates exist

`contra-nes` and `contra-core` take opposite approaches to the same goal
(faithful Contra behavior), and both are useful:

- **`contra-nes`** runs the *actual game code*. Once a ROM is loaded, every
  system Konami wrote - physics, hitboxes, enemy AI, RNG, the works - comes
  along automatically, because it's their 6502 code executing on an emulated
  6502. This is strictly better fidelity than hand-porting, and it's what
  `contra-pc` uses whenever a ROM is available.
- **`contra-core`** is a hand-written reimplementation of specific pieces
  (currently: vertical physics, walk/jump velocity, the RNG mechanism),
  verified against the disassembly rather than derived from running it. It
  exists for three reasons: it drives the placeholder demo when no ROM is
  loaded, it documents *why* the game behaves the way it does (comments cite
  exact disassembly routines - useful even once `contra-nes` is running the
  real thing), and it's the natural home for a future "Custom Difficulty"
  system that pokes the *emulator's* live RAM using the same address map,
  since a memory poke needs to know what address to poke.

An Android front-end (`apps/contra-android/`, not yet started) would depend
on the same `contra-core` and `contra-assets` crates via `cargo-ndk` / JNI,
reusing every deterministic system unchanged and only replacing the
windowing/input/rendering layer - the same split that lets `contra-pc` exist
without any Android-specific code today.

## Why the simulation core has no rendering code

Two platforms (PC, Android) and a long list of tooling (replays, rollback
netcode, TAS/practice tools, the level editor, Steam Rich Presence, a future
"HD Remake" front-end) all need to drive the *same* simulation. Keeping
`contra-core` free of `winit`/`wgpu`/Android-specific dependencies means:

- it compiles fast and can be fuzzed/tested headlessly (see the unit tests in
  every module - physics, RNG, replays, and save states are all covered
  without opening a window);
- a replay recorded on PC can, in principle, play back on Android and
  vice versa, because both link the exact same crate;
- a future dedicated server for online play can embed `contra-core` directly
  for rollback netcode without dragging in a renderer.

## Determinism boundary

Everything that must produce bit-identical results given the same inputs
lives in `contra-core`:

- [`fixed`](../crates/contra-core/src/fixed.rs) - the two-byte
  fractional/fast velocity representation the NES code itself uses, ported
  operation-for-operation (see module docs and docs/FIDELITY.md).
- [`rng`](../crates/contra-core/src/rng.rs) - the NES's actual idle-loop
  accumulator RNG (modeled, not yet cycle-exact - see docs/FIDELITY.md) plus
  a seedable RNG for every non-"Original" mode.
- [`replay`](../crates/contra-core/src/replay.rs) - records inputs, not
  video, so a run is a few KB and can be scrubbed/resumed ("Take Control").
- [`savestate`](../crates/contra-core/src/savestate.rs) - generic over the
  game's own snapshot struct; owns slot management, undo-load, and the
  rewind ring buffer, not the struct's contents.

Everything else (rendering style, CRT filters, UI scale, screen shake
intensity) is presentation and lives in `contra-pc`/`Config`, never in the
simulation.

## Config as the single source of truth

[`contra_core::config::Config`](../crates/contra-core/src/config.rs) is one
serde-friendly tree covering every option surface described in the README -
fidelity, video, audio, input, gameplay, accessibility, practice - even
where the underlying system is still a stub. This means the `config.toml`
format is stable from day one, and a settings UI (not yet built) just needs
to render whichever `Config` fields the current platform supports, rather
than the format growing piecemeal as features land.

## Game routine state machine

[`state_machine::GameRoutine`](../crates/contra-core/src/state_machine.rs)
plays the same role `GAME_ROUTINE_INDEX` plays in the original code (see
`ram.asm` in the reference disassembly): one enum, one `transition(event)`
function, every legal state change visible in one place instead of scattered
`if`s. Illegal transitions are a no-op rather than a panic, since a
pause-button press and a stage-clear event can legitimately race across a
frame boundary.
