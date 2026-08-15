# Roadmap

This is the full vision, organized the way the project was originally
scoped: three phases, each a coherent, shippable milestone rather than a
grab-bag. Status per item:

- `[x]` - implemented and tested in this repository today
- `[~]` - scaffolded (types/config/architecture exist) but not wired to real
  gameplay yet
- `[ ]` - planned, not started

Nothing in Phase 2 or 3 is blocked on being "designed" - the config schema,
save-state format, and replay format in `contra-core` already have fields
for most of it (see [ARCHITECTURE.md](docs/ARCHITECTURE.md)). What's missing
is the actual game - see the note at the bottom.

## Phase 1 - Fidelity, controls, and the two platforms

The goal: **Original mode is indistinguishable from a real cartridge**, and
everything else is an opt-in layered on top, never a replacement.

**NES emulation core (`contra-nes`) - the actual "play the real game" path**
- [x] 6502/2A03 CPU: all official opcodes, correct flag behavior, the
      JMP-indirect page-boundary bug, NMI/IRQ/BRK/RESET (`cpu.rs`, 21 tests
      against hand-assembled original programs - no ROM needed to verify it)
- [x] 2C02 PPU: background rendering (nametables/attributes/scrolling via
      the real `v`/`t`/fine-x loopy registers), sprite rendering (8x8/8x16,
      correct OAM-index draw priority, flip), sprite 0 hit, sprite overflow,
      palette mirroring (`ppu.rs`) - **scanline-granular, not per-dot**, see
      docs/FIDELITY.md for exactly what that does and doesn't reproduce
- [x] Validated against a real US retail ROM (title screen, stage intro,
      in-level gameplay with enemies/items all render correctly; zero
      illegal opcodes hit across ~900 frames) - see docs/FIDELITY.md. This
      run found and fixed a real sprite draw-priority bug.
- [x] Mapper 2 (UxROM): PRG bank switching, fixed last bank, CHR-RAM
      (`mapper.rs`)
- [x] Standard controller shift-register protocol, 2 ports (`controller.rs`)
- [x] Full-state save/rewind snapshots that skip the static PRG-ROM
      (`Nes::snapshot`/`restore`, `NesSnapshot`)
- [x] APU: pulse 1/2, triangle, and noise channels, frame sequencer
      (4-step/5-step), length counters, envelopes, sweep, the standard
      non-linear mixing formula, real-time playback via `cpal` in
      `contra-pc` (`apu.rs`, 27 tests) - **DMC (sample playback) not
      implemented**, registers accepted but silent
- [x] Fixed audible audio delay: the playback ring buffer was capped at 2
      full seconds, so once production briefly outran consumption the
      buffer would fill and *stay* nearly 2 seconds behind, since draining
      only removed from the front at real-time speed. Capped at 150ms
      (`apps/contra-pc/src/audio.rs`) and set to drop the *oldest* samples
      past that cap, so playback actively catches back up to low latency
      instead of accumulating a permanent backlog
- [x] Opt-in "no sprite flicker" mode: lifts the real 8-sprites-per-scanline
      hardware limit up to all 64 OAM sprites, off by default (`Original`
      mode stays hardware-accurate); the overflow status flag is still
      reported even when not enforced, so code polling it sees the same
      thing a real cartridge would (`unlimited_sprites`, tested)
- [x] Live "Extended" widescreen: render width can change every frame (not
      just on/off) to track a resizable window's aspect ratio, up to a
      safe cap. The cap was **empirically tuned against the real ROM**,
      not guessed: 380px (62px/side) renders clean at every scroll
      position tested; 420px already showed black (undrawn nametable data)
      creeping into the trailing edge; Contra's engine only pre-draws the
      direction it auto-scrolls *toward*, so the trailing edge runs out of
      valid data before the leading edge does. Front-ends should pillarbox
      rather than request more (`wide_width`, `EXTENDED_WIDTH`, tested)
