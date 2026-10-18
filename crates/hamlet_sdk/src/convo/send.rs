//! Addressing the chat with an agent, and sending to one as the
//! chief.

use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use hamlet_core::agents::{self, Mayor};
use hamlet_core::session::{self, SessionKey, UserMessage};
use hamlet_rules::hamlet::HOME;

use super::Me;

/// The session a chat with `agent` is kept under, and the agent's
/// name. `None` for the name of a mayor, and nothing at all for an
/// agent that is not there.
pub fn address(
    world: &World,
    agent: Entity,
) -> Option<(SessionKey, Option<String>)> {
    let name = world.get::<Name>(agent)?;
    let with = world
        .get::<Mayor>(agent)
        .is_none()
        .then(|| name.as_str().to_string());
    let me = world.resource::<Me>();
    let key = match &with {
        Some(name) => SessionKey::with_agent("app", &me.user, name),
        None => SessionKey::with_mayor(
            "app",
            &me.user,
            agents::hamlet_of(world, agent).unwrap_or(HOME),
        ),
    };
    Some((key, with))
}

/// Sends `text` to the agent named `agent` as the chief, in the line
/// `[sent by plugin <by>]` so that the agent and the transcript tell
/// it from the chief's own words. The line also keeps a message that
/// starts with `/` from being read as a chief's command. An error
/// says why nothing was sent.
pub fn send_as_chief(
    world: &mut World,
    agent: &str,
    by: &str,
    text: &str,
) -> Result<(), String> {
    if world.get_resource::<Me>().is_none() {
        return Err("there is no chat".into());
    }
    let Some(entity) = agents::named(world, agent) else {
        return Err(format!("no agent `{agent}`"));
    };
    let Some((key, _)) = address(world, entity) else {
        return Err(format!("no agent `{agent}`"));
    };
    let user = world.resource::<Me>().user.clone();
    session::ensure(world, &key);
    world.trigger(UserMessage {
        key,
        user,
        text: format!("[sent by plugin {by}] {text}"),
    });
    Ok(())
}
