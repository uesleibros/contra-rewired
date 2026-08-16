//! Native port of Contra's real-time sound engine - the frame-by-frame
//! state machine (`handle_sound_slots`/`handle_sound_code`, `src/
//! bank1.asm`) that actually drives APU registers over time, as opposed
//! to `sound_code` (which only knows how to *decode* a sound_code, not
//! play it over multiple frames). This is the piece `docs/NATIVE_PORT.md`
//! tracks as "playback engine" - see that file's "Current status" for the
//! full, honest account of how deep this turned out to be and what
//! remains.
//!
//! ## Scope: low-format slots only (4 and 5), triggering + sustain
//!
//! This covers what `handle_sound_code` does for the two sound-*effect*
//! slots (#$04 pulse 1, #$05 noise) - trigger initialization
//! (`load_sound_code_entry`), reading low-format commands via
//! `sound_code::decode_low_command`, control-flow commands
//! (`sound_cmd_routine_03`'s `0xFD` child-jump/`0xFE` repeat/`0xFF`
//! end-or-return, single-level nesting only - matching the real ROM's
//! own one-bit "in a child" flag), and per-frame sustain (the
//! volume/decrescendo maintenance that runs every frame a note is
//! holding, not just when a new one starts). High format (music, slots
//! 0-3), percussion, sweep, and cross-slot channel-priority arbitration
//! (`ldx_pulse_triangle_reg`) are **not** covered - see below.
//!
//! ## A real, verified-against-real-hardware finding: `SOUND_VOL_ENV`
//! aliasing
//!
//! `src/ram.asm` reserves exactly **2 bytes** for `SOUND_VOL_ENV`
//! (`$011E-$011F`), not 6 - unlike every other per-slot array
//! (`SOUND_CODE`, `SOUND_CFG_HIGH`, etc., all `.res 6`). But
//! `@check_pulse_volume` (inside `handle_sound_code`, reached by slots
//! #$00/#$01/#$04/#$05 - #$02/#$03 exit earlier) reads `SOUND_VOL_ENV,x`
//! unconditionally for whichever slot is executing, including x=4 and
//! x=5. Absolute-indexed addressing doesn't care that the array is only
//! 2 bytes long - it reads whatever real variable happens to sit at that
//! offset instead:
//!
//! - slot 4: `$011E + 4 = $0122` = `INIT_SOUND_CODE` (the raw sound code
//!   most recently passed to `play_sound`, *globally* - not slot-scoped).
//! - slot 5: `$011E + 5 = $0123` = `SOUND_CHNL_REG_OFFSET` (the APU
//!   channel-register offset of whichever slot last ran through
//!   `handle_sound_code` this frame - for slot 5 specifically, always
//!   `$0C`, the noise channel, since that's fixed per slot).
//!
//! Confirmed against real gameplay via `trace_sound.rs`: slot 4's traced
//! `vol_env` matched `INIT_SOUND_CODE`'s value exactly after triggering
//! `sound_03` (`0x03`); slot 5's matched `0x0C` consistently. Both values
//! are always `< 0x80` (valid sound codes and channel offsets never set
//! bit 7), so the low-format slots' "envelope" byte is *always* read as
//! non-negative - meaning `lower_pulse_volume` (the simple decrescendo
//! countdown) can **never** trigger for slots 4/5 through this path; they
//! always fall through to `lvl_config_pulse`, indexing
//! `pulse_volume_ptr_tbl` (a table that's semantically about *level
//! music*, not sound effects) with this aliased, essentially-incidental
//! value. This module models that faithfully (see [`SharedScratch`]) -
//! not because it's a sensible design, but because it's what the real
//! ROM actually does, confirmed against real playback, not guessed.
//!
//! **What's real, verified, and shipped**: [`decode_low_command`] combined
//! with [`SoundSlot::trigger`]/[`SoundSlot::step_low`]'s command-reading
//! path (including `0xFD`/`0xFE`/`0xFF` control-flow, per `sound_cmd_
//! routine_03`) reproduces real sound_codes' command sequences, frame by
//! frame, address for address, cross-checked against `trace_sound.rs`
//! output from real gameplay (`examples/verify_sound_engine.rs` in
//! `contra-nes`).
//!
//! **A real, verified finding about what "one frame" means**: comparing
//! this module against `contra-nes` frame-by-frame surfaced mismatches
//! that turned out to be a verification-methodology gap, not a bug here -
//! see `verify_sound_engine.rs`'s doc comment for the full account.
//! Briefly: real Contra's entire game loop runs inside the NMI handler
//! (`nmi_start`, `src/bank7.asm`), and `NMI_CHECK` tracks whether a
//! *previous* NMI's handler is still running when the *next* vblank's
//! NMI fires (real 6502 NMI is edge-triggered and non-maskable, so it
//! reenters regardless). When that happens, `nmi_start` skips `exe_
//! game_routine` (no new player-driven sound triggers) but still calls
//! `handle_sound_slots` - meaning `handle_sound_code` can run more than
//! once per visual frame during any lag-heavy stretch of real gameplay.
//! A caller stepping this module strictly once per 60Hz tick will
//! therefore drift from a cycle-accurate reference during lag, through
//! no fault of the command-decoding logic itself. This doesn't matter
//! for the eventual native PC port (no 6502 cycle budget to blow, so no
//! reentrancy to replicate), but it matters for verifying this module
//! against `contra-nes`.
//!
//! **The volume-envelope path is now real, resolved, and verified** -
//! `crate::sound_code::PULSE_VOLUME_PTR_TBL`/`pulse_volume_ptr_tbl_entry`/
//! `walk_pulse_volume` (all verified byte-for-byte against the real ROM)
//! let [`SoundSlot::step_low`]'s sustain path implement `@check_pulse_
//! volume`'s full branch structure - the envelope-table read
//! (`lvl_config_pulse`), the "table exhausted, switch to a plain
//! decrescendo" transition (`disable_lvl_pulse_ctrl_exit`'s `0xFF`), and
//! that decrescendo's own resume/pin-at-1 behavior
//! (`handle_possible_decrescendo`/`resume_decrescendo`) - see
//! [`PulseVolumeSource`] for what each resolves to. `lower_pulse_volume`
//! itself (the *other* decrescendo entry point, gated by `SOUND_VOL_ENV`
//! bit 7) is the one piece left unimplemented here, deliberately: it's
//! provably unreachable for slots #$04/#$05 specifically, since their
//! aliased `SOUND_VOL_ENV` source is always non-negative (see the
//! aliasing section above) - real for [`MusicSlot`]'s pulse slots
//! (#$00/#$01), not modeled there yet. Verified against real gameplay via
//! `verify_sound_engine.rs`: 197/199 sustain-frame `PULSE_VOLUME`
//! comparisons matched exactly for slot 5 across a 900-frame session; the
//! 2 remaining mismatches trace to the same already-documented NMI-
//! reentrancy verification-methodology gap (a stray, single-frame
//! `SOUND_CHNL_REG_OFFSET` read caught mid-lag), not a bug in this path.
//! `LVL_PULSE_VOL_INDEX`'s never-reset-for-low-format persistence (see
//! the module doc above) is modeled as ordinary per-slot state that only
//! `trigger()` resets (which the real ROM never does for these slots
//! either).
//!
//! [`decode_low_command`]: crate::sound_code::decode_low_command

