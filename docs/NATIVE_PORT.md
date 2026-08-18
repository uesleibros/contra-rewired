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
- [x] **`quadrant_aim_dir::get_quadrant_aim_dir`**
      (`crates/contra-native/src/quadrant_aim_dir.rs`) - ported from
      `get_quadrant_aim_dir` (`bank7.asm`, `$f55e`-`$f5ab`): the core
      "which way should this enemy aim at its target" routine - computes
      the absolute Y/X distance to a target (a real `sec`/`sbc`/`bcs`
      idiom for unsigned-subtract-with-sign-tracking, ported as
      `abs_diff_with_borrow_flag`), buckets each into a coarse row/column,
      and extracts a nibble-packed angle from one of 3 real 32-byte
      tables (`quadrant_aim_dir_00`/`_01`/`_02`, `$f5b2`-`$f611`, hand-
      transcribed as plain `[u8; 32]` consts - unlike `initialize_enemy`'s
      property table, these have an unambiguous, fully-documented row/
      col/nibble layout with no contradictory comments, so transcription
      was safe here). Unit-tested (same-position edge case, both quadrant
      bits independently, a hand-traced row/column bucketing example, and
      the bit-5-of-distance nibble-selection switch). Live-verified
      (`VERIFY_QUADRANT_AIM_DIR=1`, hooking the real entry `$f55e` and the
      routine's single real exit `$f5ab`, right before the `quadrant_aim_
      dir_lookup_ptr_tbl` label): 16 real calls across a 9000-frame
      session, with real, observed variety - tables 0 *and* 1 (table 2,
      the level 3 dragon boss's seeking arm, wasn't reached this session -
      noted honestly), aim directions `1`/`2`/`5`/`6`, and **all 4 real
      quadrant values** - zero mismatches. Not yet integrated live, same
      status as every routine above.
- [x] **`quadrant_aim_dir::get_quadrant_aim_dir_for_player`** (same
      module) - ported from `get_quadrant_aim_dir_for_player` (`bank7.
      asm`, `$f52c`-`$f55d`): `get_quadrant_aim_dir`'s real caller one
      level up - resolves *which* player position to target: the
      requested player if `PLAYER_STATE` says normal, else their
      teammate if *that* player is normal, else a fixed off-screen
      fallback (`Y=$ff, X=$80`) if neither is - then delegates to
      `get_quadrant_aim_dir`. Unit-tested (all 3 resolution outcomes,
      plus the indoor-only Y-position override leaving X alone). Live-
      verified (`VERIFY_QUADRANT_AIM_DIR_FOR_PLAYER=1`, hooking real entry
      `$f52c` - before `player_index` moves out of `a` - and the same
      shared exit `get_quadrant_aim_dir` itself uses, since this routine
      has no `rts` of its own): 16 real calls across a 9000-frame
      session, and notably **the riskiest branch - neither player
      normal, full off-screen fallback - was genuinely exercised live**
      (player 1's `PLAYER_STATE` toggled between `1` normal and `2` not-
      normal across the session; player 2 was always inactive/`0` in
      this single-player run, so whenever player 1 wasn't normal the
      real routine fell all the way through to the fixed fallback
      position) - zero mismatches on either outcome. The middle "fall
      back to teammate" branch wasn't observed live this session (needs
      an active, normal player 2 while player 1 isn't - not reachable in
      a single-player run) - noted honestly, unit-tested only so far.
      Not yet integrated live, same status as every routine above.
- [x] **`create_enemy_bullet_if_attack_enabled` / `aim_and_create_enemy_bullet`**
      (`crates/contra-native/src/create_enemy_bullet.rs`, extends the
      module above) - `create_enemy_bullet_if_attack_enabled` (`bank7.
      asm`, `$f2d8`-`$f2e3`) is a small refactor pulling the attack-flag
      gate that used to be inlined in `create_enemy_bullet_angle_a` out
      into its own function (matching the real ASM, where it always was
      a real, separately-named, shared routine both real callers jump
      into) - no behavior change, `create_enemy_bullet_angle_a`'s own
      tests still pass unmodified. `aim_and_create_enemy_bullet` (`$f29e`-
      `$f2b3`) is the real top-level "aim at a target and spawn a bullet"
      entry point: resolves aim via `quadrant_aim_dir::get_quadrant_aim_
      dir`/`get_quadrant_aim_dir_for_player` (always through table 1),
      merges the result with a pre-shifted bullet type, then delegates to
      `create_enemy_bullet_if_attack_enabled`. Unit-tested (both real aim
      paths cross-checked against calling `get_quadrant_aim_dir`/`_for_
      player` directly, and the attack-flag gate). **Live verification
      gap, documented honestly in this module's own doc comment**: this
      routine has exactly one real caller in the whole ROM
      (`dragon_arm_orb_fire_projectile`, the level 3 dragon boss's arm-orb
      attack) - not reachable by this project's current scripted
      playthrough, so `VERIFY_AIM_AND_CREATE_ENEMY_BULLET=1` had 0 real
      hits this session. Confidence rests on unit tests plus every
      building block already being independently live-verified. A
      genuinely interesting side-finding from tracing that one real
      caller: it always passes an aim-target value with bit 7 clear, so
      this routine's own "aim at an already-known fixed position" branch
      (bit 7 set) appears **unreachable by any real code in the ROM** -
      ported faithfully anyway, flagged rather than removed, the same
      treatment this crate already gives `adjust_bullet_velocity`'s speed
      code 8 and the dead-code `clear_enemy` top label.
- [x] **`player_enemy_distance::player_enemy_x_dist` / `player_enemy_y_dist`**
      (`crates/contra-native/src/player_enemy_distance.rs`) - ported from
      `player_enemy_x_dist`/`player_enemy_y_dist` (`bank7.asm`, `$ecf5`-
      `$ed4b`, sharing a tail at `lda_closer_distance`, `$ed24`): the
      single most-reused routine ported in this crate so far - 21 real
      `jsr player_enemy_x_dist` call sites alone, across nearly every
      enemy AI routine in `bank0.asm`. Computes each player's absolute
      distance to the enemy on one axis (`u8::abs_diff`, equivalent to
      the real `sec`/`sbc`/overflow-flip idiom), overrides a non-normal
      player's distance with a sentinel (`$fe` p1, `$ff` p2 - deliberately
      unequal so p1 wins a tie when *both* are inactive, matching the
      disassembly's own documented behavior), then picks whichever is
      smaller. Unit-tested (both players active, both directions of
      "closer", each single-player-inactive fallback, and the documented
      both-inactive tie-break). Live-verified (`VERIFY_PLAYER_ENEMY_
      DIST=1`, hooking both real entries `$ecf5`/`$ed0e` and their one
      shared exit `$ed4b`): 207 real calls across a 9000-frame session -
      by far the largest sample of any port in this crate - zero
      mismatches, **after finding and fixing a bug in the verification
      harness itself** (it had the X-axis and Y-axis branches reading
      each other's `SPRITE_X_POS`/`SPRITE_Y_POS` addresses - a mistake in
      the hook code, not in the ported routine; caught immediately by the
      first run showing real, non-trivial mismatches, then confirmed
      fixed by the harness fix alone making all 207 pass with no other
      change). Not yet integrated live, same status as every routine
      above.
- [x] **`find_far_segment::find_far_segment_for_a` / `find_far_segment_for_x_pos`**
      (`crates/contra-native/src/find_far_segment.rs`) - ported from
      `find_far_segment_for_a`/`find_far_segment_for_x_pos` (`bank7.asm`,
      `$ed4c`-`$ed66`): buckets an X position into a 0-6 "horizontal
      segment" code by scanning 7 ascending thresholds tightest-first.
      **No live verification this time** - both real callers found
      (`create_roller`, `grenade_launcher_routine_01`) are indoor/base-
      level-only enemies, unreachable by this project's current
      scripted playthrough (level 1, outdoor) - documented honestly in
      this module's own doc comment rather than silently skipped.
      Confidence instead rests on an exhaustive test cross-checking
      *every one of the 256 possible inputs* against an independently-
      written re-implementation of the real 6502 loop's own control flow
      (not just this port's more idiomatic version) - the first port in
      this crate verified this way instead of via `contra-nes` gameplay
      capture, appropriate for a small pure table lookup with a fully
      enumerable input domain. Also unit-tested: hand-traced bucket
      boundaries (confirming `<` not `<=` at each threshold) and the
      real ASM's own "shouldn't happen" safety-fallback case
      (`x_pos=$ff`, where even the loosest threshold isn't satisfied).
- [x] **`add_scroll_to_enemy_pos::add_scroll_to_enemy_pos`**
      (`crates/contra-native/src/add_scroll_to_enemy_pos.rs`) - ported
      from `add_scroll_to_enemy_pos`/`add_horizontal_scroll` (`bank7.
      asm`, `$e8a7`-`$e8c6`, sharing a "remove" tail with `remove_enemy`/
      `set_sprite_0`, `$e809`-`$e813`): **the single most-reused routine
      found in this codebase** - 82 real call sites (75 in `bank0.asm`,
      7 in `bank7.asm`) - applied every frame to essentially every
      enemy/bullet object to keep its position in sync with camera
      scroll and flag it for removal once far enough off-screen. Only
      touches one axis per call (`LEVEL_SCROLLING_TYPE` selects
      horizontal-subtract-X or vertical-add-Y) - the actual removal
      side effect (`remove_enemy` zeroing `ENEMY_ROUTINE`/`ENEMY_
      SPRITES`) is *not* ported here, only the `should_remove` decision
      a caller would need to act on - noted explicitly in this module's
      own doc comment. Unit-tested (both axes, both removal boundaries
      exactly at their real `<`/`>=` cutoffs, and the real 6502 wrap-
      around-on-underflow behavior ported faithfully rather than
      "fixed"). Live-verified (`VERIFY_ADD_SCROLL_TO_ENEMY_POS=1`,
      hooking real entry `$e8a7` and all 3 real exits - vertical/no-
      removal `$e8b8`, horizontal/no-removal `$e8c6`, and the shared
      removed-tail `$e813` - comparing both the resulting position *and*
      which exit fired against this port's own `should_remove`
      prediction): **4427 real calls across a 9000-frame session - by
      far the largest live-verification sample of any port in this
      crate** - zero mismatches. Not yet integrated live, same status as
      every routine above.
- [x] **`update_enemy_pos`** (`crates/contra-native/src/update_enemy_pos.rs`)
      - ported from `update_enemy_pos`/`update_enemy_x_pos`/`update_enemy_
      y_pos` and their `_with_scroll` variants, plus `remove_enemy`
      (`bank7.asm`, `$e809`-`$e969`): the enemy-object analog of
      `player_physics::integrate_y_position` - applies each axis's
      fixed-point velocity to its position (shared integrator, ported
      once as a private `update_axis` and reused by both axes' public
      functions), adds camera scroll to whichever axis matches
      `LEVEL_SCROLLING_TYPE`, and removes the enemy if either axis ends
      up off-screen. **Completes `add_scroll_to_enemy_pos`'s own
      previously-unported `remove_enemy` side effect** (now a real,
      tested `remove_enemy()` function both modules can use). The
      trickiest real control-flow detail, ported faithfully: a removal
      triggered by the *first*-checked axis (X for horizontal levels, Y
      for vertical) skips the second axis's update **entirely** - not
      just its removal check, the whole `jsr` never runs - so this
      port's `removed: Some(..)` case returns the second axis's
      `AxisUpdate` as an exact passthrough of the original input, not a
      computed value; a unit test asserts this explicitly rather than
      just checking the removal flag. Live-verified (`VERIFY_UPDATE_
      ENEMY_POS=1`, hooking real entry `$e837` and both real exits -
      success `$e849`, removed `$e813` shared with `add_scroll_to_enemy_
      pos`): 3732 real calls across a 9000-frame session - the second-
      largest live sample in this crate - zero mismatches. Not yet
      integrated live, same status as every routine above.
- [x] **`add_with_enemy_pos::add_with_enemy_pos` / `set_08_09_to_enemy_pos`**
      (`crates/contra-native/src/add_with_enemy_pos.rs`) - ported from
      `add_with_enemy_pos`/`set_08_09_to_enemy_pos` (`bank7.asm`,
      `$eb2f`-`$eb3f`): adds an offset to an enemy's position without
      modifying the enemy itself, writing the result to the `$08`/`$09`
      scratch pair most of this crate's other aiming/bullet-creation
      ports already take as plain `source_y`/`source_x` parameters
      (`create_enemy_bullet`, `quadrant_aim_dir`). `set_08_09_to_enemy_
      pos` is the zero-offset special case (real ASM literally sets
      `a=0`/`y=0` before falling into `add_with_enemy_pos`) - together,
      **29 real call sites** (21 for the zero-offset form alone), among
      the most-reused small utilities found so far. Unit-tested (offset
      addition, 6502 wraparound, the zero-offset case matching `add_
      with_enemy_pos(0, 0, ..)` exactly). Live-verified (`VERIFY_ADD_
      WITH_ENEMY_POS=1`, hooking both real entries and their one shared
      exit `$eb3f`): 40 real calls across a 9000-frame session, zero
      mismatches. Not yet integrated live, same status as every routine
      above.
- [x] **`enemy_collision_flags`** (`crates/contra-native/src/
      enemy_collision_flags.rs`) - ported from `disable_bullet_enemy_
      collision`/`disable_enemy_collision`/`enable_enemy_player_
      collision_check`/`enable_bullet_enemy_collision`/`enable_enemy_
      collision` (`bank7.asm`, `$eb03`-`$eb1e`): 5 tiny real routines
      toggling two bits of `ENEMY_STATE_WIDTH` (bit 0: player-enemy
      collision checked/skipped; bit 7: bullets pass through or collide)
      - **31 real call sites** combined, reused by nearly every enemy
      type's spawn/death/invulnerability handling. Unit-tested (each
      toggle in isolation confirming it touches *only* its own bit(s),
      plus a disable-then-enable round trip proving non-flag bits
      survive untouched). Live-verified (`VERIFY_ENEMY_COLLISION_
      FLAGS=1`, hooking all 5 real entries and their one shared exit,
      `set_enemy_state_width_to_a`'s own rts at `$eb1e`): 55 real calls
      across a 9000-frame session, with 3 of the 5 real toggles observed
      (`disable_enemy_collision`, `enable_enemy_collision`, `enable_
      bullet_enemy_collision`) - the other 2 weren't reached this
      session, noted honestly - zero mismatches. Not yet integrated
      live, same status as every routine above.
- [x] **`enemy_position_utils`** (`crates/contra-native/src/
      enemy_position_utils.rs`) - ported 5 tiny, widely-reused mutators:
      `add_a_to_enemy_y_pos`/`add_a_to_enemy_x_pos` (`$eb1f`-`$eb2e`, 17
      real call sites combined), `add_10_to_enemy_y_fract_vel`/`add_a_to_
      enemy_y_fract_vel` (`$eb40`-`$eb51`, 10 real call sites combined,
      the first a real ASM fallthrough into the second with `a` preset to
      `$10`), and `reverse_enemy_x_direction` (`$e91e`-`$e92f`, 8 real
      call sites, the same 16-bit two's-complement negation
      `bullet_physics::negate16` already implements - made `pub(crate)`
      and reused rather than duplicated). Unit-tested (wraparound on both
      position adders, carry-into-fast on the velocity adder, the
      $10-preset relationship, and a double-reversal round-trip). Live-
      verified (`VERIFY_ENEMY_POSITION_UTILS=1`, hooking all 5 real
      entries and each one's own real exit): 385 real calls across a
      9000-frame session - `add_a_to_enemy_y_pos` (8 calls) and `add_a_to_
      enemy_y_fract_vel` (377 calls, the large sample also covering every
      real `add_10_to_enemy_y_fract_vel` call, since that entry point
      falls straight through into the same code) - zero mismatches.
      `add_a_to_enemy_x_pos` and `reverse_enemy_x_direction` weren't
      reached this session, noted honestly. Not yet integrated live, same
      status as every routine above.
- [x] **`enemy_routine_transition`** (`crates/contra-native/src/
      enemy_routine_transition.rs`) - ported Contra's enemy state-machine
      transition primitives, **the most-reused routines found in this
      codebase**: `advance_enemy_routine` (`$e78e`-`$e796`, 75 real call
      sites alone), `set_enemy_routine_to_a` (`$e81a`-`$e822`, 29 real
      call sites), and `set_enemy_delay_adv_routine` (`$e78b`-`$e78d`, a
      real fallthrough into `advance_enemy_routine`, 29 more). All three
      share one real guard: an enemy slot whose `ENEMY_ROUTINE` is
      already `0` never gets a new value (only `ENEMY_SPRITES` gets
      cleared, via the shared `set_sprite_0` exit) - real ASM comment:
      "enemy routines are off by one, so setting `ENEMY_ROUTINE` to
      `#$03` results in the 2nd routine being run", i.e. `0` means "no
      active routine", not "routine index 0". Unit-tested (the guard in
      both directions for both `advance`/`set_to_a`, wraparound at
      `0xff`, and `set_enemy_delay_adv_routine`'s own real subtlety - the
      animation-delay store happens *unconditionally*, before the guard,
      unlike the routine update itself). Live-verified (`VERIFY_ENEMY_
      ROUTINE_TRANSITION=1`, hooking all 3 real entries and their real
      exits - careful not to let `$e78b`'s real fallthrough into `$e78e`
      silently drop the delay comparison): 172 real calls across a
      9000-frame session, all 3 entry points genuinely exercised (79
      `Advance`, 71 `DelayedAdvance`, 22 `SetToA`) - zero mismatches. The
      guard-rejection case (`ENEMY_ROUTINE` already `0`) wasn't reached
      this session, noted honestly. Not yet integrated live, same status
      as every routine above.
