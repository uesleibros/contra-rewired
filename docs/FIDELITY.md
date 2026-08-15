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
  directly. A column the level genuinely hasn't shown yet no longer renders
  as backdrop - it falls back to a live VRAM read wrapped to whatever real
  tile happens to land there, which is always *some* actual NES tile, just
  not necessarily the right one that far from the live camera window (a
  wide window with black gaps read as more broken than an occasionally-
  wrong tile does, and Contra was never going to be pixel-perfect in a mode
  it wasn't built for anyway - see ROADMAP.md). Only cache *hits* and safe-
  margin live reads get written back to the cache, so a wrapped guess is
  never remembered; once the level actually shows that position for real,
  the cache overwrites the guess with the correct tile instead of staying
  stuck with it. `wide_width` can now go up to `MAX_WIDE_WIDTH` (1024px) instead
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
    delta (much bigger than any real scrolling speed), since the absolute
    coordinate space it's keyed against no longer means anything consistent
    once that happens - *and*, separately, every transition from background
    rendering off to on (`Ppu::write_register`, PPUMASK), since a scroll
    delta alone missed the case where a new level starts at roughly the
    same scroll position the old one ended at (see the stage-select section
    below for the real bug this gap caused and how it was found).
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

### Stage select: works - but it took three tries to actually verify that

The Debug tab lets you jump directly to any of the 8 stages. It works now.
Getting to "works" took three separate conclusions, two of them wrong, and
the story of both wrong ones is worth keeping here since the mistakes
themselves are the useful lesson.

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

That was the second conclusion. It was wrong too - not about the CPU-side
state machine (that part held up), but about rendering. Shipped as
clickable on the strength of that verification, real play surfaced
persistent tile flicker and colliding/overlapping tiles after a jump,
severe enough to call the game unplayable afterward. The second
verification's screenshots were correct as far as they went; they just
weren't frequent enough to go far enough. They were taken every ~200-300
frames, which is exactly the kind of gap the widescreen direction-bias bug
above had already burned this project on once: real-time instability that
only shows up frame-to-frame is invisible to a snapshot every few seconds,
because each individual snapshot can land on a moment that looks fine.
Stage select's snapshots kept landing on fine moments. Nothing in between
them was ever looked at.

A third pass, this time capturing *every* frame (`SAVE_EVERY=1`, no gaps)
across the jump and well into the stage afterward, with widescreen on at
700px (the setting most likely to exercise `Ppu::tile_cache`, since that's
what it exists for), found the real cause: `tile_cache` was only ever
cleared on a *big single-frame scroll jump* (see the true-ultrawide section
below). A stage jump landing back near horizontal scroll 0 - which is
where most Contra levels start, including the one the old level was
probably also near the start of - doesn't necessarily produce a big scroll
delta at all, so the old level's cached tiles survived the transition
untouched. From then on, some screen columns kept showing the *previous*
level's tiles (served from the stale cache, which is trusted forever once
populated - a cache hit never gets re-verified against a live read) while
neighboring columns correctly showed the *new* level's tiles (live-read on
a cache miss), and which columns fell into which camp shifted as the
player moved and more cache entries filled in. That's the flicker and the
"colliding" look: two different levels' geometry, interleaved column by
column, neither side ever fully winning.

The fix doesn't live in `contra-pc` at all - it's in `Ppu::write_register`
(PPUMASK, register 1): the cache is now also cleared on every transition
from background rendering *off* to *on*. That's the standard signal every
NES game already relies on for hiding a VRAM rewrite mid-transition (title
screens, game-overs, and Contra's own level-load fade all mask rendering
off while they rewrite nametables/CHR, then flip it back on once the new
screen is ready) - so it catches every real screen change, including this
one, without depending on scroll math guessing right. Re-verified the same
way afterward: every-frame captures across the jump, widescreen on, two
different target stages - clean both times, background fully filled with
no flicker or stale-tile ghosting in either case.

The corrected lesson, again: a spaced-out sample can only tell you the
moments it happened to land on were fine. It can never tell you the gaps
were fine too. Anything claiming to fix or verify frame-to-frame rendering
stability needs every-frame evidence, not sampled evidence - this is now
the third time that exact gap has produced a wrong "looks fine" conclusion
on this project (see the widescreen bias bug and the widescreen-not-
filling-window bug above).

### Stage select: Base 1 and Base 2 hang - found and fixed

Broadening verification after the tile-cache fix (checking every stage, not
just the two that had been spot-checked) found a second, unrelated problem:
jumping to stage 2 or stage 4 (1-indexed - "Base 1"/"Base 2" in-game, index
`1`/`3` internally) doesn't render wrong, it never renders *at all*. The
game sits on the loading screen (background rendering off, plain backdrop
color) forever - not slow, not eventually-recovers, actually forever: RAM
was traced 700+ frames past the jump and stayed on `level_routine=$02`
throughout, `PPUMASK` never flipped rendering back on again. Every other
stage (0, 2, 4, 5, 6, 7 - now all individually confirmed) reaches real,
correctly-rendered gameplay within a few hundred frames of the jump, same
as the two originally verified.

