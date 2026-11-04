//! Seeds mixed from parts with integer arithmetic only, so the same
//! parts give the same seed on every platform.

use serde::{Deserialize, Serialize};

/// A 64-bit seed.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Debug,
    Default,
    Serialize,
    Deserialize,
)]
#[serde(transparent)]
pub struct Seed(pub u64);

impl Seed {
    /// This seed mixed with `part`. The order of mixing matters.
    pub fn mix(self, part: u64) -> Self {
        Self(splitmix(self.0 ^ splitmix(part)))
    }

    /// This seed mixed with the bytes of `part`.
    pub fn mix_str(self, part: &str) -> Self {
        // FNV-1a: a name becomes one part.
        let hash =
            part.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
                (hash ^ u64::from(byte))
                    .wrapping_mul(0x0100_0000_01b3)
            });
        self.mix(hash)
    }

    /// A number in `[0, 1)`.
    pub fn unit(self) -> f32 {
        (splitmix(self.0) >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn splitmix(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
