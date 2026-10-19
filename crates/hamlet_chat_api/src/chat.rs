//! The open chat: sending a line to it, giving a hamlet a chief's
//! command, and the chat's views that other plugins show.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy_fynix::ViewExt;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::column;
use hamlet_agents_api::agents;
use hamlet_agents_api::hamlet::{HOME, Target, switch};
use hamlet_agents_api::session::{
    Entry, Reply, SessionKey, Transcript, UserMessage,
};
use hamlet_sdk::{AnyView, with_cx};

use crate::me::{Me, talk_to, talk_to_mayor_of};

/// The chat's views that other plugins show. The chat sets it.
#[derive(Resource, Clone, Copy)]
pub struct Views {
    pub transcript: fn() -> AnyView,
    pub message_bar: fn() -> AnyView,
    pub entry: fn(Entity, u64) -> AnyView,
    pub markdown: fn(&str, Tone) -> AnyView,
}

/// Folds that are open, by their conversation and first entry.
#[derive(Resource, Default)]
pub struct Unfolded(pub Vec<(Entity, u64)>);

/// The conversation the chat is with, as the chat draws it.
pub fn transcript() -> AnyView {
    shown(|views| (views.transcript)())
}

/// The one place to type, under the conversation it adds to.
pub fn message_bar() -> AnyView {
    shown(|views| (views.message_bar)())
}

/// The entry `id` of the conversation `session`, as the chat draws
/// it.
pub fn entry_view(session: Entity, id: u64) -> AnyView {
    shown(move |views| (views.entry)(session, id))
}

/// The view `view` makes of the chat's [`Views`], or nothing where
/// the chat is not there.
fn shown(view: impl FnOnce(&Views) -> AnyView + 'static) -> AnyView {
    with_cx(move |cx| match cx.world.get_resource::<Views>() {
        Some(views) => view(views),
        None => column(()).boxed(),
    })
}

/// Sends `text` to the open chat.
pub fn send(world: &mut World, text: String) {
    let text = text.trim().to_string();
    if text.is_empty() {
        return;
    }
    // `/hamlet <name>` and `/agent <name>` change who the chat is
    // with.
    match switch(&text) {
        Some(Target::Mayor(hamlet)) => {
            return talk_to_mayor_of(world, &hamlet);
        }
        Some(Target::Agent(name)) => {
            if let Some(agent) = agents::named(world, &name) {
                talk_to(world, agent);
            }
            return;
        }
        None => {}
    }
    let me = world.resource::<Me>();
    let (key, user) = (me.key.clone(), me.user.clone());
    world.trigger(UserMessage { key, user, text });
}

/// The conversation kept under `key`.
fn kept(world: &mut World, key: &SessionKey) -> Option<Entity> {
    world
        .query::<(Entity, &SessionKey)>()
        .iter(world)
        .find(|(_, kept)| *kept == key)
        .map(|(session, _)| session)
}

/// The notices and errors in `session` after its first `skip`
/// entries.
fn answers(
    world: &World,
    session: Entity,
    skip: usize,
) -> Vec<String> {
    world
        .get::<Transcript>(session)
        .into_iter()
        .flat_map(|said| said.items().iter().skip(skip))
        .filter_map(|item| match &item.entry {
            Entry::Notice(text) | Entry::Error(text) => {
                Some(text.clone())
            }
            _ => None,
        })
        .collect()
}

fn entries(world: &World, session: Option<Entity>) -> usize {
    session
        .and_then(|session| world.get::<Transcript>(session))
        .map_or(0, |said| said.items().len())
}

/// Gives a chief's command to the hamlet that is the entity
/// `hamlet`, as [`decree`] does.
pub fn decree_for(world: &mut World, hamlet: Entity, text: String) {
    let Some(name) = world
        .get::<agents::Settlement>(hamlet)
        .map(|settlement| settlement.name.clone())
    else {
        return;
    };
    decree(world, &name, text);
}

/// Gives a chief's command to the hamlet `hamlet`: in the open chat
/// when it is with that hamlet, and with that hamlet's mayor
/// otherwise. The answer is shown in the open chat either way.
pub fn decree(world: &mut World, hamlet: &str, text: String) {
    let me = world.resource::<Me>();
    let here = agents::hamlet_of(world, me.agent).unwrap_or(HOME);
    let (open, user) = (me.session, me.user.clone());
    let key = if here == hamlet {
        me.key.clone()
    } else {
        SessionKey::with_mayor("app", &user, hamlet)
    };
    let elsewhere = key != me.key;
    let there = kept(world, &key);
    let before = (entries(world, there), entries(world, Some(open)));
    world.trigger(UserMessage {
        key: key.clone(),
        user,
        text,
    });
    world.flush();
    if !elsewhere {
        return;
    }
    let Some(there) = kept(world, &key) else {
        return;
    };
    // The core tells every chat of some answers itself.
    let told = answers(world, open, before.1);
    let answers = answers(world, there, before.0);
    if let Some(mut said) = world.get_mut::<Transcript>(open) {
        for answer in answers {
            if !told.contains(&answer) {
                said.apply(&Reply::Notice(answer));
            }
        }
    }
}
