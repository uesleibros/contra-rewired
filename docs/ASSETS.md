# Assets & the legal ROM model

**contra-rewired ships zero Konami-owned bytes.** No graphics, no music, no
sound effects, no level data, no ROM. What's in this repository is:

- an NES emulation core (`crates/contra-nes`) and an engine, both written
  from scratch in Rust - general-purpose console hardware behavior, not
  Contra's game code;
- documentation and a handful of physics/RNG values in `contra-core`
  *verified against* the public
  [vermiceli/nes-contra-us](https://github.com/vermiceli/nes-contra-us)
  disassembly (facts about how the original game behaves - not its code or
  prose, which are that project's own work);
- tooling that reads a ROM **you** already own and dumped yourself, at run
  time, on your own machine, and runs it on the emulation core.

## Graphics/audio decoding: back on, for real this time

Earlier in this project, the stance here was that graphics/audio decoding
wasn't needed because `contra-nes` already decompresses everything live at
run time the same way real hardware does. That was true as far as it went,
but it settled for "a working emulator-based port," not "a real
decompilation-based port" in the Ship of Harkinian/SM64-decomp sense -
**those never touch the original ROM after a one-time extraction step, and
this project has since committed to the same end state** (see
docs/NATIVE_PORT.md's "The actual end state: zero ROM dependency at
runtime"). An emulator that decodes graphics live is still emulating
*something*, even if the CPU logic around it is eventually all native.

So extraction is a real, active workstream again, and `apps/contra-extract`
is where it lives. First slice landed: **graphics.** `write_graphic_data_to_
ppu` - the real routine Contra uses to unpack RLE-compressed CHR pattern
tiles from PRG-ROM - has been ported to native Rust
(`contra-native::graphics`, see its doc comment for the exact format) and
proven correct against real hardware behavior: decoding level 1's graphics
straight from PRG-ROM bytes (no emulation involved at all) and diffing the
result against `contra-nes`'s live CHR-RAM after actually playing into the
level came back byte-for-byte identical across all 8192 CHR bytes
(`cargo run -p contra-nes --release --example extract_graphics`).

**Palettes landed too.** `load_palette_colors_to_cpu`'s `game_palettes`
table plus level-header `LEVEL_PALETTE_INDEX` resolution is ported to
`contra-native::palette`, verified the same way (level 1's background
palette 0 decoded from PRG-ROM matched live PPU palette RAM exactly), and
combining it with the graphics decoder produces a real, correctly-colored
level 1 tile sheet straight from PRG-ROM bytes - no emulation anywhere in
that pipeline.

**Full levels landed too, generalized to all 8.** Contra's super-tile
system (a level's nametable + attribute table, decoded from a *second*
RLE scheme different from the graphics one, plus plain per-super-tile
tile/palette data) is ported to `contra-native::{level,supertile}`, and
proven the same rigorous way: level 1's CHR content, nametable, and
attribute table all independently matched live PPU state after actually
playing into it. Getting this to all 8 levels (not just level 1) meant
reading the ROM's own `level_graphic_data_tbl`/`graphic_data_ptr_tbl`
lookup tables instead of hardcoding per-level data - which caught a real
bug along the way: one byte in that table packs *both* a bank number and
a horizontal-flip flag, and a first attempt misread the flag as part of
the bank number and crashed on level 2. Fixed properly (real bit-reversal
on flipped tile data, not just a crash fix) - which also fixed a **silent
mirrored-wrong tile sheet** `--dump-graphics` had already been shipping
for `graphic_data_10`. All 8 levels now render correctly, verified
visually (level 2's indoor corridors and doors, level 3's waterfalls, the
`graphic_data_0a`/`_10` pair rendering as genuine mirror images of each
other).

**Audio's first slice landed too: DPCM samples.** Contra's 2 raw DPCM
(delta pulse-code modulation) waveforms - real audio data, not a
bytecode - are ported to `contra-native::audio` and decode straight from
PRG-ROM to WAV. The address/length encoding and delta-decode algorithm
are standard 2A03 DMC hardware behavior, not Contra-specific, so what's
actually ported is `dpcm_sample_data_tbl`, the small table saying which
PRG bytes are sample data. `contra-nes`'s APU doesn't emulate the DMC
channel, so there's no live playback to diff against here - instead, the
computed byte ranges were diffed against the disassembly's own
separately-shipped copies of the same samples, and came back identical.

**Enemy placement landed too, for outdoor levels.** Each level's
hard-coded, same-every-playthrough enemy spawns (not the *random* soldier
generation levels also do at runtime, which is gameplay logic, not
static data) are ported to `contra-native::enemy_spawn` and decode
straight from PRG-ROM. Getting this right took correcting two real
mistakes along the way - a Y-position bit-layout read that matched the
docs' diagram but not their own worked example, and a pointer-table
lookup taken from the docs' prose that produced a garbage decode - both
times, the actual ROM bytes turned out to disagree with the
documentation's prose/diagrams, and the bytes won. Verified against the
docs' worked example through the real pointer-table walk, not just a
synthetic test.

**Sound effects' and music's actual bytecode is fully extracted now, not
just the DPCM waveforms.** Following an explicit "extract everything,
nothing missing" directive: the DPCM samples above were only ever part of
the story - the game's music and sound effects are really driven by a
custom bytecode Contra's CPU interprets frame by frame, and that bytecode
itself hadn't been touched. `contra-native::sound_code` now ports **all
three** of its sub-grammars - low format (sound effects), high format
(music), and percussion - covering all 94 of `sound_table_00`'s entries,
verified by hand against several real sounds' bytes before any code was
trusted, then mechanically against every entry at once. Two real bugs
were caught and fixed in the process (an incorrect assumption that
high-format commands had runtime-dependent lengths, and an early
implementation that reused low format's narrower control-command trigger
condition for high/percussion format, which would have silently
corrupted every music track's parsing past its first `0xF0`-`0xFC` byte),
plus genuine structural findings that turned out correct rather than
buggy (`sound_08`'s repeat command targets its own start address,
looping part of itself; `sound_2a`'s targets partway into its own
already-scanned range instead, retracing its own tail to the same
terminator). Wired into `contra-extract --dump-sound-codes <dir>` - all
94 sound codes, 232 distinct blobs, from the real ROM.

**A playback engine - the piece that actually turns this bytecode into
sound - has genuinely started too**, but it's a much bigger undertaking
than extraction, and only the first slice is done:
`contra_native::sound_code::decode_low_command` decodes low-format bytes
into their real meaning (note pitch/volume, config commands), verified
against a real sound effect's full command sequence by hand. Reading deep
enough into the real interpreter to write that showed the actual scope of
what's left: real-time per-frame state (a countdown timer gates when the
next command is read, so this can't be a one-shot decode), a
decrescendo/volume-envelope system reading a per-level table not yet
ported, and priority arbitration across all 6 sound slots competing for 4
physical APU channels - realistically on the order of the *entire* rest
of the CPU-logic-porting workstream, not a small remaining piece.

**A real, steppable engine now exists for the two sound-effect slots.**
`contra_native::sound_engine::SoundSlot` handles trigger initialization,
note-by-note reading, and full `0xFD`/`0xFE`/`0xFF` control-flow
(child-jump/repeat/end), mechanically verified frame-by-frame against
real gameplay rather than hand-picked examples
(`contra-nes/examples/verify_sound_engine.rs`). That verification caught
a real bug in the *tool itself* (it was seeding a sound's start address
from RAM sampled after the trigger frame had already advanced past it -
fixed by resolving the address from `sound_table_00` directly, the same
way the real trigger routine does), and traced the remaining mismatches
to their true cause: real Contra's game loop runs entirely inside the
NMI handler, and during any lag-heavy stretch a second NMI can genuinely
reenter it before the first finishes - real, edge-triggered 6502
behavior `contra-nes`'s cycle-accurate emulation faithfully reproduces -
so sound processing can run more than once per visual frame. That's a
verification-methodology gap (the tool steps once per frame; real
hardware doesn't always), not an engine bug, and it doesn't affect the
eventual native PC port, which has no 6502 cycle budget to blow.

**High-format (music) and percussion now have a real, verified engine
too.** `contra_native::sound_code::decode_high_command` and
`contra_native::sound_engine::MusicSlot` mirror the low-format pieces
above - hand-verified against two real sounds byte-for-byte first
(TITLE's music and its percussion track, both matched on the first
attempt), then mechanically verified across all 4 music slots during real
gameplay. That verification caught a second real trigger-address bug: a
multi-slot sound (like the TITLE theme, which spans all 4 music slots)
sets the per-slot "currently playing" variable to the *original*
triggering code for every slot it touches, not that slot's own table
entry - the fix walks consecutive table entries for the one whose
embedded slot number actually matches, the same way the real trigger
routine does. One slot's 23-for-23 matched note-trigger commands is the
clearest evidence the core decoding logic is right; the remaining
mismatches cluster around the same two already-understood causes above
(trigger-frame observation timing, and NMI-reentrancy/lag), not new
bugs.

**Two of the three missing data tables are ported too, both verified
byte-for-byte against the real ROM.** `sound_code::NOTE_PERIOD_TBL` (24
real APU period values) resolves a music note's pitch; `sound_code::
PERCUSSION_TBL` (8 sound codes) resolves which DMC sample or sound_code a
percussion trigger actually plays. The volume-envelope table
(`pulse_volume_ptr_tbl`) turned out to be a per-level array of pointers
to many separate envelope byte streams - genuinely its own extraction
workstream (comparable in scope to the enemy-spawn or level-data
extraction already done elsewhere in this document), left for a
follow-up rather than rushed. Still not started: that envelope data, and
cross-slot channel-priority arbitration.

Indoor-level enemy placement (levels 2 and 4) uses a different, undecoded
format. See docs/NATIVE_PORT.md's "Current status" for the up-to-date
breakdown.

## What `contra-extract` does today

```
cargo run -p contra-extract -- path\to\your\baserom.nes
```

1. Reads the iNES header, slices out PRG-ROM/CHR-ROM.
2. Computes an MD5 and tells you whether it matches the documented US retail
   hash (`7bdad8b4a7a56a634c9649d20bd3011b`) - informational only, so you know
   what you pointed it at. We never bundle, cache, or transmit the ROM.
3. Reports sizes/mapper, and if the mapper is 2 (UxROM, what Contra USA
   uses), prints the exact `contra-pc` command to play it.

With `--dump-graphics <dir>`, it additionally decodes all 27 documented
`graphic_data_XX` blobs (every level, menus, endings) straight from
PRG-ROM into pattern-table tile-sheet PNGs in `<dir>` - no emulation
involved, just the ported decompressor above:

```
cargo run -p contra-extract -- path\to\your\baserom.nes --dump-graphics .\assets\graphics
```

With `--dump-palettes <dir>`, it renders all 110 `game_palettes` groups as
color swatches into `<dir>/game_palettes.png`, also straight from PRG-ROM:

```
cargo run -p contra-extract -- path\to\your\baserom.nes --dump-palettes .\assets\palettes
```

With `--dump-levels <dir>`, it renders each of the 8 levels' full
nametable (every screen, side by side, fully colored) to
`level{1..8}_full.png`, straight from PRG-ROM:

```
cargo run -p contra-extract -- path\to\your\baserom.nes --dump-levels .\assets\levels
```

With `--dump-audio <dir>`, it decodes both DPCM samples to 8-bit PCM WAV
files, also straight from PRG-ROM:

```
cargo run -p contra-extract -- path\to\your\baserom.nes --dump-audio .\assets\audio
```

With `--dump-enemies <dir>`, it writes each outdoor level's hard-coded
enemy placements to a plain text file:

```
cargo run -p contra-extract -- path\to\your\baserom.nes --dump-enemies .\assets\enemies
```

With `--dump-sound-codes <dir>`, it extracts every sound_code's raw
bytecode - sound effects, music, and percussion alike (deduplicated -
shared blobs are written once):

```
cargo run -p contra-extract -- path\to\your\baserom.nes --dump-sound-codes .\assets\sound_codes
```

`contra-pc` doesn't consume these files yet - today it still plays the ROM
directly through `contra-nes`, same as before. Extraction and "make
`contra-pc` actually load the extracted files instead of the ROM" are
separate steps; this is the first one.

## What's still a limitation

- **Mapper 2 (UxROM) only.** That's what Contra (USA) uses, so it's enough
  for this project's purpose, but a ROM using a different mapper won't run
  (`contra-pc` detects this and shows the in-app Load ROM screen with the
  reason, rather than failing silently).

## Mods and assets

Community texture/music packs under `/mods/` are a conceptually different
thing from "the ROM": swapping the *rendered* output requires hooking the
PPU after the game's own decompression has already populated CHR-RAM (a
CHR-RAM snapshot/patch mechanism, not yet built - see ROADMAP.md and
[MODDING.md](MODDING.md)), rather than replacing files the way an
asset-extraction pipeline would have made possible. A mod never needs
`baserom.nes` itself; it ships its own original or properly-licensed art.
