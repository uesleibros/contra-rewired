//! Manual (de)serialize helpers for the handful of fixed-size byte arrays
//! in this crate that are larger than serde's derive supports out of the
//! box, so the crate doesn't need an external `serde-big-array` dependency
//! for a few fields. One module per distinct size, reused wherever that
//! size appears (e.g. `arr_0x800` backs both PPU VRAM and CPU RAM).

macro_rules! big_array_module {
    ($name:ident, $len:expr) => {
        pub mod $name {
            use serde::{Deserializer, Serializer};

            pub fn serialize<S: Serializer>(arr: &[u8; $len], s: S) -> Result<S::Ok, S::Error> {
                s.serialize_bytes(arr)
            }

            pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; $len], D::Error> {
                let bytes: Vec<u8> = serde::Deserialize::deserialize(d)?;
                let mut arr = [0u8; $len];
                let len = bytes.len().min($len);
                arr[..len].copy_from_slice(&bytes[..len]);
                Ok(arr)
            }
        }
    };
}

big_array_module!(arr_0x800, 0x800);
big_array_module!(arr_256, 256);
big_array_module!(arr_0x2000, 0x2000);