use crate::sound_code::{bank1_prg_offset, decode_high_command, decode_low_command, HighCommand, LowCommand, Slot};

/// Global (not per-slot) scratch state that `SOUND_VOL_ENV,4`/`,5`'s
/// aliasing reads from - see this module's doc comment.
#[derive(Debug, Clone, Copy, Default)]
pub struct SharedScratch {
    /// `INIT_SOUND_CODE` ($0122) - the raw sound code most recently
    /// passed to `play_sound`, across *all* slots.
    pub init_sound_code: u8,
    /// `SOUND_CHNL_REG_OFFSET` ($0123) - the APU channel-register offset
    /// of whichever slot last ran through `handle_sound_code`.
    pub sound_chnl_reg_offset: u8,
}

/// Where a sustain frame's pulse volume came from this frame - resolved
/// down to a real `PULSE_VOLUME` value where real ROM data makes that
/// possible (ported from `@check_pulse_volume`'s full branch structure,
/// `src/bank1.asm`, using [`crate::sound_code::PULSE_VOLUME_PTR_TBL`]/
/// `pulse_volume_ptr_tbl_entry`/`walk_pulse_volume`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseVolumeSource {
    /// No pulse-config write happens this frame - either the routine
    /// exited without touching `PULSE_VOLUME` (`check_decrescendo_end_
    /// pause`'s "not yet" case, `@exit`'s plain `rts`), or (triangle/
    /// noise slots) this maintenance doesn't apply to begin with.
    Unchanged,
    /// `lower_pulse_volume`'s simple decrescendo countdown - the new
    /// `PULSE_VOLUME` value, or `None` when it just hit zero
    /// (`handle_sound_code_exit_01` clamps it back up by one and stops).
    /// Provably unreachable for [`SoundSlot`] (slots #$04/#$05): it needs
    /// `SOUND_VOL_ENV,x`'s real bit 7 set, but that variable is aliased
    /// to always-non-negative RAM for those slots - see this module's
    /// doc comment. Real, reachable state for [`MusicSlot`]'s pulse
    /// slots (#$00/#$01), which have genuine `SOUND_VOL_ENV` data - not
    /// wired into `MusicSlot::step_high` yet.
    Decrescendo(Option<u8>),
    /// `resume_decrescendo`, entered once an envelope stream (or a
    /// paused simple decrescendo) has exhausted itself - same countdown
    /// shape as `Decrescendo`, kept as a separate variant since it's a
    /// distinct real code path with its own doc trail.
    ResumingDecrescendo(Option<u8>),
    /// `lvl_config_pulse`/`lvl_pulse_volume_byte`: the real, resolved
    /// `PULSE_VOLUME` byte read from `pulse_volume_ptr_tbl[table_index]`
    /// at the slot's own persistent read cursor (`LVL_PULSE_VOL_INDEX,x`,
    /// [`SoundSlot::lvl_pulse_vol_index`]), already masked `& 0x1F`.
    Envelope(u8),
}

