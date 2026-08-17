//! Background collision - ported from `bank7.asm`'s `get_bg_collision`
//! (entry `$e0bb` in the real ROM's fixed bank) and the `read_bg_collision_
//! byte`/`bg_collision_logic` code it falls through into. This is the same
//! routine the community's own Mesen/FCEUX Lua debug scripts
//! (`nes-contra-us/docs/lua_scripts/*/Show Background Collisions.lua`)
//! reimplement for visualization - this port is a stricter, full-precision
//! translation of the real 6502 arithmetic (see [`bg_collision`]'s doc
//! comment for one case where that matters and the Lua scripts' simpler
//! version doesn't handle it), not a copy of that script.

/// What a background collision point resolves to - `BG_COLLISION_DATA`
/// stores 2 bits per point, and `collision_code_lookup_tbl` in the
/// disassembly maps all 4 possible 2-bit values to exactly these codes (no
/// 5th/reserved value exists in the lookup table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionCode {
    Empty,
    Floor,
    Water,
    Solid,
}

impl CollisionCode {
    /// From the raw 2-bit `BG_COLLISION_DATA` value via
    /// `collision_code_lookup_tbl: .byte $00,$01,$02,$80`.
    fn from_raw(code2: u8) -> Self {
        match code2 & 0x03 {
            0 => CollisionCode::Empty,
            1 => CollisionCode::Floor,
            2 => CollisionCode::Water,
            _ => CollisionCode::Solid,
        }
    }

    /// The exact byte `get_bg_collision` leaves in the accumulator
    /// (`collision_code_lookup_tbl`'s raw values, `$00`/`$01`/`$02`/`$80`) -
    /// for tests comparing directly against a captured real-hardware `a`
    /// register, and for any future caller that needs the original
    /// encoding rather than this enum.
    pub fn to_raw_byte(self) -> u8 {
        match self {
            CollisionCode::Empty => 0x00,
            CollisionCode::Floor => 0x01,
            CollisionCode::Water => 0x02,
            CollisionCode::Solid => 0x80,
        }
    }
}

/// `BG_COLLISION_DATA`'s real size: the offset computed below
/// (`nametable_bit(0x40) | vy_bits(0x00-0x3c) | hx_bits(0x00-0x03)`) can
/// reach at most `0x7f`, so the buffer is 128 bytes (`$0680-$06ff` in the
/// real ROM's RAM map, per `ram.asm`) - two nametables' worth, 64 bytes
/// each, matching `Ppu`'s own 2KB `vram` covering exactly 2 nametables.
pub const BG_COLLISION_DATA_LEN: usize = 128;