- [ ] Per-dot PPU timing for mid-scanline register-change effects
- [ ] Additional mappers, if a future ROM needs one (Contra only needs UxROM)
- [ ] Undocumented/illegal 6502 opcodes (currently a recorded no-op; add
      only if some game is found to need one - Contra likely doesn't)

**Hand-ported simulation layer (`contra-core`) - RNG/physics facts, config,
save-state and input plumbing; also drives the placeholder demo when no ROM
is loaded**
- [x] Deterministic fixed-point vertical physics ported from the
      disassembly (`fixed.rs`, `physics.rs`) - see docs/FIDELITY.md
- [x] NES idle-loop RNG mechanism modeled (`rng.rs`)
- [x] Horizontal walk speed ported exactly (±1 px/frame, `WALK_SPEED`)
- [x] Jump takeoff velocity ported exactly for outdoor/indoor stages
      (`JUMP_VELOCITY_OUTDOOR`/`JUMP_VELOCITY_INDOOR`) + death-bounce
      velocity (`DEATH_BOUNCE_VELOCITY`)
- [x] "Original NES" fidelity flag exists in config
- [x] 60Hz fixed-timestep simulation, presentation decoupled from logic
- [ ] RAM-address-based tooling (Custom Difficulty pokes, Practice overlays,
      trainers) that reads/writes the *emulator's* live memory using the
      address map `ram.asm` documents - this is now the practical path to
      most of the "custom difficulty"/"practice mode" wishlist, now that
      there's a real running game to poke at instead of a hand-ported one

**Video**
- [x] Config surface for: integer scaling, 4:3 / 8:7 / native / ultrawide,
      overscan, CRT filter, scanlines, composite/ghosting sim, palette
      swaps, NTSC/PAL, widescreen borders, windowed/borderless/fullscreen
- [x] Real window + integer-scaled framebuffer presentation (`contra-pc`)
- [x] Event loop switched from `ControlFlow::Poll` (busy-spin, redraws
      hundreds of thousands of times/sec) to `ControlFlow::WaitUntil` paced
      to the actual 60Hz frame boundary - this was real, measurable wasted
      CPU/GPU work that could starve the audio thread and make everything
      feel sluggish, even though pure emulation runs at ~30x real-time (see
      `crates/contra-nes/examples/perf_test.rs`)
- [x] **"Extended" widescreen mode**, real and working: toggling it on in
      the pause menu immediately targets the full empirically-tested safe
      cap (`EXTENDED_WIDTH` = 380px), independent of the current window
      size - the scale-to-fit blit handles however that ends up displayed.
      (Fixed two rounds in a row: first, it used to derive its target width
      from the window's *current* size via a resize-event-driven
      `compute_wide_width`, so toggling it on without also resizing the
      window produced no visible change - removed entirely in favor of
      always targeting the cap. That alone still wasn't enough: with the
      window left at its narrow default, the wider content just got
      squeezed back down by the fill-scaling blit, so almost nothing
      visibly changed. `redraw()` now detects the setting change frame to
      frame and calls `window.request_inner_size` immediately, growing/
      shrinking the window's width to match the new content width at
      whatever per-pixel scale is already in effect - the way other NES PC
      ports resize when widescreen is turned on. No-op in fullscreen, where
      the compositor owns the size.) Never touches RAM/collision/spawn
      logic (presentation-only), verified by a test asserting the center
      256px exactly matches normal rendering byte-for-byte, plus a test
      covering arbitrary in-between widths (not just the max, and not just
      the framebuffer's row stride, which had its own now-fixed bug - see
      docs/FIDELITY.md)
- [x] **Widescreen extension is fixed-centered - direction bias was tried
      and reverted.** A direction-biased extension (putting most of the
      extra width on the side the camera was scrolling toward, easing
      between 10%/90% instead of a flat 50/50) was built to dodge stale
      trailing-edge tiles in some stages. It made a worse problem: since
      the "normal" 256px window's position within the wide frame moved
      with the bias, the player's on-screen position visibly drifted left/
      right as scroll direction changed - reported as "the camera moves
      more than normal." Reverted entirely (`Ppu::wide_bias_frac`,
      `frame_scroll_dir`, `step_wide_bias`, `update_frame_scroll_dir` all
      removed) back to fixed centering (`x_offset = extra / 2`, always),
      which keeps the player's screen position identical to narrow mode at
      all times. This isn't reopening the original trailing-edge concern:
      `EXTENDED_WIDTH` (380px) was already empirically tuned *with* fixed
      centering (see its docs) and found clean, so centering was the
      already-verified-safe setup the whole time
