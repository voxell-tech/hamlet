//! Colours as the world's files write them: `"#rrggbb"` in sRGB, or
//! their parts as numbers. Each is read as the linear RGB a mesh, a
//! material or a light is given. `linear` reads numbers as linear
//! already; the module itself reads them as sRGB.

use glam::Vec3;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

/// A colour as a file writes it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Written {
    Hex(String),
    Parts(Vec<f32>),
}

impl Written {
    /// The colour's sRGB parts.
    fn srgb<E: Error>(self) -> Result<Vec3, E> {
        match self {
            Self::Hex(hex) => parse(&hex).ok_or_else(|| {
                E::custom(format!("{hex} is no colour"))
            }),
            Self::Parts(parts) => parts3(&parts),
        }
    }

    /// Numbers are linear already; a hex colour is sRGB.
    fn linear<E: Error>(self) -> Result<Vec3, E> {
        match self {
            Self::Parts(parts) => parts3(&parts),
            hex => hex.srgb().map(to_linear),
        }
    }
}

fn parts3<E: Error>(parts: &[f32]) -> Result<Vec3, E> {
    match parts {
        [r, g, b] | [r, g, b, _] => Ok(Vec3::new(*r, *g, *b)),
        _ => Err(E::custom("a colour has 3 or 4 parts")),
    }
}

/// `#rrggbb` or `#rrggbbaa`, its alpha left out, in 0..1.
fn parse(hex: &str) -> Option<Vec3> {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    if !matches!(digits.len(), 6 | 8) {
        return None;
    }
    let part = |at: usize| {
        u8::from_str_radix(digits.get(at..at + 2)?, 16)
            .ok()
            .map(|byte| f32::from(byte) / 255.0)
    };
    Some(Vec3::new(part(0)?, part(2)?, part(4)?))
}

/// An sRGB colour as linear RGB.
pub fn to_linear(srgb: Vec3) -> Vec3 {
    let channel = |c: f32| {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    Vec3::new(channel(srgb.x), channel(srgb.y), channel(srgb.z))
}

/// A colour written in sRGB, as linear RGB.
pub fn deserialize<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<Vec3, D::Error> {
    Written::deserialize(from)?.srgb().map(to_linear)
}

/// A colour whose numbers are linear.
pub mod linear {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        from: D,
    ) -> Result<Vec3, D::Error> {
        Written::deserialize(from)?.linear()
    }
}

/// A list of colours whose numbers are linear.
pub mod linear_list {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        from: D,
    ) -> Result<[Vec3; N], D::Error> {
        let colours = Vec::<Written>::deserialize(from)?
            .into_iter()
            .map(Written::linear)
            .collect::<Result<Vec<_>, _>>()?;
        let count = colours.len();
        colours.try_into().map_err(|_| {
            D::Error::custom(format!("{count} colours, not {N}"))
        })
    }
}