/// Ported from `get_bg_collision` (`bank7.asm`, CPU address `$e0bb` in the
/// real ROM - found by searching the ROM's raw bytes for this routine's
/// known opening instructions and converting the match to a CPU address,
/// the same technique the Base 1/Base 2 stage-select hang used). Answers
/// "what's at world position `(x, y)`, adjusted for the current camera
/// scroll" - the same question `soldier_generation_01`'s ground search and
/// the player's own falling/landing logic both ask before placing anything.
///
/// `x`/`y` are screen-relative sprite-style coordinates (not adjusted for
/// scroll yet - this function does that), matching what `get_bg_collision`
/// itself takes in the `a`/`y` registers. `vertical_scroll`/
/// `horizontal_scroll` and `ppuctrl_settings` are `VERTICAL_SCROLL`
/// (`$fc`), `HORIZONTAL_SCROLL` (`$fd`), and `PPUCTRL_SETTINGS` (`$ff`)
/// from `ram.asm` - the CPU-side shadow copies the game keeps of what it
/// last wrote to the PPU's actual scroll/control registers, since the PPU
/// registers themselves are write-only and can't be read back.
///
/// One place this diverges from the community's simpler Lua debug-overlay
/// version of this same routine: the real 6502 arithmetic for the
/// vertical-scroll adjustment can genuinely overflow a single byte (e.g.
/// `y=200, vertical_scroll=100`: `y+vertical_scroll=300`, which 8-bit
/// hardware truncates to `44` *before* deciding whether to add the `$10`
/// wraparound correction - and it always does add it here, since `BCS`
/// after the initial `ADC` catches this overflow case unconditionally,
/// regardless of what the truncated value ends up being). A version that
/// works entirely in un-truncated integers (as the Lua scripts do, treating
/// `y + VERTICAL_SCROLL` as a plain sum up to 510) happens to still get the
/// right *final* answer for this specific case through a different path,
/// but this port matches the hardware's actual 8-bit-at-each-step math
/// directly (`u8::overflowing_add`, mirroring the real `ADC`/`BCS`
/// sequence instruction for instruction) rather than relying on that
/// coincidence holding for every input.
pub fn bg_collision(
    x: u8,
    y: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
) -> CollisionCode {
    // bg_collision_logic: vertical adjustment.
    //   tya; sta $15 (y kept aside, unmodified, for the row-guard below)
    //   clc; adc VERTICAL_SCROLL; bcs @vert_overflow
    //   cmp #$f0; bcc @continue
    // @vert_overflow: adc #$0f   (carry is 1 on both paths into here, so
    //                             this is "+ 0x10" over the raw sum)
    // @continue: (raw unchanged)
    let (raw_vy, overflowed) = y.overflowing_add(vertical_scroll);
    let vy = if overflowed || raw_vy >= 0xF0 { raw_vy.wrapping_add(0x10) } else { raw_vy };

    // lda $13 (x); clc; adc HORIZONTAL_SCROLL; sta $12
    let (hx, hx_overflowed) = x.overflowing_add(horizontal_scroll);

    // lda PPUCTRL_SETTINGS; eor $10 (always 0 via this entry point - only
    // the hangar-mine-cart entry point, `get_cart_bg_collision`, leaves
    // $10 nonzero, and this function only ports the plain `get_bg_collision`
    // entry); and #$01; bcc @bg_collision_data; eor #$01
    // (the bcc tests the carry *from the ADC HORIZONTAL_SCROLL above* -
    // AND doesn't touch carry on 6502 - so this flips the nametable bit
    // exactly when x+horizontal_scroll overflowed a byte)
    let base_nametable_bit = ppuctrl_settings & 0x01;
    let nametable_number = if hx_overflowed { base_nametable_bit ^ 0x01 } else { base_nametable_bit };

    // lda $11 (vy); lsr; lsr; and #$3c
    let vy_bits = (vy >> 2) & 0x3C;
    // lda $12 (hx); lsr x4 -> $12; lsr x2 more (continues on the same
    // accumulator) -> hx >> 6; ora vy_bits; ora level_screen_mem_offset_tbl_01[nt]
    let hx_high_bits = hx >> 6;
    let nametable_offset_tbl = [0x00u8, 0x40u8]; // level_screen_mem_offset_tbl_01: .byte $00,$40
    let bg_collision_offset = (hx_high_bits | vy_bits | nametable_offset_tbl[nametable_number as usize]) as usize;
    // lda $12 (hx >> 4, from the earlier 4x lsr); and #$03
    let column = (hx >> 4) & 0x03;

    // read_bg_collision_byte: row-guard uses the ORIGINAL y (saved in $15
    // before any scroll adjustment), not vy - `cmp #$e0; bcs @set_code_exit`
    // (`lda #$00` already loaded before the branch, so the guard's result
    // is unconditionally "empty").
    if y >= 0xE0 {
        return CollisionCode::Empty;
    }

    let byte = bg_collision_data[bg_collision_offset];
    // column 0 -> bits 6-7 (shift right 6), 1 -> bits 4-5, 2 -> bits 2-3,
    // 3 -> bits 0-1 (no shift) - `read_bg_collision_byte`'s dey-chain.
    let shift = match column {
        0 => 6,
        1 => 4,
        2 => 2,
        _ => 0,
    };
    CollisionCode::from_raw((byte >> shift) & 0x03)
}