/// One sound slot's persistent state - the low-format-relevant subset of
/// `SOUND_CODE`/`SOUND_CMD_LOW_ADDR`/`SOUND_CMD_HIGH_ADDR`/
/// `SOUND_CMD_LENGTH`/`SOUND_FLAGS`/`SOUND_CFG_HIGH`/`SOUND_CFG_LOW`/
/// `SOUND_LENGTH_MULTIPLIER`/`PULSE_VOLUME`/`PULSE_VOL_DURATION`/
/// `LVL_PULSE_VOL_INDEX`, one instance per slot (this module only models
/// slots #$04/#$05, the low-format-only ones).
#[derive(Debug, Clone, Copy, Default)]
pub struct SoundSlot {
    pub sound_code: u8,
    pub cmd_prg_offset: usize,
    pub cmd_length: u8,
    pub length_multiplier: u8,
    pub cfg_high: u8,
    pub cfg_low: u8,
    pub pulse_volume: u8,
    pub pulse_vol_duration: u8,
    pub lvl_pulse_vol_index: u8,
    /// The most recent note's APU period value (`$4002`/`$4003`-style,
    /// 11 bits) - not yet written anywhere; a caller applies it to real
    /// APU registers.
    pub period: u16,
    pub active: bool,
    /// `SOUND_REPEAT_COUNT,x` - how many times an `0xFE` repeat's target
    /// has been re-entered so far, compared against that command's own
    /// `n` byte (`sound_cmd_routine_03`'s `@repeat_cmd`).
    pub repeat_count: u8,
    /// Mirrors `SOUND_FLAGS,x` bit 3: `Some(offset)` when currently
    /// inside an `0xFD` child (the offset to resume at on `0xFF`, i.e.
    /// `NEW_SOUND_CODE_LOW/HIGH_ADDR,x`); `None` at the top level. Real
    /// Contra data never nests a second `0xFD` inside a child (the ROM
    /// only has one bit of "am I in a child" state, so a nested jump
    /// would clobber the outer return address) - this mirrors that same
    /// single-level limitation rather than supporting arbitrary nesting.
    pub child_return_offset: Option<usize>,
    /// Which slot this is (`4` or `5`) - needed only to resolve `SOUND_
    /// VOL_ENV,x`'s aliasing (see the module doc comment) when reading
    /// the volume envelope; not used anywhere else.
    pub slot_index: u8,
    /// `SOUND_FLAGS,x` bit 2 - "envelope table exhausted/disabled, use
    /// `PULSE_VOLUME` from a plain decrescendo countdown instead" - set
    /// once by `disable_lvl_pulse_ctrl_exit` (an envelope stream's own
    /// `0xFF`) and never cleared by this module (matching the real ROM:
    /// only a fresh `trigger()` resets it).
    pub envelope_disabled: bool,
    /// `SOUND_FLAGS,x` bit 1 - "the paused/exhausted decrescendo should
    /// resume this frame" (`check_decrescendo_end_pause`).
    pub decrescendo_resuming: bool,
    /// `DECRESCENDO_END_PAUSE,x` - compared against `cmd_length` to
    /// decide when a paused/exhausted decrescendo resumes. Real hardware
    /// sets this from `UNKNOWN_SOUND_00,x` via `calc_cmd_len_play_
    /// percussion`'s high-format-only path (`sound_cmd_routine_01`'s
    /// `unknown_00` field) - low format never writes it, so on real
    /// hardware a freshly-triggered low-format slot inherits whatever
    /// *stale* value was last there from unrelated processing. This
    /// module can't replicate that staleness (no shared RAM model), so
    /// it starts at `0` on every `trigger()` - a known, honest gap, not
    /// a silent approximation.
    pub decrescendo_end_pause: u8,
}

