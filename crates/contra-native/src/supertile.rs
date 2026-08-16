//! Native port of Contra's super-tile system: how a level's nametable
//! (which pattern-table tile goes where) and attribute table (which
//! palette applies where) are actually built, as opposed to `graphics`
//! (which pattern-table *tiles exist at all*) or `palette` (which *colors*
//! a palette group resolves to). Ported from `read_supertiles_screen_ptr_
//! table`/`load_supertile_indexes_starting_at_y` (bank 7, `$e16b`) and the
//! plain (uncompressed) `level_X_supertile_data`/`level_X_palette_data`
//! tables (bank 3) - see `docs/Graphics Documentation.md`'s "Super-Tiles"
//! section in `vermiceli/nes-contra-us` for the format this is ported
//! from.
//!
//! A super-tile is a 4x4 block of pattern-table tiles (16 bytes, one tile
//! index per byte) that also happens to be exactly the NES's attribute-
//! table granularity - so `level_X_palette_data[supertile_id]` is
//! literally one real PPU attribute-table byte (4 packed 2-bit palette
//! codes, one per 2x2-tile quadrant) for whichever screen position that
//! super-tile ID currently occupies. A level's on-screen layout (which
//! super-tile ID sits at which of the 56 (horizontal) or 64 (vertical)
//! positions in a screen) is itself RLE-compressed with a *different*
//! algorithm than `graphics`'s - including a back-reference command for
//! repeating an earlier row verbatim - decoded by [`decompress_screen`].

/// Decompresses one `level_X_supertiles_screen_XX` blob into the flat list
/// of super-tile IDs it specifies, stopping once `expected_len` IDs have
/// been produced (matching the real routine's `cpx #$38`/`cpx #$40` exit
/// check - horizontal levels are 56 (`0x38`) IDs per screen in an 8-wide,
/// 7-tall grid; vertical levels are 64 (`0x40`) in an 8x8 grid; the source
/// data itself carries no end marker, unlike `graphics`'s `0xFF`).
///
/// Command bytes (read from `data` in order):
/// - `< 0x80` - a literal super-tile ID, appended as-is.
/// - `0x80..=0xEF` - RLE run: bits 0-6 are a repeat count, and the
///   following byte is that ID repeated that many times.
/// - `0xF0..=0xFF` - row back-reference: bits 0-3 select one of the
///   *already-decoded* rows of this same screen (row `r` = IDs
///   `[r*8, r*8+8)`), and those 8 IDs are appended again verbatim. This is
///   how, e.g., a flat stretch of ground reuses an earlier row's tiling
///   without repeating it in the compressed data.
pub fn decompress_screen(data: &[u8], expected_len: usize) -> Vec<u8> {
    let mut pos = 0usize;
    let mut out = Vec::with_capacity(expected_len);

    while out.len() < expected_len {
        let b = data[pos];
        pos += 1;
        if b < 0x80 {
            out.push(b);
        } else if b < 0xf0 {
            let count = b & 0x7f;
            let value = data[pos];
            pos += 1;
            for _ in 0..count {
                out.push(value);
            }
        } else {
            let row = (b & 0x0f) as usize;
            let src = row * 8;
            for i in 0..8 {
                out.push(out[src + i]);
            }
        }
    }

    out.truncate(expected_len);
    out
}

/// One super-tile's 16 pattern-table tile indices (a 4x4 grid, row-major:
/// `tiles[row*4+col]`), read directly from `level_X_supertile_data` -
/// plain data, no compression (`docs/Level Headers.md`: "This data is not
/// RLE-encoded").
pub fn supertile_tiles(supertile_data: &[u8], supertile_id: u8) -> [u8; 16] {
    let start = supertile_id as usize * 16;
    supertile_data[start..start + 16].try_into().unwrap()
}

/// One super-tile's packed attribute byte, read directly from
/// `level_X_palette_data` - also plain, uncompressed data, one byte per
/// super-tile ID (`(LEVEL_SUPERTILE_PALETTE_DATA),y` in `bank7.asm`,
/// indexed by super-tile ID exactly like `supertile_tiles`).
pub fn supertile_attribute_byte(palette_data: &[u8], supertile_id: u8) -> u8 {
    palette_data[supertile_id as usize]
}

/// Splits a packed attribute byte into its 4 quadrants' `game_palettes`
/// group... no - into its 4 quadrants' raw 2-bit palette *codes* (0-3,
/// selecting which of a level's 4 nametable palettes applies), in the
/// standard NES order: `[top_left, top_right, bottom_left, bottom_right]`,
/// each quadrant covering a 2x2-tile area of the super-tile.
pub fn attribute_quadrants(attribute_byte: u8) -> [u8; 4] {
    [attribute_byte & 0x03, (attribute_byte >> 2) & 0x03, (attribute_byte >> 4) & 0x03, (attribute_byte >> 6) & 0x03]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_bytes_pass_through_unchanged() {
        let data = [0x01, 0x02, 0x03];
        assert_eq!(decompress_screen(&data, 3), vec![0x01, 0x02, 0x03]);
    }

    #[test]
    fn rle_run_repeats_the_following_byte() {
        // 0x85 = repeat count 5 (0x80 | 5), then value 0x07
        let data = [0x85, 0x07, 0x01];
        assert_eq!(decompress_screen(&data, 6), vec![0x07, 0x07, 0x07, 0x07, 0x07, 0x01]);
    }

    #[test]
    fn row_backreference_repeats_an_earlier_row_of_eight() {
        let mut data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]; // row 0, literal
        data.push(0xf0); // back-reference to row 0
        data.push(0x09); // extra literal after, to prove decoding continues correctly
        let out = decompress_screen(&data, 17);
        assert_eq!(&out[0..8], &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
        assert_eq!(&out[8..16], &[0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
        assert_eq!(out[16], 0x09);
    }

    #[test]
    fn supertile_tiles_reads_16_bytes_at_the_right_offset() {
        let mut data = vec![0u8; 32];
        data[16..32].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(supertile_tiles(&data, 1), [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
    }

    #[test]
    fn attribute_quadrants_unpacks_in_standard_nes_order() {
        // 0b11_10_01_00: bottom_right=3, bottom_left=2, top_right=1, top_left=0
        assert_eq!(attribute_quadrants(0b11_10_01_00), [0, 1, 2, 3]);
    }
}