/// The real routine's zero-page scratch addresses, exactly as
/// `get_bg_collision` leaves them when it returns - `$10`/`$11`/`$12`/
/// `$13`/`$15` in `ram.asm` (`$14`, the collision code byte, isn't here -
/// it's already available as `bg_collision(...).to_raw_byte()`, so a
/// caller needing both just calls both). `bg_collision` above only
/// returns the *documented* output (the collision code, in `a`/carry);
/// this is for [`contra_nes::HookAction::ReturnNow`] integration, where
/// skipping the real routine's body means these never get written unless
/// something writes them back explicitly - and since zero page is shared,
/// tightly reused scratch space across many unrelated routines in this
/// game (not exclusive to this one), *some* other routine reading one of
/// these addresses expecting *its own* last write, not whatever this
/// routine left stale from a previous unrelated call, is a real,
/// plausible source of drift a cycle-accurate charge alone can't fix. All
/// five addresses are computed identically regardless of whether the row
/// guard (`y >= 0xe0`) short-circuits `bg_collision`'s buffer read - that
/// branch is only reached in `read_bg_collision_byte`, after every one of
/// these is already set - so this needs no separate row-guard case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScratchState {
    /// `$10` - always `0` through this entry point (`get_bg_collision`,
    /// as opposed to the hangar-mine-cart-only `get_cart_bg_collision`
    /// entry this crate doesn't port).
    pub s10: u8,
    /// `$11` - final value is `(vy >> 2) & 0x3c`, *not* the adjusted `vy`
    /// itself (that's an intermediate value at a different point in the
    /// routine, overwritten before return).
    pub s11: u8,
    /// `$12` - final value is the column (`(hx >> 4) & 0x03`).
    pub s12: u8,
    /// `$13` - final value is `bg_collision_offset` (the index into
    /// `BG_COLLISION_DATA` the routine read from).
    pub s13: u8,
    /// `$15` - the original, unadjusted `y` input.
    pub s15: u8,
}

/// Computes [`ScratchState`] for the same inputs [`bg_collision`] would
/// take - see that struct's doc comment for why a `ReturnNow` integration
/// needs this in addition to the collision code itself.
pub fn bg_collision_scratch(x: u8, y: u8, vertical_scroll: u8, horizontal_scroll: u8, ppuctrl_settings: u8) -> ScratchState {
    let (raw_vy, overflowed) = y.overflowing_add(vertical_scroll);
    let vy = if overflowed || raw_vy >= 0xF0 { raw_vy.wrapping_add(0x10) } else { raw_vy };
    let (hx, hx_overflowed) = x.overflowing_add(horizontal_scroll);
    let base_nametable_bit = ppuctrl_settings & 0x01;
    let nametable_number = if hx_overflowed { base_nametable_bit ^ 0x01 } else { base_nametable_bit };
    let vy_bits = (vy >> 2) & 0x3C;
    let hx_high_bits = hx >> 6;
    let nametable_offset_tbl = [0x00u8, 0x40u8];
    let bg_collision_offset = hx_high_bits | vy_bits | nametable_offset_tbl[nametable_number as usize];
    let column = (hx >> 4) & 0x03;

    ScratchState { s10: 0, s11: vy_bits, s12: column, s13: bg_collision_offset, s15: y }
}

/// The *exact* real-hardware cycle cost of a [`bg_collision`] call with
/// these same inputs (entry `$e0bb` to the routine's own `rts`,
/// inclusive) - for [`contra_nes::HookAction::ReturnNow`] integration
/// (see docs/NATIVE_PORT.md), which needs to charge `cpu.cycles` honestly
/// when this port stands in for the real routine instead of letting it
/// execute.
///
/// Derived from an exhaustive measurement, not estimated: every
/// combination of this routine's five real branches (the row guard, the
/// vertical-scroll add's three outcomes - no adjustment / `cmp`-triggered
/// adjustment / genuine byte overflow - the horizontal-scroll add's
/// overflow, and (when the row guard doesn't short-circuit) the column's
/// shift count) was driven directly through the real ROM's code via
/// `contra-nes`'s cycle-accurate CPU (`dump_frames.rs`'s
/// `EXHAUSTIVE_BG_COLLISION_CYCLES=1` - a synthetic `jsr`/`rts` harness,
/// not sampled gameplay) and found to combine *perfectly additively*: a
/// column-and-row-guard-dependent base, plus a flat `+1` if the horizontal
/// add overflowed, plus `+1`/`-2`/`0` for the vertical add's
/// `cmp`/`overflow`/`none` outcome respectively - no interaction terms,
/// confirmed against all 30 combinations tested. This replaced two earlier,
/// both-wrong attempts (a single flat guess, then a real-gameplay-sampled
/// two-value split that turned out to hide 9 real distinct values because
/// of an unrelated measurement bug) - see docs/NATIVE_PORT.md's full
/// account of both.
pub fn bg_collision_cycles(x: u8, y: u8, vertical_scroll: u8, horizontal_scroll: u8) -> u64 {
    let (raw_vy, vy_overflowed) = y.overflowing_add(vertical_scroll);
    let vy_delta: i64 = if vy_overflowed {
        -2
    } else if raw_vy >= 0xF0 {
        1
    } else {
        0
    };
    let (hx, hx_overflowed) = x.overflowing_add(horizontal_scroll);
    let hx_delta: i64 = if hx_overflowed { 1 } else { 0 };

    let base: i64 = if y >= 0xE0 {
        129
    } else {
        match (hx >> 4) & 0x03 {
            0 | 1 | 2 => 158,
            _ => 156,
        }
    };
    (base + vy_delta + hx_delta) as u64
}

