//! Legal asset pipeline: contra-rewired ships **zero** Konami-owned
//! graphics, audio, or level data. This crate reads a copy of the ROM the
//! *user* already legally owns and dumped themselves, validates it, and
//! exposes its raw contents so a (still-in-progress, see ROADMAP.md)
//! decoder can turn compressed graphic/audio blobs into usable assets.
//!
//! This mirrors how the reference disassembly this project is built
//! against works (<https://github.com/vermiceli/nes-contra-us>): it is
//! source + build scripts only, and requires the user to supply
//! `baserom.nes` themselves before anything will build.

pub mod rom;

pub use rom::{NesRom, RomIdentity, RomLoadError};
