# Roadmap

This is the full vision, organized the way the project was originally
scoped: three phases, each a coherent, shippable milestone rather than a
grab-bag. Status per item:

- `[x]` — implemented and tested in this repository today
- `[~]` — scaffolded (types/config/architecture exist) but not wired to real
  gameplay yet
- `[ ]` — planned, not started

Nothing in Phase 2 or 3 is blocked on being "designed" — the config schema,
save-state format, and replay format in `contra-core` already have fields
for most of it (see [ARCHITECTURE.md](docs/ARCHITECTURE.md)). What's missing
is the actual game — see the note at the bottom.

## Phase 1 — Fidelity, controls, and the two platforms

The goal: **Original mode is indistinguishable from a real cartridge**, and
everything else is an opt-in layered on top, never a replacement.

**Engine core**
- [x] Deterministic fixed-point vertical physics ported from the
      disassembly (`fixed.rs`, `physics.rs`) — see docs/FIDELITY.md
- [x] NES idle-loop RNG mechanism modeled (`rng.rs`) — not yet cycle-exact
- [x] Horizontal walk speed ported exactly (±1 px/frame, `WALK_SPEED`)
- [x] Jump takeoff velocity ported exactly for outdoor/indoor stages
      (`JUMP_VELOCITY_OUTDOOR`/`JUMP_VELOCITY_INDOOR`) + death-bounce
      velocity (`DEATH_BOUNCE_VELOCITY`, not yet wired to a death state)
- [ ] Weapon recoil, water-state movement modifiers, knockback
- [ ] Hitboxes ported from the disassembly's collision routines
- [ ] Enemy patterns/spawn tables ported (`Enemy Routines.md`, `Enemy
      Glossary.md` in the reference disassembly)
- [ ] All 8 stages + bosses, real levels, real sprites/audio (via the asset
      pipeline below)
- [x] "Original NES" fidelity flag exists in config; not yet feature-complete
- [ ] Bit-exact original RNG (cycle-accurate idle-tick simulation, or a
      static recompilation — see docs/FIDELITY.md)
- [ ] Optional slowdown emulation
- [x] 60Hz fixed-timestep simulation, presentation decoupled from logic

**Video**
- [x] Config surface for: integer scaling, 4:3 / 8:7 / native / ultrawide,
      overscan, CRT filter, scanlines, composite/ghosting sim, palette
      swaps, NTSC/PAL, widescreen borders, windowed/borderless/fullscreen
- [x] Real window + integer-scaled framebuffer presentation (`contra-pc`)
- [ ] CRT/scanline/composite shaders (currently config fields with no
      renderer behind them yet)
- [ ] "Extended" widescreen mode with camera/spawn-safe extra world space

**Controls**
- [x] Fully rebindable action system (`input.rs`), hold/toggle fire modes
- [x] Keyboard support (`contra-pc`)
- [x] Gamepad support via `gilrs` (`contra-pc`): d-pad + left stick with
      deadzone for movement, south/east face buttons for shoot/jump, Start
      for pause — works alongside keyboard, first connected pad only
- [ ] Per-controller-type button glyphs (DualSense/Switch Pro/Xbox), full
      `Bindings`-driven gamepad rebinding (today it's a fixed mapping, not
      yet routed through the `PhysicalInput::GamepadButton/Axis` bindings)
- [ ] Hotplug beyond gilrs' own detection, multi-pad P1/P2 assignment,
      turbo, vibration, input display
- [ ] **Dual-Stick Contra** mode (left stick move / right stick aim /
      trigger fire) — `Action::AimFire` and `ActionState::aim_vector`
      already exist for this

**Save states / checkpoints / difficulty**
- [x] Save slot manager: manual/quick/autosave/suspend, undo-load, rewind
      ring buffer (`savestate.rs`)
- [x] Quick save/load (F5/F9) and rewind (Backspace) wired end-to-end in
      `contra-pc` against the placeholder player state — real once there's
      real gameplay state to snapshot
- [x] Checkpoint modes: Original / Casual / Modern / Practice
      (`checkpoint.rs`)
- [x] Difficulty presets + full Custom Difficulty slider set with a
      shareable text code, e.g. `CONTRA-NOCONTINUE-BOSSHP400-EDEN200`
      (`difficulty.rs`, round-trip tested)
- [x] Hardcore mode as a hard override (forces save states/rewind off)
- [ ] PC <-> Android save sync

**Practice tooling**
- [x] Config surface (hitbox/spawn-marker/frame-counter/coordinates/boss-HP
      overlays, fixed RNG seed)
- [ ] Actually wired to a renderer/simulation once one exists
- [ ] Frame advance / slow-motion (25–200%)

**Android**
- [ ] Not started. `contra-core`/`contra-assets` are already
      platform-agnostic (no windowing/rendering deps) so an
      `apps/contra-android` front-end can reuse them directly — see
      ARCHITECTURE.md.
- [ ] Touch controls (repositionable, resizable, presets), "drag the fire
      button to aim" scheme, hide-on-controller-connect, pause-on-minimize,
      save-on-kill

## Phase 2 — Online, replays, speedrun tooling, achievements

- [ ] Rollback netcode (2P online), lobby/invite/room codes/LAN/spectator
- [ ] 4-player local chaos mode
- [ ] Co-op: shared vs. individual lives, revive, drop-in/out
- [x] Input-only replay format with "Take Control" mid-playback handoff
      (`replay.rs`) — recording/playback loop not yet built
- [ ] Speedrun tools: internal timer, splits, PB/SoB, LiveSplit integration,
      Speedrun.com preset mode
- [ ] Steam/internal achievements, statistics screen, leaderboards
- [ ] Weapon Randomizer / Draft / Lock / Gun Game modes
- [ ] Daily/Weekly Challenge with a shared seed
      (`rng::ModernRng::seed_from_str` exists for this)

## Phase 3 — "We've completely lost track" — editor, mods, roguelike, more

- [x] Mod manifest format + registry/load-order/dependency checking
      (`contra-mods`)
- [x] Lua scripting host, feature-gated (`contra-mods`, `--features
      contra-mods/lua`) — minimal event API, not yet wired to gameplay
- [ ] Full gameplay-hook API for Lua mods (typed events, entity access, new
      weapons/enemies)
- [ ] Level editor + `.contramap` format + campaign editor
- [ ] Full Randomizer (enemies/bosses/order/backgrounds/music/palettes)
- [ ] Roguelike mode (room-to-room upgrades, permadeath, leaderboards)
- [ ] Boss Rush, Horde/Survival, New Game+ loops
- [ ] Challenges (One Bullet, Pacifist, Glass Cannon, ...) and freely
      combinable Mutators (Mirror Mode, Giant Enemies, Low Gravity, ...)
- [ ] Museum (gallery/music/bestiary/regional-version comparison), sprite
      viewer, in-game guide/bestiary
- [ ] TAS mode (rerecord, input editor, branching)
- [ ] Photo mode

## A note on scope

Everything above the fixed-point physics and RNG mechanism in Phase 1 is
**design and plumbing**, not a finished game. Reproducing all 8 stages,
every enemy, every boss, and every weapon byte-for-byte from the disassembly
— then building rollback netcode, a level editor, and a roguelike mode on
top — is realistically years of work for a small team, not a single
session. This roadmap exists so that work is legible and resumable: every
`[ ]` here is either a config field already waiting in `contra-core`, or a
clearly-scoped porting task citing the exact disassembly routine to port
next (see docs/FIDELITY.md).
