//! The chief's records in the home: its look, and where it set its
//! Hearth and the library down.

use serde::{Deserialize, Serialize};

/// The chief at this window, as the home keeps it.
#[derive(
    Clone, PartialEq, Debug, Default, Serialize, Deserialize,
)]
pub struct ChiefRecord {
    /// The name the chief signs in with.
    pub user: String,
    pub look: Look,
    /// Where the chief set its Hearth down, as x and z; none while
    /// it stands where the land put it.
    pub hearth: Option<(f32, f32)>,
    /// Where a chief set the library down, as x and z.
    pub library: Option<(f32, f32)>,
}

/// A chief's look. A choice its file leaves out is the first of its
/// kind.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(default)]
pub struct Look {
    pub cloak: Cloak,
    pub light: Light,
    pub hat: Hat,
    pub holds: Holds,
}

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

/// A chief's headwear.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Hat {
    #[default]
    Pointed,
    Hood,
    Crown,
    Wreath,
    Bare,
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
