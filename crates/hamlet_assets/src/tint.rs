//! A colour as a file writes it: `"#rrggbb"`, `"#rrggbbaa"`, or its
//! parts as numbers. `#[serde(with = "tint")]` reads a [`Color`], and
//! `tint::linear` the linear RGB a mesh is coloured with.

use bevy::prelude::*;
use serde::de::Error;
use serde::{Deserialize, Deserializer};

#[derive(Deserialize)]
#[serde(untagged)]
enum Written {
    Hex(String),
    Parts(Vec<f32>),
}

impl Written {
    fn srgb<E: Error>(self) -> Result<Srgba, E> {
        match self {
            Self::Hex(hex) => Srgba::hex(&hex).map_err(|_| {
                E::custom(format!("`{hex}` is not a hex colour"))
            }),
            Self::Parts(parts) => match parts[..] {
                [r, g, b] => Ok(Srgba::rgb(r, g, b)),
                [r, g, b, a] => Ok(Srgba::new(r, g, b, a)),
                _ => Err(E::custom("a colour has 3 or 4 parts")),
            },
        }
    }

    /// Numbers are linear already; a hex colour is sRGB.
    fn linear<E: Error>(self) -> Result<Vec3, E> {
        match self {
            Self::Parts(parts) => match parts[..] {
                [r, g, b] => Ok(Vec3::new(r, g, b)),
                _ => Err(E::custom("a linear colour has 3 parts")),
            },
            hex => Ok(LinearRgba::from(hex.srgb()?).to_vec3()),
        }
    }
}

/// Linear RGB from an sRGB hex string such as `#C9B98F`.
pub fn hex(s: &str) -> Vec3 {
    LinearRgba::from(Srgba::hex(s).expect("valid hex colour"))
        .to_vec3()
}

pub fn deserialize<'de, D: Deserializer<'de>>(
    from: D,
) -> Result<Color, D::Error> {
    Written::deserialize(from)?.srgb().map(Color::Srgba)
}

pub mod linear {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>>(
        from: D,
    ) -> Result<Vec3, D::Error> {
        Written::deserialize(from)?.linear()
    }
}

pub mod linear_list {
    use super::*;

    pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(
        from: D,
    ) -> Result<[Vec3; N], D::Error> {
        let colours = Vec::<Written>::deserialize(from)?
            .into_iter()
            .map(Written::linear)
            .collect::<Result<Vec<_>, _>>()?;
        let found = colours.len();
        colours.try_into().map_err(|_| {
            D::Error::custom(format!(
                "{N} colours wanted, {found} written"
            ))
        })
    }
}