/// Native port of `read_bg_collision_byte_unsafe` (`$e0b5`) - the same
/// byte-lookup-and-shift `bg_collision` itself does internally, but
/// without the row guard (real ASM: `$15 = 0` before jumping into the
/// shared `read_bg_collision_byte` tail, so `y >= 0xe0`'s early-exit
/// never triggers). "Unsafe" per the real ASM's own comment: `offset`
/// must already be a valid, correctly-computed `BG_COLLISION_DATA`
/// index - example real use is checking one row *below* ground the
/// player/enemy is already standing on, where the offset is known good.
pub fn read_bg_collision_byte_unsafe(bg_collision_data: &[u8; BG_COLLISION_DATA_LEN], offset: u8, column: u8) -> CollisionCode {
    let byte = bg_collision_data[offset as usize];
    let shift = match column & 0x03 {
        0 => 6,
        1 => 4,
        2 => 2,
        _ => 0,
    };
    CollisionCode::from_raw((byte >> shift) & 0x03)
}

/// Native port of `floor_get_next_row_bg_collision` (`$e08a`-`$e0ba`) -
/// if `original` (the collision code already found at some position)
/// isn't [`CollisionCode::Floor`], returns it unchanged; otherwise looks
/// one supertile half-row further down (`offset + 4`, wrapping within
/// the same nametable half via `& 0x3f` then re-merging the preserved
/// `offset & 0xc0` nametable-selection bits) and upgrades the result to
/// [`CollisionCode::Solid`] if *that* row is solid - real use case (per
/// the real ASM's own comment on the routine this composes into,
/// `get_bg_collision_far`): checking one point ahead of a fast-moving
/// object so it doesn't visually clip into solid ground for a frame
/// before its own collision response catches up.
pub fn floor_get_next_row_bg_collision(
    original: CollisionCode,
    offset: u8,
    column: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
) -> CollisionCode {
    if original != CollisionCode::Floor {
        return original;
    }
    let preserved_high_bits = offset & 0xC0;
    let next_row_offset = (offset.wrapping_add(4) & 0x3F) | preserved_high_bits;
    let below = read_bg_collision_byte_unsafe(bg_collision_data, next_row_offset, column);
    if below == CollisionCode::Solid {
        CollisionCode::Solid
    } else {
        original
    }
}

