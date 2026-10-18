//! A new version that asks for more, held until the chief answers.

use bevy::prelude::*;
use hamlet_sdk::capability::{Capability, Choices};

use super::Modified;

/// On the entity of an installed plugin: the capabilities its new
/// version asks for that the chief is yet to answer. The new version
/// is not built until they are.
#[derive(Component, Debug)]
pub struct Waiting(pub Vec<Capability>);

/// Has a plugin whose new asks were all answered looked at again.
pub fn answered(
    waiting: Query<(Entity, &Waiting, &Choices), Changed<Choices>>,
    mut modified: MessageWriter<Modified>,
) {
    for (plugin, Waiting(asked), choices) in &waiting {
        if asked.iter().all(|c| choices.choice(*c).is_some()) {
            modified.write(Modified(plugin));
        }
    }
}
