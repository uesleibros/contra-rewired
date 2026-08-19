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
//! time, via `contra-nes`'s `HookAction::ReturnNow` (see [`physics::
//! collision`] and [`physics::player_physics`]). [`world::graphics`] is a
//! different kind: a one-time *asset extractor*, ported from `write_
//! graphic_data_to_ppu` so it can decode Contra's RLE-compressed CHR data
//! straight from PRG-ROM into plain image files, offline, with the ROM
//! touched exactly once - see docs/NATIVE_PORT.md's "The actual end
//! state" section for why that's a separate, equally necessary piece of
//! the same project.
//!
//! ## Module layout
//!
//! - [`enemy`] - enemy AI/state, spawning, and shared enemy-lifecycle
//!   logic (the plain soldier's full routine table lives at [`enemy::
//!   soldier`]).
//! - [`physics`] - background collision and player/bullet velocity
//!   integration.
//! - [`graphics_buffer`] - the live nametable-write-queue subsystem many
//!   enemy families (bridges, weapon boxes, wall cores/turrets, ...)
//!   funnel through to redraw parts of the nametable at runtime -
//!   started, not complete (see that module's own doc comment).
//! - [`audio`] - DPCM samples, the sound-code bytecode format, and its
//!   playback engine.
//! - [`world`] - graphics decompression, palettes, super-tiles, and
//!   level headers.
//!
//! ## Current status
//!
//! See docs/NATIVE_PORT.md for the up-to-date, routine-by-routine list -
//! logic-side, over 40 routines are ported and live-verified against real
//! gameplay so far, most still not yet wired into a running game via
//! `HookAction::ReturnNow` (see that doc's "Integration strategy"
//! section). Asset-side, graphics, palettes, all 8 levels' layout,
//! outdoor enemy spawns, DPCM samples, and the full sound-code audio
//! bytecode all decode straight from PRG-ROM and are verified
//! byte-identical against `contra-nes`'s live state. Realistically
//! hundreds more logic routines remain.

pub mod audio;
pub mod enemy;
pub mod graphics_buffer;
pub mod physics;
pub mod world;