Diagnosing this without a local disassembly to read went as far as: added
`RAM_DUMP_FRAME=N` to `dump_frames.rs` (writes the full 2KB RAM to a
`.bin` file at a given frame, for byte-for-byte diffing two runs with
`cmp -l` when a RAM trace alone can't localize what's different) and
compared a stuck run's own RAM at frame 900 against the same run at frame
1590 - 690 frames, well over 11 real seconds, apart. Every byte in
`$0000-$00FF` and `$0300-$07FF` was bit-identical between the two
snapshots; the *only* difference anywhere in RAM was call-depth noise in
the `$0100-$01FF` hardware stack. That rules out a slow-moving counter or
timer stalling somewhere in the cleared range - the CPU isn't waiting on
anything that increments, it's parked in a genuine infinite loop that
touches no RAM outside the stack, meaning whatever condition it's spinning
on either lives outside the ranges this jump clears/sets, or depends on
something a `poke_ram` can't establish at all (mapper/bank state tied to
that specific level's data, most likely, though unconfirmed). A follow-up
binary search - re-running the jump four times, each time *skipping* the
RAM clear over one quarter of `$0040-$00F0` to see if preserving that
quarter's old contents was the missing piece - came back negative on all
four quarters, ruling out "one leftover byte in the range we already
clear" as the cause too.

**Update: consulted the real disassembly, still unresolved.** The
RAM-address comments elsewhere in this codebase come from general
knowledge, not a vendored copy of the source - so this round actually
fetched `vermiceli/nes-contra-us` (the real, public, annotated Contra US
disassembly) and read `level_routine_02` directly:

```
level_routine_02:
    jsr decrement_delay_timer  ; decrement timer (sets/clears zero flag)
    bne draw_the_scores_1      ; jump if the timer has not yet elapsed
    jsr zero_out_nametables    ; timer elapsed - reset nametables
    jsr load_level_graphics    ; load level graphics
    ...
    inc LEVEL_ROUTINE_INDEX    ; advance to level_routine_03
```

`decrement_delay_timer` just counts down `DELAY_TIME_LOW_BYTE`/
`DELAY_TIME_HIGH_BYTE` (`$2a`/`$2b`, set to `#$c0`/unused by
`level_routine_01`) - nothing in this code branches on level type
(indoor/Base vs. outdoor); the dispatch table and the generic
`run_routine_from_tbl_below` jump helper that reaches it are also
level-agnostic. So the disassembly doesn't explain the hang by itself - it
just narrows where to look next. Re-ran the RAM diff at single-frame
granularity (`$2a`/`$2b` specifically, frame N vs. N+1 while stuck): both
bytes bit-identical frame to frame, meaning `decrement_delay_timer` isn't
being reached at all for these two levels, not that its countdown is
merely running slowly (which the earlier 690-frame-apart diff was already
consistent with, but didn't rule out a many-frames-per-tick wraparound
case - the consecutive-frame check does rule that out). That rules out
"the timer bytes are wrong", but not "the level_routine dispatcher itself
isn't reaching `level_routine_02`'s code for some other reason" - which
needed a real CPU trace of the frozen loop's actual program counter to
confirm, not something `dump_frames.rs`'s black-box RAM-diffing alone
could do. Shipped disabled (greyed out, with an honest tooltip) for these
two stages while the other six remained real and working, as a known,
documented gap rather than a silently-accepted one.

**Resolved: added a real PC trace, found a genuine emulation-adjacent
data bug.** `dump_frames.rs`'s RAM-diffing could only ever say *what*
wasn't changing, never *why* - answering that needed to see the actual
instruction stream, which `Nes::run_frame`'s frame-at-a-time granularity
can't expose. Added `Nes::run_frame_with_pc_trace` (`contra-nes`, a real,
reusable addition - not a throwaway script): identical to `run_frame`, but
calls a closure with the CPU's `pc` before every single instruction that
frame. `dump_frames.rs`'s `PC_TRACE_FRAME=N` env var uses it to build a
histogram of instruction counts per address and print the busiest ones -
the loop a CPU is stuck in is, definitionally, whatever addresses that
histogram is dominated by.

Run against the stuck state, it wasn't subtle: five addresses
(`$cc70`-`$cc78`) accounted for ~1859 of that single frame's instructions,
with the CPU still parked at `$cc70` at the very end of the run. Extracting
the raw ROM bytes at that address (UxROM's fixed bank is the ROM's last
16KiB, mapped straight to `$c000-$ffff`, so the file offset is directly
computable) and decoding them by hand matched, byte for byte, this stretch
of `bank7.asm`:

