# Architecture

## Workspace layout

```
contra-rewired/
├── crates/
│   ├── contra-core/     engine-agnostic simulation: fixed-point physics, RNG,
│   │                     config schema, input mapping, save states, replays,
│   │                     difficulty, checkpoints, the top-level state machine.
│   │                     No rendering, audio, windowing, or filesystem UI code.
│   │
│   ├── contra-assets/    legal ROM loading + (future) asset extraction.
│   │                     Ships zero Konami-owned bytes — see docs/ASSETS.md.
│   │
│   └── contra-mods/      mod manifest/registry + optional Lua host (`lua`
│                          feature, requires a C toolchain — see docs/MODDING.md).
│
├── apps/
│   ├── contra-pc/        desktop shell: window, framebuffer presentation,
│   │                     keyboard/gamepad input, config load/save.
│   │
│   └── contra-extract/   CLI that validates/extracts a user-supplied ROM.
│
└── docs/                 this folder.
```

An Android front-end (`apps/contra-android/`, not yet started) would depend
on the same `contra-core` and `contra-assets` crates via `cargo-ndk` / JNI,
reusing every deterministic system unchanged and only replacing the
windowing/input/rendering layer — the same split that lets `contra-pc` exist
without any Android-specific code today.

## Why the simulation core has no rendering code

Two platforms (PC, Android) and a long list of tooling (replays, rollback
netcode, TAS/practice tools, the level editor, Steam Rich Presence, a future
"HD Remake" front-end) all need to drive the *same* simulation. Keeping
`contra-core` free of `winit`/`wgpu`/Android-specific dependencies means:

- it compiles fast and can be fuzzed/tested headlessly (see the unit tests in
  every module — physics, RNG, replays, and save states are all covered
  without opening a window);
- a replay recorded on PC can, in principle, play back on Android and
  vice versa, because both link the exact same crate;
- a future dedicated server for online play can embed `contra-core` directly
  for rollback netcode without dragging in a renderer.

## Determinism boundary

Everything that must produce bit-identical results given the same inputs
lives in `contra-core`:

- [`fixed`](../crates/contra-core/src/fixed.rs) — the two-byte
  fractional/fast velocity representation the NES code itself uses, ported
  operation-for-operation (see module docs and docs/FIDELITY.md).
- [`rng`](../crates/contra-core/src/rng.rs) — the NES's actual idle-loop
  accumulator RNG (modeled, not yet cycle-exact — see docs/FIDELITY.md) plus
  a seedable RNG for every non-"Original" mode.
- [`replay`](../crates/contra-core/src/replay.rs) — records inputs, not
  video, so a run is a few KB and can be scrubbed/resumed ("Take Control").
- [`savestate`](../crates/contra-core/src/savestate.rs) — generic over the
  game's own snapshot struct; owns slot management, undo-load, and the
  rewind ring buffer, not the struct's contents.

Everything else (rendering style, CRT filters, UI scale, screen shake
intensity) is presentation and lives in `contra-pc`/`Config`, never in the
simulation.

## Config as the single source of truth

[`contra_core::config::Config`](../crates/contra-core/src/config.rs) is one
serde-friendly tree covering every option surface described in the README —
fidelity, video, audio, input, gameplay, accessibility, practice — even
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
