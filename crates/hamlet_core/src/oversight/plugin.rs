//! A plugin's petition: it waits in the queue like any other and is
//! answered by the chief the same way, and the answer goes back to
//! whoever raised it.

use bevy_ecs::prelude::*;
use hamlet_rules::chief::rulings::Choice;

use super::queue::{Kind, Raised, Waiting};

/// A plugin's petition was answered. The petition is still there
/// while this is observed, and goes right after.
#[derive(EntityEvent, Clone, Debug)]
pub struct Settled {
    pub entity: Entity,
    pub choice: Choice,
}

/// Puts a petition about the plugin named `plugin` in the queue. The
/// caller adds to the entity what it needs to know the petition by.
pub fn petition(
    world: &mut World,
    plugin: &str,
    about: String,
    answers: Vec<Choice>,
) -> Entity {
    world.init_resource::<Raised>();
    let order = world.resource_mut::<Raised>().next();
    world
        .spawn(Waiting {
            id: format!("plugin-{order}"),
            kind: Kind::Plugin,
            by: plugin.to_string(),
            about,
            since: std::time::Instant::now(),
            order,
            asked: 1,
            answers,
            session: None,
        })
        .id()
}

/// Passes the chief's `choice` to the observers of [`Settled`] and
/// takes the petition out of the queue.
pub(super) fn settle(
    world: &mut World,
    entity: Entity,
    choice: Choice,
) {
    world.trigger(Settled { entity, choice });
    world.flush();
    if let Ok(entity) = world.get_entity_mut(entity) {
        entity.despawn();
    }
}
