//! `contra-core`: the engine-agnostic, platform-agnostic simulation layer
//! for contra-rewired.
//!
//! This crate deliberately has **no** rendering, audio, windowing, or file
//! dialog code in it - see `apps/contra-pc` for that. What lives here is
//! everything that must be identical across PC and Android, and everything
//! that must be *provably* deterministic: fixed-point physics, RNG,
//! save states, replays, difficulty, and the config schema that drives all
//! of it.
//!
//! Ported/verified-against-hardware pieces cite the exact routine and file
//! in the community disassembly they were checked against
//! (<https://github.com/vermiceli/nes-contra-us>). Anything not yet ported
//! says so explicitly in its module docs rather than guessing - see
//! ROADMAP.md for what's tracked.

pub mod checkpoint;
pub mod config;
pub mod difficulty;
pub mod fixed;
pub mod input;
pub mod physics;
pub mod replay;
pub mod rng;
pub mod savestate;
pub mod state_machine;