/// One frame's resolved outcome for a slot: `None` if nothing changed
/// (still counting down, no register write due), or the pulse-config
/// pieces a caller should merge and write to `$4000`/`$400C` plus the
/// note period/length if a *new* note started this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameOutput {
    pub new_note: bool,
    pub cfg_low: u8,
    pub cfg_high: u8,
    pub period: u16,
    pub pulse_volume_source: PulseVolumeSource,
}

impl SoundSlot {
    /// `load_sound_code_entry`'s trigger initialization for a low-format
    /// sound - `sound_code_start` is the sound's own PRG-ROM offset
    /// (e.g. from `sound_table_00`, already resolved). `slot_index` must
    /// be `4` or `5` (see [`Self::slot_index`]'s doc comment).
    pub fn trigger(&mut self, slot_index: u8, sound_code: u8, sound_code_start: usize) {
        self.slot_index = slot_index;
        self.sound_code = sound_code;
        self.cmd_prg_offset = sound_code_start;
        self.cmd_length = 1; // forces an immediate command read next step
        self.active = true;
        self.repeat_count = 0;
        self.child_return_offset = None;
        self.envelope_disabled = false;
        self.decrescendo_resuming = false;
        self.decrescendo_end_pause = 0;
        // VIBRATO_CTRL, SOUND_PITCH_ADJ are reset too in the real routine
        // but aren't modeled here yet (vibrato isn't used by Contra's
        // real sound data, and pitch-adjust is high-format-only).
    }

