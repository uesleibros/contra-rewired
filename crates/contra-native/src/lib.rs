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
//! ## Why this is trustworthy without hand-verifying the disassembly itself
//!
//! This project doesn't have a local `ca65`/`cc65` toolchain to personally
//! reassemble `nes-contra-us` and re-confirm its claimed byte-match (that
//! would need installing a full C toolchain purely to double-check a claim
//! a mature, actively-used, specifically-hashed community project already
//! makes - not a good use of a large, semi-irreversible system change). The
//! trust model instead is: the disassembly's claim is specific and
//! falsifiable (an exact hash), the project is well-established, and every
//! port in this crate gets its own independent verification anyway - see
//! below - so an error in the *source* disassembly would show up as a
//! verification failure regardless of whether it was hand-confirmed
//! up front.
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
//! ## Current status
//!
//! Just started - see docs/NATIVE_PORT.md. `collision::bg_collision` is the
//! first port, chosen because it's small, self-contained, referenced by a
//! lot of other game logic (soldier generation, player collision, the
//! `get_bg_collision` Lua debug scripts already published in
//! `nes-contra-us/docs/lua_scripts`), and has no dependency on any other
//! not-yet-ported routine.

pub mod collision;
