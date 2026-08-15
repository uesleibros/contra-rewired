//! Save states: manual slots, quick save/load, autosave, and the
//! versioned binary snapshot format they all share.
//!
//! Snapshots are opaque `Vec<u8>` blobs (bincode-encoded by the caller's own
//! state struct — `contra-core` doesn't know the full simulation state, so
//! it stays generic over `S: Serialize + DeserializeOwned`). This module
//! owns slot management, metadata, and the undo-load / rewind ring buffer.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::VecDeque;

pub const SAVESTATE_FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveStateMeta {
    pub format_version: u16,
    pub slot: SlotId,
    pub stage: u8,
    pub checkpoint_index: u8,
    pub playtime_frames: u64,
    /// Optional PNG/JPEG-encoded thumbnail bytes for the slot picker UI.
    pub screenshot: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotId {
    Quick,
    Manual(u8),
    AutosaveStageEntry,
    AutosaveArea,
    Suspend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveState<S> {
    pub meta: SaveStateMeta,
    pub payload: S,
}

/// Manages save slots for one profile/save file. Generic over the game's
/// own snapshot type `S`.
pub struct SaveStateManager<S> {
    slots: Vec<SaveState<S>>,
    undo_buffer: Option<SaveState<S>>,
    rewind: VecDeque<S>,
    rewind_capacity: usize,
}

impl<S: Clone + Serialize + DeserializeOwned> SaveStateManager<S> {
    pub fn new(rewind_capacity_frames: usize) -> Self {
        Self {
            slots: Vec::new(),
            undo_buffer: None,
            rewind: VecDeque::with_capacity(rewind_capacity_frames),
            rewind_capacity: rewind_capacity_frames,
        }
    }

    pub fn save(&mut self, slot: SlotId, meta_without_slot: SaveStateMeta, payload: S) {
        let meta = SaveStateMeta {
            slot,
            format_version: SAVESTATE_FORMAT_VERSION,
            ..meta_without_slot
        };
        self.slots.retain(|s| s.meta.slot != slot);
        self.slots.push(SaveState { meta, payload });
    }

    /// Loads a slot, stashing the *current* state into the undo buffer
    /// first so [`Self::undo_load`] can revert an accidental load.
    pub fn load(&mut self, slot: SlotId, current: SaveState<S>) -> Option<&S> {
        self.undo_buffer = Some(current);
        self.slots
            .iter()
            .find(|s| s.meta.slot == slot)
            .map(|s| &s.payload)
    }

    pub fn undo_load(&mut self) -> Option<SaveState<S>> {
        self.undo_buffer.take()
    }

    pub fn slot(&self, slot: SlotId) -> Option<&SaveState<S>> {
        self.slots.iter().find(|s| s.meta.slot == slot)
    }

    pub fn all_slots(&self) -> impl Iterator<Item = &SaveState<S>> {
        self.slots.iter()
    }

    /// Pushes one frame of rewind history; drops the oldest frame once the
    /// buffer is full (ring buffer, O(1) push).
    pub fn push_rewind_frame(&mut self, state: S) {
        if self.rewind.len() == self.rewind_capacity {
            self.rewind.pop_front();
        }
        self.rewind.push_back(state);
    }

    /// Pops the most recent rewind frame (i.e. steps time backward once).
    pub fn rewind_step(&mut self) -> Option<S> {
        self.rewind.pop_back()
    }

    pub fn rewind_history_len(&self) -> usize {
        self.rewind.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Dummy(u32);

    fn meta() -> SaveStateMeta {
        SaveStateMeta {
            format_version: SAVESTATE_FORMAT_VERSION,
            slot: SlotId::Manual(0),
            stage: 1,
            checkpoint_index: 0,
            playtime_frames: 0,
            screenshot: None,
        }
    }

    #[test]
    fn save_then_load_round_trips() {
        let mut mgr: SaveStateManager<Dummy> = SaveStateManager::new(60);
        mgr.save(SlotId::Manual(1), meta(), Dummy(42));
        let current = SaveState { meta: meta(), payload: Dummy(0) };
        let loaded = mgr.load(SlotId::Manual(1), current).cloned();
        assert_eq!(loaded, Some(Dummy(42)));
    }

    #[test]
    fn undo_load_restores_previous_state() {
        let mut mgr: SaveStateManager<Dummy> = SaveStateManager::new(60);
        mgr.save(SlotId::Quick, meta(), Dummy(99));
        let current = SaveState { meta: meta(), payload: Dummy(7) };
        mgr.load(SlotId::Quick, current);
        let undone = mgr.undo_load().unwrap();
        assert_eq!(undone.payload, Dummy(7));
    }

    #[test]
    fn rewind_buffer_is_a_bounded_ring() {
        let mut mgr: SaveStateManager<Dummy> = SaveStateManager::new(3);
        for i in 0..5 {
            mgr.push_rewind_frame(Dummy(i));
        }
        assert_eq!(mgr.rewind_history_len(), 3);
        assert_eq!(mgr.rewind_step(), Some(Dummy(4)));
        assert_eq!(mgr.rewind_step(), Some(Dummy(3)));
    }
}