- [ ] **Enemy/bullet spawn-ahead in widescreen** - investigated, not yet
      implemented. The random soldier-generation edges
      (`soldier_generation_01` in the reference disassembly's `bank2.asm`,
      constants `#$0a`/`#$fa`) are a real, scoped patch target once
      widescreen is on. The blocker: the collision buffer the game checks
      before spawning (`BG_COLLISION_DATA`, `$0680`) is documented in
      `ram.asm` as covering only the two currently-loaded nametables -
      exactly the same window that bounds the visual extension, not a
      wider always-valid map. Spawning at the wide edge is only safe where
      that buffer is already populated (the leading-edge bias above should
      cover it in the scrolling direction); level-specific hard-coded
      screen enemies and bosses would need their own case-by-case check.
      Doing this safely means adding a real, tested PC/bank-scoped
      instruction hook to `contra-nes::Cpu` (currently a clean, general
      6502 core with no per-game hooks) - a properly scoped follow-up, not
      a same-pass addition on top of the rendering work above
- [x] **Freely resizable window with dynamic fill scaling** (not
      integer-locked): default behavior now scales fractionally to cover as
      much of the window as possible while preserving aspect ratio - drag
      the window to any size, or maximize onto any monitor shape, and the
      content fills it, the way the Switch Pokemon/Link's Awakening ports
      handle a resizable/dockable display. A "Pixel Perfect" toggle in the
      pause menu switches back to strict integer scaling (crisp NES pixels,
      possible letterbox bars) for players who prefer that instead
- [x] **Zoom** (50-300%), adjustable from the pause menu, layered on top of
      either scaling mode
- [ ] CRT/scanline/composite shaders (currently config fields with no
      renderer behind them yet)
- [ ] True ultrawide (32:9 and beyond) is bounded by `EXTENDED_WIDTH`'s
      hardware-imposed cap, not a software limitation - see docs/FIDELITY.md
      for why going further shows wrong data, not just an arbitrary block

**Controls**
- [x] Fully rebindable action system (`input.rs`), hold/toggle fire modes
- [x] Keyboard support (`contra-pc`) - fixed a bug where every keyboard
      binding silently never matched (`format!("{physical_key:?}")` on
      winit's `PhysicalKey` prints `"Code(Enter)"`, not `"Enter"`, so it
      never equaled what `Bindings` stores); regression-tested in
      `main.rs` so this class of bug can't come back quietly
- [x] Gamepad support via `gilrs` (`contra-pc`): d-pad + left stick with
      deadzone for movement, south/east face buttons for shoot/jump, Start
      for pause - works alongside keyboard, first connected pad only
- [x] Tab as an alternate pause/menu key alongside Escape/Start
- [ ] Per-controller-type button glyphs (DualSense/Switch Pro/Xbox), full
      `Bindings`-driven gamepad rebinding (today it's a fixed mapping, not
      yet routed through the `PhysicalInput::GamepadButton/Axis` bindings)
- [ ] Hotplug beyond gilrs' own detection, multi-pad P1/P2 assignment,
      turbo, vibration, input display
- [ ] **Dual-Stick Contra** mode (left stick move / right stick aim /
      trigger fire) - `Action::AimFire` and `ActionState::aim_vector`
      already exist for this

**Save states / checkpoints / difficulty**
- [x] Save slot manager: manual/quick/autosave/suspend, undo-load, rewind
      ring buffer (`savestate.rs`)
