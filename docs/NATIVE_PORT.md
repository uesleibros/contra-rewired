# The native port: turning contra-rewired into a real decompilation-based PC port

## What this is, honestly

`contra-rewired` today (everything outside this initiative) is an accurate
NES emulator (`contra-nes`) plus a desktop front-end (`contra-pc`) with
quality-of-life features layered on top - widescreen, save states, rewind,
mod scripting, and so on. The actual game logic - enemy AI, collision,
scoring, level flow, all of it - is still the original 1988 6502 machine
code, interpreted instruction by instruction by an emulator. That's a
legitimate, honest way to build a PC port, and it's what most of this
project has been so far.

It is **not** the same kind of thing as Ship of Harkinian (Ocarina of
Time) or the various SM64/Zelda 1/etc. decompilation-based ports. Those are
built on a *decompilation*: the entire original game rewritten as
human-readable, compilable C that - critically - has been verified to
reassemble/recompile back to a byte-for-byte match of the original binary.
Once you have that, the C source *is* the real source of truth, and you can
freely edit game logic (camera range, spawn conditions, resolution,
whatever) the same way you'd edit any other program, because you're editing
the actual logic, not working around a sealed binary from outside.

This document is the plan for actually becoming that second kind of thing,
one verified piece at a time. It's a large, multi-session undertaking -
this file exists so that's tracked honestly rather than quietly implied to
be further along than it is.

## Why this is realistic here specifically

Three things make this tractable in a way "decompile an NES game from
scratch" normally wouldn't be:

1. **A verified, byte-matching disassembly already exists.**
   [`vermiceli/nes-contra-us`](https://github.com/vermiceli/nes-contra-us)
   is annotated 6502 assembly (not C) that the project's own build scripts
   reassemble back into a ROM matching a specific, documented SHA-512 hash
   of the real US retail ROM. This project hasn't personally re-run that
   build (see "What we didn't do" below) but the claim is specific,
   falsifiable, and comes from a mature, actively-maintained community
   project - a reasonable thing to build on.
2. **`contra-nes` is already an accurate, working emulator.** That makes it
   a ready-made *reference oracle*: run the real ROM through it with some
   input, and you get ground-truth behavior to check a native port against,
   without needing real NES hardware or a second independent implementation.
3. **`Nes::run_frame_with_hook`** (added for the `enemy_spawn` mod event,
   `crates/contra-nes/src/nes.rs`) can capture a specific routine's exact
   inputs and outputs from real gameplay, by hooking its real CPU address.
   That turns "does this Rust function behave like the real 6502 routine"
   into an automatable, repeatable test - not something that has to be
   eyeballed from reading assembly.

## What we didn't do, and why

No `ca65`/`cc65` toolchain was installed to personally reassemble
`nes-contra-us` and re-confirm its hash-matching claim. Getting one
working on this machine would have meant installing a genuinely new,
sizeable piece of system software (MSYS2 or a mingw-w64 distribution -
several hundred MB, PATH changes) purely to double-check a specific,
already-documented claim from a mature project, rather than to do any of
the actual porting work. That's a real cost for a check this project's
*own* verification method (below) makes redundant anyway: an error in the
source disassembly would show up as a mismatch in per-routine verification
regardless of whether the disassembly's overall byte-match claim was
personally re-confirmed first. If a contributor already has cc65 on their
machine, personally verifying the build is a welcome, cheap sanity check -
just not worth a fresh system-wide install to obtain.

## The methodology, per routine

1. **Find it.** Read the relevant disassembly file(s) from
   `vermiceli/nes-contra-us` (fetched from GitHub - not vendored in this
   repo) for the routine in question, understand what it does and why.
2. **Locate its real CPU address**, if a hook will be needed to verify it
   (not always necessary - some routines are pure functions of inputs
   already available another way). Search the ROM's raw bytes for the
   routine's known opening (and, if needed, closing) instructions - unique
   enough to match exactly once - and convert the file offset to a CPU
   address using UxROM's known bank layout (fixed bank = ROM's last 16KiB,
   mapped to `$c000-$ffff`; switchable bank = whatever `bank_select`
   currently points at, mapped to `$8000-$bfff`). This is the same
   technique that found the Base 1/Base 2 stage-select hang's location and
   `initialize_enemy`'s entry point - see docs/FIDELITY.md.
3. **Port it.** Write the equivalent Rust in `crates/contra-native`,
   translating the real 6502 semantics precisely (register-width
   arithmetic, actual carry/overflow behavior, not an approximation) -
   see `collision::bg_collision`'s doc comment for a concrete example of a
   case where an "obvious" simplification would have quietly diverged from
   the real hardware for some inputs.
4. **Verify it against real gameplay**, not just hand-written test cases.
   Hook the routine's entry (and exit, if the output isn't otherwise
   observable) with `Nes::run_frame_with_hook`, capture real inputs/outputs
   during actual play (`dump_frames.rs` env vars, e.g.
   `VERIFY_BG_COLLISION=1` - see that file), and assert the native port's
   output for those same inputs matches the real ROM's, across as much
   real, varied gameplay as practical (movement, different stages/terrain,
   not just one static scene). A port isn't "done" until this passes with
   zero mismatches over a real play session, not just over the cases the
   port's author happened to think of by hand.
5. **Land it disabled.** A verified port doesn't automatically replace the
   emulated version yet - see "Integration strategy" below.

## Integration strategy (not yet started)

Verifying a port's *outputs* match is necessary but not sufficient to
*use* it in place of the real ROM's code - that also needs a way to
redirect execution: when the game would normally run the original
routine, run the native Rust version instead and skip the 6502 code
entirely, while everything else keeps running through the emulator as
before. This is the actual "hybrid native/emulated" execution model real
decomp-based ports use during their transition period, and it isn't built
yet - `run_frame_with_hook` can *observe* a routine, not yet *replace* one.
Building that (likely: a hook that, when it fires, computes the routine's
effect directly via the native port, writes the resulting state back to
RAM/registers, and forces the CPU's `pc` to the routine's return address
instead of executing its body) is the next real milestone once a few more
routines are ported, not before - no point building the swap mechanism
before there's more than one verified routine to swap in.

## Current status

- [x] **`collision::bg_collision`** (`crates/contra-native/src/collision.rs`)
      - ported from `get_bg_collision` (`bank7.asm`, `$e0bb`-`$e12a` in the
      real ROM). Verified against real gameplay (`VERIFY_BG_COLLISION=1`,
      multiple thousand-frame sessions including a stage jump for varied
      terrain) - zero mismatches across every real call observed so far.
      Landed as a pure, standalone function - not yet wired to actually
      replace the emulated routine during play (see "Integration strategy").
- [ ] Everything else. This is one routine out of what's realistically
      hundreds across 8 PRG banks - `bank7.asm` alone (the fixed,
      always-mapped bank) is close to 11,000 lines of assembly by itself.
      No claim is made here about which routine comes next or on what
      timeline; this file should be updated as real ports land, the same
      way ROADMAP.md tracks everything else.

## Where to look

- `crates/contra-native/` - the ported code itself; each module's doc
  comment names its source routine(s) and real CPU address(es).
- `crates/contra-nes/examples/dump_frames.rs` - the `VERIFY_*` env vars
  that capture real-gameplay verification data per routine.
- `crates/contra-nes/src/nes.rs` - `Nes::run_frame_with_hook`, the
  instruction-hook infrastructure this all depends on.
- docs/FIDELITY.md - the CPU-address-finding technique's origin story
  (the Base 1/Base 2 stage-select hang) and the general emulator-fidelity
  notes this project is separate from but builds on.
