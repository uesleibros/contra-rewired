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

## The actual end state: zero ROM dependency at runtime

Worth being explicit about, since it's easy to undersell by only talking
about CPU logic: "real decompilation-based port" means the ROM stops being
needed at all once the port is finished, the same way Ship of
Harkinian/SM64-decomp work. Concretely, that's **two** things, not one:

1. **Game logic** (this document's main focus so far) - every routine
   ported to native Rust and integrated via `HookAction::ReturnNow` (or,
   once enough of the game is ported, running standalone with no emulated
   CPU underneath at all).
2. **Assets** - graphics, audio, and level/enemy data currently live
   *inside* the ROM and get read from it (via `contra-nes`'s PPU/APU
   emulation, or a native port peeking at PRG-ROM bytes directly) every
   time the game needs them. The real end state extracts all of it
   **once** - decoded to modern formats (PNG spritesheets, audio samples,
   plain Rust/RON data tables for level headers and enemy placement) via
   an extraction tool that runs against the player's own legally-owned
   ROM - and from then on `contra-pc` reads only those extracted files,
   never the ROM again. This keeps the legal model intact (a ROM is still
   required *once*, from the player, to produce the assets - this project
   still ships none of Konami's work) while actually reaching "runs
   without emulation."

Neither piece is optional for calling this "done"; a port that ported
every CPU routine but still decoded graphics from the ROM's compressed CHR
data at runtime, or vice versa, would still be emulating *something*. Both
are real, substantial, and not yet started beyond the game-logic piece
above - tracked here as they begin, the same honest way everything else in
this document is.

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
decomp-based ports use during their transition period.

**Built, and functionally proven - with one real, open precision problem.**
[`HookAction::ReturnNow(cycles)`](`crate::HookAction`) (`crates/contra-nes/
src/nes.rs`) is the mechanism: a hook can now compute a routine's effect
via its native port, write the result into the exact registers/RAM the
real routine's contract promises, then return this to make
[`Cpu::force_return`] simulate an `RTS` - the routine's body never executes
at all. `dump_frames.rs`'s `INTEGRATE_BG_COLLISION=1` does exactly this for
`collision::bg_collision`, and it works: the game runs, renders correctly
(checked visually, including water collision - the exact thing this
routine computes), and hits no illegal opcodes with the real routine
substituted out entirely, for a real multi-thousand-frame session.

**Now fully solved: bit-perfect parity, proven, not assumed.** Getting
there took three real bugs, found and fixed in order - the honest account
is worth keeping, the same way the stage-select saga is:

1. **Cycle cost: guessed, then measured wrong, then measured exhaustively
   and exactly.** A flat 6-cycle guess (a bare `RTS`'s own cost) was the
   first attempt - wrong, since it ignores the entire skipped body. A
   "measured" 151/154-cycle two-case model came next, but the probe that
   produced it declared its histogram *inside* the per-frame loop, so it
   silently reset every frame and only ever saw whichever 1-2 costs
   recurred early each frame - the real range turned out to be **nine**
   distinct values once the histogram was moved outside the loop
   (`MEASURE_BG_COLLISION_CYCLES=1`). Even a call-count-weighted average
   over those nine wasn't exact enough - real divergence remained.
   **Fixed for real** by not sampling gameplay at all:
   `EXHAUSTIVE_BG_COLLISION_CYCLES=1` drives the real ROM's code directly
   through `contra-nes`'s cycle-accurate CPU (a synthetic `jsr`/`rts`
   harness - poke the routine's documented inputs, fake a call, single-step
   until it returns, sum the real cost), tested against every real branch
   combination this routine has. The costs combine *perfectly additively*
   (a row-guard/column-dependent base, `+1` for a vertical-scroll `cmp`
   adjustment, `-2` for a vertical-scroll byte overflow, `+1` for a
   horizontal-scroll overflow, no interaction terms) - `collision::
   bg_collision_cycles` implements that exact formula, and a test encodes
   all 30 measured cases as a regression check so it can't silently drift
   from what real hardware does.
2. **Zero-page scratch state was never written back.** `ReturnNow`
   originally only set the routine's *documented* output (`a`, carry) -
   but the real routine also leaves five zero-page addresses (`$10`-`$13`,
   `$15`) in specific states, and zero page in this game is shared,
   tightly reused scratch space across many unrelated routines - some
   *other* routine reading one of those addresses expecting its own last
   write, not whatever `get_bg_collision` left stale from a previous
   unrelated call, is a real desync source that no amount of cycle-cost
   precision fixes. `collision::bg_collision_scratch` computes all five;
   the integration hook writes them back exactly like the real routine's
   own `sta`s would.
3. **N/Z flags were never set.** The real routine's *last* instruction
   before its `rts` is `lda $14` (reloading the collision code) - an
   ordinary `LDA` that sets `N` to the loaded byte's bit 7 and `Z` to
   whether it's zero, same as any load. `ReturnNow` skips that instruction
   along with the rest of the body, so without setting `N`/`Z` explicitly,
   they were left however some *earlier* instruction happened to leave
   them - and at least one real caller (`bank7.asm`: `jsr get_bg_collision;
   bpl @apply_gravity`) branches on `N` immediately after the call. This
   was silently changing real control flow, not just leaving a flag
   unread - the most consequential of the three bugs, and the one that
   finally explained the stubborn residual divergence the first two fixes
   didn't close.

**Proof, not assertion**: an A/B `RAM_DUMP_FRAME` diff with all three
fixes in place - full 2048-byte RAM *and* every CPU register/flag/cycle
count - came back **byte-for-byte, bit-for-bit identical** to a
fully-emulated baseline, across two separate sessions (3000 frames plain;
8000 frames including a stage jump, which exercises heavy RAM churn via
the graphics-buffer flush). Not "close enough" - identical. This is the
first routine in the project verified end-to-end as a true drop-in
replacement for its real 6502 code, cycle for cycle.

## Current status

- [x] **`collision::bg_collision`** (`crates/contra-native/src/collision.rs`)
      - ported from `get_bg_collision` (`bank7.asm`, `$e0bb`-`$e12a` in the
      real ROM). Verified against real gameplay (`VERIFY_BG_COLLISION=1`,
      multiple thousand-frame sessions including a stage jump for varied
      terrain) - zero mismatches across every real call observed so far.
      **Fully integrated, not just verified** - `INTEGRATE_BG_COLLISION=1`
      substitutes this port for the real routine live via `HookAction::
      ReturnNow` (exact per-branch cycle cost via `bg_collision_cycles`,
      full zero-page scratch state via `bg_collision_scratch`, correct N/Z/
      carry flags), and an A/B `RAM_DUMP_FRAME` diff against a
      fully-emulated baseline came back byte-for-byte, bit-for-bit
      identical across a 3000-frame and an 8000-frame (with a stage jump)
      session. First routine in the project proven as a true, cycle-exact
      drop-in replacement - see the "Integration strategy" section above
      for the three real bugs closing that gap actually took.
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
- [ ] Everything else, logic side. Two routines out of what's realistically
      hundreds across 8 PRG banks - `bank7.asm` alone (the fixed,
      always-mapped bank) is close to 11,000 lines of assembly by itself.
      No claim is made here about which routine comes next or on what
      timeline; this file should be updated as real ports land, the same
      way ROADMAP.md tracks everything else.
- [x] **Asset extraction - started: graphics, first slice proven
      byte-perfect.** `graphics::decompress`/`apply_chr_writes`
      (`crates/contra-native/src/graphics.rs`) is a native Rust port of
      `write_graphic_data_to_ppu` (bank 7, `$c9a1`), the RLE decompressor
      the real game uses to unpack `graphic_data_XX` blobs from PRG-ROM
      into CHR pattern-table tiles - documented in `nes-contra-us`'s
      `docs/Graphics Documentation.md` (note: that doc's own pseudocode
      has a transcription bug where the RLE branch writes the count byte
      instead of a separate payload byte; this port follows the prose
      description and worked example instead, which are unambiguous and
      match). Verified for real, not just unit-tested: decoding all 7
      `graphic_data_XX` blobs `level_1_graphic_data` loads, straight from
      PRG-ROM with **no emulation involved**, and comparing the result
      against `contra-nes`'s live CHR-RAM after actually playing into
      level 1 came back **byte-for-byte identical across all 8192 CHR
      bytes** (`cargo run -p contra-nes --release --example
      extract_graphics`). `contra-extract --dump-graphics <dir>` uses the
      same decoder to dump all 27 documented `graphic_data_XX` blobs
      (every level, menus, endings) to tile-sheet PNGs from PRG-ROM alone
      - confirmed visually correct (Bill/Lance sprites, letters, power-up
      icons all render recognizably). Not done: audio (DPCM samples, music
      sequences) and level/enemy data tables (super-tiles, palettes,
      spawn data) haven't been started; nametable/attribute-bound blobs
      (`graphic_data_00`/`_02`/`_18`) decode correctly but aren't rendered
      as tile sheets since they're not CHR data.
      **Palettes, also proven byte-perfect:** `palette::resolve_palette_rgb`
      (`crates/contra-native/src/palette.rs`) ports `load_palette_colors_to_
      cpu` (bank 7, `$d227`'s `game_palettes` table) plus level-header
      `LEVEL_PALETTE_INDEX` resolution (`level_headers`, bank 2 `$b319`,
      confirmed 32-byte-per-level layout and the `LEVEL_PALETTE_INDEX`
      offset two independent ways: counting header fields from
      `src/bank2.asm`, and RAM-address subtraction `$50 - $40 = 0x10`).
      Verified the same way: decoding level 1's background palette 0
      straight from PRG-ROM and comparing against `contra-nes`'s live PPU
      palette RAM ($3F00-$3F03) after actually playing into the level came
      back identical. `contra-extract --dump-palettes <dir>` renders all
      110 `game_palettes` groups as color swatches; combining this with
      the graphics decoder already produces a properly-colored, real
      in-game-accurate level 1 tile sheet (see `extract_graphics.rs`'s
      `decoded_chr_colored.png` output) from PRG-ROM bytes alone. The NES
      2C02's 64-color master palette isn't ROM data (it's a fixed PPU
      hardware property) - `palette::NES_MASTER_PALETTE` is its own copy,
      kept byte-identical to `contra_nes::ppu::NES_PALETTE` by an
      assertion in the same verification example, so `contra-native` still
      doesn't depend on the emulator crate at all. Not done: which palette
      group applies to which *tile* (the attribute table / super-tile
      palette assignment) isn't wired up yet, so today's colored output
      uses one fixed palette across a whole tile sheet rather than the
      real per-tile assignment; and only level 1's indexes have been
      exercised so far (the other 7 levels' headers use the same decode
      path but haven't been individually verified).

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
