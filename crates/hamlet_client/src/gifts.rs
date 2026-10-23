//! Gifts to an agent: skills the server names, and files uploaded in
//! parts, walked and read on the runtime's blocking threads, which
//! wake the window for the update that collects them.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use base64::Engine;
use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use hamlet_protocol::client as protocol;
use tokio::sync::oneshot;

use crate::Tokio;
use crate::connection::Wake;
use crate::messages::{Reconnected, Response};
use crate::outbox::{Outbox, RequestId};
use crate::realm::{self, Deeds};

/// The most bytes of a file sent in one part.
const PART: u64 = 512 * 1024;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Gifts>()
        .init_resource::<GiftReports>()
        .init_resource::<Reading>()
        .add_observer(
            |response: On<Response>, mut commands: Commands| {
                let response = response.event().clone();
                commands.queue(move |world: &mut World| {
                    answered(world, response);
                });
            },
        )
        .add_observer(lost)
        .add_systems(Update, collect.run_if(reading));
}

/// A skill gift listed by the server.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub skill: String,
    pub from: Option<String>,
    source: protocol::GiveSkillSource,
}

/// The server's replies to gifts, for the chief.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct GiftReports(pub Vec<(String, bool)>);

/// A skill gift the server handed to its agent.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct SkillGiven {
    pub entity: Entity,
}

#[derive(Resource, Default)]
struct Gifts(HashMap<RequestId, Gift>);

struct Gift {
    agent: Entity,
    files: Vec<GiftFile>,
    upload: Option<String>,
    skill: bool,
}

struct GiftFile {
    path: String,
    base: PathBuf,
    file: Option<File>,
    size: u64,
    offset: u64,
}

/// Gifts whose files are being walked or read off the frame.
#[derive(Resource, Default)]
struct Reading(Vec<Pending>);

enum Pending {
    /// A letter's paths walked into its manifest.
    Walk {
        agent: Entity,
        to: protocol::Address,
        done: oneshot::Receiver<io::Result<Vec<GiftFile>>>,
    },
    /// The next part of the first file of `gift`.
    Part {
        gift: Gift,
        done: oneshot::Receiver<
            io::Result<(GiftFile, protocol::GivePart)>,
        >,
    },
}

/// A skill gift sent to the receiving agent.
pub fn give(world: &mut World, agent: Entity, offer: &Offer) {
    let Some(to) = realm::address_of(world, agent) else {
        return;
    };
    if !Deeds::is_allowed(world, "give") {
        return;
    }
    queue(
        world,
        protocol::ClientMessage::Give(protocol::Give::Skill {
            id: RequestId::new(),
            to,
            skill: offer.source.clone(),
        }),
        Gift {
            agent,
            files: Vec::new(),
            upload: None,
            skill: true,
        },
    );
}

