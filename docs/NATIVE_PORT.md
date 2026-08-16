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
data at runtime, or vice versa, would still be emulating *something*. The
two tracks are at very different stages: **assets are substantially
along** - graphics, palettes, all 8 levels' layout, outdoor enemy spawns,
DPCM samples, and the full sound_code audio bytecode (all 94 sounds) all
decode straight from PRG-ROM and are proven byte-identical against
`contra-nes`'s live state, with a verified frame-by-frame playback engine
for that audio bytecode too (see "Current status" below for exactly
what's done versus still open - volume envelopes, channel-priority
arbitration). **Game logic is still early** - two routines ported and
verified out of realistically hundreds. Tracked here as each piece lands,
the same honest way everything else in this document is.

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
   **Optional accelerant**: `tools/nesrecomp-reference/` has a reference
   build of [`mstan/nesrecomp`](https://github.com/mstan/nesrecomp) (a
   static 6502->C recompiler) already configured for Contra - once built
   (see that directory's README), its `generated/contra_full.c` has a
   mechanically-translated `func_<ADDR>` for most real routines (register
   moves, flags, and memory access already made explicit), which is
   usually faster to read than re-deriving the same thing from raw 6502
   by hand, especially for longer routines. **It's a reading aid only,
   never a source of truth** - it can have the same kind of gaps this
   project found and fixed while evaluating nesrecomp itself (wrong
   function boundaries, misidentified data, dispatch patterns that
   silently break without the right config) - step 4 below is still the
   only thing that makes a port real, exactly as it always has been.
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
- [x] **`bullet_physics::adjust_bullet_velocity`**
      (`crates/contra-native/src/bullet_physics.rs`) - ported from
      `adjust_bullet_velocity`/`bullet_velocity_adjust_00`-`_07` (`bank7.
      asm`, `$f3a5`-`$f419`; speed code 8/2x is real ROM data but is
      genuinely unreachable - the only caller masks with `& 0x07` before
      dispatch - so it isn't ported, per this crate's standing policy). The
      first port built using the `tools/nesrecomp-reference` accelerant
      (nesrecomp's mechanically-translated C as a faster first read of the
      routine, alongside the raw 6502) - still verified the normal way, not
      against the generated C. Hand-verified against the disassembly's own
      3 worked examples (speed codes 0, 6, 7) as unit tests, then against
      real gameplay (`VERIFY_BULLET_VELOCITY=1`, hooking entry at `$f3a5`
      and both real call sites' return addresses at `$f345`/`$f359`, since
      the routine dispatches via the same `run_routine_from_tbl_below`
      inline-jump-table pattern as `game_routine`/`level_routine`/
      `player_state` and never executes its own `rts`): 24 real calls
      captured across a 3600-frame session (all real weapon fire hit speed
      code 3, one of the routine's data-dependent-branch cases), zero
      mismatches. Not yet integrated live (`HookAction::ReturnNow`) - unit-
      and gameplay-verified only so far, same status `bg_collision` and
      `player_physics` had before their own integration passes.
- [x] **`bullet_physics::calc_bullet_velocities`**
      (`crates/contra-native/src/bullet_physics.rs`) - ported from
      `calc_bullet_velocities` (`bank7.asm`, `$f334`-`$f37e`; real comment:
      "used by bullets, eye projectile, and spinning bubbles" - despite the
      "enemy" naming of the RAM it writes through, real gameplay capture
      confirms it's also on the *player's own* bullet-firing path, since
      Contra's bullet objects share the enemy-slot object pool). Looks up
      a base X/Y fractional velocity for a 0-23 `aim_dir` (`bullet_fract_
      vel_dir_lookup_tbl`/`bullet_fract_vel_tbl`, both ported as plain
      tables), scales both axes through the already-ported
      `adjust_bullet_velocity`, then negates whichever axis a `quadrant`
      bit says should flip - a 16-bit two's-complement negation
      (`negate16`) rather than anything speed-code-specific. Unlike
      `adjust_bullet_velocity`, this is a normal `jsr`/`rts` call (single
      real call site, `set_bullet_velocities` at `$f313`), so verification
      could hook its own entry/exit directly instead of needing a call-site
      workaround. Unit-tested via `adjust_bullet_velocity`'s speed-code-2
      identity case (isolates the table lookup/negation logic from needing
      to re-verify the scaling math) for both negation branches, then
      against real gameplay (`VERIFY_CALC_BULLET_VELOCITIES=1`, entry hook
      at `$f334`, exit hook at the call site's return address `$f316`; the
      scripted input also aims up periodically under this flag only, to
      exercise more than one aim direction): 12 real calls across a
      9000-frame session, with real, observed variety in every input - 6
      distinct `aim_dir` values (all within the table's real 0-23 domain,
      confirming the wider `& $1f`/0-31 range every caller masks to is
      never actually reached), all 4 `quadrant` bit combinations (both
      negation paths and neither), and 3 different speed codes - zero
      mismatches. Also not yet integrated live, same status as the routine
      above.
- [x] **`enemy_slots::find_next_enemy_slot` / `find_next_enemy_slot_6_to_0`**
      (`crates/contra-native/src/enemy_slots.rs`) - ported from
      `find_next_enemy_slot`/`find_next_enemy_slot_6_to_0` (`bank7.asm`,
      `$edca`-`$edd9`, sharing a loop body at `find_next_enemy_slot_x_to_0`).
      Scans `ENEMY_ROUTINE` (16 bytes, `$04b8`) from a starting slot down
      to 0, returning the first (highest-indexed) free slot, `None` if
      none are free below the start - the general-purpose "claim a slot in
      the shared enemy/bullet/pickup object pool" utility almost every
      spawn path in the game calls first; the most-called routine ported
      so far (13 real `jsr` sites across banks 0, 2, and 7, vs. 1-4 for
      every prior port). The restricted `_6_to_0` variant is real
      soldier-generation's own reservation scheme (`bank2.asm`'s
      `exe_soldier_generation` path) - keeps random soldier spawns out of
      the top slots so bosses/bullets/hard-coded placements can't get
      crowded out. Unit-tested against synthetic slot arrays (all-free,
      all-occupied, the slot-0 edge case, the restricted variant's own
      ceiling). Live-verified (`VERIFY_ENEMY_SLOT=1`, hooking both real
      entry points at `$edce`/`$edca` for the `ENEMY_ROUTINE` snapshot and
      the one shared internal exit label, `find_enemy_routine_slot_exit`
      at `$edd8`, for the real x-register/zero-flag result - no per-call-
      site workaround needed, unlike `adjust_bullet_velocity`): 67 real
      calls across a 9000-frame session (52 full-range, 15 restricted,
      confirming both variants are genuinely reachable; results spanned
      many different slot indices, confirming the scan itself is
      exercised, not just a single trivial case), zero mismatches. Also not
      yet integrated live, same status as the two routines above.
- [x] **`enemy_clear`** (`crates/contra-native/src/enemy_clear.rs`) -
      ported from the `clear_enemy_*` family (`bank7.asm`, `$edf1`-`$ee46`):
      3 real, reachable entry points (`clear_sprite_and_pt_3`,
      `clear_enemy_custom_vars`, `clear_enemy_pt_2`) that zero
      progressively wider, chained subsets of one enemy slot's fields.
      The routine's own top label, `clear_enemy` (`$edfc`, which also
      clears `ENEMY_ROUTINE`/`ENEMY_HP`/`ENEMY_TYPE`), is *not* ported -
      its only caller (`bank_7_unused_label_07`) is the disassembly's own
      documented dead code, genuinely unreachable. Unit-tested per entry
      point (sentinel-filled struct, assert exactly the right fields hit
      zero and every other field is untouched - including confirming
      `clear_enemy_custom_vars` and `clear_enemy_pt_4` are byte-for-byte
      identical, as the real ASM's shared fallthrough implies). Live-
      verified (`VERIFY_ENEMY_CLEAR=1`, hooking all 3 real entry addresses
      to snapshot every relevant field's pre-call value, and the one
      shared exit - the `rts` right after `clear_enemy_pt_4`'s stores,
      `$ee46` - to compare the real post-call RAM against the pure
      function applied to that snapshot): 60 real calls across a
      9000-frame session, zero mismatches. All 60 were the widest entry
      point, `clear_enemy_pt_2` (`initialize_enemy`'s own real caller,
      the universal per-spawn helper) - which transitively exercises
      `clear_enemy_pt_3`/`_pt_4` for free, but means the other two named
      entry points (`clear_sprite_and_pt_3`'s extra `sprites` clear,
      `clear_enemy_custom_vars`) are unit-tested only so far, not yet
      seen live (they need a dropped weapon pickup / specific enemy-death
      state the current scripted playthrough doesn't reach) - noted
      honestly rather than claimed as fully live-verified. Not yet
      integrated live, same status as every routine above.
- [x] **`initialize_enemy`** (`crates/contra-native/src/initialize_enemy.rs`)
      - ported from `initialize_enemy` (`bank7.asm`, `$ee47`-`$ee8b`): the
      universal "set up a freshly-claimed enemy slot" helper, composing
      the already-ported `enemy_clear::clear_enemy_pt_2` with a per-
      `(ENEMY_TYPE, CURRENT_LEVEL)` property lookup (`ENEMY_STATE_WIDTH`/
      `ENEMY_SCORE_COLLISION`/`ENEMY_HP`/`ENEMY_VAR_A`, from
      `enemy_prop_ptr_tbl`/`enemy_prop_00`-`_07`, `$ee8d`-`$efb7`).
      **The property lookup reads raw PRG-ROM bytes at runtime instead of
      a hand-transcribed table** - the disassembly's own inline comments
      turned out to be unreliable evidence of the real indexing scheme
      (they restart mid-table more than once, and one table is prefixed
      with a section comment - `"; level 3 enemies"` above `enemy_prop_06`
      - that flatly contradicts `enemy_prop_ptr_tbl`'s own real level
      assignment for that label). Rather than guess, this port replicates
      the CPU's exact byte-offset arithmetic (`ENEMY_TYPE < $10` selects
      a shared pointer, else `CURRENT_LEVEL*2` selects a per-level one;
      the actual record read is at `table_ptr + ENEMY_TYPE*4`, using the
      raw type byte unadjusted) directly against the ROM's bytes - the
      same approach `graphics`/`level`/`enemy_spawn` already use for
      data too ambiguous or bulky to hand-copy. See this module's own doc
      comment for the full reasoning; see this module's tests for the
      routine's fields (`InitializedEnemy`'s `routine`/`hp`/`fields`,
      wrapping `EnemyClearFields`) around synthetic PRG-ROM data. Live-
      verified (`VERIFY_INITIALIZE_ENEMY=1`, hooking the real entry
      address `$ee47` for `ENEMY_TYPE`/`CURRENT_LEVEL`, and the routine's
      own single internal `rts` at `$ee8c` - immediately before the
      `enemy_prop_ptr_tbl` label, one exit for every real caller - for the
      real result): 60 real calls across a 9000-frame session, with real,
      observed `ENEMY_TYPE` values `$01`-`$06` (shared-table path) *and*
      `$12` (per-level-table path, `CURRENT_LEVEL=$00`) - confirming both
      real branches of the property lookup, not just the common case -
      zero mismatches. Not yet integrated live, same status as every
      routine above.
- [x] **`create_enemy_bullet`**
      (`crates/contra-native/src/create_enemy_bullet.rs`) - ported from
      `@create_enemy_bullet`/`set_bullet_velocities`/`bullet_gen_exit`
      (`bank7.asm`, `$f2e4`-`$f333`): the real "spawn an enemy bullet"
      routine, and this crate's first port that's *purely composition* of
      prior, independently live-verified building blocks -
      `enemy_slots::find_next_enemy_slot`, `initialize_enemy::
      initialize_enemy`, `bullet_physics::calc_bullet_velocities` - with
      only two small caller-side transforms of its own: splitting the
      packed `bullet_type_and_angle` byte (`>>5` for `ENEMY_VAR_1`,
      `&0x1f` for the aim direction), and *saturating* (not masking)
      `speed_code` to a max of 7 - notably narrower than `adjust_bullet_
      velocity`'s own internal `&0x07`, meaning this caller path can
      never trigger that function's wrap-to-0 case. Unit-tested against a
      synthetic PRG-ROM (slot selection, field composition, and the
      saturate-vs-mask distinction verified directly by comparing against
      `adjust_bullet_velocity`'s own masking behavior). Live-verified
      (`VERIFY_CREATE_ENEMY_BULLET=1`, hooking real entry `$f2e4` and both
      real exits - success at `$f32e`, failure at `$f333` - deriving the
      expected slot from the pure function applied to the same
      `ENEMY_ROUTINE` snapshot rather than trusting `cpu.x` at either
      exit, since the real routine's own last step before either `rts` is
      `ldx ENEMY_CURRENT_SLOT`, discarding the real found slot before
      returning to its own caller): 12 real calls across a 9000-frame
      session, all on the success path, with real, observed variety in
      every input (9 distinct destination slots, 6 aim angles, 3 speed
      codes, all 4 quadrants, varied X/Y positions) - zero mismatches. The
      failure path (`bullet_gen_exit`, no free slot) wasn't observed live
      this session - noted honestly, matches this port's unit test only
      so far. Not yet integrated live, same status as every routine above.
- [x] **`create_enemy_bullet_angle_a`**
      (`crates/contra-native/src/create_enemy_bullet.rs`, same module) -
      ported from `create_enemy_bullet_angle_a`/`create_enemy_bullet_if_
      attack_enabled` (`bank7.asm`, `$f2bf`-`$f2e3`): `create_enemy_
      bullet`'s real caller one level up. Computes the aim quadrant from
      a raw angle byte (`quadrant_from_angle`, a direct mechanical
      translation of the real chained `cmp #$07`/`cmp #$12`/`cmp #$0d`
      sequence rather than a derived bucket list), then gates on
      `ENEMY_ATTACK_FLAG` - except for one bullet type (the level 1
      boss's large cannonball) that always fires regardless. Unit-tested
      (all 4 quadrant boundaries exhaustively, the type-bits-vs-angle-bits
      masking, both gating outcomes, the always-fire override). Live-
      verified (`VERIFY_CREATE_ENEMY_BULLET_ANGLE_A=1`, hooking real entry
      `$f2bf` - before the routine's own first two instructions move
      `a`/`y` into `$0a`/`$06`, so the registers are read directly - and
      the same two real exits `create_enemy_bullet` uses, since both real
      failure reasons, attack-flag declined or `create_enemy_bullet`'s
      own "no free slot", funnel to the identical shared exit): 17 real
      calls across a 9000-frame session, with **both real outcomes
      observed** - 12 successes (`ENEMY_ATTACK_FLAG=1`) and 5 real gate
      rejections (`ENEMY_ATTACK_FLAG=0`, correctly producing no bullet) -
      zero mismatches on either path. Not yet integrated live, same
      status as every routine above.
- [ ] Everything else, logic side. Nine routines out of what's
      realistically hundreds across 8 PRG banks - `bank7.asm` alone (the
      fixed, always-mapped bank) is close to 11,000 lines of assembly by
      itself. No claim is made here about which routine comes next or on
      what timeline; this file should be updated as real ports land, the
      same way ROADMAP.md tracks everything else.
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
- [x] **Super-tiles: nametable + attribute-table layout, both proven
      byte-perfect.** `supertile::decompress_screen` ports
      `read_supertiles_screen_ptr_table`/`load_supertile_indexes_starting_
      at_y` (bank 7, `$e16b`) - a *different* RLE scheme than `graphics`'s,
      discovered by reading the real routine rather than assumed: literal
      bytes, `0x80-0xEF` repeat-runs, and (new, not present in the
      graphic-data format) `0xF0-0xFF` row back-references that replay an
      earlier row of the *same screen* verbatim. Combined with
      `supertile::supertile_tiles`/`supertile_attribute_byte` (plain,
      uncompressed per-super-tile data - a super-tile is exactly one NES
      attribute-table byte's worth of area, 4x4 tiles) and `palette`, this
      assembles a full level-1-screen-0 nametable (which tile) and
      attribute table (which of the level's 4 palettes) straight from
      PRG-ROM. Verified against `contra-nes`'s live PPU state after
      actually playing into the level (`Ppu::peek`/`Nes::peek_ppu`, added
      this session as the read counterpart to the existing `poke`/
      `poke_ppu`): **both the decoded nametable (896 tiles) and the
      decoded attribute table (56 bytes) came back byte-for-byte
      identical** to live PPU state
      (`cargo run -p contra-nes --release --example extract_level`).
      **The colored-render gap from earlier this session is now closed,
      and fully explained rather than worked around.** Rendering the
      assembled screen initially showed ~36% (32/89) of used tiles wrong;
      root cause was self-inflicted, not a decoder bug: a speculative
      extra blob (`graphic_data_01`, HUD letters/numbers) had been added
      to the CHR set to *guess* at fixing an earlier, different visual
      oddity, and its PPU write range ($0ce0-$1f80) overlaps
      `graphic_data_1a`'s (up to tile `0xdb`) - applied after it, so it
      silently overwrote correct level tiles with HUD glyph data. Removing
      that guess and going back to `level_1_graphic_data`'s own literal,
      ROM-confirmed 7-blob list (`03,13,19,1a,14,16,05,ff` - the same list
      already proven byte-for-byte complete against a full 8KB live
      CHR-RAM dump, see `graphics`'s status above) fixed it outright:
      **CHR content, nametable, and attribute table now all read 0
      mismatches simultaneously** - three independent live-PPU comparisons,
      one PRG-ROM-only decode, zero emulation, zero divergence. The
      rendered `level1_screen0.png` this produces is therefore proven
      pixel-exact, not just plausible-looking (the blocky
      grass/foliage-and-vine tile art in it reads as text-like at a glance
      - that's genuine Contra level 1 art, not a bug).
      **Generalized to the whole level, not just screen 0.** Level 1 has
      exactly 13 screens (`level_1_supertiles_screen_00`-`_0c`) - confirmed
      by reading `level_1_supertiles_screen_ptr_table`'s raw pointer bytes
      directly rather than assuming: it holds 14 little-endian pointers,
      the first 13 matching every real screen label in
      `docs/rom-symbols.txt` exactly, and the 14th duplicating the first
      (a defensive wrap-around entry, not a real 14th screen -
      `level_2`'s own table starts immediately after). `extract_level.rs`
      now decodes and renders all 13 screens side by side into one
      3328x224px image, straight from PRG-ROM - a real, coherent Contra
      level (sky/mountains fading into jungle canopy, the bridge, a
      closing wall at the end), not just an isolated tile. **CHR content
      across all 13 screens' distinct tiles (176 of them) matches live
      CHR-RAM with 0 mismatches**, and screen 0's nametable+attribute
      table are (still) proven byte-perfect the same way as before.
      Screens 1-12 use the identical, already-proven decode path (and the
      same independently-cross-checked pointer-table walk) but aren't
      individually re-verified against live PPU state in this session -
      reaching screen 1 in VRAM safely turned out to need scripted play
      past the level's obstacles (a naive "hold right" attempt walked Bill
      into something fatal well before the screen boundary, confirmed by
      CHR-RAM and RAM state both flipping to a death/respawn sequence's) -
      a materially different, larger task than verifying decoding, noted
      here rather than either skipped silently or forced through with a
      fragile hack.
      **Generalized further: all 8 levels, via the ROM's own lookup
      tables, no hardcoded per-level data anywhere.** Ported
      `level_graphic_data_tbl`/`graphic_data_ptr_tbl` (bank 7, `$c8e3`/
      `$c950`) so a level's graphic-data blob list, and each blob's exact
      PRG offset, come from the same two table lookups
      `load_level_graphic_data` performs at real load time - not a
      hand-transcribed list. Doing this **found and fixed a real,
      previously-shipped bug**: `graphic_data_ptr_tbl`'s third byte packs
      *both* a bank number (bits 0-2) *and* a horizontal-flip flag (bit
      7) - confirmed directly against the real consuming code
      (`write_graphic_data_to_ppu`'s `and #$07` / `and #$80`, and its
      `horizontal_flip_graphic_byte` bit-reversal routine, bank 7 area
      `$c9a1`). A first attempt at the general lookup used the byte's low
      7 bits as "the bank" and panicked instantly on level 2 (bank 132,
      wildly out of range) - the fix reads bits 0-2 for the bank and bit 7
      as `flip`, and `graphics::decompress`/`apply_chr_writes` now
      properly bit-reverse every data byte and skip the reused blob's own
      (now-irrelevant) embedded PPU-address header when `flip` is set,
      exactly matching the real routine. This also means
      `apps/contra-extract`'s already-shipped `--dump-graphics` had been
      silently emitting a **mirrored-wrong** tile sheet for
      `graphic_data_10` (Base indoor/base tiles) - fixed now that it uses
      this same general, flip-aware lookup instead of its own hardcoded
      offset table. Verified visually: `graphic_data_0a` and
      `graphic_data_10`'s tile sheets are now genuine horizontal mirrors
      of each other, not one correct and one corrupted. All 8 levels now
      decode and render (`extract_level` example, and
      `contra-extract --dump-levels <dir>`) without hardcoding anything
      level-specific - horizontal levels (7 screen-rows) and the one
      vertical level (level 3, "Waterfall," 8 screen-rows) both handled
      correctly by the same code path, and all 8 renders look like real,
      coherent Contra levels on inspection (level 2's indoor corridors and
      red doors; level 3's waterfalls and rock ledges). Level 1's
      byte-perfect live-PPU proof (CHR, nametable, attribute table) still
      holds unchanged; levels 2-8 use the identical proven pipeline but
      aren't individually live-verified (same reachability caveat as
      screens 1-12 above).
- [x] **Audio - first slice: DPCM samples.** Contra's 2 DPCM (delta
      pulse-code modulation) samples - real raw waveform data, used by 3
      percussion sound effects - are ported to `contra-native::audio`
      (`dpcm_table_entry`/`decode_dpcm`) and extracted to WAV via
      `contra-extract --dump-audio <dir>`. Unlike graphics/palette/level,
      the address/length encoding and the delta-decode algorithm aren't
      Contra-specific - they're standard 2A03 DMC hardware behavior
      (`$4012`/`$4013` register formulas, LSB-first delta decode) - so
      what's actually ported here is just
      `dpcm_sample_data_tbl` (bank 1, `$88db`), the small table saying
      *which* PRG bytes are sample data. `contra-nes`'s APU doesn't
      emulate the DMC channel (see `crates/contra-nes/src/apu.rs`), so
      there's no live playback to verify against the way graphics/
      palette/level were - instead, the computed address/length
      reproduced `docs/Sound Documentation.md`'s worked examples exactly,
      and the resulting byte ranges were diffed (`cmp`) against
      `nes-contra-us`'s own separately-shipped
      `dpcm_sample_{00,01}.bin` files - both came back identical, an
      independent cross-check even without an emulator oracle.
      **What this doesn't cover, and why it's out of scope here:**
      Contra's actual music and most sound effects aren't a decodable
      "asset" at all - they're driven by a custom bytecode sequencer
      (`docs/Sound Documentation.md`'s "sound_code Parsing" section: low/
      high sound commands, percussion commands, note-period tables, a
      `sound_cmd_ptr_tbl` dispatch). Reproducing that means porting a real
      playback *engine*, the same category of work as `collision`/
      `player_physics`, not a one-time extraction - not started.
      **Update, following an explicit "extract everything, nothing can be
      missing" directive**: the DPCM samples above were genuinely only
      part of Contra's audio - the game's actual sound_code bytecode
      (every music track and sound effect's real command data) was
      entirely unextracted. `contra-native::sound_code` now ports the
      **low-format** half of that bytecode (`interpret_sound_byte`/
      `read_low_sound_cmd`, `src/bank1.asm`) - the format used by sound
      *effects* (footsteps, shots, explosions, pickups, etc. - 44 of
      `sound_table_00`'s 94 entries). Verified two ways: (1) by hand,
      tracing two real sounds' raw ROM bytes byte-by-byte against the
      grammar before writing any code (`sound_03` = 17 bytes, `sound_05` =
      55 bytes, both matching `nes-contra-us`'s own separately-shipped
      `.bin` files' sizes exactly); (2) mechanically, across every single
      low-format entry in `sound_table_00` at once
      (`extract_sounds.rs`) - for entries with no shared/repeated
      sub-blocks, the computed length matched the disassembly's `.bin`
      file size exactly (6-for-6 checked); for entries that reference a
      shared child via `$FD`/`$FE`, the computed length matched
      `(sum of the disassembly's own split .bin files) + 2 address bytes
      per distinct reference` in every case checked (9 more, including
      `sound_08`, which turned out to have a genuinely
      self-referential `$FE` - it repeats its own opening phrase by
      pointing the repeat command back at its own start address, a real
      structural finding, not a bug: the disassembly's own `.bin`
      splitting omits self-referential address bytes it writes as
      `.addr sound_08` symbolically instead of as raw incbin data, which
      is exactly why a naive "does my length match the .bin file" check
      would have looked wrong there without understanding why). Wired
      into `contra-extract --dump-sound-codes <dir>`, which extracts
      every low-format sound's raw bytecode - deduplicating shared blobs
      so nothing is written twice - plus an index tying sound codes back
      to files.
      **Update: the high-format half (music) is ported too now - all 94
      of 94 `sound_table_00` entries are extracted, nothing left
      unaccounted for.** Reading `read_high_sound_cmd`/
      `parse_percussion_cmd`/`sound_cmd_routine_00`-`03` further showed
      the earlier "runtime-dependent length" concern didn't actually hold
      up: `sound_cmd_routine_01`'s branch on `UNKNOWN_SOUND_01` (a real
      RAM variable, confirmed via `src/ram.asm`) only changes which
      *interpreter path* runs, not how many bytes the *data* occupies -
      the bytes are compiled into the ROM at a fixed length either way.
      The one place a command's length genuinely varies with the data
      itself (`sound_cmd_routine_02`'s vibrato command, one byte shorter
      when the byte right after it happens to be `$FF`) is handled by
      peeking that byte, the same pattern as the low format's `0xF8`
      escape - real, ROM-fixed variability, not ambiguity. **A second
      real bug was caught before shipping**: an early version reused low
      format's `$FD`/`$FE`/`$FF`-dispatch condition (byte literally
      `>= 0xFD`) for high/percussion format too, which actually dispatches
      to the same control handling for *any* byte `>= 0xF0` - left
      unfixed, this would have silently misparsed `0xF0`-`0xFC` bytes in
      music data as 1-byte units instead of real 3-byte child-jumps,
      corrupting every subsequent length in the blob. Verified against 3
      more real sounds: `sound_26` (22 bytes, no children - exact),
      `sound_29` (10 bytes, the percussion sub-format specifically -
      exact), and `sound_2a` (843 bytes) - which surfaced a genuine
      structural finding worth documenting rather than just fixing: its
      one repeat command targets an address 29 bytes into its *own*
      already-scanned range (not the very start, unlike `sound_08`'s
      low-format self-reference) - walking that "child" from scratch
      retraces the parent's own tail and lands on the exact same
      terminator (`29 + 814 == 843` exactly), which is correct,
      self-consistent behavior (a repeat replaying a middle section back
      to the phrase's own end), not a bug. `contra-extract
      --dump-sound-codes <dir>` now extracts all 94 sound codes across
      all three sub-formats - 232 distinct blobs total from the real US
      ROM.
      **Playback engine: genuinely started, first slice landed, and now a
      far better-informed scope estimate for the rest.** Every sound_code
      byte extracted above is still just *bytecode* - a program, not a
      sound. `sound_code::decode_low_command` is the first real step
      toward making it playable: turns each low-format unit `walk_low`
      already knew the byte-length of into its actual meaning
      (`SetLengthAndConfig`/`Sweep`/`FlattenNoteFlag`/`Note{cfg_low,
      period}` - the same `SOUND_LENGTH_MULTIPLIER`/`SOUND_CFG_HIGH`/APU
      note-period values the real routine computes), verified against
      `sound_03`'s full real command sequence by hand. **What that still
      isn't**: a playback engine, and reading deep enough into
      `handle_sound_code`/`lvl_config_pulse`/`ldx_pulse_triangle_reg` to
      write this decoder revealed the *real* remaining scope more clearly
      than before - it's substantially larger than "interpret one
      command":
        - **Per-frame real-time state**, not a one-shot decode:
          `SOUND_CMD_LENGTH` counts down every video frame, and a new
          command is only read once it hits zero - so a correct engine
          has to be steppable frame-by-frame, not just "decode the whole
          sound_code up front".
        - **Decrescendo/volume-envelope logic** (`@check_pulse_volume`/
          `lvl_config_pulse`/`lower_pulse_volume`/`resume_decrescendo`)
          runs *every frame regardless of format*, reading a per-level,
          per-segment volume envelope table (`pulse_volume_ptr_tbl`, 1
          per level, not yet ported) that low-format sound effects and
          high-format music both interact with.
        - **Channel-priority arbitration across all 6 sound slots**
          (`ldx_pulse_triangle_reg`): two slots can compete for the same
          physical APU channel, and the real routine's slot-priority
          order decides which one's register writes actually reach
          hardware each frame - this can't be modeled one slot at a time
          in isolation.
        - **DMC/percussion sample triggering** ties back into
          `contra-native::audio`'s DPCM decode, itself still unverified
          against live playback since `contra-nes`'s APU doesn't emulate
          the DMC channel (see `audio`'s own status above).
      Porting all of that, correctly, is realistically comparable in
      scope to the CPU-logic-porting workstream's *entire* remaining
      backlog (see "Everything else, logic side" below) - not a small
      remaining piece. Not started beyond the single-command decoder
      above.
      **Follow-up session: committed to the full engine, built real
      verification infrastructure, and found the scope is even deeper
      than the estimate above - concretely, not just in the abstract.**
      Given the choice between stopping at low-format-only, attempting
      the full engine anyway, or pivoting to gameplay logic, this
      project's owner explicitly chose "all of it, accepting the real
      risk of not finishing." Two pieces of real, reusable infrastructure
      came out of pursuing that: `Apu::last_write` (`crates/contra-nes/
      src/apu.rs`) - the raw byte last written to each `$4000`-`$401F`
      register, added since the decoded internal `Pulse`/`Noise` state
      wasn't enough to compare against a native engine's own raw
      computed bytes - and `trace_sound.rs` (`crates/contra-nes/
      examples/`), which snapshots all 6 sound slots' RAM state around
      every real frame during actual gameplay and logs every change,
      turning "read assembly and hope the derivation is right" into "diff
      against captured ground truth" the same way `VERIFY_BG_COLLISION`
      did originally. A captured trace immediately, concretely confirmed
      `decode_low_command`'s hand-derived `sound_03` sequence byte-for-
      byte against real frame-by-frame play (command addresses and
      decoded values matching exactly) - real validation, not just
      internal consistency.
      It also surfaced a genuinely new, deeper problem: `SOUND_VOL_ENV`'s
      per-slot array (`$11E` + slot, 6 slots) overlaps `INIT_SOUND_CODE`
      (`$122`) at slot 4's specific offset - meaning slot 4 (one of the
      two sound-*effect*-only slots) doesn't have a real, independent
      `SOUND_VOL_ENV` byte at all; the decrescendo-check code that runs
      unconditionally for every active slot every frame reads whatever
      `INIT_SOUND_CODE` (an unrelated trigger-time scratch variable) last
      held there instead. Whether that's inert for low-format sounds in
      practice or a genuine, subtle interaction with the per-level
      `pulse_volume_ptr_tbl` machinery isn't yet known - it would need
      dedicated investigation, the same rigor every other finding in this
      document got, not a guess. This is exactly the kind of undocumented,
      hardware-level detail that only shows up from tracing real execution,
      not from reading the disassembly's own comments, and it's a concrete
      illustration of why the scope estimate above undersells the real
      difficulty: correctness here isn't blocked on writing more Rust, it's
      blocked on *discovering* rules like this one that nothing has written
      down anywhere. Given the depth still being uncovered even in the
      simplest (low-format) case, a full, bit-perfect, all-formats engine
      was not reached this session. What's real and lands cleanly: the
      command decoder (verified against real playback, not just hand
      tracing) and the trace-capture/APU-instrumentation infrastructure,
      both immediately reusable for whoever continues this.
      **Second follow-up session: `sound_engine::SoundSlot` (`crates/
      contra-native/src/sound_engine.rs`) is a real, steppable, frame-by-
      frame engine for low-format slots #$04/#$05** - trigger
      initialization, `decode_low_command`'s note-reading loop, and now
      full `0xFD`/`0xFE`/`0xFF` control-flow (child-jump/repeat/end-or-
      return-to-parent, `sound_cmd_routine_03`, single-level nesting only
      - matching the real ROM's own one-bit "in a child" flag). A new
      mechanical verification tool, `examples/verify_sound_engine.rs`
      (`contra-nes`), runs real gameplay, spins up a matching engine
      instance on every real trigger, and compares its computed
      `cfg_low`/`cfg_high`/`cmd_length` against real RAM every frame - not
      hand-picked examples, an exhaustive per-frame diff. This caught and
      fixed a real bug on the first run: the tool (not the engine) was
      seeding a sound's start address from live `SOUND_CMD_LOW/HIGH_ADDR`
      RAM sampled *after* the trigger frame's own immediate first-command
      read had already advanced it - fixed by resolving the true start
      address directly from `sound_table_00`, the same way `load_sound_
      code_entry` does. That fix alone took slot 4 from 4/10 to 8/10
      matched frames and slot 5 from 8/199 to 21/199; adding control-flow
      handling took slot 5 to 28/199.
      The remaining gap was chased all the way to its real root cause
      rather than left as "some mismatches remain": it isn't a
      `sound_code` bug at all. Real Contra's entire game loop runs inside
      the NMI handler (`nmi_start`, `src/bank7.asm`), and `NMI_CHECK`
      (`$001B`) tracks whether a *previous* NMI's handler is still
      running when the next vblank's NMI fires. Traced directly: during
      this test's ordinary movement/combat gameplay, `NMI_CHECK` sits at
      `0x01` continuously - `contra-nes` is cycle-accurate (`Nes::
      run_frame`'s scanline-paced CPU budget), so this is real,
      hardware-accurate slowdown being faithfully reproduced, not an
      emulator bug. Real 6502 NMI is edge-triggered and non-maskable, so
      it reenters `nmi_start` regardless; `nmi_start` detects this and
      takes an alternate path that skips `exe_game_routine` (no new
      player-driven triggers) but *still* calls `handle_sound_slots`
      (`src/bank7.asm:355-369`) - meaning `handle_sound_code` can run
      more than once per visual frame during any lag-heavy stretch.
      `verify_sound_engine.rs` steps the native engine exactly once per
      `run_frame()`, so it necessarily drifts from the cycle-accurate
      reference during lag - a verification-methodology gap, not a
      correctness bug in the engine itself, and irrelevant to the
      eventual native PC port (no 6502 cycle budget to blow, so nothing
      to replicate there). A fully accurate version of this tool would
      step once per actual `handle_sound_slots` invocation (hookable via
      `Nes::run_frame_with_hook`) instead of once per visual frame - not
      yet built.
      **Same session, continued: a real, verified engine now exists for
      high-format (music, slots #$00-#$02) and percussion (slot #$03)
      too** - `sound_code::decode_high_command`/`HighCommand` (the
      semantic decoder, mirroring `decode_low_command`) and `sound_
      engine::MusicSlot`/`step_high` (the frame-by-frame state machine,
      mirroring `SoundSlot`), covering `simple_sound_cmd`'s notes,
      `sound_cmd_routine_00`-`_02`'s mute/config/period-rotate/vibrato/
      pitch-adjust commands, percussion's delay/trigger commands, the
      same `0xFD`/`0xFE`/`0xFF` control-flow as low format (confirmed
      shared - both formats dispatch into the identical `sound_cmd_
      routine_03`), and `calc_cmd_delay`'s real `SOUND_CMD_LENGTH =
      length_multiplier * (low_nibble + 1)` formula. `decode_high_command`
      was hand-verified against two real sounds byte-for-byte before any
      mechanical testing - `sound_26` (TITLE, pulse 1, all of `simple_
      sound_cmd`/`ConfigChannel`/`PeriodRotate`/control-flow) and
      `sound_29` (TITLE percussion) - both matched on the first attempt.
      A new tool, `examples/verify_music_engine.rs` (`contra-nes`), then
      ran the exhaustive per-frame comparison across all 4 music slots
      during real gameplay, and immediately caught a second real
      trigger-address bug distinct from the one `verify_sound_engine.rs`
      found: a multi-slot sound (e.g. `sound_26`'s TITLE theme, which
      spans slots #$00-#$03 via 4 consecutive `sound_table_00` entries
      `0x26`-`0x29`) sets `SOUND_CODE,x = INIT_SOUND_CODE` - the
      *original* triggering code - for *every* slot it touches, not that
      slot's own table entry index (`load_sound_code_entry`, `src/
      bank1.asm:1655-1656`). So peeking `SOUND_CODE` for slot 1 during
      TITLE reads back `0x26`, not `0x27`, even though slot 1 is actually
      running `sound_27`'s data - the verification tool was looking up the
      wrong table entry for 3 of the 4 slots. Fixed by walking consecutive
      `sound_table_00` entries starting at the observed code and picking
      the one whose own embedded slot number matches, the same way
      `play_sound`'s own `$eb`/`$ea` loop does. That fix alone took slot 1
      from 0/552 to 454/552 matched frames, slot 2 from 148/552 to
      522/552, and slot 3 from 0/552 to 498/552; slot 0 (already correct)
      stayed at 466/552, and its 23/23 matched new-note commands is the
      clearest single piece of evidence that the command-decoding and
      `SOUND_CMD_LENGTH` timing logic itself is right - the remaining
      mismatches cluster around the same two already-understood,
      already-documented causes (trigger-frame observation timing, and
      NMI-reentrancy/lag), not new bugs in the engine. Full workspace
      build + test sweep green throughout.
      **Same session: two of the three still-missing data tables are now
      ported too**, both verified byte-for-byte against the real ROM
      before being trusted: `sound_code::NOTE_PERIOD_TBL` (24 real APU
      period values, `note_period_tbl` at CPU `$86D5`) resolves
      `HighNoteSource::Note`'s `pitch_offset` to an actual pitch, and
      `sound_code::PERCUSSION_TBL` (8 sound codes, `percussion_tbl` at
      CPU `$82CD`) resolves `HighNoteSource::Percussion`'s
      `percussion_tbl_index` to which DMC sample or sound_code
      `play_percussive_sound` actually triggers.
      **Follow-up, same session: the volume-envelope table
      (`pulse_volume_ptr_tbl`) is ported and wired into the engine too.**
      It turned out to be a flat, 54-entry pointer table (not split by
      "level" at the code level despite the disassembly's own per-level
      comment grouping - `SOUND_VOL_ENV,x`'s raw value indexes it
      directly), each entry pointing to a separate volume-envelope byte
      stream with a deliberately simple grammar (bytes are volume values
      until a real `0xFF`; the `0xFE` control code is provably dead in
      real data). Extracted programmatically from PRG-ROM (`sound_code::
      PULSE_VOLUME_PTR_TBL`) rather than hand-transcribed, verified
      byte-for-byte against the real ROM's 108 raw table bytes, and
      cross-checked mechanically: all 54 real streams walk cleanly to a
      genuine `0xFF` terminator with zero `0xFE` occurrences, confirming
      the "dead code" claim empirically rather than just trusting the
      disassembly's comment. Real hardware indexes this table with **no
      bounds check** (`lda pulse_volume_ptr_tbl,y`), and slot 4's index
      (the aliased `INIT_SOUND_CODE`) can genuinely exceed the table's 53
      real entries - `sound_code::pulse_volume_ptr_tbl_entry` reproduces
      that unchecked read against raw PRG-ROM bytes rather than panicking
      or clamping, matching real behavior even in that edge case.
      `sound_engine::SoundSlot`'s previously-placeholder sustain path now
      implements `@check_pulse_volume`'s full reachable branch structure
      (envelope-table read, table-exhausted transition, decrescendo
      resume/pin-at-1) and resolves real `PULSE_VOLUME` values instead of
      a symbolic "not yet resolved" marker - see `sound_engine`'s module
      doc comment for exactly which branch (`lower_pulse_volume`) is
      provably unreachable for these two slots specifically, and why.
      Verified against real gameplay: 197/199 sustain-frame `PULSE_VOLUME`
      comparisons matched exactly across a 900-frame session; the 2
      remaining mismatches trace to the same already-documented NMI-
      reentrancy verification-methodology gap, not a bug in the new logic.
      Full workspace build + test sweep green (49 `contra-native` tests,
      32 `contra-nes` tests). Still not started: wiring this same envelope
      path into `MusicSlot` (slots #$00/#$01, where `lower_pulse_volume`
      *is* reachable, unlike the low-format slots), and cross-slot
      channel-priority arbitration.
- [x] **Enemy placement - hard-coded spawns for outdoor levels.**
      `contra-native::enemy_spawn` ports the fixed, same-every-playthrough
      enemy placements each level defines per screen (`docs/Enemy
      Routines.md`'s "Level Enemies" section) - not the *random* soldier
      generation levels 1/3/5/6/7 also do at runtime
      (`exe_soldier_generation`), which is real gameplay logic, not static
      data, and isn't covered here. **Two real mistakes were made and
      corrected while porting this, both worth keeping honest track of:**
      (1) the doc's own diagram for the Y-position byte (`YYYYY AAA`)
      implies `Y = byte >> 3`, but the doc's own worked example only
      reproduces if `Y = byte & 0xF8` instead (top 5 bits, *not* shifted
      down) - the worked example was trusted over the diagram, confirmed
      by a test encoding that exact example; (2) the doc's prose ("the
      first entry in this table is associated to the *second* screen of
      the level") reads as describing a one-entry offset between a level's
      enemy-screen pointer table and its real screens, and a first attempt
      implemented exactly that - but reading the table's raw bytes
      directly showed entry index equals screen index with **no** offset
      (entry 9 resolves to exactly `level_1_enemy_screen_09`'s real
      address), and trusting the prose instead produced a garbage/runaway
      decode by reading one entry past the table's actual end. Both times,
      the fix was the same principle: when documentation and raw ROM bytes
      disagree, the bytes win. Verified via the real pointer-table walk
      (not just a synthetic ROM): level 1's 24 hard-coded enemies decode
      cleanly across its 12 non-empty screens, and screen 9 reproduces the
      doc's worked example exactly end-to-end
      (`cargo run -p contra-nes --release --example extract_enemies`).
      Wired into `contra-extract --dump-enemies <dir>` for all 6 outdoor
      levels; indoor levels (2, 4) use a different, 3-byte fixed format
      this doesn't decode yet (no worked example exists in the docs to
      verify a final pixel-offset formula against, so nothing is shipped
      rather than shipping a guess) - their output files say so plainly
      instead of silently producing nothing.

## A possible shortcut: static recompilation (evaluated, not adopted yet)

Everything above is hand-porting: read the real assembly, translate one
routine at a time, verify each against `contra-nes`. That's slow by
construction - "realistically hundreds more routines" is the honest
scope. [`mstan/nesrecomp`](https://github.com/mstan/nesrecomp) is a
different approach entirely: a **static recompiler** that translates an
*entire* 6502 ROM to C at build time (not an emulator - `JSR` becomes a C
function call, branches become `goto`s), with an interpreter fallback for
anything its function-discovery pass doesn't confidently identify as
code. If it worked well for Contra, it could reach "zero ROM dependency"
for the *logic* side far faster than routine-by-routine porting - the
same payoff this whole document is working toward, via a different route.

**Evaluated, not chosen yet** - this is a real, hands-on finding, not a
plan:

- Its own compatibility table lists UxROM (Contra's mapper) as "Not yet
  supported," but the runtime code (`runner/src/mapper.c`) already
  implements it correctly (bank switching, fixed-last-bank behavior) -
  the table was just stale.
- It genuinely runs against the real Contra ROM without crashing:
  auto-discovery found ~7000 candidate functions on the first pass.
- Found and locally fixed a real bug: the static analyzer's bank-switch
  tracking only followed register A at the `JSR` call site, but Contra's
  bank-switch routine (`set_rom_bank_to_y`, `$C139`) takes the bank
  number in Y - a one-line fix (also check `known_y`) took "bank switches
  detected" from 0 to 794 on Contra's ROM. Likely relevant to any other
  UxROM/Y-convention game trying this tool, not just Contra.
- Built a reusable bridge from this project's own verified extraction
  code to nesrecomp's `[[data_region]]` config (which tells its function
  finder "this is data, not code"): `contra_native::graphics::
  decompressed_len`, `contra_native::supertile::decompress_screen_len`,
  and `contra_native::enemy_spawn::decompress_outdoor_enemy_screen_len`
  (new - mirror their sibling decode functions but return consumed byte
  length instead of decoded content) plus the sound_code walkers already
  built for this project's own extraction pipeline let `contra-nes/
  examples/emit_data_regions.rs` (new, exploratory - not part of the
  native port or any shipped binary) auto-generate accurate data regions
  instead of hand-transcribing addresses. Feeding all of it in dropped
  auto-discovered functions from ~7000 to 5883 as real data got correctly
  excluded, and cross-referencing confirmed several of nesrecomp's own
  flagged "false positive suspects" really were inside verified real data.

**What this is not yet**: a working alternative. A real function count
for a game this size is almost certainly a small fraction of 5883, so
substantial data is still misidentified as code (this project hasn't
extracted collision maps, weapon/bullet tables, or enemy-AI state tables
at all, so `emit_data_regions.rs` can't exclude what it doesn't know
about yet).

**Follow-up: it actually runs the real game.** Built a minimal runner
executable (own `CMakeLists.txt`/`extras.c`, skipping the `recomp-ui`
launcher/watchdog/verify-mode machinery FaxanaduRecomp's reference
project carries - just the documented `game_extras.h` stub interface) via
MSYS2's `ucrt64` toolchain (MinGW gcc + CMake + Ninja + SDL2, no Visual
Studio needed) - real snags along the way, each with a real fix: a
Windows path-length limit hit while working from a deep scratch
directory (relocated to a short path); MinGW's SDL2main static lib needs
listing *before* SDL2 on the link line, not after (GNU `ld` resolves
undefined symbols left-to-right); the runner's `launcher.c` defines a
plain `main()` rather than relying on SDL2's `main`->`SDL_main` macro
rename, so linking against `SDL2main` at all was wrong for this codepath
(`SDL_MAIN_HANDLED` + plain `SDL2` instead). Once linked, the resulting
`ContraRecompExperiment.exe` - the real ROM's ~6700 functions, statically
recompiled to C - **booted the real Contra ROM and ran 274+ real frames of
the actual game loop without crashing**: correctly identified 8 PRG banks/
mapper 2/all three CPU vectors (`RESET=$C001`, matching this project's own
independently-confirmed reset address), opened a real audio device,
and kept running - spurious `BRK`s from still-misidentified data get
logged and silently skipped rather than crashing, and 2 genuine dispatch
misses (calls to addresses the static discovery pass didn't find) fell
through to the interpreter fallback exactly as designed, each logged as a
ready-to-paste `extra_func` config line. This is a real, running,
partially-native execution of Contra's actual code - not proof the port
is "done" (most of what's executing is still the interpreter fallback
for undiscovered functions), but a concrete, replicable answer to "is
this viable at all": yes, further.

**Checked visually - the real title screen renders correctly.** First
attempt (patching in the runner's own normally-disabled periodic
screenshot hook) showed solid black at frames 0/120/240/360/480 - looked
like a real rendering gap, with a plausible lead (Contra's CHR is RAM,
populated at runtime by the exact routine `contra_native::graphics::
decompress` already ports and verified byte-for-byte; if the boot code
calling it wasn't discovered, CHR-RAM would stay blank). Checked that
lead directly before trusting it: `func_C9A1` (`write_graphic_data_to_
ppu`, the graphics loader, at its real CPU address) **is** discovered and
has 3 real call sites in bank 7's recompiled code - the lead was wrong.
Switched to the runner's `NESRECOMP_NT_DUMP` env-gated debug tap (dumps
the raw nametable independent of the presentation path) instead of the
patched screenshot hook, at frame 300 - and it rendered Contra's **real
title screen**, correctly: the Konami logo, the Contra logo, "PLAY
SELECT / 1 PLAYER / 2 PLAYERS", and the real copyright text, all
pixel-legible. The earlier all-black screenshots were a bug in the
ad-hoc patch itself (capturing the wrong buffer or the wrong point in the
frame pipeline), not a real rendering failure - corrected here rather
than left standing now that better evidence exists. This is the
strongest evidence yet that the recompiled build isn't just "not
crashing" - it's correctly executing enough of Contra's real boot
sequence (CHR-RAM population, nametable/attribute writes, palette setup)
to produce the exact real title screen a player would see.

**Chased down, corrected, and narrowed: simulated Start presses don't
advance past the title screen, but input reading itself is confirmed
correct.** Used the runner's scripted-input system (`--script`, plain-text
`HOLD`/`WAIT`/`RELEASE`/`ASSERT_RAM8`/`WAIT_RAM8` commands) to hold
`START`; still the exact same title screen hundreds of frames later.
First lead (`ASSERT_RAM8` on `$00` showing `0x33` instead of `0x10`)
turned out to be a **testing mistake, not a bug** - caught and corrected
rather than left standing: reading `read_controller_state`'s real
assembly (`src/bank7.asm`) shows `$00`/`$01` are only a *scratch backup*
`load_controller_state` uses internally for its own documented double-
read confirmation (a real, deliberate workaround for a genuine NES DMC/
DPCM hardware bug - reads the controller twice and falls back to the
last known-good value if they disagree), not the stable value game logic
actually consults. That's `CONTROLLER_STATE_DIFF` (`$F5`, confirmed via
`docs/rom-symbols.txt`) - a one-frame edge signal (sets only on the exact
frame a button transitions released->pressed) several real routines key
off directly (`dec_theme_delay_check_user_input`, `level_routine_07`,
pause handling). Re-tested against *that* address with `WAIT_RAM8`
instead of a fixed-delay `ASSERT_RAM8`, to catch the single right frame:
**`$F5` correctly reads `0x10` at exactly the expected frame** the moment
`START` starts being held. Input reading, the DMC-bug double-read
workaround, and edge detection are all confirmed correct in the
recompiled code - a real, positive, tested result, not just a ruled-out
false lead. Re-checked the screen at frame 600 (300 frames after the
confirmed-correct edge) anyway: still the identical title screen. The
blocker is now narrowed to somewhere *after* input detection - whichever
game-state-machine logic is supposed to react to a confirmed-correct
Start signal and change what's on screen isn't doing so (undiscovered/
misexecuting function, a missing precondition this session doesn't know
about yet, or something else) - a real, narrower target for whoever
continues this, not a guess about where the gap is.

**Root-caused and fixed - and it's much bigger than the title screen.**
Added a temporary diagnostic (`extras.c`'s `game_post_nmi` hook, printing
`GAME_ROUTINE_INDEX`/`$18` - the real top-level game-state-machine index,
confirmed via `docs/rom-symbols.txt` and cross-checked directly against
`src/bank7.asm`'s `exe_game_routine`/`game_routine_pointer_table`) and
found the real bug: `GAME_ROUTINE_INDEX` was incrementing by exactly 1
*every single frame* from boot, regardless of any real game condition -
while `GAME_ROUTINE_INIT_FLAG` and the delay timer it should gate on sat
frozen the entire time, proving the individual `game_routine_XX` handlers
weren't running their real logic at all. The cause: `run_routine_from_
tbl_below` (`$C857`) is the classic 6502 "read the return address off the
stack, treat it as an inline jump-table base, jump to `table[A]`" trick -
the table lives as raw bytes immediately after the `JSR` that calls it,
and the routine never really returns to its caller. Contra uses this
*shared helper* for four different major systems (`run_game_routine`,
`run_level_routine`, `run_player_state_routine`, `adjust_bullet_
velocity`), all silently broken the same way: naively recompiled, there's
no real 6502 stack to read a "return address" from, so it read garbage
and jumped somewhere undefined every time - explaining not just the
frozen title screen but likely a meaningful share of the false-positive/
dispatch-miss noise seen throughout this whole evaluation. Fixed with a
single `[[inline_dispatch]] addr = 0xC857` config line (pointing at the
*shared helper's own address*, not each of the four call sites
individually - confirmed via the generated code, which now emits a real
`switch (g_cpu.A) { case 0: ...; case 1: ...; }` with the correct,
discovered function addresses for every entry instead of a broken direct
call). Verified with the same diagnostic: `GAME_ROUTINE_INDEX` now stays
correctly fixed at `1` for exactly 255 frames while `HORIZONTAL_SCROLL`
(`$FD`) counts up one per frame and wraps - the real intro scroll-in
animation, bit-for-bit the shape the disassembly describes - then
transitions cleanly through the real "PLAY SELECT" wait state. **Replayed
the scripted Start-press test against this fixed build: it worked.**
`GAME_ROUTINE_INDEX` advanced 3 -> 4 -> 5 in response to the real Start
edge, `game_routine_04`'s `init_score_player_lives` ran, and the
nametable dump at frame 900 shows **real, correct level 1 (jungle)
gameplay terrain** - mountains, water, grass-covered rock platforms, and
a power-up item box, matching real Contra's actual opening level exactly.
This is no longer just "boots and shows a title screen": a real,
previously-blocking bug is now understood, fixed, and confirmed to
unblock actual gameplay-level content, from a single-line config change
once the real root cause was found - not a guess, not a partial
workaround.

Whether to keep pushing this track, treat it as a complement to
hand-porting, or set it aside remains an open call.

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
