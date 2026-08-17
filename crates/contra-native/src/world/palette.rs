//! Native port of Contra's palette resolution: the NES 2C02's fixed master
//! palette, the `game_palettes` lookup table (`$d227`, bank 7 - a flat
//! table of NES-palette-index bytes, 3 per group, ported from
//! `load_palette_colors_to_cpu` in `bank7.asm`), and level headers'
//! `LEVEL_PALETTE_INDEX` bytes that select which groups a level actually
//! uses (`level_headers`, bank 2 `$b319` - see
//! `docs/Level Headers.md`/`docs/Graphics Documentation.md`'s "Palette"
//! section in `vermiceli/nes-contra-us` for the format this is ported
//! from).
//!
//! Every PPU 4-color palette the real game builds always starts with a
//! hard-coded universal black (`lda #$0f` in `load_palette_colors_to_cpu`,
//! before it ever reads `game_palettes`); the remaining 3 colors come from
//! one `game_palettes` group, selected by a `LEVEL_PALETTE_INDEX` byte.

/// The NES 2C02's master palette isn't ROM data - it's a fixed property of
/// the PPU chip itself, identical across every NES game. Deliberately its
/// own copy here rather than a dependency on `contra-nes` (this crate's
/// point is to stand on its own, without an emulator underneath), kept
/// byte-identical to `contra_nes::ppu::NES_PALETTE` by a cross-check test
/// in `crates/contra-nes/examples/extract_graphics.rs`. Packed
/// `0x00RRGGBB`, indexed by a raw 6-bit NES palette index (`& 0x3F`).
#[rustfmt::skip]
pub const NES_MASTER_PALETTE: [u32; 64] = [
    0x00666666, 0x00002A88, 0x001412A7, 0x003B00A4, 0x005C007E, 0x006E0040, 0x006C0600, 0x00561D00,
    0x00333500, 0x000B4800, 0x00005200, 0x00004F08, 0x0000404D, 0x00000000, 0x00000000, 0x00000000,
    0x00ADADAD, 0x00155FD9, 0x004240FF, 0x007527FE, 0x00A01ACC, 0x00B71E7B, 0x00B53120, 0x00994E00,
    0x006B6D00, 0x00388700, 0x000C9300, 0x00008F32, 0x00007C8D, 0x00000000, 0x00000000, 0x00000000,
    0x00FFFEFF, 0x0064B0FF, 0x009290FF, 0x00C676FF, 0x00F36AFF, 0x00FE6ECC, 0x00FE8170, 0x00EA9E22,
    0x00BCBE00, 0x0088D800, 0x005CE430, 0x0045E082, 0x0048CDDE, 0x004F4F4F, 0x00000000, 0x00000000,
    0x00FFFEFF, 0x00C0DFFF, 0x00D3D2FF, 0x00E8C8FF, 0x00FBC2FF, 0x00FEC4EA, 0x00FECCC5, 0x00F7D8A5,
    0x00E4E594, 0x00CFEF96, 0x00BDF4AB, 0x00B3F3CC, 0x00B5EBF2, 0x00B8B8B8, 0x00000000, 0x00000000,
];

/// `game_palettes` (bank 7, CPU `$d227` - fixed bank, so no bank-switch
/// ambiguity): PRG-ROM offset = `7*0x4000 + (0xd227-0xc000)`.
pub const GAME_PALETTES_PRG_OFFSET: usize = 0x1D227;
/// `$6e * $03 = 0x14a` bytes per `game_palettes`'s own comment: 110 groups
/// of 3 raw NES-palette-index bytes each.
pub const GAME_PALETTES_LEN: usize = 0x14A;

/// `level_headers` (bank 2, CPU `$b319`): PRG-ROM offset =
/// `2*0x4000 + (0xb319-0x8000)`.
pub const LEVEL_HEADERS_PRG_OFFSET: usize = 0xB319;
/// Confirmed against `src/bank2.asm`'s literal header layout (location
/// type, scroll type, 3 pointers, alt-graphics byte, 3 collision bytes, 4
/// palette-cycle bytes, then this) and cross-checked independently via
/// `LEVEL_PALETTE_INDEX`'s RAM address ($50) minus `LEVEL_LOCATION_TYPE`'s
/// ($40) - both agree on 16.
pub const LEVEL_PALETTE_INDEX_OFFSET_IN_HEADER: usize = 16;
pub const LEVEL_HEADER_LEN: usize = 32;

/// Byte offset of level `level_index` (0-based, so level 1 = `0`)'s header
/// within PRG-ROM.
pub fn level_header_prg_offset(level_index: usize) -> usize {
    LEVEL_HEADERS_PRG_OFFSET + level_index * LEVEL_HEADER_LEN
}

