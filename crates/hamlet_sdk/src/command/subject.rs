//! What a command is run on.

use serde::{Deserialize, Serialize};

/// The kinds of thing a [`Subject`] can be.
pub mod kind {
    /// An agent besides the mayor, as a robot.
    pub const AGENT: &str = "agent";
    /// An agent's house.
    pub const HOUSE: &str = "house";
    /// The mayor, or its house.
    pub const MAYOR: &str = "mayor";
    /// The remains of the house of an agent that left.
    pub const RUIN: &str = "ruin";
    /// A hamlet, by its common, its workbench or its clockwork.
    pub const HAMLET: &str = "hamlet";
    /// The chief's mascot.
    pub const CHIEF: &str = "chief";
    /// The chief's Hearth.
    pub const HEARTH: &str = "hearth";
    pub const LIBRARY: &str = "library";
    /// The bare ground.
    pub const GROUND: &str = "ground";

    pub const ALL: [&str; 9] = [
        AGENT, HOUSE, MAYOR, RUIN, HAMLET, CHIEF, HEARTH, LIBRARY,
        GROUND,
    ];
}

/// A thing of the world that a command is run on, such as the one a
/// menu was opened on. A plugin is given it as the arguments of the
/// command, written with `postcard`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Subject {
    /// What it is, one of [`kind`].
    pub kind: String,
    /// Names it among the things of its kind while it is there.
    pub id: String,
    /// What the window calls it.
    pub name: String,
}

impl Subject {
    pub fn new(kind: &str, id: &str, name: &str) -> Self {
        Self {
            kind: kind.into(),
            id: id.into(),
            name: name.into(),
        }
    }

    /// The bytes a plugin is given.
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    /// The subject written in `bytes`.
    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        postcard::from_bytes(bytes).map_err(|e| e.to_string())
    }
}
