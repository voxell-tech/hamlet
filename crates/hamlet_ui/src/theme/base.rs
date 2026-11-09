//! What a theme file holds for one variant: fifteen colours, from
//! which [`super::Palette::derive`] makes every role.

use bevy::color::Color;
use hamlet_assets::tint;
use serde::Deserialize;

/// A hue of a variant: the colours beside the ramp.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hue {
    Red,
    Orange,
    #[default]
    Yellow,
    Green,
    Blue,
    Purple,
}

/// Whether a variant is drawn on paper or in the dark. It decides
/// which constants the derivation reads and which way the ramp runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Light,
    Dark,
}

/// One variant of a theme as its file writes it: a name, a ramp of
/// nine neutrals and six hues. `base0` is the background end of the
/// ramp and `base8` the text end: in dark `base0` is the deepest
/// background and `base8` the brightest text, in light `base0` is the
/// lightest paper and `base8` the darkest ink.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Base {
    pub name: String,
    #[serde(with = "tint")]
    pub base0: Color,
    #[serde(with = "tint")]
    pub base1: Color,
    #[serde(with = "tint")]
    pub base2: Color,
    #[serde(with = "tint")]
    pub base3: Color,
    #[serde(with = "tint")]
    pub base4: Color,
    #[serde(with = "tint")]
    pub base5: Color,
    #[serde(with = "tint")]
    pub base6: Color,
    #[serde(with = "tint")]
    pub base7: Color,
    #[serde(with = "tint")]
    pub base8: Color,
    #[serde(with = "tint")]
    pub red: Color,
    #[serde(with = "tint")]
    pub orange: Color,
    #[serde(with = "tint")]
    pub yellow: Color,
    #[serde(with = "tint")]
    pub green: Color,
    #[serde(with = "tint")]
    pub blue: Color,
    #[serde(with = "tint")]
    pub purple: Color,
    /// The hue of the brand colour: what is selected, picked and
    /// lit.
    #[serde(default)]
    pub accent: Hue,
}

impl Base {
    /// The ramp, background end first.
    pub fn ramp(&self) -> [Color; 9] {
        [
            self.base0, self.base1, self.base2, self.base3,
            self.base4, self.base5, self.base6, self.base7,
            self.base8,
        ]
    }

    /// The colour of `hue`.
    pub fn hue(&self, hue: Hue) -> Color {
        match hue {
            Hue::Red => self.red,
            Hue::Orange => self.orange,
            Hue::Yellow => self.yellow,
            Hue::Green => self.green,
            Hue::Blue => self.blue,
            Hue::Purple => self.purple,
        }
    }
}