- [x] Quick save/load (F5/F9) and rewind (Backspace) wired end-to-end in
      `contra-pc` - against real full emulator snapshots when a ROM is
      loaded, or the placeholder player state otherwise
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
- [x] **Hitbox overlay, real and wired** (Settings tab / F4): outlines every
      active OAM sprite (`Nes::bus::ppu::oam`, hidden entries at Y≥0xEF
      skipped) in the same screen space the game image is drawn in,
      including wide mode's offset (`Nes::wide_x_offset`) so the boxes
      stay lined up under widescreen. This is the *visual* sprite bounding
      box (8x8 or 8x16 per `PPUCTRL` bit 5), not necessarily Contra's exact
      per-entity collision box, which the disassembly doesn't document as
      a single fixed table - a real, honest approximation rather than a
      guessed-at exact hitbox. Verified correct and complete against a
      headless real-ROM render (`dump_frames.rs`'s new `HITBOXES=1`,
      draws the same OAM loop directly into the saved PNG) after a report
      that the overlay looked incomplete in-app - that report coincided
      with the direction-bias camera-drift bug above, which is the more
      likely explanation, since the underlying OAM loop itself checks out
      frame-for-frame against the real ROM
- [x] **Frame advance and slow motion, both real.** F12 freezes stepping
      without opening the pause menu (rendering keeps happening, so you
      can see the frozen frame clearly - unlike `GameRoutine::Paused`,
      which shows the menu over it); `.` steps exactly one simulated frame
      while frozen. A Speed slider (25-200%, Settings tab) scales how much
      real time each simulated frame takes rather than the emulation
      itself - the game still runs its own unmodified logic one real
      frame at a time, just paced slower or faster. Both share one
      `step_gameplay_frame` helper with the normal per-tick path, so
      freeze/advance/slow-motion behave exactly like a normal frame in
      every way except *when* they happen

**Menu / UI - v3, real `egui`, real widgets**
- [x] **Rendering moved off `softbuffer` (raw CPU framebuffer blit) onto
      `wgpu` + `egui`**, the standard Rust answer to "I want Dear ImGui":
      immediate-mode widgets (checkboxes, sliders, scroll areas, buttons)
      with retained interaction state, instead of a hand-rolled 5x7 bitmap
      font and manual click-rect hit-testing. The NES framebuffer is
      uploaded as a GPU texture (`egui::TextureHandle`, `NEAREST` filtering
      for crisp NES pixels, no blur) and painted as the background of an
      `egui` frame each redraw (`apps/contra-pc/src/main.rs::redraw`); the
      pause menu and Load ROM screen are real `egui::Window`/
      `egui::CentralPanel` content drawn on top (`apps/contra-pc/src/
      menu.rs`). `menu.rs` shrank from ~500 lines of manual layout/hit-
      testing to widgets bound directly to `Settings` fields - a checkbox
      *is* the toggle now, no separate action-dispatch layer for anything
      that's just a plain value flip
