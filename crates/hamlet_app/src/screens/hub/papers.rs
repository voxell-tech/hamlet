//! The papers of whoever each Hub shows, and its document requests.

use std::collections::HashMap;

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use hamlet_client::chats::TalksTo;
use hamlet_client::messages::{JobFinished, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::{OfProfile, Profile, id_of};
use hamlet_desktop::editor::{Edit, Edited};
use hamlet_protocol::client as protocol;

use super::thread::Briefing;
use super::{Hub, Showing};

/// The text a new memory file starts with in the editor.
const NEW_FILE: &str = "---\ndescription: \n---\n\n";

/// The memory, character and skills of whoever a Hub shows.
#[derive(Component, Default, PartialEq)]
pub struct Papers {
    pub(super) memory: Vec<protocol::MemoryFile>,
    /// The paths its worktree changed against its profile.
    pub(super) learned: Vec<String>,
    pub(super) character: String,
    pub(super) skills: Vec<(String, String)>,
    pub(super) failed: Option<String>,
}

/// Whose papers a Hub last asked for, and its latest reads of them
/// and of its agent's prompt.
#[derive(Component, Default)]
pub(super) struct Asked {
    keeper: Option<protocol::Keeper>,
    papers: Option<RequestId>,
    prompt: Option<RequestId>,
}

#[derive(Resource, Default)]
pub(super) struct Requests {
    /// Each request waiting for its reply, with the Hub that asked.
    pending: HashMap<RequestId, (Entity, Request)>,
    documents: Vec<Document>,
}

/// What a document in the editor is of.
#[derive(Clone, PartialEq)]
enum Target {
    Character(protocol::ProfileId),
    Memory(protocol::Keeper, String),
}

struct Document {
    key: String,
    target: Target,
    /// The Hub it was opened from, and whose papers it showed then.
    hub: Entity,
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

/// Gives `hub` the papers it shows and its reads of them.
pub(super) fn equip(world: &mut World, hub: Entity) {
    world.entity_mut(hub).insert((
        Papers::default(),
        Asked::default(),
        Briefing::default(),
        super::memory::Opened::default(),
    ));
}

fn hubs(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, With<Hub>>()
        .iter(world)
        .collect()
}

/// The agent of `hub`'s chat, whose papers it reads.
fn agent(world: &World, hub: Entity) -> Entity {
    crate::convo::agent_of(world, hub)
}

/// The id of the agent of `hub`'s chat.
pub(super) fn selected(
    world: &World,
    hub: Entity,
) -> Option<protocol::AgentId> {
    id_of(world, agent(world, hub))
}

/// Whose papers `hub` shows: a profile's, else its chat agent's.
fn keeper(world: &World, hub: Entity) -> Option<protocol::Keeper> {
    if let Some(Showing::Profile(profile)) =
        super::showing(world, hub)
    {
        let id = world.get::<Profile>(profile)?.0.id;
        return Some(protocol::Keeper::Profile(id));
    }
    selected(world, hub).map(protocol::Keeper::Agent)
}

/// Sends `message` for `hub` and waits for its reply as `request`.
fn ask(
    world: &mut World,
    hub: Entity,
    request: Request,
    message: impl FnOnce(RequestId) -> protocol::ClientMessage,
) -> RequestId {
    let id = world.resource_mut::<Outbox>().send(message);
    world
        .resource_mut::<Requests>()
        .pending
        .insert(id.clone(), (hub, request));
    id
}

fn read(world: &mut World, hub: Entity, of: protocol::Keeper) {
    let id = ask(world, hub, Request::Papers(of), |id| {
        protocol::ClientMessage::Read(protocol::Read::Papers {
            id,
            of,
            full: None,
        })
    });
    if let Some(mut asked) = world.get_mut::<Asked>(hub) {
        asked.papers = Some(id);
    }
}

/// Reads `of`'s papers again in every Hub that shows them, after a
/// change made from any one of them.
fn read_everywhere(world: &mut World, of: protocol::Keeper) {
    for hub in hubs(world) {
        if keeper(world, hub) == Some(of) {
            read(world, hub, of);
        }
    }
}

/// Reads the papers of each Hub's newly shown agent or profile.
pub fn keep_papers(world: &mut World) {
    for hub in hubs(world) {
        let wanted = keeper(world, hub);
        let Some(mut asked) = world.get_mut::<Asked>(hub) else {
            continue;
        };
        if asked.keeper == wanted {
            continue;
        }
        asked.keeper = wanted;
        world.entity_mut(hub).insert(Papers::default());
        if let Some(of) = wanted {
            read(world, hub, of);
        }
    }
}

/// Reads the whole prompt of `agent` for `hub`.
pub(super) fn prompt(world: &mut World, hub: Entity, agent: Entity) {
    let Some(agent) = id_of(world, agent) else {
        return;
    };
    let id = ask(world, hub, Request::Prompt(agent), |id| {
        protocol::ClientMessage::Read(protocol::Read::Prompt {
            id,
            agent,
        })
    });
    if let Some(mut asked) = world.get_mut::<Asked>(hub) {
        asked.prompt = Some(id);
    }
}

/// The profile whose character `hub` shows.
fn profile_shown(
    world: &World,
    hub: Entity,
    of: protocol::Keeper,
) -> Option<protocol::ProfileId> {
    match of {
        protocol::Keeper::Profile(profile) => Some(profile),
        // The Hand is no agent of a profile: it has no character.
        protocol::Keeper::Agent(_) => {
            world.get::<OfProfile>(agent(world, hub)).map(|of| of.0)
        }
    }
}

/// Says why the last change to `hub`'s papers failed, or nothing.
fn fail(world: &mut World, hub: Entity, failed: Option<String>) {
    if let Some(mut papers) = world.get_mut::<Papers>(hub) {
        papers.failed = failed;
    }
}

/// Reads the whole of `target` for `hub`, to open it in the editor.
fn read_whole(world: &mut World, hub: Entity, target: Target) {
    let Some(of) = keeper(world, hub) else {
        return;
    };
    fail(world, hub, None);
    ask(world, hub, Request::Whole(of, target), |id| {
        protocol::ClientMessage::Read(protocol::Read::Papers {
            id,
            of,
            full: Some(true),
        })
    });
}

/// Opens the character `hub` shows in the editor.
pub(super) fn edit_character(world: &mut World, hub: Entity) {
    let Some(of) = keeper(world, hub) else {
        return;
    };
    match profile_shown(world, hub, of) {
        Some(profile) => {
            read_whole(world, hub, Target::Character(profile));
        }
        None => fail(
            world,
            hub,
            Some(
                "This agent has no profile, so no character to edit."
                    .into(),
            ),
        ),
    }
}

/// Opens the memory file `path` `hub` shows in the editor.
pub(super) fn edit_memory(
    world: &mut World,
    hub: Entity,
    path: String,
) {
    let Some(of) = keeper(world, hub) else {
        return;
    };
    read_whole(world, hub, Target::Memory(of, path));
}

/// Opens a new memory file named `name` in the editor, in the memory
/// `hub` shows.
pub(super) fn add_memory(world: &mut World, hub: Entity, name: &str) {
    let name = name.trim().trim_start_matches('/');
    if name.is_empty() {
        return;
    }
    let path = if name.contains('.') {
        name.to_string()
    } else {
        format!("{name}.md")
    };
    let Some(of) = keeper(world, hub) else {
        return;
    };
    open_document(
        world,
        hub,
        of,
        Target::Memory(of, path),
        NEW_FILE.to_string(),
    );
}

/// Removes the memory file `path` `hub` shows: an empty text does.
pub(super) fn remove_memory(
    world: &mut World,
    hub: Entity,
    path: String,
) {
    let Some(of) = keeper(world, hub) else {
        return;
    };
    fail(world, hub, None);
    let id = ask(world, hub, Request::Remove(of), |id| {
        protocol::ClientMessage::Edit(protocol::Edit::Memory {
            id,
            of,
            path,
            text: String::new(),
        })
    });
    crate::saving::track(world, id);
}

/// Opens `text` in the editor as the document of `target`, from
/// `hub`.
fn open_document(
    world: &mut World,
    hub: Entity,
    shown: protocol::Keeper,
    target: Target,
    text: String,
) {
    let mut requests = world.resource_mut::<Requests>();
    let key = match requests
        .documents
        .iter_mut()
        .find(|doc| doc.target == target)
    {
        Some(doc) => {
            (doc.hub, doc.shown) = (hub, shown);
            doc.key.clone()
        }
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
                hub,
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
            .map(|doc| (doc.target.clone(), doc.hub, doc.shown));
        let Some((target, hub, of)) = document else {
            return;
        };
        let id = ask(world, hub, Request::Save(of), |id| {
            protocol::ClientMessage::Edit(match target {
                Target::Character(profile) => {
                    protocol::Edit::Character {
                        id,
                        profile,
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
        if let Some(doc) = requests
            .documents
            .iter_mut()
            .find(|doc| doc.key == saved.key)
        {
            doc.latest = Some(id);
        }
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

/// Whether `message` says a change was made.
fn is_done(message: &protocol::ServerMessage) -> bool {
    matches!(
        message,
        protocol::ServerMessage::Reply(reply)
            if reply.kind == protocol::ReplyKind::Notice
    )
}

pub(super) fn response(reply: On<Response>, mut commands: Commands) {
    let reply = reply.event().clone();
    commands.queue(move |world: &mut World| {
        let Some((hub, request)) = world
            .resource_mut::<Requests>()
            .pending
            .remove(&reply.to)
        else {
            return;
        };
        let failed = failure(&reply.message);
        let asked =
            |world: &World,
             read: fn(&Asked) -> &Option<RequestId>| {
                world.get::<Asked>(hub).is_some_and(|asked| {
                    read(asked).as_ref() == Some(&reply.to)
                })
            };
        match request {
            Request::Papers(of) => {
                if keeper(world, hub) != Some(of)
                    || !asked(world, |asked| &asked.papers)
                {
                    return;
                }
                if let protocol::ServerMessage::Read(
                    protocol::ReadResult::Papers { papers, .. },
                ) = reply.message
                    && let Some(mut shown) =
                        world.get_mut::<Papers>(hub)
                {
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
                if selected(world, hub) != Some(agent)
                    || !asked(world, |asked| &asked.prompt)
                {
                    return;
                }
                let Some(mut briefing) =
                    world.get_mut::<Briefing>(hub)
                else {
                    return;
                };
                match (reply.message, failed) {
                    (
                        protocol::ServerMessage::Read(
                            protocol::ReadResult::Prompt {
                                prompt,
                                ..
                            },
                        ),
                        _,
                    ) => briefing.prompt = prompt,
                    (_, Some(why)) => briefing.failed = Some(why),
                    _ => {}
                }
            }
            Request::Whole(of, target) => {
                if let Some(failed) = failed {
                    if keeper(world, hub) == Some(of) {
                        fail(world, hub, Some(failed));
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
                    open_document(world, hub, of, target, text);
                }
            }
            Request::Save(of) => {
                let latest = world
                    .resource::<Requests>()
                    .documents
                    .iter()
                    .any(|doc| {
                        doc.latest.as_ref() == Some(&reply.to)
                    });
                if !latest {
                    return;
                }
                if keeper(world, hub) == Some(of) {
                    fail(world, hub, failed);
                }
                if is_done(&reply.message) {
                    read_everywhere(world, of);
                }
            }
            Request::Remove(of) => {
                if keeper(world, hub) == Some(of) {
                    fail(world, hub, failed);
                }
                if is_done(&reply.message) {
                    read_everywhere(world, of);
                }
            }
        }
    });
}

/// Reads again the papers a Hub shows when its agent's turn ends.
pub(super) fn papers_on_job(
    job: On<JobFinished>,
    mut commands: Commands,
) {
    let chat = job.chat;
    commands.queue(move |world: &mut World| {
        for hub in hubs(world) {
            let agent = agent(world, hub);
            let ours = world
                .get::<TalksTo>(chat)
                .is_some_and(|to| to.0 == agent)
                || crate::convo::chat_of(world, hub) == chat;
            if ours && let Some(of) = keeper(world, hub) {
                read(world, hub, of);
            }
        }
    });
}
