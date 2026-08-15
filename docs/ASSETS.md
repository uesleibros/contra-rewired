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

## Graphics/audio decoding is no longer this project's problem

Earlier in this project, the plan was to write a decoder for Contra's
custom-compressed graphics/audio data (documented in the disassembly's
`Graphics Documentation.md`/`Sound Documentation.md`) and extract it into
plain asset files. That's no longer necessary: **`contra-nes` runs the
original 6502 code, which decompresses its own graphics into CHR-RAM and
plays its own audio at run time, the same way it does on real hardware.**
There's nothing to extract - the PPU core reads pattern data straight out of
the CHR-RAM the game itself populated. This is a direct consequence of the
architecture decision in the main README ("why an emulator core, not a
hand-ported reimplementation"): running the real code subsumes the asset
pipeline that reimplementing it would have needed.

`apps/contra-extract` still exists for a narrower, still-useful job:
validating a ROM before you point `contra-pc` at it.

## What `contra-extract` does today

```
cargo run -p contra-extract -- path\to\your\baserom.nes
```

1. Reads the iNES header, slices out PRG-ROM/CHR-ROM.
2. Computes an MD5 and tells you whether it matches the documented US retail
   hash (`7bdad8b4a7a56a634c9649d20bd3011b`) - informational only, so you know
   what you pointed it at. We never bundle, cache, or transmit the ROM.
3. Reports sizes/mapper, and if the mapper is 2 (UxROM, what Contra USA
   uses), prints the exact `contra-pc` command to play it. `contra-pc` does
   the same validation internally, so this is mainly for a quick sanity
   check or scripting - not a required step before playing.

`contra-pc` itself needs no separate extraction step - pass it the ROM path
directly (`cargo run -p contra-pc -- path\to\your\baserom.nes`) or drop a
`baserom.nes` next to the executable.

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
