//! Native Rust reimplementations of real Contra (US) game-logic routines -
//! the "real PC port" project (see docs/NATIVE_PORT.md at the repo root for
//! the full plan, methodology, and status), as distinct from `contra-nes`
//! (the 6502/2C02 emulator that runs the original, unmodified ROM) and
//! `contra-core` (the earlier, honestly-labeled hand-ported *placeholder*
//! layer used only when no ROM is loaded).
//!
//! ## What "ported" means here
//!
//! Every function in this crate is a line-by-line translation of a specific
//! routine in the verified, byte-matching disassembly
//! ([`vermiceli/nes-contra-us`](https://github.com/vermiceli/nes-contra-us) -
//! its own README documents reassembling the source back into a byte-for-
//! byte match of the real ROM, confirmed by a specific SHA-512 hash in its
//! build script), not a guess or a cleaned-up approximation. Each function's
//! doc comment names the disassembly routine and source file it's ported
//! from, and (where practical) the CPU address it lives at in the real ROM,
//! found the same way the Base 1/Base 2 stage-select hang and the
//! `INITIALIZE_ENEMY_PC` hook were: searching the ROM's raw bytes for a
//! routine's known opening/closing instructions and converting the match's
//! file offset to a CPU address via UxROM's bank layout.
//!
//! ## Why this is trustworthy
//!
//! `nes-contra-us` isn't just trusted on its word: this project built
//! `cc65` locally and personally reassembled the disassembly against a
//! legally-owned `baserom.nes`, confirming byte-for-byte identical output
//! (see docs/rom-symbols.txt's header for the exact steps). On top of
//! that, every port in this crate gets its own independent verification
//! anyway - see below - so an error that somehow survived both checks
//! would still show up as a verification failure.
//!
//! ## Verification methodology
//!
//! `contra-nes` (the real, accurate 6502/2C02 emulator elsewhere in this
//! workspace) already runs the actual ROM correctly - that makes it a
//! ready-made *reference oracle*. `Nes::run_frame_with_hook` (added for the
//! `enemy_spawn` mod event) can be pointed at a ported routine's real CPU
//! address to capture (inputs at entry, output at exit) pairs from actual
//! gameplay, which then become test cases the native Rust port must
//! reproduce exactly. A port isn't "done" until it has tests built this
//! way, not just tests the author wrote by hand from reading the ASM - see
//! each module's tests for the exact capture command used.
//!
//! ## Two kinds of port live here
//!
//! Most of this crate replaces 6502 *game logic* live, one routine at a
//! time, via `contra-nes`'s `HookAction::ReturnNow` (see `collision` and
//! `player_physics`). `graphics` is a different kind: a one-time *asset
//! extractor*, ported from `write_graphic_data_to_ppu` so it can decode
//! Contra's RLE-compressed CHR data straight from PRG-ROM into plain
//! image files, offline, with the ROM touched exactly once - see
//! docs/NATIVE_PORT.md's "The actual end state" section for why that's a
//! separate, equally necessary piece of the same project.
//!
//! ## Current status
//!
//! Just started - see docs/NATIVE_PORT.md for the up-to-date list.
//! `collision::bg_collision` and `player_physics` are ported and verified
//! so far (logic side); `graphics` can decompress the documented RLE
//! format and is unit-tested against the format's own worked example, but
//! not yet cross-checked against real ROM output (asset side). Everything
//! else in the game (realistically hundreds more logic routines, plus
//! audio/level-data extraction) is not started.

pub mod audio;
pub mod bullet_physics;
pub mod collision;
pub mod enemy_clear;
pub mod enemy_slots;
pub mod enemy_spawn;
pub mod graphics;
pub mod initialize_enemy;
pub mod level;
pub mod palette;
pub mod player_physics;
pub mod sound_code;
pub mod sound_engine;
pub mod supertile;
