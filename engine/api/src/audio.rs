//! Sound: what a sound a plugin plays is for, which the person's
//! settings scale, and the target of `audio.play`.

use serde::{Deserialize, Serialize};

/// The target of `audio.play`: the engine's audio output.
pub const OUTPUT: &str = "output";

/// What a sound is for. The person sets a volume for each, under the
/// master volume.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Debug,
    Default,
    Serialize,
    Deserialize,
)]
pub enum Category {
    /// The panels, menus and controls.
    Interface,
    /// What happens in a scene.
    #[default]
    World,
    Music,
    /// Speech and the sounds of those who speak.
    Voices,
}

impl Category {
    pub const ALL: [Self; 4] =
        [Self::Interface, Self::World, Self::Music, Self::Voices];

    /// Its name, as the person's settings show it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::World => "World",
            Self::Music => "Music",
            Self::Voices => "Voices",
        }
    }
}
