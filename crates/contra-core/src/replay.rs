//! Input-only replays (TAS-style): record a per-frame button bitmask
//! instead of video, so files stay tiny and playback can be scrubbed,
//! slowed down, or handed control back to a live player mid-replay
//! ("Take Control").
//!
//! Determinism is the whole point: given the same RNG seed
//! ([`crate::rng::ModernRng`] or a recorded idle-tick trace for "Original"
//! mode) and the same input log, the simulation must reproduce the exact
//! same run. `contra-core` guarantees the *format*; the simulation crate
//! that eventually owns the full game state is responsible for actually
//! being deterministic frame-to-frame.

use serde::{Deserialize, Serialize};

pub const REPLAY_FORMAT_VERSION: u16 = 1;

/// Tiny hand-rolled bitflags so `contra-core` doesn't need the `bitflags`
/// crate for a single 16-bit struct. Mirrors the subset of its API used
/// below (`const` associated flags, `bits()`, bitwise combination).
macro_rules! bitflags_like {
    (
        $(#[$meta:meta])*
        pub struct $name:ident: $ty:ty {
            $(const $flag:ident = $value:expr;)*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
        pub struct $name(pub $ty);

        impl $name {
            $(pub const $flag: $name = $name($value);)*

            pub const fn empty() -> Self { $name(0) }
            pub const fn bits(self) -> $ty { self.0 }
            pub fn contains(self, other: $name) -> bool { (self.0 & other.0) == other.0 }
            pub fn insert(&mut self, other: $name) { self.0 |= other.0; }
            pub fn remove(&mut self, other: $name) { self.0 &= !other.0; }
        }

        impl std::ops::BitOr for $name {
            type Output = $name;
            fn bitor(self, rhs: $name) -> $name { $name(self.0 | rhs.0) }
        }
    };
}

bitflags_like! {
    /// One frame's worth of buttons for one player, packed into a byte —
    /// mirrors the NES controller shift-register bit order closely enough
    /// to be familiar, while adding the modern extras as high bits.
    pub struct InputFrame: u16 {
        const UP           = 1 << 0;
        const DOWN         = 1 << 1;
        const LEFT         = 1 << 2;
        const RIGHT        = 1 << 3;
        const SHOOT        = 1 << 4;
        const JUMP         = 1 << 5;
        const START        = 1 << 6;
        const SELECT       = 1 << 7;
        const AIM_FIRE     = 1 << 8;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayHeader {
    pub format_version: u16,
    pub game_version: String,
    pub rng_seed: u32,
    pub player_count: u8,
    pub difficulty_code: String,
    pub started_at_stage: u8,
    pub started_at_checkpoint: u8,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Replay {
    pub header: Option<ReplayHeader>,
    /// `frames[frame_index][player_index]`.
    pub frames: Vec<Vec<InputFrame>>,
}

impl Replay {
    pub fn new(header: ReplayHeader) -> Self {
        Self { header: Some(header), frames: Vec::new() }
    }

    pub fn record_frame(&mut self, inputs: Vec<InputFrame>) {
        self.frames.push(inputs);
    }

    pub fn len_frames(&self) -> usize {
        self.frames.len()
    }

    /// Truncates the log at `frame_index`, used by "Take Control": stop
    /// trusting recorded input from this frame onward and let a live
    /// player drive the rest.
    pub fn truncate_for_takeover(&mut self, frame_index: usize) {
        self.frames.truncate(frame_index);
    }

    pub fn encode(&self) -> Result<Vec<u8>, bincode::Error> {
        bincode::serialize(self)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> ReplayHeader {
        ReplayHeader {
            format_version: REPLAY_FORMAT_VERSION,
            game_version: "0.1.0".into(),
            rng_seed: 1234,
            player_count: 1,
            difficulty_code: "CONTRA".into(),
            started_at_stage: 1,
            started_at_checkpoint: 0,
        }
    }

    #[test]
    fn input_frame_combines_flags() {
        let f = InputFrame::RIGHT | InputFrame::SHOOT;
        assert!(f.contains(InputFrame::RIGHT));
        assert!(f.contains(InputFrame::SHOOT));
        assert!(!f.contains(InputFrame::JUMP));
    }

    #[test]
    fn replay_round_trips_through_bincode() {
        let mut r = Replay::new(header());
        r.record_frame(vec![InputFrame::RIGHT | InputFrame::JUMP]);
        r.record_frame(vec![InputFrame::empty()]);
        let bytes = r.encode().unwrap();
        let decoded = Replay::decode(&bytes).unwrap();
        assert_eq!(decoded.len_frames(), 2);
        assert_eq!(decoded.header.unwrap().rng_seed, 1234);
    }

    #[test]
    fn takeover_truncates_the_log() {
        let mut r = Replay::new(header());
        for _ in 0..10 {
            r.record_frame(vec![InputFrame::empty()]);
        }
        r.truncate_for_takeover(4);
        assert_eq!(r.len_frames(), 4);
    }
}