- [x] **`enemy_position_utils::add_4_to_enemy_y_pos` / `add_a_with_vert_scroll_to_enemy_y_pos`**
      (same module) - a real, *non-trivial* Y adder despite the small-
      utility family it sits in: rather than a plain `pos + a`, it rounds
      `ENEMY_Y_POS` down to the nearest 16-pixel boundary **relative to
      the current `VERTICAL_SCROLL` phase** first (real ASM comment:
      "accounting for `VERTICAL_SCROLL` overflow on vertical levels").
      Unit-tested (a hand-traced worked example, confirming it's not a
      plain add for non-boundary-aligned input, the boundary-aligned
      no-op case, and that a different `VERTICAL_SCROLL` phase genuinely
      shifts the result for the same position). Live-verified
      (`VERIFY_VERT_SCROLL_Y_ADD=1`, hooking both real entries and their
      one shared exit): 17 real calls across a 9000-frame session,
      including a real, observed non-boundary-aligned input (`before=
      $64` under `VERTICAL_SCROLL=$e0` correctly stayed at `$64` after
      adding 4, not `$68` - the rounding step genuinely firing, not just
      passing through boundary-aligned inputs) - zero mismatches. Not yet
      integrated live, same status as every routine above.
- [x] **`soldier::soldier_routine_00`** (`crates/contra-native/src/
      soldier.rs`) - ported from `soldier_routine_00` (`bank0.asm`,
      `$861e`-`$8633`): the soldier enemy's first AI state, run once
      right after spawning - nudges its position down slightly (so it
      visually stands on the ground) and sets a per-attribute initial
      animation delay before advancing to `soldier_routine_01`. **This
      crate's first composed enemy AI state** - every step is a call
      into an already independently-verified building block
      (`add_scroll_to_enemy_pos`, `update_enemy_pos::remove_enemy`,
      `enemy_position_utils::add_4_to_enemy_y_pos`, `enemy_routine_
      transition::set_enemy_delay_adv_routine`), no new arithmetic of
      its own beyond a 4-bit attribute shift and a 4-entry table lookup -
      the real ASM composes the exact same 4 calls. Faithfully ported
      the real "runs regardless" ordering too: even when `add_scroll_to_
      enemy_pos` decides to remove the enemy, the rest of the routine
      (position offset, animation delay store, guarded routine advance)
      still executes - the guard on the last step just naturally rejects
      since removal already zeroed `ENEMY_ROUTINE`. Unit-tested (the
      normal-advance path cross-checked against each composed step
      called directly, all 4 real animation-delay-table entries, and the
      off-screen-removal path's "still runs the rest, guard rejects"
      behavior). Live-verified (`VERIFY_SOLDIER_ROUTINE_00=1`, hooking
      real entry `$861e` and the same 2 shared exits `enemy_routine_
      transition`'s own pass uses): 11 real calls across a 9000-frame
      session, zero mismatches - the first full multi-step enemy AI
      state confirmed correct end-to-end against real gameplay, not just
      its individual pieces in isolation. Not yet integrated live, same
      status as every routine above.
- [x] **`soldier::soldier_routine_01` / `soldier_set_x_velocity` / `soldier_stop_y_set_x_velocity` / `update_enemy_pos::set_enemy_y_velocity_to_0`**
      (`crates/contra-native/src/soldier.rs` and `update_enemy_pos.rs`) -
      ported from `soldier_routine_01` (`bank0.asm`, `$8665`-`$86a3`) and
      its 3 real sub-dependencies (`soldier_set_x_velocity` `$863e`,
      `soldier_stop_y_set_x_velocity` `$8638`, `set_enemy_y_velocity_to_0`
      `$e8d0`): the soldier's "standing, about to start walking" state -
      waits out `ENEMY_ANIMATION_DELAY`, checks for ground beneath it
      (`add_y_to_y_pos_get_bg_collision`, already live-verified above),
      and either removes itself (no valid footing, e.g. a destroyed
      bridge) or plants its X position, re-enables collision, sets its
      walking velocity from `soldier_x_vel_tbl`, and advances to the next
      routine. **Faithfully reproduces a real, easy-to-miss quirk**: on
      the "running left, screen scrolled this frame" path, the real ASM's
      `@continue` label decrements `ENEMY_ANIMATION_DELAY` once, and only
      if that *doesn't* reach zero falls through into a *second*
      decrement of the same counter in the same call - every other path
      (vertical levels, horizontal with no scroll this frame, running
      right on an odd frame) decrements once, and running right on an
      even frame doesn't decrement at all. Ported as `SoldierRoutine01Outcome::DelayNotYetZero`'s
      `decremented_twice` flag rather than silently normalizing to a
      single decrement. Unit-tested (all 4 decrement-path shapes, the
      table lookup in `soldier_set_x_velocity` against all 4 real
      `soldier_x_vel_tbl` entries, the no-floor removal, and both running-
      directions' advance tail, including the running-right `ENEMY_X_POS`
      snap to `$0a`). **Live-verified** (`VERIFY_SOLDIER_ROUTINE_01=1`,
      hooking real entry `$8665` and 3 real exits - `soldier_routine_exit`
      `$865c`, and the 2 shared exits `soldier_routine_00`'s own pass
      uses, `$e796`/`$e813`): 31 real calls across a 25000-frame session,
      zero mismatches - critically, this sample included the tricky
      double-decrement path firing repeatedly (`decremented_twice: true`,
      confirmed via temporary debug instrumentation, since removed) as
      well as the full ground-check/collision-enable/velocity-set advance
      tail, both matching real hardware exactly. One real subtlety
      required disambiguating two different meanings of the same PC:
      `$865c` (`soldier_routine_exit`) is *also* the address of `soldier_
      set_x_velocity`'s own `rts`, hit mid-flight on the advance path via
      a real nested `jsr` inside `soldier_stop_y_set_x_velocity` - the
      verification hook tells the two apart by peeking the 6502 return
      address on the stack (the nested call's return address is always
      `$863a`, which a genuine routine exit can't coincidentally match).
      Not yet integrated live, same status as every routine above.
- [x] **`collision::get_bg_collision_far` / `floor_get_next_row_bg_collision` / `read_bg_collision_byte_unsafe`**
      (`crates/contra-native/src/collision.rs`, extends the module
      `bg_collision` already lives in) - ported from `get_bg_collision_
      far`/`floor_get_next_row_bg_collision`/`read_bg_collision_byte_
      unsafe` (`bank7.asm`, `$e087`-`$e0ba`): a "look one supertile
      half-row further down" floor upgrade on top of the already cycle-
      exact-verified `bg_collision` - purely composing it with `bg_
      collision_scratch` (which already exposed exactly the `$12`/`$13`
      scratch this needed) and one small new byte-decode helper, no new
      arithmetic beyond that. Unit-tested (every real branch: floor
      upgrades to solid, floor stays floor, non-floor codes pass through
      untouched, and the nametable-high-bit-preserving wraparound at the
      `BG_COLLISION_DATA` offset's edges). **Live-verified indirectly**:
      this routine's real callers are all enemy-specific "about to walk
      into a wall" checks, none reachable by a direct hook of their own
      within this project's scripted level-1 playthrough - but `soldier_
      routine_02_jumping`'s own live-verification pass (see below)
      exercises this exact floor-lookahead logic through `check_enemy_
      collision_solid_bg` (confirmed identical to this function), giving
      it 96 real zero-mismatch calls by the time that routine's port
      landed. Not yet integrated live, same status as every routine
      above.
- [x] **`collision::add_a_y_to_enemy_pos_get_bg_collision` / `add_y_to_y_pos_get_bg_collision`**
      (same module) - ported from `add_a_y_to_enemy_pos_get_bg_collision`/
      `add_y_to_y_pos_get_bg_collision` (`bank7.asm`, `$ec33`-`$ec48`):
      offsets an enemy's position by `(x_offset, y_offset)` *without
      modifying its real position* and checks background collision
      there - purely composing the already cycle-exact-verified `bg_
      collision` once the offset position is computed. **Confirmed by
      real CPU addresses, not assumed**: `get_enemy_bg_collision`
      (`$e0bd`) - the entry point this jumps into - turned out to be
      only 2 bytes after `get_bg_collision` (`$e0bb`) in the same fixed
      bank, i.e. it's the *same* underlying collision logic entered one
      step later (skipping the `sta $13` `get_bg_collision` already did
      itself), not the separate collision subsystem an initial reading
      of the disassembly's local line ordering suggested. Faithfully
      ported the real Y-overflow early exit ("exit if overflow, i.e.
      enemy Y position is off-screen towards bottom") as a direct
      `CollisionCode::Empty`, skipping `bg_collision` entirely rather
      than clamping or wrapping. Unit-tested (offset composition matches
      calling `bg_collision` directly at the pre-offset position, the Y-
      overflow early exit skips the data buffer entirely, and the zero-
      offset special case). Live-verified (`VERIFY_ADD_Y_POS_BG_
      COLLISION=1`, hooking both real entries and both real exits - the
      early Y-overflow exit and the same shared success exit `bg_
      collision`'s own verification relies on): **3262 real calls across
      a 9000-frame session - the third-largest live sample in this
      crate** - zero mismatches. Not yet integrated live, same status as
      every routine above.
- [x] **`soldier::soldier_routine_02_jumping` / `set_soldier_sprite` / `soldier_change_direction` / `soldier_apply_vel_check_solid_collision` / `collision::check_enemy_collision_solid_bg`**
      (`crates/contra-native/src/soldier.rs` and `collision.rs`) - ported
      from `soldier_routine_02` (`bank0.asm`, `$86af`-`$8709`, **jumping
      sub-path only** - see below), `set_soldier_sprite` (`$891a`),
      `soldier_change_direction` (`$87cb`), `soldier_apply_vel_check_
      solid_collision` (`$8794`), and `check_enemy_collision_solid_bg`
      (`$ec27`, confirmed mathematically identical to the already-ported
      `get_bg_collision_far` rather than duplicated - a zero-offset `add_
      a_y_to_enemy_pos_get_bg_collision` immediately fed into the same
      floor-lookahead). Composes the soldier's jump-landing check (reuses
      `add_y_to_y_pos_get_bg_collision`, `add_4_to_enemy_y_pos`,
      `soldier_stop_y_set_x_velocity`, `add_10_to_enemy_y_fract_vel` -
      all already verified) with a new shared tail: bail to `soldier_
      routine_09` if embedded in solid ground, otherwise (up to twice a
      second) probe 8px ahead and turn around if that's solid, update the
      sprite from a 12-entry table, and apply velocity/scroll via `update_
      enemy_pos`.
      **Only the jumping sub-path (`ENEMY_VAR_3 != 0`) is ported** - the
      walking/firing-decision/ledge-detection sub-path is deliberately
      **not** ported yet: `get_soldier_num_bullets` (used there) computes
      `adc $08` with no preceding `clc`, meaning its result depends on the
      carry flag inherited from well outside this routine (traced back
      through 6 flag-preserving instructions to whatever the *caller* of
      `soldier_routine_02` left carry as) - this needs to be captured
      empirically from real hardware (`cpu` status flags at the call
      site) rather than guessed, left for a follow-up pass rather than
      risking a silently wrong port of the RNG-driven bullet count or
      jump-off-ledge velocity selection.
      Unit-tested (27 new tests: the sprite table lookup and gun-recoil
      countdown, direction-flip/turn-counting, the solid-at-own-position
      bailout, the var_4-gated and off-screen-clamped ledge probe, the
      turn-around case, and all 4 real landing shapes - still-rising,
      solid, water, and checked-but-nothing).
      **Live-verified** (`VERIFY_SOLDIER_ROUTINE_02_JUMPING=1`, hooking
      real entry `$86af` gated on `ENEMY_VAR_3 != 0`, and 3 real exits -
      `$e849` `apply_vel_exit`, plus the 2 shared exits earlier soldier
      routines already use, disambiguated from the water-landing case's
      own nested `jsr set_enemy_routine_to_a` return the same way `soldier_
      routine_01`'s hook disambiguates its shared exit): **96 real calls
      across a 25000-frame session, zero mismatches** - including the
      water-landing case's own routine switch and the full ground-check/
      collision-enable/velocity-set advance tail, both matching real
      hardware exactly. (An earlier pass of this same hook briefly showed
      96/97 with one unexplained mismatch; that turned out to be a bug in
      the *verification harness itself*, not this port - see the "$8000-
      $bfff is a switchable bank window" note under `VERIFY_SOLDIER_
      ROUTINE_03` below for the root cause and fix, which applies equally
      here.) Not yet integrated live, same status as every routine above.
- [x] **`soldier::soldier_routine_03` / `bullet_generation` / `set_soldier_sprite_add_scroll_01`**
      (`crates/contra-native/src/soldier.rs`) - ported from `soldier_
      routine_03` (`bank0.asm`, `$8803`-`$8863`): the soldier's "try and
      fire a bullet" state - crouches or stands based on `ENEMY_ATTRIBUTES`
      bit 3, waits out `ENEMY_ATTACK_DELAY`, then either fires one of
      `ENEMY_VAR_3` remaining bullets (computing its spawn position from a
      per-direction/per-stance offset table and bailing without even
      attempting a spawn if that position is off-screen) or, once all
      bullets are spent, resets state and returns to `soldier_routine_02`.
      Composes `create_enemy_bullet_angle_a` (already ported and unit-
      tested, from an earlier session) through the one-instruction real
      caller-side transform `bullet_generation` (`$f2be`, a bare `asl`).
      **No `RANDOM_NUM`/inherited-carry dependency anywhere in this
      routine** (unlike `soldier_routine_02`'s still-unported walking
      sub-path) - every branch is a plain, deterministic bit test or
      unsigned comparison, verified by re-reading the full real ASM
      instruction-by-instruction rather than assumed. Caught one real bug
      during that re-reading, before it ever reached a test: the gun
      recoil timer (`ENEMY_VAR_1`) is stored *immediately before* falling
      into the shared `set_soldier_sprite_add_scroll_01` tail, so `set_
      soldier_sprite` (which reads and decrements `ENEMY_VAR_1` as part of
      its own logic) sees the *freshly-set* `$06` on the same call a
      bullet fires, not whatever `ENEMY_VAR_1` was on entry - an early
      draft threaded the original input straight through instead.
      Unit-tested (14 new tests: crouch vs. standing frame/collision-box
      selection, the waiting path, all-bullets-fired reset, both off-
      screen-abort directions via the real unsigned-overflow arithmetic,
      a successful spawn with correct slot/position/recoil threading, and
      the no-free-slot decline case).
      **Live-verification uncovered a real bug in the verification
      harness itself, not this port**: `bank0.asm` (where every `soldier_
      routine_0N` lives) occupies the switchable `$8000-$bfff` UxROM
      window, only actually mapped there when the mapper's `bank_select()
      == 0` - the hook was originally missing that gate, so it also
      "verified" 212 calls firing at `$8803` while a *different* bank
      was mapped (confirmed by reading the real bytes there: `85 ea a5`
      - a plain `sta $ea`, not this routine's actual `bd a8 05` - `lda
      ENEMY_ATTRIBUTES,x` - with the enemy slot register `x` holding
      garbage values as large as `32`, past the real 16-slot range).
      Fixed by gating all four `soldier_routine_0N` entry hooks
      (`$861e`/`$8665`/`$86af`/`$8803`) on `bank_select() == 0`; this
      also retroactively explained `soldier_routine_02_jumping`'s one
      previously-unexplained mismatch (see above) - it vanished
      completely once the same gate was added. With the gate in place,
      `soldier_routine_03` itself had **0 real hits** across a
      45000-frame session - this scripted level-1 playthrough's soldiers
      never happen to get an unobstructed, on-screen shot at the player
      within the captured window - noted honestly rather than claimed as
      live-verified; confidence rests on the unit tests above and on
      every building block this composes already being independently
      live-verified in its own right. Not yet integrated live, same
      status as every routine above.
- [x] **`soldier::soldier_routine_04` / `soldier_routine_05` / `update_enemy_pos::set_enemy_x_velocity_to_0`**
      (`crates/contra-native/src/soldier.rs` and `update_enemy_pos.rs`) -
      ported from `soldier_routine_04` (`bank0.asm`, `$88c3`-`$88ff`,
      "soldier hit, begin destroying soldier") and `soldier_routine_05`
      (`$8900`-`$8939`, "soldier hit, apply negative gravity") plus the
      one small new real dependency, `set_enemy_x_velocity_to_0` (`$e8d9`,
      the X-axis sibling of the already-ported `set_enemy_y_velocity_to_0`).
      `soldier_routine_04` launches the destroyed soldier upward with a
      fixed velocity, zeroing the X component instead if it's near either
      screen edge (real ASM checks *both* `< $10` and `>= $f0` into the
      same zeroing step) and reversing it if the soldier was facing right
      (the fixed velocity is authored assuming left-facing, the same
      convention `soldier_x_vel_tbl` uses). `soldier_routine_05` applies
      gravity every call and either advances immediately if the soldier
      drifted off the top of the screen (skipping `update_enemy_pos`
      entirely - a real, faithfully-reproduced short-circuit) or updates
      its position and advances once its animation delay elapses. Both
      compose entirely already-verified building blocks (`disable_enemy_
      collision`, `reverse_enemy_x_direction`, `add_scroll_to_enemy_pos`,
      `set_enemy_delay_adv_routine`, `add_a_to_enemy_y_fract_vel`,
      `update_enemy_pos`, `advance_enemy_routine`, `set_soldier_sprite`) -
      no new arithmetic beyond the edge-zeroing bit test and the off-
      screen check. Unit-tested (10 new tests: mid-screen/both-edge X
      velocity handling, the right-facing reversal composing correctly
      with edge-zeroing down to zero, and all 3 real `soldier_routine_05`
      outcomes).
      **Live verification attempted but had 0 real hits for either
      routine** across a 25000-frame session - both are only reached once
      a soldier is actually shot and killed, and this project's current
      scripted level-1 playthrough (a walk-forward capture) never happens
      to do that within the captured window, even though soldiers
      themselves are confirmed present and active (`soldier_routine_02_
      jumping`'s own 96 real calls prove that) - noted honestly rather
      than claimed as live-verified; confidence rests on the unit tests
      above and on every composed building block already being
      independently live-verified in its own right. Not yet integrated
      live, same status as every routine above.
- [x] **`soldier::soldier_routine_09` / `soldier_routine_0a` / `soldier_set_y_pos_sprite_add_scroll`**
      (`crates/contra-native/src/soldier.rs`) - ported from `soldier_
      routine_09` (`bank0.asm`, `$888c`-`$88a0`, "soldier landing in
      water") and `soldier_routine_0a` (`$88a1`-`$88b9`, "continue splash
      animation and begin removing soldier"), plus `soldier_set_y_pos_
      sprite_add_scroll` (`$88ba`). `soldier_routine_09` sets the water-
      splash sprite frame and nudges the soldier `$10`px down into the
      water; `soldier_routine_0a` waits out the splash animation frame by
      frame, removing the soldier once it's played through.
      **Caught a genuinely surprising real-ASM quirk by reading the
      disassembly instruction-by-instruction rather than assuming**:
      `soldier_routine_09` calls `set_soldier_sprite`/`add_scroll_to_
      enemy_pos` **twice**, not once - `jsr soldier_set_y_pos_sprite_add_
      scroll` already falls all the way through that exact pair itself
      (confirmed via real addresses in `docs/rom-symbols.txt`, not just
      the local disassembly text's line order), and the routine's next
      two lines call `jsr set_soldier_sprite`/`jsr add_scroll_to_enemy_
      pos` again, separately - meaning camera scroll is applied to the
      position *twice* and the gun-recoil timer is decremented *twice* on
      this specific call, both faithfully reproduced (see [`crate::
      soldier::SoldierRoutine09Result::second`]'s doc comment) rather
      than "corrected" as a suspected typo. Live verification (below)
      confirms this reading is exactly right.
      Unit-tested (7 new tests, including one asserting the second pass's
      scroll re-applies on top of the first's already-adjusted position
      and the recoil timer decrements twice when it started nonzero).
      **Live-verified**: `soldier_routine_09` - 1 real call across a
      25000-frame session, zero mismatches (this transition only fires
      once per water-landing enemy, so a low sample count is expected;
      what matters is that the one real call, including its double-call
      quirk, matched exactly). `soldier_routine_0a` - 16 real calls, zero
      mismatches. Not yet integrated live, same status as every routine
      above.
- [x] **`update_enemy_pos::enemy_routine_remove_enemy`**
      (`crates/contra-native/src/update_enemy_pos.rs`) - ported from
      `enemy_routine_remove_enemy` (`bank7.asm`, `$e806`-`$e808`): a real,
      *shared* enemy-routine-table entry - not soldier-specific, used by
      dozens of enemy types across the ROM as their "scroll then remove
      this enemy" terminal state. Composes `add_scroll_to_enemy_pos`
      (already verified) with the already-verified `remove_enemy`,
      keeping the real ASM's position-writing side effect even though its
      *result* is otherwise discarded. This crate's completion of the
      plain soldier's entire routine table (`soldier_routine_ptr_tbl`,
      11 entries: `00`-`05` and `09`-`0a` are soldier-specific and now
      all ported; the remaining 3 entries, `enemy_routine_init_explosion`/
      `enemy_routine_explosion`/`enemy_routine_remove_enemy`, are this
      same kind of shared entry - only the last of the three is ported so
      far, the other two need `play_sound` ported first).
      Unit-tested (2 new tests: the composition matches calling both
      pieces directly, and the scrolled position is kept even when that
      scroll's own internal check would have triggered removal too).
      **Live-verified** (`VERIFY_ENEMY_ROUTINE_REMOVE_ENEMY=1`, hooking
      real entry `$e806` and its one real exit, `$e813` - disambiguated
      from a nested return through the same address via the real `jsr
      add_scroll_to_enemy_pos`'s own internal removal path, the same
      stack-peek technique used throughout this session): **24 real
      calls across a 25000-frame session, zero mismatches** - a solid
      sample for a routine shared this widely. Not yet integrated live,
      same status as every routine above.
- [x] **`enemy_explosion::enemy_routine_init_explosion`** (`crates/
      contra-native/src/enemy_explosion.rs`, new module) - ported from
      `enemy_routine_init_explosion` (`bank7.asm`, `$e74b`-`$e75d`):
      another real, shared enemy-routine-table entry (the plain
      soldier's own entry 6, among dozens of other enemy types) - marks
      the enemy destroyed, optionally triggers the destruction sound
      (`sound_19`), re-palettes its sprite to palette 2, and either
      removes it immediately (no sprite left) or hides it for one frame
      before the real explosion animation (`enemy_routine_explosion`,
      still not ported - needs more of `show_explosion_a`'s own branches
      worked out) takes over. `play_sound` (`$c16b`) itself is
      deliberately **not** ported as a function - it's a real bank-switch
      wrapper around the sound engine (`jsr load_bank_1; jsr init_sound_
      code_vars; jsr local_previous_1_bank`), not a pure RAM transform
      like everything else in this crate; this port instead returns
      *whether and which* sound code would fire as plain data (`Option<u8>`),
      the same way `create_enemy_bullet` returns a bullet's fields rather
      than performing the spawn - a caller integrating this into live
      gameplay is responsible for actually invoking the sound engine.
      Real, faithfully-reproduced detail: the sound-trigger check tests
      the *new* `state_width` (after the unconditional `|= $81`), not the
      original input.
      Unit-tested (6 new tests: the unconditional destroyed-bits set, the
      sound gate on both sides, the palette override preserving other
      bits, and both real outcomes).
      **Live-verified** (`VERIFY_ENEMY_ROUTINE_INIT_EXPLOSION=1`, hooking
      real entry `$e74b`, `play_sound`'s own real entry `$c16b` while a
      call is pending - to confirm the `sound` field against real
      hardware despite `play_sound` itself not being ported - and the 2
      shared exits, disambiguated the same way as `enemy_routine_remove_
      enemy`'s hook): **32 real calls across a 25000-frame session, zero
      mismatches**, including the sound-code check. Not yet integrated
      live, same status as every routine above.
- [x] **`enemy_explosion::enemy_routine_explosion` / `show_explosion_a`**
      (`crates/contra-native/src/enemy/enemy_explosion.rs`) - ported from
      `enemy_routine_explosion` (`bank7.asm`, `$e7b0`-`$e7bb`, the plain
      soldier's own routine-table entry 7) and the shared animation
      driver it falls into, `show_explosion_a` (`$e7bc`-`$e805`, also used
      by `roller_routine_04`/`shared_enemy_routine_03`, not ported here):
      cycles through one of 4 real fixed sprite-code sequences
      (`explosion_type_ptr_tbl`, `$e823` - `EXPLOSION_TYPE_00`-`03` in
      this port), one frame every `$0a` game-frames, disabling collision
      right before the *last* frame, then advancing to the next real
      routine once the sequence finishes. Composes already-verified
      pieces (`add_scroll_to_enemy_pos`, `disable_enemy_collision`,
      `advance_enemy_routine`) with one small new table-lookup. Faithful
      to a real subtlety: `enemy_routine_explosion` always passes `$00`
      for `show_explosion_a`'s own explosion-type override, so the type
      actually used is derived from `ENEMY_STATE_WIDTH` bit 3 a *second*
      time inside `show_explosion_a` itself - both checks read the same
      bit, so they can't disagree for this specific caller, but the port
      keeps the real two-step structure rather than collapsing it.
      Unit-tested (14 new tests: all 4 outcomes, the override-bypasses-
      state-width case, and both derived-type branches).
      **Live-verified** (`VERIFY_ENEMY_ROUTINE_EXPLOSION=1`, hooking real
      entry `$e7b0` and 3 real exits - `show_explosion_a`'s own dedicated
      `$e805`, plus the 2 shared exits, `$e813` disambiguated from a
      nested return through the routine's own early `jsr add_scroll_to_
      enemy_pos` the usual way): **1312 real calls across a 25000-frame
      session, zero mismatches** - by far the largest live sample so far
      this session, unsurprising given how constantly *something* is
      exploding in this game. Not yet integrated live, same status as
      every routine above.
- [x] **`red_blue_soldier::red_blue_soldier_routine_00`** (`crates/
      contra-native/src/enemy/red_blue_soldier.rs`, new module) - ported
      from `red_blue_soldier_routine_00` (`bank0.asm`, `$a157`-`$a17d`),
      entry 0 of both `blue_soldier_routine_ptr_tbl` and `red_soldier_
      routine_ptr_tbl` - this project's **first enemy type beyond the
      plain soldier**, though it already reuses the plain soldier's
      shared explosion/removal routine-table entries (`enemy_routine_
      init_explosion`/`enemy_routine_explosion`/`enemy_routine_remove_
      enemy`, all already ported). Places the enemy at one of 4 fixed
      screen corners and gives it an initial horizontal running velocity,
      both picked from `ENEMY_ATTRIBUTES`, then advances to the next
      routine - pure table lookups plus the already-verified `advance_
      enemy_routine`, no new arithmetic. Real ASM doesn't mask
      `ENEMY_ATTRIBUTES` before indexing the 4-entry position table (only
      the 2-entry velocity table gets an explicit `and #$01`); this port
      masks the position index defensively too, the same reasoning
      `soldier::soldier_set_x_velocity` already documents for an
      analogous case - every real spawn placement for this enemy type
      uses attributes `0`-`3` exactly, so this is unreachable in
      practice, not a behavior change.
      Unit-tested (all 4 real attribute values' corner/direction pairs,
      and the guarded routine-advance behavior).
      **Live verification attempted but had 0 real hits** across a
      25000-frame session - this enemy type doesn't appear in this
      project's current scripted level-1 playthrough - noted honestly
      rather than claimed as live-verified; confidence rests on the unit
      tests above. Not yet integrated live, same status as every routine
      above.
- [x] **`red_blue_soldier::blue_soldier_routine_01` / `_02` / `_03` / `red_blue_soldier_set_run_frame` / `red_blue_soldier_set_bg_priority`**
      (`crates/contra-native/src/enemy/red_blue_soldier.rs`) - completes
      the blue soldier's own routine table beyond entry 0 (`red_blue_
      soldier_routine_00`, ported above): `blue_soldier_routine_01`
      (`$a18a`-`$a19f`, run across the screen, then check real proximity
      to a player before committing to the jump-attack), `_02` (`$a1f7`-
      `$a240`, jump-attack windup animation, then set jump velocity from
      direction), and `_03` (`$a245`-`$a266`, fall under gravity showing
      one of two sprites). Plus the 2 small helpers both blue *and* red
      soldiers share: `red_blue_soldier_set_run_frame` (`$a1c5`, cycles
      the run-cycle animation every 4th frame) and `red_blue_soldier_set_
      bg_priority` (`$a1db`, forces background draw priority near either
      screen edge so the soldier draws behind pillar/wall decorations
      there). Composes already-verified pieces (`update_enemy_pos`,
      `player_enemy_x_dist`, `enable_enemy_collision`, `add_10_to_enemy_
      y_fract_vel`, and `set_enemy_delay_adv_routine` reused directly for
      the real ASM's own local duplicate, `set_anim_delay_adv_enemy_
      routine_01` - mathematically identical code at a different bank0
      address, the same "thin alias" reasoning already used for `check_
      enemy_collision_solid_bg` and `set_soldier_sprite_add_scroll`).
      **Corrected a misleading real-ASM comment during porting, not just
      followed it**: `red_blue_soldier_set_bg_priority`'s own branch
      comment says `bcs @continue` is taken "if to the right of `$dc`
      (not behind pillar)", but tracing pure control flow (branch
      targets, not comment text) shows `@continue` is exactly the
      *behind-pillar* path (matching that label's own separate comment
      two lines later) - this port follows the traced control flow, and
      says so in its own doc comment rather than silently trusting either
      comment.
      Unit-tested (15 new tests: the run-frame cycle/wrap, both bg-
      priority edges plus bit-preservation, all of routine_01's outcomes
      including the pre-override-frame sprite subtlety, all of routine_
      02's 3 outcomes, and routine_03's zero-delay/nonzero-delay sprite
      choice).
      **Live verification attempted but had 0 real hits for all 3**
      across 25000-frame sessions - same reason as `red_blue_soldier_
      routine_00` above (this enemy type isn't present in the current
      scripted playthrough). Not yet integrated live, same status as
      every routine above.
- [x] **`red_blue_soldier::red_soldier_routine_01` / `red_soldier_routine_02`**
      (`crates/contra-native/src/enemy/red_blue_soldier.rs`) - completes
      the red soldier's own routine table (it shares entry 0, `red_blue_
      soldier_routine_00`, and the 2 running/bg-priority helpers, with
      the blue soldier, all ported above). `red_soldier_routine_01`
      (`$a266`-`$a29f`) runs across the screen, then once inside a real X
      trigger range checks real proximity to a player - with a minimum
      attack distance itself picked from `ENEMY_ATTRIBUTES` bit 1 (`$10`
      or `$30`, not a single fixed value) - before committing to fire.
      `red_soldier_routine_02` (`$a2bb`-`$a2fd`) fires up to 3 bullets via
      the already-verified `aim_and_create_enemy_bullet`, one every `$30`
      frames, stripping a recoil sprite-attribute bit at one specific
      real point in the cycle (`ENEMY_ATTACK_DELAY == $2c` exactly) before
      returning to `red_soldier_routine_01` once all 3 are spent.
      `play_sound`-style non-port: `aim_and_create_enemy_bullet` was
      already ported in an earlier session and needed no changes.
      Unit-tested (9 new tests: `red_soldier_routine_01`'s already-fired
      short-circuit, both X-range exits, the attack commit, and the bit-
      1-driven distance widening; `red_soldier_routine_02`'s exact-`$2c`
      recoil strip, the plain-wait no-op case, a successful bullet fire,
      and the all-bullets-fired transition).
      **Live verification attempted but had 0 real hits for both**
      across 25000-frame sessions - same reason as the rest of this
      enemy family. Not yet integrated live, same status as every
      routine above.
- [x] **`red_blue_soldier::red_blue_soldier_gen_routine_00` / `_01`**
      (`crates/contra-native/src/enemy/red_blue_soldier.rs`) - the level
      4 boss-screen generator that spawns both soldier types, completing
      this enemy family end to end (generator through both soldier
      types' full routine tables). `_00` (`$a304`) just sets the initial
      generation delay. `_01` (`$a309`-`$a366`) is the real interesting
      one: once its delay elapses, it reads through a **hand-authored
      28-byte spawn script** (`red_blue_soldier_data_tbl`, `$a368`) that
      can spawn **multiple soldiers in a single call** - every consecutive
      positive byte spawns one (via the already-verified `find_next_
      enemy_slot`/`initialize_enemy`) before the read hits a negative
      byte, which sets the next delay and stops; hitting the table's
      `$ff` terminator wraps the read offset back to `0` and keeps going
      *within the same call*, not next frame. Ported as a bounded loop
      (`Vec<RedBlueSoldierSpawn>` output, one entry per real spawn),
      threading a local mutable copy of the enemy-slot occupancy array
      through it so a slot claimed by the *first* spawn this call is
      correctly seen as taken by `find_next_enemy_slot` if a *second*
      spawn happens right after in the same call - the same kind of
      real, easy-to-miss statefulness already handled in this session's
      other multi-step compositions.
      **Corrected the real ASM's own comments again, not just followed
      them**: they claim the spawn byte's "bits 0, 1, and 2" pick
      `ENEMY_ATTRIBUTES` and "bit 3" picks red/blue, but the actual
      instructions are `and #$03` (bits 0-1 only) and an *unmasked*
      `byte >> 2` (not a single bit) for the color selector - confirmed
      against the table's own per-run comments (`$00`-`$03` marked "red",
      `$04`-`$07` marked "blue", exactly matching `byte & 3` / `byte >>
      2`, not the prose description).
      Unit-tested (11 new tests: both trivial exits, wall-plating
      removal, a full 4-soldier spawn run with slot-order verification,
      slot exhaustion mid-run still advancing the read offset correctly,
      the `$ff` wraparound, and blue-soldier color decoding).
      **Live verification attempted but had 0 real hits for both**
      across 25000-frame sessions - this generator only exists on the
      level 4 boss screen, not reachable from the current scripted
      playthrough. Not yet integrated live, same status as every routine
      above.
- [x] **`indoor_soldier::indoor_soldier_routine_00` / `indoor_soldier_routine_01` / `init_indoor_enemy_pos_and_vel` / `apply_enemy_velocity_set_bg_priority` / `init_sprite_from_frame` / `create_indoor_bullet` / `enemy_launch_grenade` / `create_roller` / `create_roller_with_segment_a`**
      (`crates/contra-native/src/enemy/indoor_soldier.rs`) - this
      project's first step into a **new enemy family**: `indoor_soldier_
      routine_ptr_tbl` (`bank0.asm`, `$92c8`-onward) is actually shared by
      *4 real enemy types* (`$15` indoor soldier, `$16` jumping soldier,
      `$17` grenade launcher, `$18` group of four soldiers), all built on
      the same 3 shared helpers this port carries so far - `init_indoor_
      enemy_pos_and_vel` (`$9697`, places the enemy at one of 2 fixed X
      positions with a per-type initial velocity), `apply_enemy_velocity_
      set_bg_priority` (`$96c1`, the family's X-only velocity integrator -
      indoor enemies never move vertically - with the same "draw behind
      background near either screen edge" shape `red_blue_soldier_set_bg_
      priority` already has), and `init_sprite_from_frame` (`$9316`, the
      run-cycle animation, same 4th-frame cadence as `red_blue_soldier_
      set_run_frame`). Plus the indoor soldier's own first two table
      entries: `indoor_soldier_routine_00` (`$92c8`, "initializes indoor
      soldier: sets position, velocity and attack delay") and `indoor_
      soldier_routine_01` (`$92d5`, waits for `ENEMY_ATTACK_DELAY` then
      fires one of 3 weapons based on `(ENEMY_ATTRIBUTES >> 1) & 3`) -
      the other 3 enemy types' own `_00`/`_01` entries still aren't
      ported.
      `indoor_soldier_routine_01`'s weapon-type branch composes 3 new
      real sub-routines: `create_indoor_bullet` (`$9784`, weapon type
      `0`, gated by both an on-screen X range *and* `ENEMY_ATTACK_FLAG`),
      `enemy_launch_grenade` (`$9743`, weapon type `1` - but real ASM
      only actually launches it every *other* time this branch is
      reached, via an `ENEMY_VAR_1` parity check, effectively doubling
      the attack delay for this weapon specifically), and `create_roller`/
      `create_roller_with_segment_a` (`$9700`/`$9703`, weapon types `2`
      *and* `3` alike - real ASM's `dey; bne @create_roller` only
      special-cases weapon type `1` for the grenade, so type `3` silently
      falls into the same roller path as type `2`, not a distinct 4th
      weapon). All 3 compose the already-ported `find_next_enemy_slot`/
      `initialize_enemy`/`find_far_segment_for_x_pos` pipeline, same
      shape as `create_enemy_bullet`.
      One real quirk ported faithfully rather than "fixed": the roller's
      `ENEMY_ATTRIBUTES` comes from a stale `$0a` scratch byte that
      *nothing* in `indoor_soldier_routine_01`'s own call chain (nor the
      master `exe_enemy_routine_loop` dispatcher) ever writes - modeled
      as an explicit `attributes_scratch` parameter threaded straight
      through rather than guessed at. A second quirk: `apply_enemy_
      velocity_set_bg_priority`'s off-screen removal is a plain `jmp
      remove_enemy` (not `jsr`), so execution returns straight back into
      `indoor_soldier_routine_01` and keeps going (can decrement the
      attack delay and even fire a weapon in the same frame an enemy was
      just removed) - harmless since `ENEMY_ROUTINE` is now `0`, but real,
      faithfully preserved control flow.
      Unit-tested (10 new tests on top of the 9 pre-existing helper/
      `routine_00` tests, 19 total in the module: every real branch of
      all 3 new sub-routines including their own gates/range checks, the
      weapon-type dispatch for all 4 attribute values, the grenade parity
      gate firing and skipping, the stale-`$0a` roller quirk, and the
      attack-delay/range gate).
      **Live verification attempted but had 0 real hits** across an
      1800-frame session - indoor soldiers only appear on indoor/base
      levels, not reachable from the current scripted outdoor level-1
      playthrough. Not yet integrated live, same status as every routine
      above.
- [x] **`indoor_soldier::shared_enemy_routine_00` / `shared_enemy_routine_01`
      / `enemy_explosion::shared_enemy_routine_03`**
      (`crates/contra-native/src/enemy/indoor_soldier.rs` and `enemy_
      explosion.rs`) - the 3 remaining table entries every one of the 4
      indoor-family enemy types ($15-$18) shares verbatim, completing the
      generic portion of `indoor_soldier_routine_ptr_tbl`'s 7 entries
      (only the 2 already-shared explosion entries and these 3 - not
      `_00`/`_01`, which are per-type). `shared_enemy_routine_00`
      (`$9346`, "soldier has been hit by player bullet") composes the
      already-ported `disable_enemy_collision`, `set_enemy_x_velocity_
      to_0`, and `set_enemy_delay_adv_routine` - the last one standing in
      for the real `set_anim_delay_adv_enemy_routine_00` (`$8e77`), a
      bank0.asm-local byte-for-byte duplicate of the same logic, not
      separately modeled. `shared_enemy_routine_01` (`$9360`, "perform
      enemy hit by bullet animation") composes the already-ported
      `update_enemy_pos` and `add_a_to_enemy_y_fract_vel`, applying
      velocity to position *before* adding gravity for next frame,
      matching the real instruction order. `shared_enemy_routine_03`
      (`$e7aa`, "show explosion_type_02") is a one-line wrapper around
      the already-ported `show_explosion_a` with a fixed
      `(explosion_type_override=2, max_sprites=3)` pair - same shape as
      `enemy_routine_explosion` (the plain soldier's own equivalent
      entry), just a different fixed pair and a real, separate call
      site.
      Unit-tested (7 new tests: `shared_enemy_routine_00`'s full
      composition and its guard-rejected case, `shared_enemy_routine_01`'s
      waiting/advancing outcomes plus its position and gravity math cross-
      checked directly against `update_enemy_pos`/`add_a_to_enemy_y_
      fract_vel`, and `shared_enemy_routine_03`'s output matching
      `show_explosion_a(2, 3, ...)` exactly).
      **Live verification attempted but had 0 real hits for all 3**
      across 1800-frame sessions - same as every other routine in this
      family, indoor/base levels aren't reachable from the current
      scripted outdoor level-1 playthrough.
- [x] **`jumping_soldier::jumping_soldier_routine_00` / `jumping_soldier_routine_01`**
      (`crates/contra-native/src/enemy/jumping_soldier.rs`, new module) -
      the jumping soldier's (`$16`) own `_00`/`_01` table entries; the
      other 5 entries of its `jumping_soldier_routine_ptr_tbl` are the
      same shared routines every indoor-family type reuses, already
      ported. `jumping_soldier_routine_00` (`$9380`, "see if red soldier,
      if so mark flag, advance routine") gates `ENEMY_ATTRIBUTES` bit 1
      (the level's one special "red" jumping soldier, which will drop a
      weapon item on death once `jumping_soldier_routine_04` is ported):
      only the first eligible candidate *after* `INDOOR_ENEMY_ATTACK_
      COUNT` has advanced past round 0 actually keeps the bit - round 0
      or a red one already claimed this screen silently demotes it via
      `INDOOR_RED_SOLDIER_CREATED`. Composes the already-ported
      `init_indoor_enemy_pos_and_vel` (logical index `1` - real ASM's
      `ldy #$02` is a raw byte offset into the table's 2-byte entries,
      `2/2=1`) and `advance_enemy_routine`.
      `jumping_soldier_routine_01` (`$93a5`, "set sprite, and perform
      jump animation") always computes the run/jump sprite (3-way cadence
      off `ENEMY_ANIMATION_DELAY`) and the direction-flipped, red-or-
      default-palette sprite attribute, then branches: `ENEMY_ANIMATION_
      DELAY == 0` applies this frame's offset from `jumping_soldier_y_
      vel_tbl` (a 20-entry signed jump arc, `ENEMY_VAR_1`-indexed,
      wrapping back to a `$10`-frame pause once finished) via the
      already-ported `apply_enemy_velocity_set_bg_priority`; otherwise it
      decrements the delay and, unless this is a red soldier (which never
      fires - real ASM gives no reason, ported as-is) and the decremented
      delay lands exactly on `$08`, fires at the closer player via the
      already-ported `player_enemy_x_dist` + `aim_and_create_enemy_
      bullet` (fixed `bullet_type=$60`, `speed_code=4`).
      `jumping_soldier_routine_04` ("soldier destroyed, if red soldier
      play explosion and create weapon item") is **not yet ported** -
      needs `play_explosion_sound`, which itself composes `create_two_
      explosion_89` and a weapon-item-creation chain, none of which exist
      in this crate yet.
      Unit-tested (13 new tests: every red-soldier-claiming branch of
      `_00` including attribute-bit preservation, `_01`'s full sprite
      cadence and palette/flip logic, the red-soldier no-fire exception,
      firing on the exact delay frame, and both jump-arc sub-cases
      including the wraparound reset).
      **Live verification attempted but had 0 real hits for both**
      across 1800-frame sessions - same as the rest of the indoor family,
      not reachable from the current scripted outdoor level-1
      playthrough.
- [x] **`find_far_segment::find_close_segment` / `grenade_launcher::grenade_launcher_routine_00`
      / `grenade_launcher_routine_01` / `grenade_launcher_apply_vel_aim`
      / `launch_grenade_if_appropriate` / `set_enemy_var_2_to_closest_x_player`**
      (`crates/contra-native/src/enemy/find_far_segment.rs` and new
      `grenade_launcher.rs`) - the grenade launcher/"seeking guy" enemy
      type's ($17) own `_00`/`_01` table entries; the other 3 entries of
      its routine table are the same shared routines every indoor-family
      type reuses, already ported. `find_close_segment` (`$967c`) is a
      real, separate routine sharing `find_far_segment_for_a`'s exact
      descending-threshold-scan shape (own 7-byte table, own real
      address) to bucket a *player's* X position instead of an enemy's -
      the scan itself got factored into one shared private helper rather
      than duplicated, even though the real ROM has two separate copies
      of the loop.
      `grenade_launcher_routine_00` (`$9468`) composes the already-ported
      `init_indoor_enemy_pos_and_vel` (logical index `3`) and
      `set_enemy_delay_adv_routine`, plus the new `set_enemy_var_2_to_
      closest_x_player` (`$9516`, resolves the closer player via the
      already-ported `player_enemy_x_dist`, swapping to the other player
      if that one isn't in a normal state - unconditionally, without
      checking the swapped-to player's own state either).
      `grenade_launcher_routine_01` (`$9479`) branches on `ENEMY_VAR_3`:
      `0` delegates entirely to `grenade_launcher_apply_vel_aim` (`$94c7`
      - moves and, once its own animation delay elapses or the enemy ran
      too far past either screen edge to move, compares its segment
      against `ENEMY_VAR_2`'s *already-stored* player to arm a grenade
      count and set a pause length); nonzero is a "cooldown" state that
      either checks `launch_grenade_if_appropriate` (`$94b1`, composes
      the already-ported `enemy_launch_grenade`) or, once its own delay
      elapses, re-resolves the closest player fresh and reverses
      direction if not already facing them.
      One real quirk ported faithfully: `grenade_launcher_apply_vel_aim`'s
      tail computes `(ENEMY_ATTRIBUTES>>1)&3` (the configured grenade
      count) into `a`, then immediately restores the *earlier*
      same-segment comparison's saved flags via `plp` - so the branch
      that follows tests the same-segment result, not whether the count
      is zero (the `and`'s own flags are silently discarded); net effect,
      `ENEMY_VAR_1` becomes the configured count only when the player
      shares the launcher's segment, `0` otherwise.
      Unit-tested (16 new tests across both files: the shared segment-
      scan cross-checked against a direct reference implementation for
      `find_close_segment` too, both `_00` composition, `_01`'s velocity-
      skip-at-screen-edge case, the `ENEMY_VAR_1` quirk verified directly
      against same-segment vs. different-segment inputs, all 3 `launch_
      grenade_if_appropriate` branches, and both top-level `_01` branches
      including the direction-reversal condition).
      **Live verification attempted but had 0 real hits for both `_00`
      and `_01`** across 1800-frame sessions - same as the rest of the
      indoor family, not reachable from the current scripted outdoor
      level-1 playthrough.
- [x] **`four_soldiers::four_soldiers_routine_00` / `_01` / `_02` /
      `four_soldiers_get_delay_offset` / `four_soldiers_set_firing_delay`**
      (`crates/contra-native/src/enemy/four_soldiers.rs`, new module) -
      the "group of four soldiers" enemy type's ($18) own table entries,
      completing every indoor-family enemy type's own `_00`/`_01`(/`_02`)
      logic except `jumping_soldier_routine_04` (still deferred - needs
      the unported weapon-item-creation subsystem). A 3-state cycle:
      `_00` initializes one soldier of the group and its first firing
      delay, then advances to `_01`; `_01` walks until its running delay
      elapses (firing a bullet on the exact frame the delay counts down
      to `$4` along the way - real ASM gives no reason for that specific
      value), decides whether to reverse direction (only soldiers `2`/`3`,
      and only after their first shot - "split soldiers so some go left,
      some go right"), then jumps straight to `_02`; `_02` applies
      velocity/sprite while standing still, and once its own delay
      elapses, sets the firing sprite, counts the shot, and jumps back to
      `_01` directly (`set_enemy_routine_to_a`, not a linear advance).
      `four_soldiers_get_delay_offset`/`four_soldiers_set_firing_delay`
      are shared by all 3 entries to index two 12-byte `(times fired,
      soldier index)` tables (running distance and firing delay).
      Unit-tested (10 new tests: the shared offset/table-lookup helpers,
      `_00`'s composition, `_01`'s waiting/firing/direction-reversal
      branches - including confirming soldiers 0/1 never reverse while
      2/3 do, but only after their first shot - and `_02`'s still-moving
      vs. fired-and-looped-back branches).
      **Live verification attempted but had 0 real hits for all 3**
      across 1800-frame sessions - same as the rest of the indoor family,
      not reachable from the current scripted outdoor level-1
      playthrough.
- [x] **`indoor_soldier_gen::indoor_soldier_gen_routine_00` / `indoor_soldier_gen_routine_01`**
      (`crates/contra-native/src/enemy/indoor_soldier_gen.rs`, new
      module) - the "green guys generator" that actually spawns every
      indoor-family enemy type this project ported over the last several
      commits (indoor soldier, jumping soldier, grenade launcher, group
      of four). Reads a level-and-screen-specific byte stream from raw
      PRG-ROM via a real pointer chase (`indoor_enemy_gen_tbl` -> `lvl_
      (2|4)_enemy_gen_tbl` -> per-screen bytes), same "walk the real
      bytes, don't hand-transcribe" approach `initialize_enemy`/`enemy_
      spawn` already use - up to `$07` "rounds" of attacks per screen
      before the generator removes itself.
      One real quirk worth calling out: the enemy-type bits are decoded
      via `rol;rol;rol;and #$03` rather than a plain shift - `rol`
      rotates through the carry flag, and the carry going into the first
      rotate here is left over from a *much earlier*, unrelated `asl`
      (picking the level-2-vs-level-4 table). Traced by hand assuming
      that carry is always `0` (per the real ASM's own comment that this
      generator's `ENEMY_ATTRIBUTES` never has bit 7 set), the 3 rotates
      are mathematically equivalent to a plain `(byte0 >> 6) & 3` - this
      port uses that simpler, verified-equivalent form.
      Composes the already-ported `find_next_enemy_slot`/`initialize_
      enemy` pipeline for all 4 spawn types, threading a local mutable
      slot-occupancy copy through the group-of-4 loop the same way
      `red_blue_soldier_gen_routine_01` already does, and stopping that
      loop early (not just skipping one entry) the moment a slot isn't
      available, matching the real ASM's own `bne indoor_soldier_gen_
      routine_exit`.
      Unit-tested (11 new tests: every gating branch, all 4 spawn types
      decoded correctly from the same byte stream, the attack-count
      increment and its cap triggering generator self-removal, the
      group-of-4 loop's descending `ENEMY_VAR_1` assignment and its
      early-stop behavior, and a slot-exhausted single-spawn case that
      still updates the generator's own delay/read-offset state).
      **Live verification attempted but had 0 real hits for both**
      across 1800-frame sessions - same as the rest of the indoor
      family, not reachable from the current scripted outdoor level-1
      playthrough.
- [x] **`indoor_roller_gen::indoor_roller_gen_routine_00` / `indoor_roller_gen_routine_01`**
      (`crates/contra-native/src/enemy/indoor_roller_gen.rs`, new
      module) - the indoor family's roller generator, reading its own
      per-generator pattern from raw PRG-ROM (`roller_gen_init_tbl` ->
      `roller_gen_init_00`/`_01`, same pointer-chase approach `indoor_
      soldier_gen` already uses) and composing the already-ported
      `create_roller_with_segment_a` - but unlike every *other* real
      caller of that routine, this one takes the horizontal segment
      directly from the level data's own high nibble rather than
      computing it via `find_far_segment_for_x_pos`. Can spawn multiple
      rollers in a single call, back to back, whenever consecutive
      pattern entries have a `0` delay byte - ported with the same
      bounded-loop approach (64 iterations) `red_blue_soldier_gen_
      routine_01` already uses for its own real-ASM-has-no-hard-limit
      spawn loop, well above the real data's own largest table (`$39`
      bytes).
      Unit-tested (8 new tests: every gating branch, a single-roller
      spawn with the segment/attributes/position decoded correctly from
      the packed byte, back-to-back multi-roller spawns via a `0` delay,
      the `$ff` wraparound sentinel restarting the pattern read from
      offset `0`, and a slot-exhausted/attack-flag-off case that still
      advances the generator's own state).
      **Live verification attempted but had 0 real hits for both**
      across 1800-frame sessions - same as the rest of the indoor
      family, not reachable from the current scripted outdoor level-1
      playthrough.
- [x] **`jumping_soldier::jumping_soldier_routine_04` / `enemy_explosion::play_explosion_sound`
      / `create_two_explosion_89` / `create_explosion_a` / `create_enemy_for_explosion`
      / `create_explosion_sequence`**
      (`crates/contra-native/src/enemy/jumping_soldier.rs` and `enemy_
      explosion.rs`) - the jumping soldier's own routine-4 entry ("soldier
      destroyed, if red soldier play explosion and create weapon item"),
      closing the last gap in this enemy type's own table (all 8 entries
      now ported). Only a red jumping soldier (`ENEMY_ATTRIBUTES` bit 1),
      inside the `$64..$9c` X range, with bit 7 clear (real comment:
      "not sure when this happens" - ported as-is) actually explodes;
      everything else just advances to the next routine.
      `create_explosion_sequence`/`create_explosion_a`/`create_enemy_for_
      explosion`/`create_two_explosion_89` are a small real family (one
      shared core, 3 thin fixed-parameter wrappers) that spawns a new
      "explosion sensor" enemy (`ENEMY_TYPE=$02`, real comment: "isn't
      important, it's just an enemy that has the `enemy_routine_init_
      explosion` routine sequence") at a given position - composes the
      already-ported `find_next_enemy_slot`/`initialize_enemy` pipeline,
      same shape as every other spawn helper in this crate.
      `play_explosion_sound` then converts the *calling* enemy's own slot
      in place into a weapon item drop (`ENEMY_TYPE=0`, `ENEMY_ROUTINE=1`)
      rather than spawning a new enemy for that part - a real, deliberate
      difference from the explosion sensor it also spawns alongside it.
      One instruction-ordering quirk worth calling out: `jumping_soldier_
      routine_04`'s own `lsr ENEMY_ATTRIBUTES,x` (twice, `>> 2`) runs
      *before* the tail-jump into `play_explosion_sound`, which itself
      reads `ENEMY_ATTRIBUTES,x` again and masks it to the low 3 bits for
      the weapon type - so the real weapon type is `(original_attributes
      >> 2) & 7`, not a fresh `& 7` of the original value. Ported by
      threading the already-shifted value through explicitly.
      Unit-tested (12 new tests across both files: every spawn-helper
      wrapper's fixed parameters, `play_explosion_sound`'s full
      composition and its attribute masking, and `jumping_soldier_
      routine_04`'s full gating logic including the X-range's exclusive
      right edge).
      **Live verification attempted but had 0 real hits** across a
      1800-frame session - same as the rest of the indoor family, not
      reachable from the current scripted outdoor level-1 playthrough.
- [x] **`weapon_item::weapon_item_routine_00` / `set_weapon_item_indoor_velocity`**
      (`crates/contra-native/src/enemy/weapon_item.rs`, new module) -
      the first entry of the weapon-item pickup's own routine table
      (`$8007`): sets the marker that lets bullets pass through it
      (`ENEMY_STATE_WIDTH`), its score/collision code, and its initial
      velocity - indoor levels pick an X velocity off the item's own
      horizontal segment (composes the already-ported `find_far_
      segment_for_a`, same shape as the indoor-family roller/grenade
      routines) with a fixed slow fall speed; outdoor levels pick one of
      3 fixed `(y, x)` velocity rows depending on the level's scrolling
      type and, for vertical levels, which half of the screen the item
      spawned on. `weapon_item_routine_01`/`_02` (falling and landing on
      the ground, then watching for the ground to disappear) are **not
      yet ported** - real ASM pulls in a much deeper dependency chain
      from there (`set_outdoor_weapon_item_vel`, `set_enemy_falling_arc_
      pos`, 2 new background-collision helpers, a sprite-selection
      routine), none of which exist in this crate yet.
      Also confirmed directly from `docs/rom-symbols.txt`: `ENEMY_VAR_B`
      and `ENEMY_ATTACK_DELAY` are real, literal aliases for the *same*
      RAM byte (`$558+x`) - a genuine space-saving trick this ROM uses,
      not a disassembly error; this routine's own `ENEMY_VAR_B` write is
      named accordingly rather than borrowing unrelated terminology.
      Unit-tested (5 new tests: the indoor branch's full composition,
      all 3 outdoor velocity rows, and the indoor velocity helper's own
      segment lookup).
      **Live verification attempted but had 0 real hits** across both an
      1800-frame and a 6000-frame session - the real ASM's own comment
      says weapon items are only created after a flying capsule, pill
      box sensor, or (indoor) red soldier is destroyed, *not* from
      regular soldier kills - the current scripted playthrough is a
      plain walk-and-shoot demo that doesn't destroy any of those
      sources, so this is expected, not a sign of a broken hook.
- [x] **`enemy_bullet::enemy_bullet_routine_00` / `_01` / `_02`**
      (`crates/contra-native/src/enemy/enemy_bullet.rs`, new module) -
      the enemy bullet *entity's* own per-frame routine table (`$814f`-
      `$8202`; entry 3, `remove_enemy`, was already ported) - a
      different thing from `create_enemy_bullet`, which only spawns
      one. `ENEMY_VAR_1` selects one of 5 real bullet types: `0` regular
      (removed on solid-bg collision if the level checks for it), `1`
      large cannonball (falls with gravity, explodes into `_02`'s 3-frame
      animation at the ground), `2` a real, documented no-op here, `3`
      indoor regular bullet (on-screen bounds check only, no gravity),
      `4` the level-3 dragon boss's fire ball (recolors/flips every 4
      frames). All 5 share one `update_enemy_pos` call before branching
      on type; level 5 ("snow field") recolors regular bullets red via a
      sprite-table-only index override that doesn't touch the real
      stored bullet type.
      **A real bug live verification caught and this port then fixed**:
      `update_enemy_pos`'s own off-screen removal is a tail `jmp remove_
      enemy`, so it can zero `ENEMY_ROUTINE`/`ENEMY_SPRITES` *mid-call*
      and execution keeps going into the bullet-type branch regardless
      (same quirk this crate has documented before) - but the first
      port of `enemy_bullet_routine_01`'s `Exploded` outcome still fed
      the *entry-time* `current_routine` into `advance_enemy_routine`,
      not the already-zeroed value a same-frame removal leaves behind.
      12 real mismatches on the first live-verification run (all
      `sprites`/`routine` disagreements on frames where a bullet's own
      `update_enemy_pos` call had just removed it) pointed straight at
      this; fixed by deriving an `effective_routine` from `position.
      removed` before any downstream routine-transition call, with a
      regression test covering the exact scenario, and a matching fix to
      the verify hook's own sprite/routine resolution (it needs the same
      "did `update_enemy_pos` already remove this" check the port itself
      now does).
      Unit-tested (16 new tests: the collision table for every real
      bullet type, all 5 branches of `_01` including the snow-field
      override and the same-frame-removal regression case, and `_02`'s
      waiting/animating/advancing outcomes).
      **Live-verified against real gameplay**: `_00` 12 real calls,
      `_01` 610 real calls, both zero mismatches after the fix above
      (across a 6000-frame session). `_02` (the level 1 boss cannonball's
      own explosion animation) had 0 real hits, as expected - not
      reachable before the boss fight in the current scripted
      walk-and-shoot playthrough.
- [x] **`flying_capsule::flying_capsule_routine_00` / `_01` / `_02` /
      `set_flying_capsule_path` / `set_flying_capsule_y_vel` /
      `set_flying_capsule_x_vel`**
      (`crates/contra-native/src/enemy/flying_capsule.rs`, new module) -
      the flying weapon capsule ("weapon zeppelin")'s own routine table
      (`$830b`-`$8376`). Flies a slow, spring-like oscillating path -
      bobbing vertically on horizontal levels, swaying side-to-side on
      the level 3 waterfall's vertical scroll - anchored to wherever it
      first spawned (`ENEMY_VAR_1`/`ENEMY_VAR_2`, captured once by `_00`).
      `set_flying_capsule_path` (the shared core both `_y_vel`/`_x_vel`
      fall into) computes `2 * (position - reference)` as a signed
      16-bit value and subtracts it from a base velocity - a linear
      restoring force that grows the further the capsule has drifted
      from its own anchor point. Real ASM's shift-count parameter
      supports an arbitrary left *or* right shift via two different
      loops, but both real callers here always pass a shift of `1` -
      the right-shift path is real, valid control flow this port still
      models, but isn't independently exercised or verified the way the
      always-used left-shift-by-1 path is. `_02` (explosion + weapon
      item drop on death) is a one-line `jmp play_explosion_sound`,
      already ported.
      Unit-tested (9 new tests: the spring-term math including a
      negative-diff and a zero-shift case, both `_00` position/velocity
      branches, both `_01` oscillation branches cross-checked directly
      against the underlying velocity helpers, and `_02`'s full
      delegation to `play_explosion_sound`).
      **Live-verified against real gameplay**: `_00` 2 real calls, `_01`
      744 real calls, both zero mismatches (across a 6000-frame
      session). `_02` had 0 real hits - the capsule flies off screen
      without being destroyed in the current scripted walk-and-shoot
      playthrough.
- [x] **`sniper::sniper_routine_00`**
      (`crates/contra-native/src/enemy/sniper.rs`, new module) - the
      sniper ("rifle man")'s own initialization entry (`$8958`): picks
      `ENEMY_ANIMATION_DELAY`/`ENEMY_FRAME` from `ENEMY_ATTRIBUTES`'s
      3 real sniper types (standing, crouching/hiding, boss-screen
      hiding), then nudges Y position down - always by `$04` (via the
      already-ported vertical-scroll-aware `add_a_with_vert_scroll_to_
      enemy_y_pos`), plus another plain `$05` for crouching snipers only.
      `sniper_routine_01`-`_05` (crouch-cycle animation, then a real
      bullet-angle-quadrant aiming/firing subsystem built around a new
      `get_rotate_01` dependency and several new sprite/offset tables)
      are **not yet ported** - substantially larger than `_00` alone,
      deferred to a future pass. Note for future work: unlike the
      `weapon_box`/`rotating_gun`/`red_turret` families (all blocked on
      an unported PPU graphics-buffer subsystem, `draw_enemy_supertile_a`
      and its bank-3 nametable-update chain), the sniper family has *no*
      such dependency - `_01`-`_05` are tractable, just large.
      Unit-tested (4 new tests: all 3 sniper types' own delay/frame rows,
      the crouching-only extra nudge, and vertical scroll threading
      through the first position update).
      **Live-verified against real gameplay**: 5 real calls, zero
      mismatches (across a 6000-frame session).
- [ ] Everything else, logic side. One hundred two routines out of what's
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
      doesn't depend on the emulator crate at all.
      **Update: per-tile palette assignment is wired up now (this note was
      stale - it landed as part of the "Super-tiles" work below, which
      also proved the attribute table byte-perfect, but this specific
      bullet was never updated to say so).** Both `extract_level.rs`
      (`crates/contra-nes`) and `contra-extract --dump-levels`
      (`apps/contra-extract/src/level.rs`) read each super-tile's real
      attribute byte (`supertile::supertile_attribute_byte`/`attribute_
      quadrants`) and pick that specific tile's real palette from it,
      not one fixed palette across the whole sheet - see the "Super-
      tiles" bullet immediately below for the byte-perfect live-PPU proof
      of the attribute table itself. Still true as originally noted: only
      level 1's palette indexes have been individually live-verified
      against real PPU state; the other 7 levels use the identical proven
      decode path but aren't separately re-verified.
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
      levels.
      **Update: indoor/base levels (2, 4) are decoded too now, closing
      this asset-extraction gap.** The blocker noted above (no worked
      example in the docs to verify a pixel-offset formula against) is
      resolved differently than originally anticipated: instead of
      needing a documented worked example, `contra_native::enemy_spawn::
      decompress_indoor_enemy_screen` was ported straight from the real
      reader routine (`load_enemy_indoor_level`, `bank2.asm`) and
      verified the same way every CPU-logic routine in this project is -
      against real gameplay, via `VERIFY_INDOOR_ENEMY_SPAWN=1` (`crates/
      contra-nes/examples/dump_frames.rs`) combined with `JUMP_STAGE` to
      reach an indoor level. Reading the real code (not the doc's own
      diagram) also caught two real mistakes the doc alone would have
      produced: the position byte's nibble order is `YYYY XXXX`, not
      `XXXX YYYY` as the doc's diagram implies, and the `C`/`D` position-
      adjustment flags each add **8**, not the `7` their own `adc #$07`
      instruction looks like in isolation (the real addition includes an
      implicit `+1` from a carry the preceding `asl` left set). A third,
      genuine control-flow subtlety was found and root-caused during
      verification itself, not guessed: the *shared* exit both the "no
      data for this screen" check and the ordinary "no more enemies on
      this screen" (a real, expected 0xFF terminator) check jump to is
      the **same** address - the routine's own separate, local `rts` is
      reached only by the edge case of exactly 16 enemies with no
      terminator, which real screens may never hit. One real screen from
      each indoor level (level 2's and level 4's own screen 0) came back
      with zero mismatches. `contra-extract --dump-enemies <dir>` now
      decodes and writes all 8 levels, no exceptions - level 2's real
      output spans 7 screens (41 hard-coded enemies total, screen 0
      matching the live-verified capture exactly, screen 6 a distinct
      16-enemy boss room).

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