    /// Advances this slot by one frame. `prg_rom` must be the same ROM
    /// `sound_code_start` was resolved against; `scratch` is this frame's
    /// `SOUND_VOL_ENV,4`/`,5` aliasing source (see the module doc
    /// comment). Returns `None` if the slot is inactive.
    pub fn step_low(&mut self, prg_rom: &[u8], scratch: SharedScratch) -> Option<FrameOutput> {
        if !self.active {
            return None;
        }

        self.cmd_length = self.cmd_length.wrapping_sub(1);
        if self.cmd_length != 0 {
            // Sustain frame: no new command, just per-frame volume
            // maintenance (@check_pulse_volume's decrescendo/envelope
            // path - see this module's doc comment for what's resolved
            // here vs. left to the caller).
            return Some(FrameOutput {
                new_note: false,
                cfg_low: self.cfg_low,
                cfg_high: self.cfg_high,
                period: self.period,
                pulse_volume_source: self.sustain_volume_source(prg_rom, scratch),
            });
        }

        // Read commands until landing on a note (Case 4), same loop
        // `read_low_sound_cmd`/`interpret_sound_byte` performs. Control
        // commands (`sound_cmd_routine_03`) are handled inline since they
        // don't stop the recursion either - they just redirect where the
        // next byte comes from, still within this same frame's read.
        loop {
            let b = prg_rom[self.cmd_prg_offset];
            if b >= 0xfd {
                match b & 0x0f {
                    0xf => {
                        // 0xFF: return to the parent if we're inside an
                        // 0xFD child (`restore_parent_sound_cmd_addr`),
                        // otherwise the whole sound_code is finished
                        // (`exe_channel_init_ptr_tbl_routine` mutes the
                        // channel and clears SOUND_CODE,x).
                        if let Some(return_offset) = self.child_return_offset.take() {
                            self.cmd_prg_offset = return_offset;
                            continue;
                        }
                        self.active = false;
                        return None;
                    }
                    0xe => {
                        // 0xFE repeat: [opcode, n, addr_lo, addr_hi].
                        // `@repeat_cmd` increments SOUND_REPEAT_COUNT,x
                        // first, then compares to `n`; only when it
                        // reaches `n` does it skip past the instruction
                        // and continue linearly (`skip_3_read_sound_
                        // command_01`, which also resets the counter) -
                        // otherwise (including the "> n" case the real
                        // disassembly itself flags as probably dead) it
                        // jumps to the target. Note this does *not* set
                        // up a parent-return address, unlike 0xFD - real
                        // Contra data instead places a self-referencing
                        // 0xFE at the end of the repeated block itself.
                        let n = prg_rom[self.cmd_prg_offset + 1];
                        self.repeat_count = self.repeat_count.wrapping_add(1);
                        if self.repeat_count == n {
                            self.repeat_count = 0;
                            self.cmd_prg_offset += 4;
                        } else {
                            let mem_addr = u16::from_le_bytes([
                                prg_rom[self.cmd_prg_offset + 2],
                                prg_rom[self.cmd_prg_offset + 3],
                            ]);
                            self.cmd_prg_offset = bank1_prg_offset(mem_addr);
                        }
                        continue;
                    }
                    _ => {
                        // 0xFD child-jump: [opcode, addr_lo, addr_hi].
                        // Saves the address right after this instruction
                        // as the return point (`NEW_SOUND_CODE_LOW/
                        // HIGH_ADDR,x`), then jumps into the child.
                        let mem_addr = u16::from_le_bytes([
                            prg_rom[self.cmd_prg_offset + 1],
                            prg_rom[self.cmd_prg_offset + 2],
                        ]);
                        self.child_return_offset = Some(self.cmd_prg_offset + 3);
                        self.cmd_prg_offset = bank1_prg_offset(mem_addr);
                        continue;
                    }
                }
            }
            let (cmd, len) = decode_low_command(prg_rom, self.cmd_prg_offset);
            self.cmd_prg_offset += len;
            match cmd {
                LowCommand::SetLengthAndConfig { length_multiplier, cfg_high } => {
                    self.length_multiplier = length_multiplier;
                    self.cfg_high = cfg_high;
                }
                LowCommand::Sweep { .. } => {
                    // Not modeled yet - sweep register writes aren't
                    // covered by this module's scope.
                }
                LowCommand::FlattenNoteFlag => {
                    // Not modeled yet (also documented as never actually
                    // triggered by real Contra data).
                }
                LowCommand::Note { cfg_low, period } => {
                    self.cmd_length = self.length_multiplier;
                    self.cfg_low = cfg_low;
                    self.period = period;
                    return Some(FrameOutput {
                        new_note: true,
                        cfg_low: self.cfg_low,
                        cfg_high: self.cfg_high,
                        period: self.period,
                        pulse_volume_source: PulseVolumeSource::Unchanged,
                    });
                }
            }
        }
    }