/// Native port of `get_bg_collision_far` (`$e087`-`$e089`) - `bg_
/// collision` plus a "look one row further down" floor upgrade, purely
/// by composing [`bg_collision`], [`bg_collision_scratch`] (for the
/// real `$12`/`$13` scratch `floor_get_next_row_bg_collision` needs),
/// and [`floor_get_next_row_bg_collision`] itself - no new arithmetic of
/// its own, matching the real ASM's own `jsr get_bg_collision` falling
/// straight through into `floor_get_next_row_bg_collision`.
///
/// Live-verification attempted (`VERIFY_BG_COLLISION_FAR=1` in `crates/
/// contra-nes/examples/dump_frames.rs`) but had 0 real hits across a
/// 20000-frame session - this routine's real callers are all enemy-
/// specific "am I about to walk into a wall" checks (e.g. soldier's own
/// walking AI turning around at an obstacle), and no soldier happened to
/// reach one within this session's scripted play - noted honestly rather
/// than claimed as live-verified. Confidence instead rests on: `bg_
/// collision` itself already being cycle-exact live-verified many times
/// over (see that function's own history), and this composition's own
/// unit tests exercising every real branch (floor-upgrades-to-solid,
/// floor-stays-floor, non-floor-passthrough, and the nametable-high-bit-
/// preserving wraparound) with hand-traced bit math.
pub fn get_bg_collision_far(
    x: u8,
    y: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
) -> CollisionCode {
    let code = bg_collision(x, y, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data);
    let scratch = bg_collision_scratch(x, y, vertical_scroll, horizontal_scroll, ppuctrl_settings);
    floor_get_next_row_bg_collision(code, scratch.s13, scratch.s12, bg_collision_data)
}

/// Native port of `add_a_y_to_enemy_pos_get_bg_collision` (`$ec35`-
/// `$ec44`) - offsets an enemy's position by `(x_offset, y_offset)`
/// *without modifying its real position* and checks background
/// collision there, purely by composing [`bg_collision`] once the
/// offset position is computed (real ASM: `get_enemy_bg_collision`, the
/// entry point this jumps into, shares the exact same underlying
/// collision logic as `bg_collision` itself - confirmed by their real
/// CPU addresses being 2 bytes apart in the same fixed bank, `get_bg_
/// collision` at `$e0bb` falling straight through into `get_enemy_bg_
/// collision` at `$e0bd` after its own `sta $13`, the one extra step
/// this composition already does itself before calling in). The real Y
/// addition's overflow is a real early-exit, not an edge case to trim:
/// "exit if overflow, i.e. enemy Y position is off-screen towards
/// bottom" (real ASM comment) - returns [`CollisionCode::Empty`]
/// directly, skipping `bg_collision` entirely.
#[allow(clippy::too_many_arguments)]
pub fn add_a_y_to_enemy_pos_get_bg_collision(
    x_offset: u8,
    y_offset: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
) -> CollisionCode {
    let x_computed = x_offset.wrapping_add(enemy_x_pos);
    let (y_computed, overflowed) = y_offset.overflowing_add(enemy_y_pos);
    if overflowed {
        return CollisionCode::Empty;
    }
    bg_collision(x_computed, y_computed, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data)
}