- [x] **Memory footprint fix**: `wgpu::Instance` was created with default
      flags, which auto-enable the Vulkan validation layer under
      `debug_assertions` (i.e. every non-`--release` build) - real,
      measurable extra memory and per-draw-call overhead on top of what an
      unoptimized debug binary already costs, and not something a player
      needs. Now created with `InstanceFlags::empty()`. A `cargo build`
      debug run vs `--release` is not a fair memory comparison in general
      (unoptimized codegen, debug info, and previously the validation
      layer all add up); `cargo run -p contra-pc --release` is the number
      worth judging idle memory against - measured ~110MB resident on this
      machine post-fix, mostly GPU driver/Vulkan instance overhead
      (`wgpu`'s baseline, not something specific to this app) rather than
      anything growing unbounded
- [x] Pause menu: two tabs plus Debug - **Settings** (Widescreen, No
      Sprite Flicker, Pixel Perfect, Zoom slider, Fullscreen, Audio Mute -
      all direct `egui::Checkbox`/`egui::Slider` bindings), **Mods** (every
      discovered mod as a click-to-toggle checkbox), **Debug** (see below).
      Opens with Tab or Escape; egui owns mouse/keyboard focus while it's
      open (`egui_winit::State::on_window_event`'s `consumed` flag gates
      whether gameplay input handling sees the event at all), and reads as
      "outside the game" the same way the old bitmap-font menu did -
      that part of the design goal didn't change, just how it's built
- [x] **Debug tab, now both players**: live cheat/trainer controls backed
      by real CPU RAM pokes (`Nes::peek_ram`/`poke_ram`) - lives (+/-
      steppers) and current weapon (a real `egui::ComboBox` dropdown, not
      a `<`/`>` cycle stepper - pick the weapon directly by name) for
      **both P1 and P2** (`$32`/`$33` and `$AA`/`$AB` in the reference
      disassembly's `ram.asm` - P2 is simply P1's address plus one,
      confirmed against the real source, not guessed), plus shared
      continues (`$3A`, single counter for both players, matching the
      arcade-style continue system). Shows "No ROM loaded" instead when
      there's no real RAM to poke
- [x] **Stats overlay** (Settings tab / F7): frame count and both players'
      X/Y position, read live from `SPRITE_X_POS`/`SPRITE_Y_POS`
      (`$0334`/`$031A`, indexed 0=P1/1=P2 - the same array
      `soldier_generation_01` reads player position from). Gives the
      "frame counter"/"coordinates" entries in `contra_core::config::
      PracticeConfig` an actual renderer to draw into - boss HP and spawn
      markers from that same config struct still don't have one
- [x] Mod enable/disable UI (Mods tab, click a mod row to toggle) - session
      only for now, resets to all-enabled on next launch; not yet persisted
      to `config.toml`
- [x] **Real "Load ROM" screen**, not an engine-only physics demo: shown
      whenever there's no ROM loaded (missing/invalid/wrong-mapper), with a
      click-to-open native file picker (`rfd`, filtered to `.nes`) and
      drag-and-drop support (`WindowEvent::DroppedFile`) - both go through
      the same `try_load_rom` validation path as the CLI arg /
      `./baserom.nes`, so a ROM picked at runtime is checked exactly the
      same way. A failed load (wrong mapper, bad file) shows the reason
      inline instead of failing silently. `contra-core`'s hand-ported
      `PlayerPhysics` still exists as the save-state fallback and
      RAM-tooling reference, it's just never driven or drawn as a
      "gameplay" screen. **Fixed**: `rfd::FileDialog::pick_file()` blocks
      the whole event loop while its native dialog is open; by the time it
      returned, `redraw`'s already-captured `egui` frame (built *before*
      the dialog opened, while still showing the no-ROM screen) was stale
      - painting it after a successful load showed one frame of "no ROM"
      over a game that had already loaded. `redraw` now skips painting
      that stale frame entirely after a pick attempt and requests a fresh
      one instead, whichever session it ends up being
- [x] **Hotkeys for every toggleable Settings entry** - F1 Widescreen, F2
      No Sprite Flicker, F3 Pixel Perfect, F4 Show Hitboxes, F8 Mute Audio,
      F11 Fullscreen. Work during gameplay, not just while the menu's open
      (same as the existing F5/F9/Backspace quicksave/quickload/rewind
      keys), and each label in the Settings tab shows its hotkey inline.
      Flipping a setting via hotkey applies the same widescreen-resize/
      `set_fullscreen` side effects a menu click would (`apply_toggle_
      side_effects`, called from both places against `prev_widescreen`/
      `prev_fullscreen` state owned by the event loop, not `redraw` -
      the first version of this had a real bug where a hotkey-driven
      change was invisible to `redraw`'s own before/after diff, since by
      the time `redraw` ran the change had already happened). Not yet
      user-rebindable - same honest gap as gameplay `Bindings` remapping
- [ ] Everything else from the config surface (CRT filter, scanlines,
      palette, keybind remapping, difficulty, checkpoint mode, ...) still
      needs a menu entry; this is the pattern to extend, not a finished
      options screen
