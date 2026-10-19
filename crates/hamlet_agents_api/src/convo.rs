//! The agent the chat is with, and whether the world has one
//! selected.

mod page;
mod send;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Commands, Res};
use bevy::ecs::world::World;
use hamlet_core::session::{Reply, SessionKey, Transcript};
use hamlet_rules::hamlet::HOME;
use hamlet_sdk::command::Notice;

pub use self::page::of_agent;
pub use self::send::{address, send_as_chief};

/// Inserts [`ChiefName`] and puts each [`Notice`] in the open chat.
pub fn plugin(app: &mut App) {
    app.insert_resource(ChiefName::from_env())
        .add_observer(notice);
}

/// The name of this window's chief: `HAMLET_USER`, then `USER`.
#[derive(Resource, Clone, PartialEq, Debug)]
pub struct ChiefName(pub String);

impl ChiefName {
    fn from_env() -> Self {
        Self(
            std::env::var("HAMLET_USER")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "user".into()),
        )
    }
}

/// The chat session this window shows and sends to.
#[derive(Resource)]
pub struct Me {
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

impl Me {
    /// The chat of `user` with `agent`, kept in `session`: with the
    /// mayor unless `with` names the agent.
    pub fn of(
        session: Entity,
        agent: Entity,
        user: &str,
        with: Option<&str>,
    ) -> Self {
        let key = match with {
            Some(name) => SessionKey::with_agent("app", user, name),
            None => SessionKey::with_mayor("app", user, HOME),
        };
        Self {
            session,
            key,
            user: user.into(),
            mayor: session,
            agent,
            with: with.map(str::to_string),
        }
    }
}

/// Whether an agent is selected in the world: the mayor, its house,
/// another agent or its house.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct AgentSelected(pub bool);

fn notice(
    notice: On<Notice>,
    me: Option<Res<Me>>,
    mut commands: Commands,
) {
    let Some(me) = me else {
        return;
    };
    let (session, text) = (me.session, notice.0.clone());
    commands.queue(move |world: &mut World| {
        if let Some(mut said) = world.get_mut::<Transcript>(session) {
            said.apply(&Reply::Notice(text));
        }
    });
}
