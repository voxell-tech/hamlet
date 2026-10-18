//! A chief's look, and the file it is kept in.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::palette::Palette;

/// The colour of the cloak, and of the roof at home.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Cloak {
    #[default]
    River,
    Ember,
    Moss,
    Plum,
    Mustard,
    Rose,
    Slate,
    Cream,
}

impl Cloak {
    pub const ALL: [Self; 8] = [
        Self::River,
        Self::Ember,
        Self::Moss,
        Self::Plum,
        Self::Mustard,
        Self::Rose,
        Self::Slate,
        Self::Cream,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::River => "River",
            Self::Ember => "Ember",
            Self::Moss => "Moss",
            Self::Plum => "Plum",
            Self::Mustard => "Mustard",
            Self::Rose => "Rose",
            Self::Slate => "Slate",
            Self::Cream => "Cream",
        }
    }

    pub fn colour(self, pal: &Palette) -> Vec3 {
        pal.cloaks[self as usize]
    }
}

/// The colour of the wick, the light a chief carries.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Light {
    #[default]
    Candle,
    Rune,
    Rosy,
    Leaf,
    Moon,
}

impl Light {
    pub const ALL: [Self; 5] = [
        Self::Candle,
        Self::Rune,
        Self::Rosy,
        Self::Leaf,
        Self::Moon,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Candle => "Candle",
            Self::Rune => "Rune",
            Self::Rosy => "Rosy",
            Self::Leaf => "Leaf",
            Self::Moon => "Moon",
        }
    }

    pub fn colour(self, pal: &Palette) -> Vec3 {
        pal.wicks[self as usize]
    }
}

/// A chief's headwear.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Hat {
    /// A wide brim and a bent point, like the roof at home.
    #[default]
    Pointed,
    Hood,
    Crown,
    Wreath,
    Bare,
}

impl Hat {
    pub const ALL: [Self; 5] = [
        Self::Pointed,
        Self::Hood,
        Self::Crown,
        Self::Wreath,
        Self::Bare,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Pointed => "Pointed hat",
            Self::Hood => "Hood",
            Self::Crown => "Crown",
            Self::Wreath => "Wreath",
            Self::Bare => "Bare",
        }
    }
}

/// The thing in a chief's hand.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Holds {
    #[default]
    Quill,
    Teacup,
    Book,
    Nothing,
}

impl Holds {
    pub const ALL: [Self; 4] =
        [Self::Quill, Self::Teacup, Self::Book, Self::Nothing];

    pub fn name(self) -> &'static str {
        match self {
            Self::Quill => "Quill",
            Self::Teacup => "Teacup",
            Self::Book => "Book",
            Self::Nothing => "Nothing",
        }
    }
}

/// A chief's look. A choice a file leaves out is the first of its
/// kind.
#[derive(
    Component,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    Default,
    Serialize,
    Deserialize,
)]
#[serde(default)]
pub struct Look {
    pub cloak: Cloak,
    pub light: Light,
    pub hat: Hat,
    pub holds: Holds,
}

impl Look {
    /// Every look there is.
    pub fn all() -> impl Iterator<Item = Look> {
        Cloak::ALL.into_iter().flat_map(|cloak| {
            Light::ALL.into_iter().flat_map(move |light| {
                Hat::ALL.into_iter().flat_map(move |hat| {
                    Holds::ALL.into_iter().map(move |holds| Look {
                        cloak,
                        light,
                        hat,
                        holds,
                    })
                })
            })
        })
    }

    /// The look in a few words.
    pub fn words(self) -> String {
        let holds = match self.holds {
            Holds::Nothing => String::new(),
            held => format!(", {}", held.name().to_lowercase()),
        };
        format!(
            "{} cloak, {}{holds}, {} light",
            self.cloak.name(),
            self.hat.name().to_lowercase(),
            self.light.name().to_lowercase()
        )
    }

    /// The file the look of the chief `user` is kept in, under the
    /// home at `home`.
    pub fn file(home: &Path, user: &str) -> PathBuf {
        let name = user
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        home.join("chiefs").join(format!("{name}.toml"))
    }

    /// The kept look of `user`, or the first look where none is
    /// kept or it cannot be read.
    pub fn read(home: &Path, user: &str) -> Look {
        std::fs::read_to_string(Self::file(home, user))
            .ok()
            .and_then(|text| toml::from_str::<Look>(&text).ok())
            .unwrap_or_default()
    }

    /// Keeps the look of `user`.
    pub fn write(
        self,
        home: &Path,
        user: &str,
    ) -> std::io::Result<()> {
        let file = Self::file(home, user);
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text =
            toml::to_string(&self).map_err(std::io::Error::other)?;
        std::fs::write(file, text)
    }
}