/// Native port of `add_y_to_y_pos_get_bg_collision` (`$ec33`) - the
/// real ASM's own `lda #$00` immediately falling into `add_a_y_to_
/// enemy_pos_get_bg_collision` (zero X offset).
#[allow(clippy::too_many_arguments)]
pub fn add_y_to_y_pos_get_bg_collision(
    y_offset: u8,
    enemy_x_pos: u8,
    enemy_y_pos: u8,
    vertical_scroll: u8,
    horizontal_scroll: u8,
    ppuctrl_settings: u8,
    bg_collision_data: &[u8; BG_COLLISION_DATA_LEN],
) -> CollisionCode {
    add_a_y_to_enemy_pos_get_bg_collision(0, y_offset, enemy_x_pos, enemy_y_pos, vertical_scroll, horizontal_scroll, ppuctrl_settings, bg_collision_data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_with(offset: usize, byte: u8) -> [u8; BG_COLLISION_DATA_LEN] {
        let mut d = [0u8; BG_COLLISION_DATA_LEN];
        d[offset] = byte;
        d
    }

    #[test]
    fn empty_when_original_y_is_past_the_last_row() {
        let data = [0xFFu8; BG_COLLISION_DATA_LEN]; // would be "solid" everywhere if read
        assert_eq!(bg_collision(0, 0xE0, 0, 0, 0, &data), CollisionCode::Empty);
        assert_eq!(bg_collision(0, 0xFF, 0, 0, 0, &data), CollisionCode::Empty);
    }

    #[test]
    fn no_scroll_no_overflow_reads_the_expected_offset_and_column() {
        // x=0x10, y=0x10, no scroll: hx=0x10 -> column=(0x10>>4)&3=1,
        // hx_high_bits=0x10>>6=0. vy=0x10 -> vy_bits=(0x10>>2)&0x3c=0x04.
        // nametable bit from ppuctrl_settings=0 (bit0=0), no hx overflow ->
        // nametable_number=0 -> table[0]=0. offset = 0|0x04|0 = 0x04.
        // column=1 -> shift 4. Put code=2 (water) in bits 4-5 of that byte.
        let data = data_with(0x04, 0b0010_0000);
        assert_eq!(bg_collision(0x10, 0x10, 0, 0, 0, &data), CollisionCode::Water);
    }

    #[test]
    fn vertical_scroll_overflow_still_adds_the_wraparound_correction() {
        // y=200, vertical_scroll=100: raw sum=300, truncates to 44 (0x2c),
        // overflowed=true -> vy = 44+16 = 60 (0x3c). vy_bits=(60>>2)&0x3c=0x0c.
        // x=0, horizontal_scroll=0 -> hx=0 -> hx_high_bits=0, column=0 (shift 6).
        // offset = hx_high_bits(0) | vy_bits(0x0c) | table[0](0) = 0x0c.
        let data = data_with(0x0C, CollisionCode::Floor.to_raw_byte() << 6);
        let result = bg_collision(0x00, 200, 100, 0, 0, &data);
        assert_eq!(result, CollisionCode::Floor);
    }

    #[test]
    fn horizontal_overflow_flips_the_nametable_bit() {
        // x=0x80, horizontal_scroll=0x90: sum=0x110, overflows a u8.
        // base nametable bit (ppuctrl_settings & 1) = 1, flipped -> 0.
        // table[0]=0x00. hx (wrapped) = 0x10, hx_high_bits=0, column=(0x10>>4)&3=1.
        // y=0, vertical_scroll=0 -> vy=0, vy_bits=0. offset=0.
        let data = data_with(0x00, 0b0000_0011 << 4); // column 1 -> shift 4 -> code 3 (solid)
        let result = bg_collision(0x80, 0x00, 0x00, 0x90, 0x01, &data);
        assert_eq!(result, CollisionCode::Solid);
    }

    #[test]
    fn to_raw_byte_matches_the_documented_lookup_table_values() {
        assert_eq!(CollisionCode::Empty.to_raw_byte(), 0x00);
        assert_eq!(CollisionCode::Floor.to_raw_byte(), 0x01);
        assert_eq!(CollisionCode::Water.to_raw_byte(), 0x02);
        assert_eq!(CollisionCode::Solid.to_raw_byte(), 0x80);
    }

    /// Every one of the 30 (x, y, vertical_scroll, horizontal_scroll) ->
    /// cycles pairs from `EXHAUSTIVE_BG_COLLISION_CYCLES=1`'s real,
    /// synthetic-`jsr` measurement against the actual ROM (see
    /// `bg_collision_cycles`'s doc comment) - this is the ground truth the
    /// formula was derived *from*, encoded as a regression test so it can't
    /// silently drift from what real hardware actually does.
    #[test]
    fn bg_collision_cycles_matches_the_exhaustive_real_hardware_measurement() {
        let cases: &[(u8, u8, u8, u8, u64)] = &[
            // row guard OFF (y=0x10 < 0xe0)
            (0x00, 0x10, 0x00, 0x00, 158), // vy=none hx=no-of col0
            (0x10, 0x10, 0x00, 0x00, 158), // col1
            (0x20, 0x10, 0x00, 0x00, 158), // col2
            (0x30, 0x10, 0x00, 0x00, 156), // col3
            (0x10, 0x10, 0x00, 0xF0, 159), // vy=none hx=overflow col0
            (0x20, 0x10, 0x00, 0xF0, 159),
            (0x30, 0x10, 0x00, 0xF0, 159),
            (0x40, 0x10, 0x00, 0xF0, 157),
            (0x00, 0x10, 0xE0, 0x00, 159), // vy=cmp hx=no-of
            (0x10, 0x10, 0xE0, 0x00, 159),
            (0x20, 0x10, 0xE0, 0x00, 159),
            (0x30, 0x10, 0xE0, 0x00, 157),
            (0x10, 0x10, 0xE0, 0xF0, 160), // vy=cmp hx=overflow
            (0x20, 0x10, 0xE0, 0xF0, 160),
            (0x30, 0x10, 0xE0, 0xF0, 160),
            (0x40, 0x10, 0xE0, 0xF0, 158),
            (0x00, 0x10, 0xF5, 0x00, 156), // vy=overflow hx=no-of
            (0x10, 0x10, 0xF5, 0x00, 156),
            (0x20, 0x10, 0xF5, 0x00, 156),
            (0x30, 0x10, 0xF5, 0x00, 154),
            (0x10, 0x10, 0xF5, 0xF0, 157), // vy=overflow hx=overflow
            (0x20, 0x10, 0xF5, 0xF0, 157),
            (0x30, 0x10, 0xF5, 0xF0, 157),
            (0x40, 0x10, 0xF5, 0xF0, 155),
            // row guard ON (y=0xe0 >= 0xe0) - column irrelevant
            (0x00, 0xE0, 0x00, 0x00, 129), // vy=none hx=no-of
            (0x10, 0xE0, 0x00, 0xF0, 130), // vy=none hx=overflow
            (0x00, 0xE0, 0x10, 0x00, 130), // vy=cmp hx=no-of
            (0x10, 0xE0, 0x10, 0xF0, 131), // vy=cmp hx=overflow
            (0x00, 0xE0, 0x20, 0x00, 127), // vy=overflow hx=no-of
            (0x10, 0xE0, 0x20, 0xF0, 128), // vy=overflow hx=overflow
        ];
        for &(x, y, vs, hs, expected) in cases {
            assert_eq!(
                bg_collision_cycles(x, y, vs, hs),
                expected,
                "x={x:#04x} y={y:#04x} vs={vs:#04x} hs={hs:#04x}"
            );
        }
    }

    #[test]
    fn read_bg_collision_byte_unsafe_matches_bg_collisions_own_column_shift() {
        // Same worked example as `no_scroll_no_overflow_reads_the_expected_offset_and_column`
        let data = data_with(0x04, 0b0010_0000);
        assert_eq!(read_bg_collision_byte_unsafe(&data, 0x04, 1), CollisionCode::Water);
    }

    #[test]
    fn read_bg_collision_byte_unsafe_ignores_the_row_guard() {
        // Real "unsafe" distinction: bg_collision would return Empty for
        // y>=0xe0 regardless of data, but this reads real data at any
        // offset the caller supplies. Raw 2-bit code 3 (Solid) at column
        // 0 (bits 6-7): 0b11 << 6 = 0xc0.
        let data = data_with(0x10, 0b1100_0000);
        assert_eq!(read_bg_collision_byte_unsafe(&data, 0x10, 0), CollisionCode::Solid);
    }

    #[test]
    fn floor_get_next_row_passes_through_non_floor_codes_unchanged() {
        let data = [0xFFu8; BG_COLLISION_DATA_LEN]; // would be Solid everywhere if read
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Empty, 0x00, 0, &data), CollisionCode::Empty);
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Water, 0x00, 0, &data), CollisionCode::Water);
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Solid, 0x00, 0, &data), CollisionCode::Solid);
    }

    #[test]
    fn floor_get_next_row_upgrades_to_solid_when_the_row_below_is_solid() {
        // offset=0x00, +4 -> 0x04, column 0 -> bits 6-7, raw code 3 (Solid).
        let data = data_with(0x04, 0b1100_0000);
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Floor, 0x00, 0, &data), CollisionCode::Solid);
    }

    #[test]
    fn floor_get_next_row_stays_floor_when_the_row_below_is_not_solid() {
        let data = data_with(0x04, 0b0100_0000); // column 0 -> bits 6-7 -> Floor, not Solid
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Floor, 0x00, 0, &data), CollisionCode::Floor);
    }

    #[test]
    fn floor_get_next_row_preserves_the_nametable_high_bits_when_wrapping() {
        // offset=0x7e (nametable-select bits 6-7 = 0x40, low bits=0x3e):
        // 0x3e+4=0x42, &0x3f=0x02, |0x40=0x42. Not a wrap in this case,
        // but confirms the high bits survive the round trip when the low
        // bits *don't* overflow past 0x3f.
        let data = data_with(0x42, 0b1100_0000);
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Floor, 0x7E, 0, &data), CollisionCode::Solid);
        // Now force an actual low-bit wrap: offset=0x7f (low=0x3f, high=0x40):
        // 0x3f+4=0x43, &0x3f=0x03, |0x40=0x43 - still within the same
        // nametable half, high bits preserved, not bled into the other one.
        let data2 = data_with(0x43, 0b1100_0000);
        assert_eq!(floor_get_next_row_bg_collision(CollisionCode::Floor, 0x7F, 0, &data2), CollisionCode::Solid);
    }

    #[test]
    fn get_bg_collision_far_composes_bg_collision_and_the_floor_lookahead() {
        // Same offset/column worked example: x=0x10,y=0x10 -> offset=0x04,
        // column=1 (shift 4). Put Floor there, and Solid one row below
        // (offset 0x08, same column).
        let mut data = [0u8; BG_COLLISION_DATA_LEN];
        data[0x04] = 0b0001_0000; // column 1 (bits 4-5), raw code 1 = Floor
        // offset 0x08 (0x04+4) left at all-zero: column 1 there is raw
        // code 0 (Empty), not Solid - confirm no upgrade happens yet.
        assert_eq!(get_bg_collision_far(0x10, 0x10, 0, 0, 0, &data), CollisionCode::Floor);
        // now put Solid (raw code 3) in column 1 (bits 4-5) at the row below:
        data[0x08] = 0b0011_0000;
        assert_eq!(get_bg_collision_far(0x10, 0x10, 0, 0, 0, &data), CollisionCode::Solid);
    }

    #[test]
    fn add_a_y_to_enemy_pos_get_bg_collision_offsets_before_checking() {
        // enemy at (0x00, 0x00), offset (0x10, 0x10) -> checks the same
        // position/column as the worked example above.
        let data = data_with(0x04, 0b0010_0000); // column 1 -> Water
        assert_eq!(add_a_y_to_enemy_pos_get_bg_collision(0x10, 0x10, 0x00, 0x00, 0, 0, 0, &data), CollisionCode::Water);
        // matches calling bg_collision directly at the already-offset position
        assert_eq!(
            add_a_y_to_enemy_pos_get_bg_collision(0x10, 0x10, 0x00, 0x00, 0, 0, 0, &data),
            bg_collision(0x10, 0x10, 0, 0, 0, &data)
        );
    }

    #[test]
    fn add_a_y_to_enemy_pos_get_bg_collision_real_position_is_never_modified() {
        // Real ASM comment: "ENEMY_X_POS and ENEMY_Y_POS are unaffected" -
        // this port takes them by value and returns only a collision
        // code, so there's nothing to mutate; this test just documents
        // the guarantee by confirming two calls with the same inputs
        // (as if made "twice in a row") give identical results.
        let data = data_with(0x04, 0b0010_0000);
        let first = add_a_y_to_enemy_pos_get_bg_collision(0x10, 0x10, 0x00, 0x00, 0, 0, 0, &data);
        let second = add_a_y_to_enemy_pos_get_bg_collision(0x10, 0x10, 0x00, 0x00, 0, 0, 0, &data);
        assert_eq!(first, second);
    }

    #[test]
    fn add_a_y_to_enemy_pos_get_bg_collision_y_overflow_is_empty_without_checking() {
        // y_offset + enemy_y_pos overflows a byte -> real ASM exits
        // immediately with Empty, never calling into bg_collision at all
        // (confirmed by using a data buffer that would report Solid
        // everywhere if it were actually read).
        let data = [0xFFu8; BG_COLLISION_DATA_LEN];
        assert_eq!(add_a_y_to_enemy_pos_get_bg_collision(0x00, 0xFF, 0x00, 0x02, 0, 0, 0, &data), CollisionCode::Empty);
    }

    #[test]
    fn add_y_to_y_pos_get_bg_collision_is_the_zero_x_offset_case() {
        let data = data_with(0x04, 0b0010_0000);
        assert_eq!(
            add_y_to_y_pos_get_bg_collision(0x10, 0x10, 0x00, 0, 0, 0, &data),
            add_a_y_to_enemy_pos_get_bg_collision(0, 0x10, 0x10, 0x00, 0, 0, 0, &data)
        );
    }
}