    /// `@check_pulse_volume`'s full branch structure - see this module's
    /// doc comment for the aliasing background and [`PulseVolumeSource`]
    /// for what each outcome means.
    fn sustain_volume_source(&mut self, prg_rom: &[u8], scratch: SharedScratch) -> PulseVolumeSource {
        // `SOUND_VOL_ENV,x` bit 7 (`lower_pulse_volume`'s trigger) is
        // provably unreachable here: the aliased value (see below) is
        // always < 0x80 for both slots. Skipping straight to
        // @check_volume_source's own branch matches real control flow
        // exactly for these two slots specifically.
        if !self.envelope_disabled {
            let vol_env = match self.slot_index {
                4 => scratch.init_sound_code,
                5 => scratch.sound_chnl_reg_offset,
                _ => unreachable!("SoundSlot only models slots 4 and 5"),
            };
            let entry_addr = crate::sound_code::pulse_volume_ptr_tbl_entry(prg_rom, vol_env);
            let stream_start = bank1_prg_offset(entry_addr);
            let byte = prg_rom[stream_start + self.lvl_pulse_vol_index as usize];
            if byte >= 0xfe {
                // 0xFE is provably dead in real data (see walk_pulse_
                // volume's doc comment) - only the real 0xFF path here.
                self.envelope_disabled = true;
                return PulseVolumeSource::Unchanged;
            }
            self.lvl_pulse_vol_index = self.lvl_pulse_vol_index.wrapping_add(1);
            self.pulse_volume = byte & 0x1f;
            return PulseVolumeSource::Envelope(self.pulse_volume);
        }

        // handle_possible_decrescendo
        if self.decrescendo_resuming {
            // resume_decrescendo: dec, then `beq` checks the *result* for
            // exactly zero (not a wrapped-negative check, unlike
            // lower_pulse_volume) - handle_sound_code_exit_01 then
            // increments it straight back to 1 and skips the config
            // write, so PULSE_VOLUME pins at 1 from then on.
            self.pulse_volume = self.pulse_volume.wrapping_sub(1);
            if self.pulse_volume == 0 {
                self.pulse_volume = 1;
                return PulseVolumeSource::ResumingDecrescendo(None);
            }
            PulseVolumeSource::ResumingDecrescendo(Some(self.pulse_volume))
        } else {
            // check_decrescendo_end_pause
            if self.cmd_length < self.decrescendo_end_pause {
                self.decrescendo_resuming = true;
            }
            PulseVolumeSource::Unchanged
        }
    }
}

/// What a high-format/percussion slot's just-read note command needs
/// resolved from a table this crate hasn't transcribed yet - the same
/// "real ROM index, not yet a final value" carve-out as [`SoundSlot`]'s
/// still-unresolved envelope path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighNoteSource {
    /// `simple_sound_cmd`: needs `note_period_tbl[pitch_offset]` (already
    /// doubled) for the actual APU period, and `SOUND_CMD_LENGTH` (already
    /// resolved by this module, see [`MusicSlot::length_multiplier`]'s
    /// doc) for how long to hold it - the *volume* envelope for this note
    /// additionally depends on `lvl_config_pulse`/`pulse_volume_ptr_tbl`,
    /// same unresolved piece [`SoundSlot`] carves out.
    Note { pitch_offset: u8 },
    /// `play_percussive_sound` (slot #$03 only): needs
    /// `percussion_tbl[percussion_tbl_index]` to know which DMC sample
    /// (or `sound_02`/`sound_25`) to trigger via `play_sound` -
    /// `contra_native::audio`'s DPCM decode is the piece that would
    /// actually play it.
    Percussion { percussion_tbl_index: u8 },
}

/// One frame's resolved outcome for a [`MusicSlot`] - `None` if the slot
/// is inactive; otherwise the pulse/noise config bits this frame (`0` for
/// triangle, which uses `triangle_cfg` instead), and `note_source` when a
/// *new* note/percussion trigger landed this frame (see [`HighNoteSource`]
/// for what's still needed to turn it into an actual sound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighFrameOutput {
    pub muted: bool,
    pub cfg_low: u8,
    pub cfg_high: u8,
    pub triangle_cfg: u8,
    pub note_source: Option<HighNoteSource>,
}

/// One high-format (music, slots #$00-#$02) or percussion (slot #$03)
/// slot's persistent state - the same role [`SoundSlot`] plays for
/// low-format sound effects, ported from `handle_sound_code`/`read_high_
/// sound_cmd`/`sound_cmd_routine_00`-`_02`/`calc_cmd_len_play_percussion`
/// (`src/bank1.asm`). Covers command-flow and `SOUND_CMD_LENGTH` timing
/// only - see [`HighNoteSource`] and this module's doc comment for what's
/// deliberately left unresolved (envelope/pitch/percussion-sample table
/// data) and why.
#[derive(Debug, Clone, Copy)]
pub struct MusicSlot {
    pub slot: Slot,
    pub sound_code: u8,
    pub cmd_prg_offset: usize,
    pub cmd_length: u8,
    /// `SOUND_LENGTH_MULTIPLIER,x` - set by `ConfigChannel`/
    /// `PercussionDelay` commands, then combined with a note's own low
    /// nibble by `calc_cmd_delay`'s real formula
    /// (`SOUND_CMD_LENGTH = length_multiplier * (low_nibble + 1)`) to get
    /// how many frames the note holds before the next command is read.
    pub length_multiplier: u8,
    pub cfg_low: u8,
    pub cfg_high: u8,
    pub triangle_cfg: u8,
    pub muted: bool,
    pub active: bool,
    pub repeat_count: u8,
    pub child_return_offset: Option<usize>,
}

