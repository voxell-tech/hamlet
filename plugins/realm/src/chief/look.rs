//! A chief's look: its cloak, its light, what is on its head and
//! what is in its hand.

use glam::Vec3;
use serde::Deserialize;

use crate::palette::Palette;

/// The colour of the cloak, and of the roof at home.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
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
    pub fn colour(self, pal: &Palette) -> Vec3 {
        pal.cloaks[self as usize]
    }
}

/// The colour of the wick, the light a chief carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
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
    pub fn colour(self, pal: &Palette) -> Vec3 {
        pal.wicks[self as usize]
    }
}

/// A chief's headwear.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
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

/// The thing in a chief's hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Holds {
    #[default]
    Quill,
    Teacup,
    Book,
    Nothing,
}

/// A chief's look, as its file keeps it. A choice a file leaves out
/// is the first of its kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Look {
    pub cloak: Cloak,
    pub light: Light,
    pub hat: Hat,
    pub holds: Holds,
}
