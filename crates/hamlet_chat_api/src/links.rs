//! The link of the chat under the pointer.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;

/// The link under the pointer.
#[derive(Resource, Default, PartialEq, Eq, Debug)]
pub struct Aimed {
    /// The span of the link, where it is one drawn in the window.
    pub span: Option<Entity>,
    /// The place it leads, in full. Empty when the pointer is on no
    /// link.
    pub address: String,
}

impl Aimed {
    /// A pointer that rests on a link to `address`.
    pub fn at(address: &str) -> Self {
        Aimed {
            span: None,
            address: address.to_string(),
        }
    }
}