/// The 8 `LEVEL_PALETTE_INDEX` bytes for one level: 4 `game_palettes`
/// group indexes for the nametable (background) palettes, then 4 for the
/// sprite palettes - read directly from the level's header in PRG-ROM, no
/// decompression needed (level headers are plain data).
pub fn level_palette_group_indexes(prg_rom: &[u8], level_index: usize) -> [u8; 8] {
    let start = level_header_prg_offset(level_index) + LEVEL_PALETTE_INDEX_OFFSET_IN_HEADER;
    prg_rom[start..start + 8].try_into().unwrap()
}

/// Resolves one `game_palettes` group index into its 3 raw NES-palette
/// color bytes (`lda ($06),y` reading 3 bytes starting at
/// `game_palettes + group_index*3`, per `load_palette_colors_to_cpu`).
pub fn palette_group_raw(prg_rom: &[u8], group_index: u8) -> [u8; 3] {
    let start = GAME_PALETTES_PRG_OFFSET + group_index as usize * 3;
    prg_rom[start..start + 3].try_into().unwrap()
}

/// A full 4-color PPU palette for one `LEVEL_PALETTE_INDEX` group: the
/// hard-coded universal black followed by the group's 3 `game_palettes`
/// colors, all resolved to packed `0x00RRGGBB` via [`NES_MASTER_PALETTE`].
pub fn resolve_palette_rgb(prg_rom: &[u8], group_index: u8) -> [u32; 4] {
    let raw = palette_group_raw(prg_rom, group_index);
    [
        NES_MASTER_PALETTE[0x0f],
        NES_MASTER_PALETTE[(raw[0] & 0x3F) as usize],
        NES_MASTER_PALETTE[(raw[1] & 0x3F) as usize],
        NES_MASTER_PALETTE[(raw[2] & 0x3F) as usize],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_rom() -> Vec<u8> {
        let mut rom = vec![0u8; GAME_PALETTES_PRG_OFFSET + GAME_PALETTES_LEN];
        // group 0: colors 0x37, 0x12, 0x0f (arbitrary but distinct)
        rom[GAME_PALETTES_PRG_OFFSET] = 0x37;
        rom[GAME_PALETTES_PRG_OFFSET + 1] = 0x12;
        rom[GAME_PALETTES_PRG_OFFSET + 2] = 0x0f;
        // group 2: colors 0x19, 0x29, 0x08 (matches real level 1's group $02, coincidentally the real values from bank7.asm's third .byte line)
        rom[GAME_PALETTES_PRG_OFFSET + 6] = 0x19;
        rom[GAME_PALETTES_PRG_OFFSET + 7] = 0x29;
        rom[GAME_PALETTES_PRG_OFFSET + 8] = 0x08;
        rom
    }

    #[test]
    fn resolves_group_zero_with_hardcoded_black_first() {
        let rom = fake_rom();
        let resolved = resolve_palette_rgb(&rom, 0);
        assert_eq!(resolved[0], NES_MASTER_PALETTE[0x0f]);
        assert_eq!(resolved[1], NES_MASTER_PALETTE[0x37]);
        assert_eq!(resolved[2], NES_MASTER_PALETTE[0x12]);
        assert_eq!(resolved[3], NES_MASTER_PALETTE[0x0f]);
    }

    #[test]
    fn group_index_multiplies_by_three_bytes() {
        let rom = fake_rom();
        let resolved = resolve_palette_rgb(&rom, 2);
        assert_eq!(resolved[1], NES_MASTER_PALETTE[0x19]);
        assert_eq!(resolved[2], NES_MASTER_PALETTE[0x29]);
        assert_eq!(resolved[3], NES_MASTER_PALETTE[0x08]);
    }

    #[test]
    fn level_1_header_palette_indexes_match_the_real_source_literal() {
        // src/bank2.asm's level_1_header: `.byte $02,$03,$04,$05` (bg) then
        // `.byte $00,$01,$22,$07` (sprite), at header offset 16 - this test
        // just locks in the offset arithmetic against a synthetic ROM laid
        // out the same way, since a real ROM isn't available in CI.
        let mut rom = vec![0u8; LEVEL_HEADERS_PRG_OFFSET + LEVEL_HEADER_LEN];
        let h = LEVEL_HEADERS_PRG_OFFSET;
        rom[h + 16..h + 20].copy_from_slice(&[0x02, 0x03, 0x04, 0x05]);
        rom[h + 20..h + 24].copy_from_slice(&[0x00, 0x01, 0x22, 0x07]);
        assert_eq!(level_palette_group_indexes(&rom, 0), [0x02, 0x03, 0x04, 0x05, 0x00, 0x01, 0x22, 0x07]);
    }
}
