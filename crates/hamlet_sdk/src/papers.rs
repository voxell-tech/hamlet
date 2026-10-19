//! The notes, the character and the skills an agent keeps in its
//! house, as its pages show them.

use bevy::ecs::resource::Resource;

/// The notes, the character and the skills the selected agent keeps
/// in its house, as its pages show them. Empty while no agent is
/// selected.
#[derive(Resource, Clone, Default, PartialEq, Debug)]
pub struct Papers {
    pub notes: String,
    pub character: String,
    /// Each skill of its own by its name, with what it is for.
    pub skills: Vec<(String, String)>,
}
