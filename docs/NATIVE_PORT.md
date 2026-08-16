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

1. **A verified, byte-matching disassembly already exists - and this
   project has personally confirmed it, not just trusted the claim.**
   [`vermiceli/nes-contra-us`](https://github.com/vermiceli/nes-contra-us)
   is annotated 6502 assembly (not C) that the project's own build scripts
   reassemble back into a ROM matching a specific, documented SHA-512 hash
   of the real US retail ROM. Once a C toolchain was available (see
   "Toolchain" below), this was personally rebuilt from source and
   diffed (`cmp`) byte-for-byte against the real `baserom.nes` this
   project already uses - genuinely identical, not just a matching hash.
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

## Toolchain

A C toolchain (MSYS2's mingw64 `gcc`/`make` on Windows; any `gcc`/`make` +
`git` elsewhere) is what `cc65` itself needs to build from source - `cc65`
doesn't ship prebuilt Windows binaries on its GitHub releases, and isn't in
MSYS2's own package repos, so building it from source is the practical
path. Once that's done:

```sh
git clone https://github.com/cc65/cc65.git && cd cc65 && make -j4
# bin/ca65, bin/ld65, etc. - put bin/ on PATH for the next step

git clone https://github.com/vermiceli/nes-contra-us.git
cd nes-contra-us
cp /path/to/your/own/baserom.nes .   # MD5 7bdad8b4a7a56a634c9649d20bd3011b
bash build.sh Contra                  # prints "File integrity matches."
```

`ld65` (invoked by `build.sh`) also writes `contra.dbg`, a complete debug
symbol table - every named routine and RAM variable's real address, no
guessing required. `docs/rom-symbols.txt` is that table, extracted and
committed to this repo (see its own header for the exact extraction
command and how to regenerate it) - use it instead of the raw-byte-search
technique described below for anything it already covers (it covers
almost everything; that technique remains valid, just slower, for
addresses `nes-contra-us` doesn't label). It already cross-checked clean
against the two addresses found the hard way before it existed -
`get_bg_collision` (`$e0bb`) and `initialize_enemy` (`$ee47`) both matched
exactly.

This toolchain is a real, semi-large install (MSYS2 itself, plus building
`cc65` from source) - worth doing once per contributor machine that wants
to work on `contra-native`, not something to redo per-session. It isn't a
runtime dependency of anything else in this repo; `contra-nes`/`contra-pc`
build and run exactly as before without it.

## The methodology, per routine

1. **Find it.** Read the relevant disassembly file(s) from
   `vermiceli/nes-contra-us` (fetched from GitHub - not vendored in this
   repo) for the routine in question, understand what it does and why.
2. **Locate its real CPU address**, if a hook will be needed to verify it
   (not always necessary - some routines are pure functions of inputs
   already available another way). Look it up in `docs/rom-symbols.txt`
   first - it covers almost every named routine. Only fall back to
   searching the ROM's raw bytes for the routine's known opening (and, if
   needed, closing) instructions - unique enough to match exactly once,
   file offset converted to a CPU address via UxROM's known bank layout
   (fixed bank = ROM's last 16KiB, mapped to `$c000-$ffff`; switchable bank
   = whatever `bank_select` currently points at, mapped to `$8000-$bfff`)
   - for the rare address that file doesn't cover. That byte-search
   technique is what found the Base 1/Base 2 stage-select hang's location
   and (before `rom-symbols.txt` existed) `initialize_enemy`'s entry point
   - see docs/FIDELITY.md.
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
   `VERIFY_BG_COLLISION=1`/`VERIFY_PLAYER_GRAVITY=1` - see that file), and
   assert the native port's output for those same inputs matches the real
   ROM's, across as much real, varied gameplay as practical (movement,
   different stages/terrain, not just one static scene). A port isn't
   "done" until this passes with zero mismatches over a real play session,
   not just over the cases the port's author happened to think of by hand.
   **A routine can have more than one real entry point** - watch for this
   before concluding a hook "never fires": `player_physics`'s first
   verification pass hooked only the combined `apply_gravity_set_y_pos`
   entry and saw almost nothing, because the actual main jump-handling code
   (`set_jump_status_and_y_velocity`) calls `apply_gravity` and
   `player_jumping_set_y_pos` *separately*, skipping the combined entry
   point entirely - only the rarer ledge-fall/landing code paths use it.
   Hooking each sub-routine's own entry/exit independently (which still
   correctly catches calls that arrive via the combined entry too, since
   that entry's `jsr`/fall-through passes through the same addresses)
   fixed it. Zero verified calls over a real play session is a signal to
   check for this, not a pass.
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
- [x] **`player_physics::apply_gravity` / `integrate_y_position`**
      (`crates/contra-native/src/player_physics.rs`) - ported from
      `apply_gravity` (`$d9ec`-`$d9f9`) and `player_jumping_set_y_pos`
      (`$d9cb`-`$d9e9`). Verified independently (see the methodology note
      above about why - the combined `apply_gravity_set_y_pos` entry
      almost never gets used by real gameplay) across multiple sessions
      including a stage jump - zero mismatches. Also the first port to
      reveal something a from-scratch reimplementation would have had no
      way to know without reading the real code: `PLAYER_HIDDEN` gets
      nudged by the same uncleared-carry `ADC` chain as the position
      update, every single airborne frame, for reasons even the original
      disassembly's own comment says aren't fully understood - real,
      verified behavior of the original game, ported faithfully regardless.
- [ ] Everything else. Two routines out of what's realistically hundreds
      across 8 PRG banks - `bank7.asm` alone (the fixed, always-mapped
      bank) is close to 11,000 lines of assembly by itself.
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
