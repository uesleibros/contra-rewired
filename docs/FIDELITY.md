# Fidelity notes

"Same physics, same RNG, same hitboxes" is a specific, checkable claim, not a
vibe. This document covers two different things, because this project takes
two different approaches to fidelity (see docs/ARCHITECTURE.md, "Two
different kinds of fidelity"):

1. **`contra-nes`** - the emulation core that runs the real ROM when one is
   loaded. Its fidelity question is "how accurately does this emulate NES
   *hardware*", not "does this match Contra specifically" - since it's
   running Contra's own code, physics/RNG/hitboxes/quirks all come along for
   free, correctly, as a consequence of correct hardware emulation. See
   below for exactly what is and isn't accurate yet.
2. **`contra-core`** - the hand-ported layer used for the placeholder demo
   when no ROM is loaded. This is where "verified against the disassembly,
   routine by routine" applies, and where the rest of this document's
   original content (below) still lives.

## `contra-nes`: emulation accuracy

### What's accurate

- **CPU**: every official 6502 opcode, correct N/V/Z/C flag behavior for
  arithmetic/shifts/compares, the well-known JMP-`($xxFF)` page-boundary
  hardware bug (reproduced deliberately, not fixed), NMI/IRQ/BRK/RESET.
  Verified by unit tests using small original hand-assembled programs - see
  `crates/contra-nes/src/cpu.rs`.
- **PPU**: background rendering through the real scroll registers (`v`/`t`/
  fine-x, the same "loopy" registers real hardware uses - see NESdev's "PPU
  scrolling" article), sprite rendering (8x8/8x16, priority, H/V flip),
  sprite 0 hit, sprite overflow, correct nametable mirroring, correct
  palette mirroring (`$3F10`/`14`/`18`/`1C` → `$3F00`/`04`/`08`/`0C`).
- **Mapper 2 (UxROM)**: PRG bank switching via any `$8000-$FFFF` write, last
  bank fixed at `$C000`, CHR-RAM.
- **Timing model**: CPU cycles are budgeted per scanline (341 PPU dots / 3),
  NMI fires once per frame at the correct scanline, OAM DMA stalls the CPU
  for 513 cycles.

### What's a known, deliberate simplification

- **PPU is scanline-granular, not per-dot.** Each visible scanline is
  rendered once, in full, using whatever `v`/`t`/mask/ctrl state is current
  *at the start of that scanline's CPU cycle budget* - not re-evaluated
  dot-by-dot. This correctly reproduces the common "split scroll" trick (a
  status bar HUD achieved by changing scroll once per frame at a fixed
  scanline) because `v`'s Y-increment and horizontal-bits-copy are applied
  at the real dot-256/dot-257 boundaries between scanlines. It does **not**
  reproduce effects that change PPU registers *in the middle* of a
  scanline's 256 pixels (rare even among NES games that use raster tricks).
  If Contra turns out to rely on one, this is the first place to look.
- **CPU cycle counts are the standard base-cost table**, including
  page-cross and branch-taken extra cycles, but not every hardware edge
  case (e.g. the dummy read some read-modify-write instructions perform on
  real silicon). This affects cycle-counting accuracy in rare corner cases,
  not correctness of the visible result.
- **Only official 6502 opcodes are implemented.** An undocumented opcode is
  treated as a recorded 2-cycle no-op rather than crashing (see
  `Cpu::illegal_opcode_hit`). Commercial NES games using illegal opcodes are
  rare; if Contra is found to use one, implementing it properly is a small,
  well-scoped addition.
- **No audio.** The APU (`apu.rs`) is a stub: it accepts every register
  write (so game code never stalls waiting on it) and reports "nothing
  pending" on every read, but performs no synthesis. This is the single
  biggest remaining gap between "runs" and "is what playing this on a real
  NES feels like" - see ROADMAP.md.
- **Only mapper 2 (UxROM).** Enough for Contra; a ROM using any other mapper
  is rejected (`contra-pc` falls back to the placeholder demo).

The unit tests in `cpu.rs`/`ppu.rs`/`nes.rs` are verified by (a) original,
hand-written 6502 programs exercising specific documented CPU/PPU behaviors,
and (b) conformance to the publicly documented NES hardware behavior on
[nesdev.org](https://www.nesdev.org) - none of them need a copy of Contra.

### Validated against the real retail ROM

The core has also been run against a legally-obtained US retail Contra ROM
(MD5 `7bdad8b4a7a56a634c9649d20bd3011b`, matching the hash documented in the
reference disassembly) using `crates/contra-nes/examples/dump_frames.rs`, a
debug tool that runs the ROM headlessly and dumps PNG snapshots. Results:

- The Konami logo, "CONTRA" title logo, "PLAY SELECT" menu, and copyright
  text render pixel-correct.
- The title screen's actual input state machine works as designed: Start
  must be pressed once (during the scroll-in intro) to reach the "PLAY
  SELECT" menu and again (once the menu is showing) to really start a game
  - a single press just fast-forwards to the menu, exactly matching
    `dec_theme_delay_check_user_input` in the reference disassembly's
    `bank7.asm`. Getting this wrong in a test script looks exactly like a
    "Start doesn't work" bug; it isn't one.
- The Stage 1 "JUNGLE" intro card, in-level terrain (water, rock, grass,
  mountain background), the player character, enemy soldiers, and item
  capsules all render correctly during real (non-demo) gameplay driven by
  synthetic held-right/jump/shoot input.
- Across ~900 frames (15 seconds) of real ROM execution spanning boot,
  title, stage-intro, and gameplay, zero undocumented ("illegal") 6502
  opcodes were hit - Contra's code path exercised so far uses only official
  opcodes, consistent with `cpu.rs` only implementing those.

This run is also what found the one confirmed real bug so far: sprite
draw order didn't respect OAM priority (see below).

### Bug found and fixed this way: sprite draw priority

`render_sprites_line` iterated OAM index 0..64 and wrote each opaque sprite
pixel to the framebuffer unconditionally. On real hardware, sprite 0 has the
*highest* display priority and sprite 63 the lowest, so when two sprites
overlap, the lower index must win. Writing in ascending index order without
tracking what a higher-priority sprite already claimed meant a *later*
(lower-priority) sprite could silently paint over an earlier, higher-priority
one wherever their pixels overlapped - backwards from hardware. This is
exactly the kind of bug that reads as "flicker" or "wrong sprite part on
top" on any multi-sprite character or overlapping-sprite effect, since which
sprite "wins" would depend on OAM ordering that shifts frame to frame as
animation advances. Fixed by tracking claimed pixels per scanline so a
higher-priority sprite's pixel can never be overwritten by a
lower-priority one; regression-tested in `ppu.rs`
(`lower_oam_index_sprite_wins_overlap_priority`).

### Widescreen ("Extended") mode: what it can and can't do

Widescreen is presentation-only by design: it never touches CPU RAM, PPU
registers, or anything else the game logic reads back, so turning it on or
off can never change gameplay - only how much of the same game state is
drawn. That guarantee shaped how true ultrawide got built.

- **Why the extra width used to be capped at `EXTENDED_WIDTH` (380px, i.e.
  62px per side beyond the real 256px) - and why that's now a *radius*, not
  a ceiling.** The NES only has two physical nametables in hardware.
  Contra's own engine only pre-draws the nametable columns in the direction
  it's currently auto-scrolling toward - the trailing edge (behind the
  camera) is never kept populated, because the real console never needed it
  to be. Reading VRAM live further than the game has actually drawn reveals
  that undrawn data as visible garbage. 380px was found empirically, by
  rendering real ROM frames with `dump_frames.rs` at several widths (350 /
  380 / 420 / 480px) and visually inspecting the trailing edge in each -
  380px is clean, 420px is not. That's still true and still enforced
  (`SAFE_LIVE_MARGIN` in `render_background_line`) - but it's no longer the
  cap on `wide_width`. `Ppu::tile_cache` remembers every tile/palette this
  level has *actually displayed*, keyed by an absolute (never-wrapping)
  tile position, as a side effect of the normal live render. Columns beyond
  the safe live radius look the cached value up instead of reading VRAM
  directly; a column the level genuinely hasn't shown yet renders as
  backdrop, never a guess. Since the cache can only ever hold real,
  previously-displayed data, this can't introduce wrong data - it can only
  turn "never shown, so blank" into "shown once, so remembered forever this
  level." `wide_width` can now go up to `MAX_WIDE_WIDTH` (1024px) instead
  of 380px, and `contra-pc` tracks the window's live aspect ratio to fill
  actual ultrawide monitors (see ROADMAP.md). Verified against the real ROM
  at 700px and 900px: already-explored terrain renders continuously across
  the full width; the CPU's final state is bit-identical across 380px/700px/
  900px runs of the same input script, confirming this is still entirely
  presentation-only.
  - **Practical effect**: the *trailing* direction (places the camera has
    already scrolled past) can extend arbitrarily far, since the game has
    necessarily drawn everything back there already. The *leading*
    direction (ahead of the camera) is still bounded by the small live
    pre-buffer margin plus whatever the level has shown before (e.g. by
    scrolling backward and forward earlier) - there's no way to show
    correct data for ground the game hasn't drawn yet without either
    guessing (rejected) or reading its level data independently of what
    it's chosen to draw so far (a much larger undertaking - see below).
  - **What still resets the cache**: an unusually large single-frame scroll
    delta (much bigger than any real scrolling speed) - a checkpoint,
    respawn, or level transition - since the absolute coordinate space it's
    keyed against no longer means anything consistent once that happens.
- **Enemies/bullets/collision still only activate at the same moment they
  would on real hardware - investigated further, not yet changed.** This is
  a different limitation than the tile one above, and the tile-cache fix
  doesn't touch it: enemies are real entities the original code spawns
  based on the player's *actual* 256px-wide camera position, exactly like
  on real hardware. Digging into *why* this can't just reuse the same cache
  trick: the collision buffer the game itself checks before placing a
  hard-coded or randomly-generated enemy (`BG_COLLISION_DATA`, `$0680` in
  the reference disassembly's `ram.asm`) is documented there as covering
  only the two currently-loaded nametables, and isn't something `contra-nes`
  can passively observe and remember the way tile data can be (spawn
  decisions happen once, at the moment the game's own code runs them - by
  the time an area is on-screen and cacheable, any spawn decision for it has
  already happened or not). There is no "ground truth" collision data
  further out to spawn correctly against; the NES doesn't keep more than
  ~2 screens resident in its 2KB of RAM, by design. Making this work for
  real (not just the random-soldier edge case, which is a scoped, identified
  patch target - `soldier_generation_01` in `bank2.asm`, constants `#$0a`/
  `#$fa`) means giving `contra-nes`'s CPU core a real, tested,
  bank-and-PC-scoped instruction hook - tracked as its own item in
  ROADMAP.md rather than folded into the presentation-only widescreen work,
  since it's a fundamentally different kind of change (it *does* touch
  game state, on purpose, gated behind widescreen being on).
- **Widescreen not visibly turning on, not visibly resizing the window, and
  the camera visibly drifting.** Three related bugs, fixed across several
  rounds. The target width used to be computed only from `Resized` events,
  so toggling it on without a manual resize did nothing; then it always
  targeted a fixed cap regardless of window size, which *also* didn't
  visibly change anything until the window was resized (the fill-scaling
  just drew the wider content smaller to fit). Both are fixed together now:
  toggling widescreen on resizes the window to the current monitor's full
  width, and `target_wide_width` tracks the window's live aspect ratio
  every frame from then on. Separately, a direction-biased extension (most
  of the extra width on the side the camera was scrolling toward, to dodge
  stale trailing-edge tiles) was tried and reverted - it moved the "normal"
  256px window's position within the wide frame as the bias tracked scroll
  direction, which made the player's on-screen position visibly drift
  as if the camera itself moved differently than normal. Reverted back to
  fixed centering (`x_offset = extra / 2`, always), which keeps the
  player's screen position identical to narrow mode at all times - and
  isn't reopening the trailing-edge problem, since the tile cache now
  handles that instead. A related buffer bug was fixed alongside the first
  fix: `wide_framebuffer` was always allocated at the old fixed cap and
  copied a slice of that fixed width per scanline regardless of the width
  actually in use that frame, corrupting the buffer's row stride whenever
  the active width was smaller. It's now sized and copied to the actual
  per-frame width.

### Stage select: works, but the first verification of it was wrong

The Debug tab lets you jump directly to any of the 8 stages. It works. That
wasn't the original conclusion, and the story of the wrong conclusion is
worth keeping here since the mistake itself is the useful lesson.

The reference disassembly's "Contra Control Flow.md" documents that
`level_routine_05` transitions between levels by incrementing
`CURRENT_LEVEL`, clearing RAM `$40-$f0` and `$300-$5ff` (enemy/object/
sprite-buffer state left over from the level that just ended), and
resetting `LEVEL_ROUTINE_INDEX` (`$2c`) to `$00` to restart level loading
from `level_routine_00`. Replicating exactly that from `contra-pc` (poke
`CURRENT_LEVEL` to the target stage, clear those two RAM ranges, reset
`LEVEL_ROUTINE_INDEX`) was tried against the real ROM via
`dump_frames.rs`'s `JUMP_STAGE` env var, checked ~80 frames afterward.
Result at the time: the CPU-side state machine looked consistent
(`GAME_ROUTINE_INDEX` stayed `$05`, no illegal opcodes) but the rendered
screen was a flat gray field - concluded broken, blamed on UxROM PRG
bank-switching being mapper state a RAM poke can't touch, and shipped as
read-only with that explanation.

The conclusion was wrong, and the flaw was in the test, not the feature:
80 frames (a bit over a second) isn't long enough. Re-tested with
`LEVEL_ROUTINE_INDEX` traced every 10 frames for 3000+ frames: it actually
*advances* the whole way, `level_routine_00 → 01 → 02 → 03 → 04`, each step
taking real time (the score-flash step in particular has a timer that
just runs longer than a second), and lands on genuine, correctly-rendered
gameplay for the target stage - confirmed visually at two different target
stages (a bright outdoor jungle-style level and a distinct blue-pipe indoor
level), each unmistakably the right stage, not a corrupted mix of two. The
gray field wasn't corruption; it was an intro/score screen the first test
simply didn't wait for. A jump to a stage where the scripted test input
wasn't suited to the terrain also correctly reached a real "GAME OVER /
CONTINUE" screen - i.e., the *game* played normally afterward, including
losing normally, not just rendering a static correct-looking frame.

The practical upshot: jumping stages costs the same 30-60 real seconds a
level-complete transition always costs in this game (it's the same code
path, after all) - `contra-pc`'s stage-select buttons don't grey out or
show a spinner during that stretch, which can look like nothing happened
if you're not expecting the wait, so the Debug tab labels it explicitly.

### Where RAM-based tooling's limits are, in general

The Debug tab's lives/weapon/rapid-fire/continues controls poke a value
the game reads passively every frame (a counter, a flag, a stat), so
there's no "the game expected something else to happen first" gap - the
next frame's game logic just reads the new value like it would've read the
old one. Stage select looks like a different, riskier kind of value
(`CURRENT_LEVEL` is only *consulted* right after a specific transition, not
polled every frame) - but turns out to be safe too, because the transition
it triggers (`level_routine_00`'s own loading code) is self-contained: it
performs whatever bank-switching or setup it needs *as part of its own
execution*, the same way it would if reached normally, regardless of how
`LEVEL_ROUTINE_INDEX` came to be `0`. The real general rule, corrected from
the earlier (wrong) one: a RAM-based trigger is safe if the code path it
triggers is self-sufficient once started, even if that path takes a while
to finish - the risk isn't "does this need multi-step setup", it's "does
verifying this need patience the test didn't give it."

## `contra-core`: hand-ported layer (placeholder demo)

The rest of this document describes `contra-core`'s hand-ported physics/RNG
- used for the engine-only placeholder demo when no ROM is loaded, and as
a reference for the RAM-poke-based tooling described in ROADMAP.md. It does
**not** describe what `contra-pc` does when you give it a real ROM - that's
entirely `contra-nes`, above.

## Verified against the disassembly

### Vertical physics (`crates/contra-core/src/fixed.rs`, `physics.rs`)

`apply_gravity` in `bank7.asm` does exactly this every frame:

```
clc
lda PLAYER_Y_FRACT_VELOCITY,x
adc #$23                      ; .1367 px/frame², added every frame
sta PLAYER_Y_FRACT_VELOCITY,x
lda PLAYER_Y_FAST_VELOCITY,x
adc #$00                      ; carry into the whole-pixel byte
sta PLAYER_Y_FAST_VELOCITY,x
```

and `player_jumping_set_y_pos` integrates position using a *second*
accumulator (`PLAYER_JUMP_COEFFICIENT`) that absorbs the fractional
carry separately from the velocity's own fractional byte:

```
lda PLAYER_JUMP_COEFFICIENT,x
clc
adc PLAYER_Y_FRACT_VELOCITY,x
sta PLAYER_JUMP_COEFFICIENT,x
lda SPRITE_Y_POS,x
adc PLAYER_Y_FAST_VELOCITY,x
sta SPRITE_Y_POS,x
```

`contra-core::fixed::{Velocity16, JumpAccumulator}` reproduces both
operations byte-for-byte (same 8-bit wraparound, same carry propagation),
and `physics::PlayerPhysics::step_vertical` calls them in the same order.
This is real, tested (`fixed.rs`, `physics.rs` unit tests), and is why a
"same gravity curve as the NES" claim is currently true for the vertical
axis specifically.

### RNG mechanism

There is no LFSR. `RANDOM_NUM` is a byte that free-runs during CPU idle time
between frames (`forever_loop` in `bank7.asm`): `RANDOM_NUM +=
FRAME_COUNTER`, over and over, as many times as fit before the next NMI.
`contra_core::rng::NesAccumulatorRng` models that update rule exactly.

### Horizontal movement (`physics.rs::WALK_SPEED`)

Not fixed-point at all - `set_player_positive_x_velocity` /
`set_player_negative_x_velocity` in `bank7.asm` set `PLAYER_X_VELOCITY` to a
literal `#$01` or `#$ff` (-1) every frame based on d-pad state, no
accumulation. `PlayerPhysics::step_horizontal` reproduces this directly
(`WALK_SPEED = 1`), which is why player horizontal movement is exactly ±1
px/frame - the same speed the NES has, not an approximation of it.

### Jump takeoff velocity (`physics.rs::JUMP_VELOCITY_{OUTDOOR,INDOOR}`)

`set_jump_status_and_y_velocity` sets `PLAYER_Y_FAST_VELOCITY`/
`PLAYER_Y_FRACT_VELOCITY` directly from a location check:

```
lda LEVEL_LOCATION_TYPE       ; 0 = outdoor; 1 = indoor
lsr
lda #$fb : ldy #$f0           ; outdoor: fast=$fb, fract=$f0
bcc @set_y_velocity
lda #$fc : ldy #$90           ; indoor:  fast=$fc, fract=$90
@set_y_velocity:
sta PLAYER_Y_FAST_VELOCITY,x
tya
sta PLAYER_Y_FRACT_VELOCITY,x
```

`JUMP_VELOCITY_OUTDOOR`/`JUMP_VELOCITY_INDOOR` store these as the exact raw
register bytes (not a decimal reinterpretation - see the note in
`physics.rs`'s doc comments about why the disassembly's own inline "-5.94"/
"-4.56" comments are a human shorthand rather than the literal two's-complement
value). Feeding the raw bytes through the same `apply_gravity`/
`JumpAccumulator::integrate` used for the rest of the fall means the
resulting arc is bit-exact by construction - there's no separate "trust me"
constant to get subtly wrong.

The player-death "pop" bounce (`DEATH_BOUNCE_VELOCITY`, from `kill_player`:
fast=`$fd`, fract=`$80`) is ported the same way but not yet wired to a death
state in `contra-pc` (there's no death state yet - no enemies to die to).

## Honest placeholders (not yet ported)

- **Weapon recoil, enemy-collision knockback, water-state movement
  modifiers.** Not yet extracted from `bank6.asm`/`bank7.asm`.
- **Enemy AI, hitboxes, spawn tables, aim math.** The disassembly's `Enemy
  Routines.md`, `Enemy Glossary.md`, and `Aim Documentation.md` are detailed
  enough to port faithfully - this is scoped, tracked work (ROADMAP.md
  Phase 1), not started.
- **Bit-exact "Original NES" RNG.** Reproducing `RANDOM_NUM`'s *exact*
  sequence for a given input log requires knowing how many idle-loop
  iterations ran between two NMIs, which depends on the exact cycle cost of
  every routine that frame (how many enemies were active, which menu was
  open, etc). Two honest paths forward, both tracked in ROADMAP.md:
  1. Cycle-accurate simulation of each frame's workload, then feed the
     resulting iteration count into `NesAccumulatorRng::tick_idle_frame`.
  2. Static recompilation of the original 6502 code (in the spirit of
     projects like the N64/SM64 PC recomps) - translate the disassembly to
     Rust routine-for-routine, so timing-dependent behavior like this falls
     out for free instead of being reverse-engineered twice.

     Approach 2 is the more "impeccable" answer if this project ever has the
     contributor bandwidth for it; it would also make hitboxes, enemy AI,
     and every other timing-sensitive system bit-exact in one pass instead
     of one system at a time. It's a substantially larger undertaking than
     hand-porting individual routines and is noted here as the long-term
     direction, not a Phase 1 commitment.

## What "Original NES" mode means today vs. eventually

Today: config flag exists (`FidelityConfig::original_nes_mode`), and the
state machine / difficulty defaults respect it, but the underlying systems
it should lock in (slowdown emulation, exact RNG, exact hitboxes) aren't
finished, so flipping it doesn't yet produce a provably bit-exact
experience. It's built as the seam those systems plug into as they land,
rather than retrofitted later.
