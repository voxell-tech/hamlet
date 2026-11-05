//! The selected agent's papers and document requests.

use std::collections::HashMap;

use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::chats::TalksTo;
use hamlet_client::messages::{JobFinished, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::{OfScholar, id_of};
use hamlet_desktop::editor::{Edit, Edited};
use hamlet_protocol::client as protocol;

use super::agent::Briefing;
use crate::convo::Convo;

/// The memory, character and skills received for the selected agent.
#[derive(Resource, Default, PartialEq)]
pub struct Papers {
    pub(super) memory: Vec<protocol::MemoryFile>,
    /// The paths its lodge changed against its scholar.
    pub(super) learned: Vec<String>,
    pub(super) character: String,
    pub(super) skills: Vec<(String, String)>,
    pub(super) failed: Option<String>,
}

#[derive(Resource, Default)]
pub(super) struct Requests {
    selected: Option<protocol::AgentId>,
    papers: Option<RequestId>,
    prompt: Option<RequestId>,
    pending: HashMap<RequestId, Request>,
    documents: Vec<Document>,
}

struct Document {
    key: String,
    agent: protocol::AgentId,
    scholar: protocol::ScholarId,
    latest: Option<RequestId>,
}

enum Request {
    Papers(protocol::AgentId),
    Prompt(protocol::AgentId),
    Character(protocol::AgentId),
    Save(protocol::AgentId),
}

pub(super) fn selected(world: &World) -> Option<protocol::AgentId> {
    id_of(world, world.resource::<Convo>().agent)
}

fn read(world: &mut World, agent: protocol::AgentId) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Papers {
            id,
            of: protocol::Keeper::Agent(agent),
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
    world.resource_mut::<Requests>().selected = agent;
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
        protocol::ClientMessage::Read(protocol::Read::Prompt {
            id,
            agent,
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
        protocol::ClientMessage::Read(protocol::Read::Papers {
            id,
            of: protocol::Keeper::Agent(agent),
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
        let document = world
            .resource::<Requests>()
            .documents
            .iter()
            .find(|doc| doc.key == saved.key)
            .map(|doc| (doc.agent, doc.scholar));
        let Some((agent, scholar)) = document else {
            return;
        };
        let id = world.resource_mut::<Outbox>().send(|id| {
            protocol::ClientMessage::Edit(protocol::Edit::Character {
                id,
                scholar,
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

fn failure(message: &protocol::ServerMessage) -> Option<String> {
    match message {
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Error =>
        {
            Some(reply.text.clone())
        }
        protocol::ServerMessage::Fault(fault) => {
            Some(fault.text.clone())
        }
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
                if let protocol::ServerMessage::Read(
                    protocol::ReadResult::Papers { papers, .. },
                ) = reply.message
                {
                    let mut shown = world.resource_mut::<Papers>();
                    shown.memory = papers.memory;
                    shown.learned = papers.learned;
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
                    protocol::ServerMessage::Read(
                        protocol::ReadResult::Prompt { prompt, .. },
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
                if let protocol::ServerMessage::Read(
                    protocol::ReadResult::Papers { papers, .. },
                ) = reply.message
                {
                    // The Hand is no scholar's hire: it has no
                    // character to edit.
                    let Some(scholar) = world
                        .get::<OfScholar>(world.resource::<Convo>().agent)
                        .map(|of| of.0)
                    else {
                        world.resource_mut::<Papers>().failed = Some(
                            "This agent has no scholar, so no \
                             character to edit."
                                .into(),
                        );
                        return;
                    };
                    let mut requests = world.resource_mut::<Requests>();
                    let key = match requests
                        .documents
                        .iter()
                        .find(|doc| doc.agent == agent)
                    {
                        Some(doc) => doc.key.clone(),
                        None => {
                            let key = format!("character-{}", agent.0);
                            requests.documents.push(Document {
                                key: key.clone(),
                                agent,
                                scholar,
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
                if latest && selected(world) == Some(agent) {
                    world.resource_mut::<Papers>().failed = failed;
                    if matches!(reply.message, protocol::ServerMessage::Reply(ref reply) if reply.kind == protocol::ReplyKind::Notice) {
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
        let convo = world.resource::<Convo>();
        let ours = chat == convo.session
            || world
                .get::<TalksTo>(chat)
                .is_some_and(|to| to.0 == convo.agent);
        if ours && let Some(agent) = selected(world) {
            read(world, agent);
        }
    });
}
