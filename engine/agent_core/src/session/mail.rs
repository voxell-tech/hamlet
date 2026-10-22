//! Messages between agents: a letter one sends without waiting, its
//! reply, and the conversations agents keep with each other.

use bevy_ecs::name::Name;
use bevy_ecs::prelude::*;
use hamlet_rules::hamlet::{HOME, Hamlet, MAYOR};

use crate::agent::Event as AgentEvent;
use crate::agents::{self, Citizen, Left, Mayor};
use crate::core::HomeDir;
use crate::session::deliver::deliver;
use crate::session::{
    MAIL, Reply, SessionKey, Transcript, UserMessage, ensure, find,
    hamlet_of,
};

/// The reply address and the task of a message between agents.
#[derive(Clone)]
pub(super) struct Mail {
    /// The session the reply goes back to. `None` when this message
    /// is itself a reply.
    pub(super) reply_to: Option<SessionKey>,
    /// The task the message belongs to.
    pub(super) task: String,
}

/// A message one agent sent another without waiting. An observer
/// delivers it to the other's own session.
#[derive(Event, Clone)]
pub(super) struct Letter {
    /// The session of the sender, where the reply returns.
    pub(super) from: SessionKey,
    pub(super) to: String,
    pub(super) text: String,
    pub(super) task: String,
}

/// Loads the conversations between `agent` and the others of its
/// hamlet that have a log and are not loaded yet.
pub fn gather(world: &mut World, agent: Entity) {
    let Some(place) =
        agents::hamlet_of(world, agent).map(str::to_string)
    else {
        return;
    };
    let lives = world.get::<agents::LivesIn>(agent).map(|l| l.0);
    let home = world.resource::<HomeDir>().0.clone();
    let hamlet = Hamlet::at(&home.root, &place);
    // Each agent of the hamlet by its address, with the folder of its
    // logs.
    let mut me = None;
    let mut others = Vec::new();
    let members = world
        .query_filtered::<(
            Entity,
            &Name,
            &agents::LivesIn,
            Has<Mayor>,
        ), (With<Citizen>, Without<Left>)>()
        .iter(world)
        .filter(|(_, _, at, _)| Some(at.0) == lives)
        .map(|(entity, name, _, mayor)| {
            (entity, name.as_str().to_string(), mayor)
        })
        .collect::<Vec<_>>();
    for (entity, name, mayor) in members {
        let sessions = match hamlet.house_dir(&name) {
            None if place == HOME => Some(home.sessions()),
            _ => hamlet.house(&name).ok().map(|h| h.sessions()),
        };
        let Some(sessions) = sessions else {
            continue;
        };
        let address = if mayor { MAYOR.to_string() } else { name };
        if entity == agent {
            me = Some((address, sessions));
        } else {
            others.push((address, sessions));
        }
    }
    let Some(me) = me else {
        return;
    };
    for other in &others {
        for (from, to) in [(&me, other), (other, &me)] {
            let mail = match to.0.as_str() {
                MAYOR => {
                    SessionKey::with_mayor(MAIL, &from.0, &place)
                }
                name => SessionKey::with_agent(MAIL, &from.0, name),
            };
            let asked = SessionKey::asked(&from.0, &to.0, &place);
            for key in [mail, asked] {
                if key.log_path(&to.1).exists() {
                    ensure(world, &key);
                }
            }
        }
    }
}

/// The conversations the agent of a session is asking others in
/// and waiting on, by whom it asks.
#[derive(Component, Default)]
struct Asking(Vec<(String, Entity)>);

/// Puts what the agent `name` does while the agent of `session`
/// waits on it into the conversation `name` keeps with that agent.
pub(super) fn mirror(
    world: &mut World,
    session: Entity,
    name: &str,
    event: &AgentEvent,
) {
    let known = world.get::<Asking>(session).and_then(|asking| {
        asking.0.iter().find(|(n, _)| n == name).map(|(_, e)| *e)
    });
    let asked = match (event, known) {
        (AgentEvent::Question { from, at, .. }, _) => {
            let Some(key) = world.get::<SessionKey>(session).cloned()
            else {
                return;
            };
            let place = hamlet_of(world, &key);
            let key = SessionKey::asked(from, name, place.name());
            let loaded = find(world, &key);
            let asked = ensure(world, &key);
            // A log read only now may already hold this question
            // and what followed it.
            if loaded.is_none()
                && let Some(mut said) =
                    world.get_mut::<Transcript>(asked)
            {
                said.forget_since(*at);
            }
            let mut session = world.entity_mut(session);
            let mut asking =
                session.take::<Asking>().unwrap_or_default();
            asking.0.retain(|(n, _)| n != name);
            asking.0.push((name.to_string(), asked));
            session.insert(asking);
            asked
        }
        (_, Some(asked)) => asked,
        (_, None) => return,
    };
    if let Some(mut said) = world.get_mut::<Transcript>(asked) {
        said.apply(&Reply::Event(event.clone()));
    }
    if let AgentEvent::Agent { name, event } = event {
        mirror(world, asked, name, event);
    }
}

/// The opening of a letter's reply when it is carried back.
pub(super) fn reply_head(from: &str) -> String {
    format!("Reply from `{from}`: ")
}

/// The reply itself, when `text` is one that `user` sent back to
/// whoever wrote it a letter.
pub fn reply_in<'a>(user: &str, text: &'a str) -> Option<&'a str> {
    text.strip_prefix(reply_head(user).as_str())
}

/// Puts a letter in the other agent's own session, where it works on
/// it beside whoever sent it.
pub(super) fn post(world: &mut World, letter: Letter) {
    let sender = letter.from.address().to_string();
    let hamlet = hamlet_of(world, &letter.from);
    let key = match letter.to.as_str() {
        MAYOR => SessionKey::with_mayor(MAIL, &sender, hamlet.name()),
        to => SessionKey::with_agent(MAIL, &sender, to),
    };
    deliver(
        world,
        UserMessage {
            key,
            user: sender,
            text: letter.text,
        },
        Some(Mail {
            reply_to: Some(letter.from),
            task: letter.task,
        }),
    );
}