/// The skills the received state offers to an agent.
pub fn offers(world: &World, agent: Entity) -> Vec<Offer> {
    if !Deeds::is_allowed(world, "give") {
        return Vec::new();
    }
    let Some(to) = realm::address_of(world, agent) else {
        return Vec::new();
    };
    let own = world.get::<realm::HouseSkills>(agent);
    let has =
        |skill: &String| own.is_some_and(|own| own.0.contains(skill));
    let mut offers = world
        .get_resource::<realm::Starters>()
        .into_iter()
        .flat_map(|starters| &starters.0)
        .filter(|starter| !has(&starter.name))
        .map(|starter| Offer {
            skill: starter.name.clone(),
            from: None,
            source: protocol::GiveSkillSource::Starter {
                starter: starter.name.clone(),
            },
        })
        .collect::<Vec<_>>();
    let mut others = world
        .try_query::<(Entity, &realm::HouseSkills)>()
        .map(|mut query| {
            query
                .iter(world)
                .filter_map(|(entity, skills)| {
                    let address = realm::address_of(world, entity)?;
                    (entity != agent).then_some((address, skills))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    others.sort_by(|(a, _), (b, _)| {
        (&a.hamlet, &a.agent).cmp(&(&b.hamlet, &b.agent))
    });
    for (address, skills) in others {
        let from = if address.hamlet == to.hamlet {
            address.agent.clone()
        } else {
            format!("{}@{}", address.agent, address.hamlet)
        };
        offers.extend(
            skills.0.iter().filter(|skill| !has(skill)).map(
                |skill| Offer {
                    skill: skill.clone(),
                    from: Some(from.clone()),
                    source: protocol::GiveSkillSource::From {
                        from: address.clone(),
                        name: skill.clone(),
                    },
                },
            ),
        );
    }
    offers
}

/// Files and folders sent to `agent` as one gift, its manifest queued
/// once they are walked.
pub fn letter(world: &mut World, agent: Entity, paths: Vec<PathBuf>) {
    let Some(to) = realm::address_of(world, agent) else {
        return;
    };
    if !Deeds::is_allowed(world, "give") {
        return;
    }
    let walk = move || {
        let mut found = Vec::new();
        for path in &paths {
            let base = path.parent().unwrap_or(Path::new(""));
            files(path, base, &mut found)?;
        }
        Ok(found)
    };
    if let Some(done) = off_frame(world, walk) {
        world.resource_mut::<Reading>().0.push(Pending::Walk {
            agent,
            to,
            done,
        });
    }
}

fn report(world: &mut World, text: String, failed: bool) {
    world.resource_mut::<GiftReports>().0.push((text, failed));
}

fn files(
    path: &Path,
    base: &Path,
    into: &mut Vec<GiftFile>,
) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        let mut entries = std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        for entry in entries {
            files(&entry, base, into)?;
        }
    } else if metadata.is_file() {
        let relative = path
            .strip_prefix(base)
            .ok()
            .and_then(Path::to_str)
            .ok_or_else(|| {
                io::Error::other("The gift needs a UTF-8 file name.")
            })?
            .replace(std::path::MAIN_SEPARATOR, "/");
        let size = metadata.len();
        into.push(GiftFile {
            path: relative,
            base: base.to_path_buf(),
            file: None,
            size,
            offset: 0,
        });
    } else {
        return Err(io::Error::other(
            "A gift can contain only regular files and folders, \
             without links.",
        ));
    }
    Ok(())
}

/// `work` run on the runtime's blocking threads, its result to be
/// collected by the update it wakes the window for.
fn off_frame<T: Send + 'static>(
    world: &mut World,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<oneshot::Receiver<T>> {
    let Some(Tokio(runtime)) = world.get_resource::<Tokio>() else {
        unread(world, &io::Error::other("the window has no runtime"));
        return None;
    };
    let wake = world.get_resource::<Wake>().cloned();
    let (sender, done) = oneshot::channel();
    runtime.spawn_blocking(move || {
        // A receiver dropped on reconnection no longer wants it, nor
        // the update that would collect it.
        if sender.send(work()).is_ok()
            && let Some(wake) = wake
        {
            wake.call();
        }
    });
    Some(done)
}

fn reading(reading: Res<Reading>) -> bool {
    !reading.0.is_empty()
}

/// Queues the manifests and parts whose files were walked or read.
fn collect(world: &mut World) {
    let pending =
        std::mem::take(&mut world.resource_mut::<Reading>().0);
    let mut waiting = Vec::new();
    for pending in pending {
        match pending {
            Pending::Walk {
                agent,
                to,
                mut done,
            } => match finished(&mut done) {
                None => {
                    waiting.push(Pending::Walk { agent, to, done });
                }
                Some(Ok(files)) => manifest(world, agent, to, files),
                Some(Err(error)) => unread(world, &error),
            },
            Pending::Part { mut gift, mut done } => {
                match finished(&mut done) {
                    None => {
                        waiting.push(Pending::Part { gift, done });
                    }
                    Some(Ok((file, part))) => {
                        gift.files.insert(0, file);
                        queue(
                            world,
                            protocol::ClientMessage::GivePart(part),
                            gift,
                        );
                    }
                    Some(Err(error)) => unread(world, &error),
                }
            }
        }
    }
    world.resource_mut::<Reading>().0.extend(waiting);
}

/// The result of `done`, or `None` while its work runs.
fn finished<T>(
    done: &mut oneshot::Receiver<io::Result<T>>,
) -> Option<io::Result<T>> {
    match done.try_recv() {
        Ok(result) => Some(result),
        Err(oneshot::error::TryRecvError::Empty) => None,
        Err(oneshot::error::TryRecvError::Closed) => {
            Some(Err(io::Error::other("the reading stopped")))
        }
    }
}

fn unread(world: &mut World, error: &io::Error) {
    report(world, format!("Could not read the gift: {error}"), true);
}

fn manifest(
    world: &mut World,
    agent: Entity,
    to: protocol::Address,
    files: Vec<GiftFile>,
) {
    let manifest = files
        .iter()
        .map(|file| protocol::GiveFile {
            path: file.path.clone(),
            size: file.size,
        })
        .collect();
    queue(
        world,
        protocol::ClientMessage::Give(protocol::Give::Files {
            id: RequestId::new(),
            to,
            files: manifest,
        }),
        Gift {
            agent,
            files,
            upload: None,
            skill: false,
        },
    );
}

fn queue(
    world: &mut World,
    mut message: protocol::ClientMessage,
    gift: Gift,
) {
    // The message is measured with a stand-in id as long as the
    // outbox's.
    if serde_json::to_vec(&message)
        .map_or(true, |bytes| bytes.len() >= 1024 * 1024)
    {
        report(
            world,
            "The gift's message is too large to send.".into(),
            true,
        );
        return;
    }
    let id = world.resource_mut::<Outbox>().send(|request| {
        match &mut message {
            protocol::ClientMessage::Give(
                protocol::Give::Files { id, .. }
                | protocol::Give::Skill { id, .. },
            ) => *id = request,
            protocol::ClientMessage::GivePart(part) => {
                part.id = request;
            }
            _ => unreachable!(),
        }
        message
    });
    world.resource_mut::<Gifts>().0.insert(id, gift);
}

fn open_gift(file: &GiftFile) -> io::Result<File> {
    #[cfg(unix)]
    {
        use rustix::fs::{Mode, OFlags, openat};

        let base = if file.base.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &file.base
        };
        let mut opened = File::open(base)?;
        let mut parts = file.path.split('/').peekable();
        while let Some(part) = parts.next() {
            let mut flags = OFlags::RDONLY
                | OFlags::NOFOLLOW
                | OFlags::NONBLOCK
                | OFlags::CLOEXEC;
            if parts.peek().is_some() {
                flags |= OFlags::DIRECTORY;
            }
            opened =
                openat(&opened, part, flags, Mode::empty())?.into();
        }
        Ok(opened)
    }
    #[cfg(not(unix))]
    File::open(file.base.join(&file.path))
}

/// The next part of `file`, read from where the last one ended.
fn read_part(
    mut file: GiftFile,
    upload: String,
) -> io::Result<(GiftFile, protocol::GivePart)> {
    let mut opened = match file.file.take() {
        Some(opened) => opened,
        None => {
            let opened = open_gift(&file)?;
            let metadata = opened.metadata()?;
            if !metadata.is_file() || metadata.len() != file.size {
                return Err(io::Error::other(
                    "The gift's file changed after it was offered.",
                ));
            }
            opened
        }
    };
    let mut bytes =
        vec![0; (file.size - file.offset).min(PART) as usize];
    opened.read_exact(&mut bytes)?;
    let offset = file.offset;
    file.offset += bytes.len() as u64;
    if file.offset < file.size {
        file.file = Some(opened);
    }
    let part = protocol::GivePart {
        id: RequestId::new(),
        upload,
        path: file.path.clone(),
        offset,
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    };
    Ok((file, part))
}

fn answered(world: &mut World, response: Response) {
    let Some(mut gift) =
        world.resource_mut::<Gifts>().0.remove(&response.to)
    else {
        return;
    };
    let reply = match response.message {
        protocol::ServerMessage::Reply(reply) => reply,
        protocol::ServerMessage::Fault(fault) => {
            report(world, fault.text, true);
            return;
        }
        _ => return,
    };
    if reply.kind != protocol::ReplyKind::Notice {
        report(world, reply.text, true);
        return;
    }
    if let Some(upload) = reply.upload {
        gift.upload = Some(upload);
    }
    gift.files.retain(|file| file.offset < file.size);
    if gift.files.is_empty() {
        if gift.skill {
            world.trigger(SkillGiven { entity: gift.agent });
        }
        report(world, reply.text, false);
        return;
    }
    let Some(upload) = gift.upload.clone() else {
        report(
            world,
            "The server did not accept the gift's upload.".into(),
            true,
        );
        return;
    };
    let file = gift.files.remove(0);
    if let Some(done) =
        off_frame(world, move || read_part(file, upload))
    {
        world
            .resource_mut::<Reading>()
            .0
            .push(Pending::Part { gift, done });
    }
}

/// Drops the gifts a lost connection ended: those already sent, and
/// the parts being read for an upload.
fn lost(
    _: On<Reconnected>,
    mut gifts: ResMut<Gifts>,
    mut reading: ResMut<Reading>,
    outbox: Res<Outbox>,
    mut reports: ResMut<GiftReports>,
) {
    let before = gifts.0.len() + reading.0.len();
    gifts.0.retain(|id, gift| {
        gift.upload.is_none()
            && outbox.iter().any(|message| match message {
                protocol::ClientMessage::Give(
                    protocol::Give::Files { id: queued, .. }
                    | protocol::Give::Skill { id: queued, .. },
                ) => queued == id,
                _ => false,
            })
    });
    reading
        .0
        .retain(|pending| matches!(pending, Pending::Walk { .. }));
    if gifts.0.len() + reading.0.len() < before {
        reports.0.push((
            "The gift's connection was lost. Check with the agent \
             before giving it again."
                .into(),
            true,
        ));
    }
}

#[cfg(test)]
mod tests;