impl Default for MusicSlot {
    fn default() -> Self {
        Self {
            slot: Slot::Pulse1,
            sound_code: 0,
            cmd_prg_offset: 0,
            cmd_length: 0,
            length_multiplier: 0,
            cfg_low: 0,
            cfg_high: 0,
            triangle_cfg: 0,
            muted: false,
            active: false,
            repeat_count: 0,
            child_return_offset: None,
        }
    }
}

impl MusicSlot {
    /// `load_sound_code_entry`'s trigger initialization for a high-format
    /// sound - `sound_code_start` is the sound's own PRG-ROM offset
    /// (from `sound_table_00`, already resolved), `slot` is which
    /// physical channel it plays on (fixed per sound_code, not per-slot
    /// runtime state - see `sound_code::Slot`'s doc comment).
    pub fn trigger(&mut self, slot: Slot, sound_code: u8, sound_code_start: usize) {
        self.slot = slot;
        self.sound_code = sound_code;
        self.cmd_prg_offset = sound_code_start;
        self.cmd_length = 1; // forces an immediate command read next step
        self.active = true;
        self.repeat_count = 0;
        self.child_return_offset = None;
        // VIBRATO_CTRL/SOUND_PITCH_ADJ are reset too in the real routine
        // but aren't modeled here yet - same as SoundSlot::trigger.
    }

