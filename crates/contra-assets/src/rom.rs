//! iNES ROM loading + identity validation.
//!
//! We only ever read the file the user points us at; nothing here fetches,
//! bundles, or caches copyrighted data anywhere in this repository.

use md5::{Digest, Md5};
use std::path::Path;
use thiserror::Error;

/// MD5 of the US retail *Contra* (NES) ROM, as documented by the public
/// disassembly project this port is built against
/// (<https://github.com/vermiceli/nes-contra-us>, README "Prerequisites").
/// Used only to tell the user *which* game they pointed us at — we never
/// ship, embed, or redistribute the ROM itself.
pub const CONTRA_US_MD5: &str = "7bdad8b4a7a56a634c9649d20bd3011b";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomIdentity {
    ContraUs,
    Probotector,
    Unknown,
}

#[derive(Debug, Error)]
pub enum RomLoadError {
    #[error("could not read ROM file: {0}")]
    Io(#[from] std::io::Error),
    #[error("file is too small to be a valid iNES ROM ({0} bytes)")]
    TooSmall(usize),
    #[error("missing 'NES\\x1A' iNES header magic")]
    BadMagic,
}

/// A loaded iNES ROM. Only the header is interpreted here; PRG/CHR bytes
/// are exposed as raw slices for higher-level (not-yet-implemented, see
/// ROADMAP.md) decoders to work with.
pub struct NesRom {
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub mapper: u8,
    pub md5_hex: String,
}

impl NesRom {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, RomLoadError> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, RomLoadError> {
        const HEADER_LEN: usize = 16;
        if bytes.len() < HEADER_LEN {
            return Err(RomLoadError::TooSmall(bytes.len()));
        }
        if &bytes[0..4] != b"NES\x1A" {
            return Err(RomLoadError::BadMagic);
        }

        let prg_units = bytes[4] as usize; // 16 KiB units
        let chr_units = bytes[5] as usize; // 8 KiB units
        let flags6 = bytes[6];
        let flags7 = bytes[7];
        let mapper = (flags6 >> 4) | (flags7 & 0xF0);

        let has_trainer = flags6 & 0x04 != 0;
        let mut offset = HEADER_LEN;
        if has_trainer {
            offset += 512;
        }

        let prg_len = prg_units * 16 * 1024;
        let chr_len = chr_units * 8 * 1024;

        let prg_rom = bytes.get(offset..offset + prg_len).unwrap_or_default().to_vec();
        let chr_rom = bytes
            .get(offset + prg_len..offset + prg_len + chr_len)
            .unwrap_or_default()
            .to_vec();

        let mut hasher = Md5::new();
        hasher.update(bytes);
        let md5_hex = format!("{:x}", hasher.finalize());

        Ok(Self { prg_rom, chr_rom, mapper, md5_hex })
    }

    pub fn identity(&self) -> RomIdentity {
        if self.md5_hex.eq_ignore_ascii_case(CONTRA_US_MD5) {
            RomIdentity::ContraUs
        } else {
            // Probotector's PAL/EU MD5 varies by dump; anything that isn't
            // the known US hash but still parses as a UxROM iNES file with
            // the right sizes is reported Unknown rather than guessed at.
            RomIdentity::Unknown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_ines(prg_units: u8, chr_units: u8, extra_prg_byte: u8) -> Vec<u8> {
        let mut v = vec![0u8; 16];
        v[0..4].copy_from_slice(b"NES\x1A");
        v[4] = prg_units;
        v[5] = chr_units;
        v.extend(vec![extra_prg_byte; prg_units as usize * 16 * 1024]);
        v.extend(vec![0u8; chr_units as usize * 8 * 1024]);
        v
    }

    #[test]
    fn parses_header_and_slices_correctly() {
        let bytes = fake_ines(2, 1, 0xAB);
        let rom = NesRom::from_bytes(&bytes).unwrap();
        assert_eq!(rom.prg_rom.len(), 2 * 16 * 1024);
        assert_eq!(rom.chr_rom.len(), 8 * 1024);
        assert_eq!(rom.prg_rom[0], 0xAB);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = fake_ines(1, 1, 0);
        bytes[0] = b'X';
        assert!(matches!(NesRom::from_bytes(&bytes), Err(RomLoadError::BadMagic)));
    }

    #[test]
    fn unknown_rom_is_reported_as_unknown_not_guessed() {
        let bytes = fake_ines(1, 1, 0);
        let rom = NesRom::from_bytes(&bytes).unwrap();
        assert_eq!(rom.identity(), RomIdentity::Unknown);
    }
}
