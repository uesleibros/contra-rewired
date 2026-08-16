//! Legal asset pipeline: contra-rewired ships **zero** Konami-owned
//! graphics, audio, or level data. This crate reads a copy of the ROM the
//! *user* already legally owns and dumped themselves, validates it, and
//! exposes its raw contents for `contra-native`'s decoders (see
//! `crates/contra-native/src/graphics.rs`, and `apps/contra-extract`'s
//! `--dump-graphics`) to turn compressed graphic/audio/level-data blobs
//! into usable assets - graphics decoding is live as of this writing;
//! audio and level data are still to come, see docs/NATIVE_PORT.md.
//!
//! This mirrors how the reference disassembly this project is built
//! against works (<https://github.com/vermiceli/nes-contra-us>): it is
//! source + build scripts only, and requires the user to supply
//! `baserom.nes` themselves before anything will build.

pub mod rom;

pub use rom::{NesRom, RomIdentity, RomLoadError};
