//! A chief's look: its cloak, its light, what is on its head and
//! what is in its hand.

use glam::Vec3;
use hamlet_engine_bevy::api::agents as api;
use serde::Deserialize;

use crate::world::palette::Palette;

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

/// The choices of a look and what each is called, one macro for the
/// four of them and their kind in the engine's records.
macro_rules! choices {
    ($($kind:ident: $($name:ident => $word:literal),+;)+) => {$(
        impl $kind {
            /// Every choice, the first being the one a chief has
            /// until they choose.
            pub const ALL: &[Self] = &[$(Self::$name),+];

            pub fn name(self) -> &'static str {
                match self {$(Self::$name => $word),+}
            }
        }

        impl From<api::$kind> for $kind {
            fn from(kind: api::$kind) -> Self {
                match kind {$(api::$kind::$name => Self::$name),+}
            }
        }

        impl From<$kind> for api::$kind {
            fn from(kind: $kind) -> Self {
                match kind {$($kind::$name => Self::$name),+}
            }
        }
    )+};
}

choices! {
    Cloak: River => "river", Ember => "ember", Moss => "moss",
        Plum => "plum", Mustard => "mustard", Rose => "rose",
        Slate => "slate", Cream => "cream";
    Light: Candle => "candle", Rune => "rune", Rosy => "rosy",
        Leaf => "leaf", Moon => "moon";
    Hat: Pointed => "pointed", Hood => "hood", Crown => "crown",
        Wreath => "wreath", Bare => "bare";
    Holds: Quill => "quill", Teacup => "teacup", Book => "book",
        Nothing => "nothing";
}

impl From<api::Look> for Look {
    fn from(look: api::Look) -> Self {
        Self {
            cloak: look.cloak.into(),
            light: look.light.into(),
            hat: look.hat.into(),
            holds: look.holds.into(),
        }
    }
}

impl From<Look> for api::Look {
    fn from(look: Look) -> Self {
        Self {
            cloak: look.cloak.into(),
            light: look.light.into(),
            hat: look.hat.into(),
            holds: look.holds.into(),
        }
    }
}