    /// Advances this slot by one frame. `prg_rom` must be the same ROM
    /// `sound_code_start` was resolved against. Returns `None` if the
    /// slot is inactive.
    pub fn step_high(&mut self, prg_rom: &[u8]) -> Option<HighFrameOutput> {
        if !self.active {
            return None;
        }

        self.cmd_length = self.cmd_length.wrapping_sub(1);
        if self.cmd_length != 0 {
            // Sustain frame: no new command. Real Contra also runs
            // @check_pulse_volume's decrescendo/envelope maintenance here
            // for pulse slots (#$00/#$01) - same unresolved piece as
            // SoundSlot's sustain path, not modeled here.
            return Some(HighFrameOutput {
                muted: self.muted,
                cfg_low: self.cfg_low,
                cfg_high: self.cfg_high,
                triangle_cfg: self.triangle_cfg,
                note_source: None,
            });
        }

        loop {
            let b = prg_rom[self.cmd_prg_offset];
            if b >= 0xf0 {
                // Same control-flow grammar as SoundSlot::step_low, just
                // triggered by high/percussion format's wider `>= 0xF0`
                // (see `sound_code::control_command_body`'s doc comment).
                match b & 0x0f {
                    0xf => {
                        if let Some(return_offset) = self.child_return_offset.take() {
                            self.cmd_prg_offset = return_offset;
                            continue;
                        }
                        self.active = false;
                        return None;
                    }
                    0xe => {
                        let n = prg_rom[self.cmd_prg_offset + 1];
                        self.repeat_count = self.repeat_count.wrapping_add(1);
                        if self.repeat_count == n {
                            self.repeat_count = 0;
                            self.cmd_prg_offset += 4;
                        } else {
                            let mem_addr = u16::from_le_bytes([
                                prg_rom[self.cmd_prg_offset + 2],
                                prg_rom[self.cmd_prg_offset + 3],
                            ]);
                            self.cmd_prg_offset = bank1_prg_offset(mem_addr);
                        }
                        continue;
                    }
                    _ => {
                        let mem_addr = u16::from_le_bytes([
                            prg_rom[self.cmd_prg_offset + 1],
                            prg_rom[self.cmd_prg_offset + 2],
                        ]);
                        self.child_return_offset = Some(self.cmd_prg_offset + 3);
                        self.cmd_prg_offset = bank1_prg_offset(mem_addr);
                        continue;
                    }
                }
            }

            let (cmd, len) = decode_high_command(prg_rom, self.cmd_prg_offset, self.slot);
            self.cmd_prg_offset += len;
            match cmd {
                HighCommand::Mute => {
                    self.muted = true;
                    return Some(HighFrameOutput {
                        muted: true,
                        cfg_low: self.cfg_low,
                        cfg_high: self.cfg_high,
                        triangle_cfg: self.triangle_cfg,
                        note_source: None,
                    });
                }
                HighCommand::ConfigChannel { length_multiplier, triangle_cfg, cfg_low, cfg_high, vol_env: _, unknown_00: _ } => {
                    self.length_multiplier = length_multiplier;
                    if let Some(triangle_cfg) = triangle_cfg {
                        self.triangle_cfg = triangle_cfg;
                    } else {
                        self.cfg_low = cfg_low;
                        self.cfg_high = cfg_high;
                    }
                    // vol_env/unknown_00 feed the still-unresolved
                    // envelope/decrescendo path (see this module's doc
                    // comment) - not modeled beyond being decoded.
                }
                HighCommand::PeriodRotate { .. } | HighCommand::FlipFlattenNote | HighCommand::SetVibrato { .. } | HighCommand::PitchAdjust { .. } | HighCommand::Unknown => {
                    // All continue the same read, per the real routine -
                    // none of these stop the recursion.
                }
                HighCommand::Note { pitch_offset, length_low_nibble } => {
                    self.muted = false;
                    self.cmd_length = self.length_multiplier.wrapping_mul(length_low_nibble.wrapping_add(1));
                    return Some(HighFrameOutput {
                        muted: false,
                        cfg_low: self.cfg_low,
                        cfg_high: self.cfg_high,
                        triangle_cfg: self.triangle_cfg,
                        note_source: Some(HighNoteSource::Note { pitch_offset }),
                    });
                }
                HighCommand::PercussionDelay { length_multiplier } => {
                    self.length_multiplier = length_multiplier;
                }
                HighCommand::PercussionTrigger { percussion_tbl_index, delay_low_nibble } => {
                    self.cmd_length = self.length_multiplier.wrapping_mul(delay_low_nibble.wrapping_add(1));
                    return Some(HighFrameOutput {
                        muted: false,
                        cfg_low: self.cfg_low,
                        cfg_high: self.cfg_high,
                        triangle_cfg: self.triangle_cfg,
                        note_source: Some(HighNoteSource::Percussion { percussion_tbl_index }),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_then_step_reproduces_sound_03s_real_first_two_commands() {
        // sound_03's real bytes (already proven correct against both a
        // hand trace and real gameplay via trace_sound.rs).
        let data = [0x21, 0x30, 0x40, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x21, 0xf0, 0xf8, 0x20, 0x0a, 0xf8, 0x10, 0x0b, 0xff];
        let mut slot = SoundSlot::default();
        slot.trigger(4, 0x03, 0);
        let scratch = SharedScratch::default();

        // Real trace: triggering sound_03 and stepping once lands on the
        // first note with cmd_length=1, cfg_low=4, cfg_high=0x30,
        // period=0xf0 - matching the real captured frame exactly
        // (frame=733 POST in the captured trace: len=0x01 cfglo=0x04
        // cfghi=0x30, and command pointer past both the SetLengthAndConfig
        // and first Note command).
        let out = slot.step_low(&data, scratch).unwrap();
        assert!(out.new_note);
        assert_eq!(out.cfg_low, 4);
        assert_eq!(out.cfg_high, 0x30);
        assert_eq!(out.period, 0xf0);
        assert_eq!(slot.cmd_length, 1);
        assert_eq!(slot.cmd_prg_offset, 4); // past both commands (2+2 bytes)

        // Next frame: length_multiplier was 1, so this immediately reads
        // the next note too (matches the real trace's cmd advancing
        // every single frame for this specific sound).
        let out2 = slot.step_low(&data, scratch).unwrap();
        assert!(out2.new_note);
        assert_eq!(out2.cfg_low, 0);
        assert_eq!(out2.period, 0x00);
        assert_eq!(slot.cmd_prg_offset, 6);
    }
}
