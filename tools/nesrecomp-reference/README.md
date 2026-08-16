# nesrecomp reference build

Not part of `contra-rewired`'s own build - this is a **translation aid**
for the `contra-native` hand-porting workstream (see
`docs/NATIVE_PORT.md`'s "A possible shortcut" section for the full
background and findings). [`mstan/nesrecomp`](https://github.com/mstan/nesrecomp)
is a static 6502->C recompiler; running it against a legally-owned Contra
ROM with the config here produces mechanically-translated C for most of
the game's real functions. That C is **not verified or trusted on its
own** - it's a starting point, not a source of truth. The only source of
truth for a `contra-native` port is still the same as always: real
behavior captured from `contra-nes` (see `docs/NATIVE_PORT.md`'s
"methodology" section). What this buys is speed - reading already-
mechanically-translated C (register moves, flag updates, memory
reads/writes already made explicit) is faster than re-deriving the same
semantics from raw 6502 assembly instruction-by-instruction, especially
for longer routines.

**This directory does not include Konami's assets or code, or the
nesrecomp project itself** - only the small config files this project
authored to drive nesrecomp against your own legally-owned ROM.

## Real, tested results (this session)

Running this config against the real US Contra ROM: the ~4700+
auto-discovered functions include a real, correct game-state dispatch
(after the `inline_dispatch` fix below), and the resulting build boots
the real ROM, correctly renders the real title screen, responds to a
real Start press, and renders real level 1 (jungle) gameplay terrain -
see `docs/NATIVE_PORT.md` for the full account, including two real bugs
found and fixed along the way (not nesrecomp being broken out of the
box - both are documented, scoped fixes):

1. nesrecomp's bank-switch static-analysis heuristic only followed
   register A at a `JSR` call site; Contra's bank-switch routine
   (`set_rom_bank_to_y`, `$C139`) passes the bank number in Y. Fixed by
   `y-register-bank-switch.patch` (apply to nesrecomp's
   `recompiler/src/function_finder.c`).
2. `run_routine_from_tbl_below` ($C857) - a shared "read the return
   address off the stack, treat it as an inline jump table, jump to
   `table[A]`" helper Contra uses for 4 major systems (game routine,
   level routine, player state, bullet velocity) - doesn't survive naive
   recompilation. Fixed by the `[[inline_dispatch]] addr = 0xC857` entry
   in `game.toml` (pointing at the shared helper's own address, not each
   call site individually).

## Reproducing

Tested against nesrecomp commit `1ee00e43d2467fa0967a2d59aca2cd8567fea38a`
(2026-08-13). Needs a C11 toolchain + CMake + Ninja + SDL2 (MSYS2 ucrt64
on Windows: `pacman -S mingw-w64-ucrt-x86_64-{cmake,ninja,gcc,SDL2}`).

```sh
# 1. Clone nesrecomp next to this checkout (or anywhere short-pathed -
#    Windows' ~250-char path limit is real; deep temp/scratch paths can
#    fail mid-build).
git clone https://github.com/mstan/nesrecomp.git
cd nesrecomp
git apply /path/to/contra-rewired/tools/nesrecomp-reference/y-register-bank-switch.patch

# 2. Build the recompiler (no SDL2 needed for this step).
cmake -S recompiler -B build/recompiler -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build/recompiler

# 3. Recompile Contra (needs your own legally-dumped ROM).
mkdir contra_build && cd contra_build
mkdir generated
../build/recompiler/NESRecomp.exe path/to/your/baserom.nes \
    --game /path/to/contra-rewired/tools/nesrecomp-reference/game.toml \
    --output-prefix contra
# Output lands in generated/ - move/copy it here if NESRecomp wrote it
# elsewhere relative to your invocation.

# 4. Build the runner executable.
cp /path/to/contra-rewired/tools/nesrecomp-reference/{CMakeLists.txt,extras.c} .
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build

# 5. Run it.
./build/ContraRecompExperiment.exe path/to/your/baserom.nes
```

`game.toml`'s `[[data_region]]` entries were generated, not hand-
transcribed, by `cargo run -p contra-nes --release --example
emit_data_regions -- <rom>` (in the main `contra-rewired` checkout) -
they cover every sound_code/graphics/level/enemy-spawn/palette data
structure `contra-native` already knows how to walk. Re-run that tool and
regenerate this section if `contra-native`'s extraction coverage grows
(more banks, more structures) - it'll produce a tighter, more accurate
exclusion list than what's checked in here today.

## Using the output as a `contra-native` translation aid

Once built, `generated/contra_full.c` contains (mostly) one `func_XXXX`
per real routine, `XXXX` being its real CPU address - the same addresses
`docs/rom-symbols.txt` and this project's own routine-by-routine
verification already use. Workflow for porting a new routine to
`contra-native`:

1. Find the routine's real address the usual way (grep `docs/rom-
   symbols.txt`, or the disassembly directly).
2. Grep `generated/contra_full.c`/the per-bank `generated/contra_full_
   bankNN*.c` files for `func_<ADDR>` - read the mechanically-translated
   C as a faster first pass than the raw 6502.
3. Write the idiomatic Rust port in `contra-native`, same as every
   existing port in this crate.
4. Verify it the same way every existing port is verified - against real
   gameplay captured through `contra-nes` (see `docs/NATIVE_PORT.md`'s
   methodology section). The generated C is a reading aid, not a
   verification oracle - it can have the same kind of static-analysis
   gaps this whole evaluation found and fixed (wrong function
   boundaries, misidentified data, control-flow patterns like `inline_
   dispatch` that don't get caught without adding the right config) so
   it is not trusted on its own, ever.