- [ ] Mod reorder UI, mod enable/disable persisted across launches
- [ ] `egui` opens the door CRT/scanline shaders were already waiting on
      (see the widescreen section above) - `egui-wgpu`'s renderer runs
      inside the same `wgpu` device/queue as the game background, so a
      post-process pass over the NES texture is now a shader away instead
      of needing its own GPU context from scratch

**Modding - Lua scripting, high-level and low-level APIs**
- [x] Low-level host API (`crates/contra-mods/src/script.rs`):
      `contra.write_ppu(addr, value)` / `contra.poke_ram(addr, value)` /
      `contra.peek_ram(addr)` / `contra.frame()` / `contra.on(...)` /
      `contra.log(...)`. RAM writes are queued and PPU writes are queued
      separately, both drained and applied to the live `Nes` once per frame
      - RAM pokes actually change game state (lives, weapon, position,
      anything else in work RAM), unlike the presentation-only PPU pokes
- [x] High-level API layered on the same primitives:
      `contra.player.set_lives/get_lives`,
      `set_weapon/get_weapon`, `set_continues/get_continues` - a mod author
      who doesn't want to know RAM addresses doesn't have to
- [x] `contra-pc --features mods` scans `./mods/`, loads every mod with an
      `entry_script` into its own Lua VM, fires `frame_tick` once per
      emulated frame, and applies queued PPU + RAM writes to the live `Nes`
      - verified against the real ROM (log-confirmed mod load, no runtime
      errors over 300+ frames), 11 tests in `script.rs`
- [x] `mods/rgb-character/` - a complete, working example mod (cycles every
      sprite palette through the NES's 64-color range every frame)
- [ ] Typed event payloads for `enemy_spawn`/`player_hit`/etc. (only
      `frame_tick` carries real data today)
- [ ] Asset-file-based overrides (`sprite_overrides`/`music_overrides` in
      the manifest schema exist but aren't consumed - see docs/MODDING.md
      for why that's a CHR-RAM-patching problem, not a file-swap one)

**Android**
- [ ] Not started. `contra-core`/`contra-assets` are already
      platform-agnostic (no windowing/rendering deps) so an
      `apps/contra-android` front-end can reuse them directly - see
      ARCHITECTURE.md.
- [ ] Touch controls (repositionable, resizable, presets), "drag the fire
      button to aim" scheme, hide-on-controller-connect, pause-on-minimize,
      save-on-kill

## Phase 2 - Online, replays, speedrun tooling, achievements

- [ ] Rollback netcode (2P online), lobby/invite/room codes/LAN/spectator
- [ ] 4-player local chaos mode
- [ ] Co-op: shared vs. individual lives, revive, drop-in/out
- [x] Input-only replay format with "Take Control" mid-playback handoff
      (`replay.rs`) - recording/playback loop not yet built
- [ ] Speedrun tools: internal timer, splits, PB/SoB, LiveSplit integration,
      Speedrun.com preset mode
- [ ] Steam/internal achievements, statistics screen, leaderboards
- [ ] Weapon Randomizer / Draft / Lock / Gun Game modes
- [ ] Daily/Weekly Challenge with a shared seed
      (`rng::ModernRng::seed_from_str` exists for this)

## Phase 3 - "We've completely lost track" - editor, mods, roguelike, more

- [x] Mod manifest format + registry/load-order/dependency checking
      (`contra-mods`)
- [x] Lua scripting host, feature-gated (`contra-mods`, `--features
      contra-mods/lua`) - minimal event API, not yet wired to gameplay
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

The emulator core (`contra-nes`) is what actually makes the game playable
today, given your own ROM - that's real, not aspirational. What's *not*
real yet: audio (APU is silent), and everything in Phase 2/3 (netcode,
speedrun tooling, a level editor, roguelike mode, and the rest of the
original wishlist). Those are still genuinely years of work for a small
team. This roadmap exists so that work is legible and resumable: every
`[ ]` here is either a config field already waiting in `contra-core`, or a
clearly-scoped task (e.g. "implement APU pulse channels", "add per-dot PPU
timing") rather than a vague aspiration.
