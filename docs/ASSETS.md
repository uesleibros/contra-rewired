# Assets & the legal ROM model

**contra-rewired ships zero Konami-owned bytes.** No graphics, no music, no
sound effects, no level data, no ROM. What's in this repository is:

- an engine written from scratch in Rust;
- documentation and physics/RNG values *verified against* the public
  [vermiceli/nes-contra-us](https://github.com/vermiceli/nes-contra-us)
  disassembly (facts about how the original game behaves — not its code or
  prose, which are that project's own work);
- tooling that reads a ROM **you** already own and dumped yourself, at
  build/run time, on your own machine.

This is the same model the reference disassembly itself uses: it is source +
build scripts, and requires you to drop your own `baserom.nes` next to it
before anything will assemble. `contra-extract` (`apps/contra-extract`)
follows the same rule for this project.

## What `contra-extract` does today

```
cargo run -p contra-extract -- path\to\your\baserom.nes
```

1. Reads the iNES header, slices out PRG-ROM/CHR-ROM.
2. Computes an MD5 and tells you whether it matches the documented US retail
   hash (`7bdad8b4a7a56a634c9649d20bd3011b`) — informational only, so you know
   what you pointed it at. We never bundle, cache, or transmit the ROM.
3. Reports sizes/mapper and exits. **It does not decode graphics or audio
   yet** — see below.

## What's not implemented yet

The NES code stores most graphics and several audio structures compressed
(RLE + custom encodings — see the disassembly's `Graphics Documentation.md`
and `Sound Documentation.md` for the algorithms). Writing a correct decoder
for those is real, scoped work, tracked in [ROADMAP.md](../ROADMAP.md) under
Phase 1. Until it lands:

- `contra-pc` renders a placeholder scene, not the real game (see the
  Phase 1 status in the README).
- No sprite/tile/music files are produced by `contra-extract`.

If you want to help: the disassembly's bank comments already document which
routines decompress what (`bank2.asm`–`bank6.asm`); porting one decoder at a
time, with a unit test comparing output against a known-good FCEUX/Mesen
memory dump, is the fastest path to a real Phase 1.

## Mods and assets

Community texture/music packs under `/mods/` are a *separate* concern from
this pipeline — see [MODDING.md](MODDING.md). A mod can replace what the
extractor eventually produces, but it never needs the extractor to exist:
mods ship their own original or properly-licensed art.
