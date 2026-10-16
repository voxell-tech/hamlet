//! The conversation the chat is in, and the agent it is with.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use hamlet_core::session::SessionKey;
use hamlet_rules::hamlet::HOME;

/// The chat session this window shows and sends to.
#[derive(Resource)]
pub struct Convo {
    pub session: Entity,
    pub key: SessionKey,
    pub user: String,
    /// The session with the mayor, whoever the chat is with.
    pub mayor: Entity,
    /// The agent the chat is with.
    pub agent: Entity,
    /// That agent's name. `None` for the mayor.
    pub with: Option<String>,
}

impl Convo {
    /// The chat of `user` with `agent`, kept in `session`: with the
    /// mayor unless `with` names the agent.
    pub fn of(
        session: Entity,
        agent: Entity,
        user: &str,
        with: Option<&str>,
    ) -> Convo {
        let key = match with {
            Some(name) => SessionKey::with_agent("app", user, name),
            None => SessionKey::with_mayor("app", user, HOME),
        };
        Convo {
            session,
            key,
            user: user.into(),
            mayor: session,
            agent,
            with: with.map(str::to_string),
        }
    }
}
