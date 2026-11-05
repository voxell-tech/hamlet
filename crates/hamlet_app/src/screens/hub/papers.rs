//! The papers of whoever the Hub shows, and its document requests.

use std::collections::HashMap;

use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::chats::TalksTo;
use hamlet_client::messages::{JobFinished, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::{OfScholar, Scholar, id_of};
use hamlet_desktop::editor::{Edit, Edited};
use hamlet_protocol::client as protocol;

use super::Showing;
use super::thread::Briefing;
use crate::convo::Convo;

/// The text a new memory file starts with in the editor.
const NEW_FILE: &str = "---\ndescription: \n---\n\n";

/// The memory, character and skills received for the agent or the
/// scholar the Hub shows.
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
    selected: Option<protocol::Keeper>,
    papers: Option<RequestId>,
    prompt: Option<RequestId>,
    pending: HashMap<RequestId, Request>,
    documents: Vec<Document>,
}

/// What a document in the editor is of.
#[derive(Clone, PartialEq)]
enum Target {
    Character(protocol::ScholarId),
    Memory(protocol::Keeper, String),
}

struct Document {
    key: String,
    target: Target,
    /// Whose papers the Hub showed when it was opened.
    shown: protocol::Keeper,
    latest: Option<RequestId>,
}

enum Request {
    Papers(protocol::Keeper),
    Prompt(protocol::AgentId),
    /// The whole text of a document, to be opened in the editor.
    Whole(protocol::Keeper, Target),
    Save(protocol::Keeper),
    Remove(protocol::Keeper),
}

pub(super) fn selected(world: &World) -> Option<protocol::AgentId> {
    id_of(world, world.resource::<Convo>().agent)
}

/// Whose papers the Hub shows: a scholar's on its page, otherwise
/// those of the agent the chat is with.
pub(super) fn keeper(world: &World) -> Option<protocol::Keeper> {
    if let Some(&Showing::Scholar(scholar)) =
        world.get_resource::<Showing>()
    {
        let id = world.get::<Scholar>(scholar)?.0.id;
        return Some(protocol::Keeper::Scholar(id));
    }
    selected(world).map(protocol::Keeper::Agent)
}

fn read(world: &mut World, of: protocol::Keeper) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Papers {
            id,
            of,
            full: None,
        })
    });
    let mut requests = world.resource_mut::<Requests>();
    requests.papers = Some(id.clone());
    requests.pending.insert(id, Request::Papers(of));
}

/// The papers read for a newly shown agent or scholar.
pub fn keep_papers(world: &mut World) {
    let wanted = keeper(world);
    if world.resource::<Requests>().selected == wanted {
        return;
    }
    world.resource_mut::<Requests>().selected = wanted;
    *world.resource_mut::<Papers>() = Papers::default();
    if let Some(of) = wanted {
        read(world, of);
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

/// The scholar whose character the Hub shows.
fn scholar_shown(
    world: &World,
    of: protocol::Keeper,
) -> Option<protocol::ScholarId> {
    match of {
        protocol::Keeper::Scholar(scholar) => Some(scholar),
        // The Hand is no scholar's hire: it has no character.
        protocol::Keeper::Agent(_) => world
            .get::<OfScholar>(world.resource::<Convo>().agent)
            .map(|of| of.0),
    }
}

/// Reads the whole of `target`, to open it in the editor.
fn read_whole(world: &mut World, target: Target) {
    let Some(of) = keeper(world) else {
        return;
    };
    world.resource_mut::<Papers>().failed = None;
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Read(protocol::Read::Papers {
            id,
            of,
            full: Some(true),
        })
    });
    world
        .resource_mut::<Requests>()
        .pending
        .insert(id, Request::Whole(of, target));
}

pub(super) fn edit_character(world: &mut World) {
    let Some(of) = keeper(world) else {
        return;
    };
    match scholar_shown(world, of) {
        Some(scholar) => {
            read_whole(world, Target::Character(scholar));
        }
        None => {
            world.resource_mut::<Papers>().failed = Some(
                "This agent has no scholar, so no character to edit."
                    .into(),
            );
        }
    }
}

/// Opens the memory file `path` in the editor.
pub(super) fn edit_memory(world: &mut World, path: String) {
    let Some(of) = keeper(world) else {
        return;
    };
    read_whole(world, Target::Memory(of, path));
}

