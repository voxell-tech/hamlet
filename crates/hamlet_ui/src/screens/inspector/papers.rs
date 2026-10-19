//! The selected agent's papers and document requests.

use std::collections::HashMap;

use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::chats::TalksTo;
use hamlet_client::messages::{JobFinished, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::address_of;
use hamlet_desktop::editor::{Edit, Edited};
use hamlet_protocol::v0;

use super::agent::Briefing;
use crate::me::Me;

/// The notes, character and skills received for the selected agent.
#[derive(Resource, Default, PartialEq)]
pub struct Papers {
    pub(super) notes: String,
    pub(super) character: String,
    pub(super) skills: Vec<(String, String)>,
    pub(super) failed: Option<String>,
}

#[derive(Resource, Default)]
pub(super) struct Requests {
    selected: Option<v0::Address>,
    papers: Option<RequestId>,
    prompt: Option<RequestId>,
    pending: HashMap<RequestId, Request>,
    documents: Vec<Document>,
}

struct Document {
    key: String,
    agent: v0::Address,
    latest: Option<RequestId>,
}

enum Request {
    Papers(v0::Address),
    Prompt(v0::Address),
    Character(v0::Address),
    Save(v0::Address),
}

pub(super) fn selected(world: &World) -> Option<v0::Address> {
    address_of(world, world.resource::<Me>().agent)
}

fn read(world: &mut World, agent: v0::Address) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Read(v0::Read::Papers {
            id,
            agent: agent.clone(),
            full: None,
        })
    });
    let mut requests = world.resource_mut::<Requests>();
    requests.papers = Some(id.clone());
    requests.pending.insert(id, Request::Papers(agent));
}

/// The papers read for a newly selected agent.
pub fn keep_papers(world: &mut World) {
    let agent = selected(world);
    if world.resource::<Requests>().selected == agent {
        return;
    }
    world.resource_mut::<Requests>().selected = agent.clone();
    *world.resource_mut::<Papers>() = Papers::default();
    if let Some(agent) = agent {
        read(world, agent);
    }
}

pub(super) fn prompt(world: &mut World) {
    let Some(agent) = selected(world) else {
        return;
    };
    let id = world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Read(v0::Read::Prompt {
            id,
            agent: agent.clone(),
        })
    });
    let mut requests = world.resource_mut::<Requests>();
    requests.prompt = Some(id.clone());
    requests.pending.insert(id, Request::Prompt(agent));
}

pub(super) fn edit_character(world: &mut World) {
    let Some(agent) = selected(world) else {
        return;
    };
    world.resource_mut::<Papers>().failed = None;
    let id = world.resource_mut::<Outbox>().send(|id| {
        v0::ClientMessage::Read(v0::Read::Papers {
            id,
            agent: agent.clone(),
            full: Some(true),
        })
    });
    world
        .resource_mut::<Requests>()
        .pending
        .insert(id, Request::Character(agent));
}

pub(super) fn edited(saved: On<Edited>, mut commands: Commands) {
    let saved = saved.event().clone();
    commands.queue(move |world: &mut World| {
        let agent = world
            .resource::<Requests>()
            .documents
            .iter()
            .find(|doc| doc.key == saved.key)
            .map(|doc| doc.agent.clone());
        let Some(agent) = agent else {
            return;
        };
        let id = world.resource_mut::<Outbox>().send(|id| {
            v0::ClientMessage::Edit(v0::Edit::Character {
                id,
                agent: agent.clone(),
                text: saved.text,
            })
        });
        crate::saving::track(world, id.clone());
        let mut requests = world.resource_mut::<Requests>();
        requests
            .documents
            .iter_mut()
            .find(|doc| doc.key == saved.key)
            .unwrap()
            .latest = Some(id.clone());
        requests.pending.insert(id, Request::Save(agent));
    });
}

fn failure(message: &v0::ServerMessage) -> Option<String> {
    match message {
        v0::ServerMessage::Reply(reply)
            if reply.kind == v0::ReplyKind::Error =>
        {
            Some(reply.text.clone())
        }
        v0::ServerMessage::Fault(fault) => Some(fault.text.clone()),
        _ => None,
    }
}

pub(super) fn response(reply: On<Response>, mut commands: Commands) {
    let reply = reply.event().clone();
    commands.queue(move |world: &mut World| {
        let Some(request) = world
            .resource_mut::<Requests>()
            .pending
            .remove(&reply.to)
        else {
            return;
        };
        let failed = failure(&reply.message);
        match request {
            Request::Papers(agent) => {
                if selected(world) != Some(agent)
                    || world.resource::<Requests>().papers.as_ref()
                        != Some(&reply.to)
                {
                    return;
                }
                if let v0::ServerMessage::Read(
                    v0::ReadResult::Papers { papers, .. },
                ) = reply.message
                {
                    let mut shown = world.resource_mut::<Papers>();
                    shown.notes = papers.notes;
                    shown.character = papers.character;
                    shown.skills = papers
                        .skills
                        .into_iter()
                        .map(|skill| (skill.name, skill.description))
                        .collect();
                }
            }
            Request::Prompt(agent) => {
                if selected(world) != Some(agent)
                    || world.resource::<Requests>().prompt.as_ref()
                        != Some(&reply.to)
                {
                    return;
                }
                let text = match reply.message {
                    v0::ServerMessage::Read(
                        v0::ReadResult::Prompt { prompt, .. },
                    ) => prompt,
                    _ => match failed {
                        Some(why) => format!(
                            "The prompt could not be read: {why}"
                        ),
                        None => return,
                    },
                };
                world.resource_mut::<Briefing>().text = text;
            }
            Request::Character(agent) => {
                if let Some(failed) = failed {
                    if selected(world) == Some(agent) {
                        world.resource_mut::<Papers>().failed =
                            Some(failed);
                    }
                    return;
                }
                if let v0::ServerMessage::Read(
                    v0::ReadResult::Papers { papers, .. },
                ) = reply.message
                {
                    let mut requests = world.resource_mut::<Requests>();
                    let key = match requests
                        .documents
                        .iter()
                        .find(|doc| doc.agent == agent)
                    {
                        Some(doc) => doc.key.clone(),
                        None => {
                            let key = format!("character-{}-{}", agent.hamlet, agent.agent);
                            requests.documents.push(Document {
                                key: key.clone(),
                                agent,
                                latest: None,
                            });
                            key
                        }
                    };
                    world.trigger(Edit {
                        key,
                        text: papers.character,
                    });
                }
            }
            Request::Save(agent) => {
                let latest = world
                    .resource::<Requests>()
                    .documents
                    .iter()
                    .any(|doc| doc.latest.as_ref() == Some(&reply.to));
                if latest && selected(world) == Some(agent.clone()) {
                    world.resource_mut::<Papers>().failed = failed;
                    if matches!(reply.message, v0::ServerMessage::Reply(ref reply) if reply.kind == v0::ReplyKind::Notice) {
                        read(world, agent);
                    }
                }
            }
        }
    });
}

pub(super) fn papers_on_job(
    job: On<JobFinished>,
    mut commands: Commands,
) {
    let chat = job.chat;
    commands.queue(move |world: &mut World| {
        let me = world.resource::<Me>();
        let ours = chat == me.session
            || world
                .get::<TalksTo>(chat)
                .is_some_and(|to| to.0 == me.agent);
        if ours && let Some(agent) = selected(world) {
            read(world, agent);
        }
    });
}
