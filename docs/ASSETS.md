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

Not done yet: audio (DPCM samples, music sequences) and enemy/spawn data
haven't been started. See docs/NATIVE_PORT.md's "Current status" for the
up-to-date breakdown.

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

`contra-pc` doesn't consume these PNGs yet - today it still plays the ROM
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