/// Opens a new memory file named `name` in the editor.
pub(super) fn add_memory(world: &mut World, name: &str) {
    let name = name.trim().trim_start_matches('/');
    if name.is_empty() {
        return;
    }
    let path = if name.contains('.') {
        name.to_string()
    } else {
        format!("{name}.md")
    };
    let Some(of) = keeper(world) else {
        return;
    };
    open_document(
        world,
        of,
        Target::Memory(of, path),
        NEW_FILE.to_string(),
    );
}

/// Removes the memory file `path`: an empty text does.
pub(super) fn remove_memory(world: &mut World, path: String) {
    let Some(of) = keeper(world) else {
        return;
    };
    world.resource_mut::<Papers>().failed = None;
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Edit(protocol::Edit::Memory {
            id,
            of,
            path,
            text: String::new(),
        })
    });
    crate::saving::track(world, id.clone());
    world
        .resource_mut::<Requests>()
        .pending
        .insert(id, Request::Remove(of));
}

/// Opens `text` in the editor as the document of `target`, which
/// the Hub showed as `shown`.
fn open_document(
    world: &mut World,
    shown: protocol::Keeper,
    target: Target,
    text: String,
) {
    let mut requests = world.resource_mut::<Requests>();
    let key = match requests
        .documents
        .iter()
        .find(|doc| doc.target == target)
    {
        Some(doc) => doc.key.clone(),
        None => {
            let key = format!(
                "{}-{}",
                match target {
                    Target::Character(_) => "character",
                    Target::Memory(..) => "memory",
                },
                requests.documents.len()
            );
            requests.documents.push(Document {
                key: key.clone(),
                target,
                shown,
                latest: None,
            });
            key
        }
    };
    world.trigger(Edit { key, text });
}

pub(super) fn edited(saved: On<Edited>, mut commands: Commands) {
    let saved = saved.event().clone();
    commands.queue(move |world: &mut World| {
        let document = world
            .resource::<Requests>()
            .documents
            .iter()
            .find(|doc| doc.key == saved.key)
            .map(|doc| (doc.target.clone(), doc.shown));
        let Some((target, of)) = document else {
            return;
        };
        let id = world.resource_mut::<Outbox>().send(|id| {
            protocol::ClientMessage::Edit(match target {
                Target::Character(scholar) => {
                    protocol::Edit::Character {
                        id,
                        scholar,
                        text: saved.text.clone(),
                    }
                }
                Target::Memory(of, path) => protocol::Edit::Memory {
                    id,
                    of,
                    path,
                    text: saved.text.clone(),
                },
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
        requests.pending.insert(id, Request::Save(of));
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
            Request::Papers(of) => {
                if keeper(world) != Some(of)
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
            Request::Whole(of, target) => {
                if let Some(failed) = failed {
                    if keeper(world) == Some(of) {
                        world.resource_mut::<Papers>().failed =
                            Some(failed);
                    }
                    return;
                }
                let protocol::ServerMessage::Read(
                    protocol::ReadResult::Papers { papers, .. },
                ) = reply.message
                else {
                    return;
                };
                let text = match &target {
                    Target::Character(_) => Some(papers.character),
                    Target::Memory(_, path) => papers
                        .memory
                        .into_iter()
                        .find(|file| &file.path == path)
                        .map(|file| file.text),
                };
                if let Some(text) = text {
                    open_document(world, of, target, text);
                }
            }
            Request::Save(of) => {
                let latest = world
                    .resource::<Requests>()
                    .documents
                    .iter()
                    .any(|doc| doc.latest.as_ref() == Some(&reply.to));
                if latest && keeper(world) == Some(of) {
                    world.resource_mut::<Papers>().failed = failed;
                    if matches!(reply.message, protocol::ServerMessage::Reply(ref reply) if reply.kind == protocol::ReplyKind::Notice) {
                        read(world, of);
                    }
                }
            }
            Request::Remove(of) => {
                if keeper(world) != Some(of) {
                    return;
                }
                world.resource_mut::<Papers>().failed = failed;
                if matches!(reply.message, protocol::ServerMessage::Reply(ref reply) if reply.kind == protocol::ReplyKind::Notice) {
                    read(world, of);
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
        if ours && let Some(of) = keeper(world) {
            read(world, of);
        }
    });
}
