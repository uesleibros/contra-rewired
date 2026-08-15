# Fidelity notes

"Same physics, same RNG, same hitboxes" is a specific, checkable claim, not a
vibe. This document tracks exactly what's been verified against the
[vermiceli/nes-contra-us](https://github.com/vermiceli/nes-contra-us)
disassembly, what's an honest placeholder, and why bit-exact "Original NES"
mode is harder than it looks.

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

Not fixed-point at all — `set_player_positive_x_velocity` /
`set_player_negative_x_velocity` in `bank7.asm` set `PLAYER_X_VELOCITY` to a
literal `#$01` or `#$ff` (-1) every frame based on d-pad state, no
accumulation. `PlayerPhysics::step_horizontal` reproduces this directly
(`WALK_SPEED = 1`), which is why player horizontal movement is exactly ±1
px/frame — the same speed the NES has, not an approximation of it.

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
register bytes (not a decimal reinterpretation — see the note in
`physics.rs`'s doc comments about why the disassembly's own inline "-5.94"/
"-4.56" comments are a human shorthand rather than the literal two's-complement
value). Feeding the raw bytes through the same `apply_gravity`/
`JumpAccumulator::integrate` used for the rest of the fall means the
resulting arc is bit-exact by construction — there's no separate "trust me"
constant to get subtly wrong.

The player-death "pop" bounce (`DEATH_BOUNCE_VELOCITY`, from `kill_player`:
fast=`$fd`, fract=`$80`) is ported the same way but not yet wired to a death
state in `contra-pc` (there's no death state yet — no enemies to die to).

## Honest placeholders (not yet ported)

- **Weapon recoil, enemy-collision knockback, water-state movement
  modifiers.** Not yet extracted from `bank6.asm`/`bank7.asm`.
- **Enemy AI, hitboxes, spawn tables, aim math.** The disassembly's `Enemy
  Routines.md`, `Enemy Glossary.md`, and `Aim Documentation.md` are detailed
  enough to port faithfully — this is scoped, tracked work (ROADMAP.md
  Phase 1), not started.
- **Bit-exact "Original NES" RNG.** Reproducing `RANDOM_NUM`'s *exact*
  sequence for a given input log requires knowing how many idle-loop
  iterations ran between two NMIs, which depends on the exact cycle cost of
  every routine that frame (how many enemies were active, which menu was
  open, etc). Two honest paths forward, both tracked in ROADMAP.md:
  1. Cycle-accurate simulation of each frame's workload, then feed the
     resulting iteration count into `NesAccumulatorRng::tick_idle_frame`.
  2. Static recompilation of the original 6502 code (in the spirit of
     projects like the N64/SM64 PC recomps) — translate the disassembly to
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