```
@set_PPU_write_address:
    ldy $00
    inx
    lda CPU_GRAPHICS_BUFFER,x
    sta PPUADDR
    inx
    lda CPU_GRAPHICS_BUFFER,x
    sta PPUADDR
@write_loop:                  ; <- $cc70, where the CPU was stuck
    inx
    lda CPU_GRAPHICS_BUFFER,x
    sta PPUDATA
    dey
    bne @write_loop
    dec $01
    bne @set_PPU_write_address
    inx
    bne @write_to_ppu          ; more data? go again - only exits on a $00 byte
```

This is the routine that flushes `CPU_GRAPHICS_BUFFER` (`$0700`, per
`ram.asm`) to the PPU - graphics/supertile data queued up by the level-load
code. It walks the buffer with an 8-bit index (`x`) and keeps going,
`inx`-ing past whatever block it just wrote, until it happens to read a
`#$00` byte back at the top (`@write_to_ppu`'s `beq @reset_graphics_buffer`
check - see that routine's own doc comment in `ram.asm`, which documents
`$0700 == #$00` as the literal "done" signal). Since `x` is 8 bits, it can
only ever address `$0700-$07ff` before wrapping back to the start of the
same 256 bytes - so if *nothing* in that entire page happens to be `#$00`
at a position the check lands on, the loop cannot mathematically terminate:
it just keeps re-reading the same bytes forever. That's exactly what was
happening. A one-byte poke of `CPU_GRAPHICS_BUFFER[0]` alone (mirroring
`@reset_graphics_buffer`'s own reset code) turned out not to be enough,
because `level_routine_00`'s legitimate graphics-loading code overwrites
that buffer with the new level's real data before the hang - the fix had
to guarantee a `#$00` exists somewhere in the buffer's full reachable
range, not just clear whatever was there before the jump.

The fix, in `apply_menu_action`'s `JumpToStage` handler (`main.rs`) and
`dump_frames.rs`'s `JUMP_STAGE` hook: also zero `$0700-$07bf`
(`CPU_GRAPHICS_BUFFER` plus the reserved bytes after it, stopping short of
`PALETTE_CPU_BUFFER` at `$07c0` and the high-score bytes past that - real
persistent state, not transition scratch) along with `GRAPHICS_BUFFER_OFFSET`
(`$21`) and `GRAPHICS_BUFFER_MODE` (`$23`) - two more bytes this jump
never used to touch, since they're below `$40` and outside its original
clear range. Re-verified every stage individually (`level_routine`
reaches `$04`, real gameplay, and `PPUMASK` shows rendering re-enabled for
all eight, not just six) and re-ran the widescreen every-single-frame
check from the tile-cache fix above against the previously-broken Base 1 -
clean, fully filled, no flicker. All 8 stages are real and clickable in
the Debug tab now; `menu::JUMP_BREAKS_STAGE` is gone.

### Stage select: making the (real, accurate) transition instant

Independent of either bug above, real play surfaced a UX problem with the
six jumps that do work: they're not fake or sped up in any way, so they
cost the same 30-60 real seconds - score flash, palette/graphics load,
supertile render, all with background rendering switched off - that a
genuine level-complete transition always costs in this game, because it's
executing the exact same unmodified game code. That's accurate to the
original, but reads as broken in a PC port: staring at a rendering-disabled
loading screen for the better part of a minute with no progress indicator
looks exactly like a freeze, whether or not it eventually resolves.

Fixed by not showing that transition at all. `apply_menu_action`'s
`JumpToStage` now: snapshots state (`Nes::snapshot`), applies the same
pokes as before, then drives the transition itself by calling
`nes.run_frame()` in a tight loop - un-presented (no frame reaches the
screen), silent (audio samples taken and discarded each iteration so a
multi-second backlog doesn't dump into the speakers as one burst on the
next real frame), and with no mod events fired (these are the game's own
internal loading frames, not real gameplay frames a mod should see) -
until `LEVEL_ROUTINE_INDEX` reaches `4` (real gameplay resumed) or a
generous frame cap is hit. Since the loop runs as fast as the host CPU
allows rather than paced at 60fps, completing 30-60 *simulated* seconds of
Contra's own loading code takes a small fraction of a real second on
current hardware - the jump reads as instant, and the loading screen -
accurate as it is - is never actually shown to the player. Hitting the cap
without reaching real gameplay restores the pre-jump snapshot instead of
stranding the player on a screen that isn't coming back; this also serves
as an automatic backstop for any jump target beyond the two known-broken
ones (Base 1/Base 2) that might turn out to hang too.

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
