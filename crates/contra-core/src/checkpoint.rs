//! Where you respawn after dying, independent of the difficulty sliders.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckpointMode {
    /// NES behavior: lose a life, respawn at the stage's fixed re-entry
    /// point (or restart the stage once continues/lives run out).
    Original,
    /// One checkpoint per stage area.
    Casual,
    /// A checkpoint before/after every meaningful set piece (room, boss
    /// door, vehicle section).
    Modern,
    /// Jump to any checkpoint on demand; used by Practice mode and is
    /// mutually exclusive with score/speedrun/leaderboard submission.
    Practice,
}

impl Default for CheckpointMode {
    fn default() -> Self {
        CheckpointMode::Original
    }
}

/// A single named respawn point, addressable as `stage.checkpoint_index`
/// (e.g. "Stage 5 -> checkpoint 3") so Practice mode / share codes can
/// target it directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointId {
    pub stage: u8,
    pub index: u8,
}

impl CheckpointId {
    pub const fn new(stage: u8, index: u8) -> Self {
        Self { stage, index }
    }
}

impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stage {} / Checkpoint {}", self.stage, self.index)
    }
}
