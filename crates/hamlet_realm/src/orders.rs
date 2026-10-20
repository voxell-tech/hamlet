//! The chief's deeds by hand, carried to the server as commands and
//! gifts.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use bevy::prelude::*;
use hamlet_client::ViewDir;
use hamlet_client::messages::{Reconnected, Response};
use hamlet_client::outbox::{Outbox, RequestId};
use hamlet_client::realm::{self as agents, Deeds, HOME, Settlement};
use hamlet_protocol::v0;

use crate::chief::{Player, Stands};
use crate::generator::society::Place;
use crate::hamlet::{hamlet_of, town_of};
use crate::hand::{self, Wish};
use crate::library::Stood;
use crate::moving::{self, SetDown};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Gifts>()
        .init_resource::<GiftReports>()
        .add_observer(
            |response: On<Response>, mut commands: Commands| {
                let response = response.event().clone();
                commands.queue(move |world: &mut World| {
                    answered(world, response)
                });
            },
        )
        .add_observer(
            |_: On<Reconnected>,
             mut gifts: ResMut<Gifts>,
             outbox: Res<Outbox>,
             mut reports: ResMut<GiftReports>| {
                let before = gifts.0.len();
                gifts.0.retain(|id, gift| {
                    gift.upload.is_none()
                        && outbox.iter().any(
                            |message| match message {
                                v0::ClientMessage::Give(
                                    v0::Give::Files {
                                        id: queued,
                                        ..
                                    }
                                    | v0::Give::Skill {
                                        id: queued,
                                        ..
                                    },
                                ) => queued == id,
                                _ => false,
                            },
                        )
                });
                if gifts.0.len() < before {
                    reports.0.push((
                        "The gift's connection was lost. Check with \
                         the agent before giving it again."
                            .into(),
                        true,
                    ));
                }
            },
        )
        .add_systems(Update, grant);
}

/// A chief's command for the hamlet `hamlet`, as the chief would
/// type it.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Decree {
    pub hamlet: String,
    pub text: String,
}

/// Words from the chief to `agent`, in its own chat.
#[derive(Event, Clone, Debug, PartialEq)]
pub struct Tell {
    pub agent: Entity,
    pub text: String,
}

fn name(world: &World, agent: Entity) -> Option<String> {
    world
        .get::<Name>(agent)
        .map(|name| name.as_str().to_string())
}

/// A chief's command sent to the mayor of `agent`'s hamlet.
pub fn decree(world: &mut World, agent: Entity, text: String) {
    let hamlet =
        agents::hamlet_of(world, agent).unwrap_or(HOME).to_string();
    world.trigger(Decree { hamlet, text });
    world.flush();
}

/// Builds a road between two agents at the chief's word.
fn road(world: &mut World, a: Entity, b: Entity) {
    if !supported(world, "road") {
        return;
    }
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/road {a_name} {b_name}"));
    }
}

/// Takes up the road between two agents at the chief's word.
pub fn part(world: &mut World, a: Entity, b: Entity) {
    if !supported(world, "unroad") {
        return;
    }
    if let (Some(a_name), Some(b_name)) =
        (name(world, a), name(world, b))
    {
        decree(world, a, format!("/unroad {a_name} {b_name}"));
    }
}

/// Has two agents greet each other and builds a road between them.
pub fn introduce(world: &mut World, a: Entity, b: Entity) {
    hand::greet(world, a, b);
    road(world, a, b);
}

/// A skill gift listed by the server.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub skill: String,
    pub from: Option<String>,
    source: v0::GiveSkillSkill,
}

/// The server's replies to gifts, for the chief.
#[derive(Resource, Default, Clone, PartialEq)]
pub struct GiftReports(pub Vec<(String, bool)>);

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

/// A skill gift sent to the receiving agent.
pub fn give(world: &mut World, agent: Entity, offer: &Offer) {
    let Some(to) = agents::address_of(world, agent) else {
        return;
    };
    if !supported(world, "give") {
        return;
    }
    queue(
        world,
        v0::ClientMessage::Give(v0::Give::Skill {
            id: String::new(),
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
    if !supported(world, "give") {
        return Vec::new();
    }
    let Some(to) = agents::address_of(world, agent) else {
        return Vec::new();
    };
    let own = world.get::<agents::HouseSkills>(agent);
    let has =
        |skill: &String| own.is_some_and(|own| own.0.contains(skill));
    let mut offers = world
        .get_resource::<agents::Starters>()
        .into_iter()
        .flat_map(|starters| &starters.0)
        .filter(|starter| !has(&starter.name))
        .map(|starter| Offer {
            skill: starter.name.clone(),
            from: None,
            source: v0::GiveSkillSkill::Starter {
                starter: starter.name.clone(),
            },
        })
        .collect::<Vec<_>>();
    let mut others = world
        .try_query::<(Entity, &agents::HouseSkills)>()
        .map(|mut query| {
            query
                .iter(world)
                .filter_map(|(entity, skills)| {
                    let address = agents::address_of(world, entity)?;
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
                    source: v0::GiveSkillSkill::From {
                        from: address.clone(),
                        name: skill.clone(),
                    },
                },
            ),
        );
    }
    offers
}

fn supported(world: &World, deed: &str) -> bool {
    world
        .get_resource::<Deeds>()
        .is_some_and(|deeds| deeds.has(deed))
}

fn report(world: &mut World, text: String, failed: bool) {
    world
        .get_resource_or_init::<GiftReports>()
        .0
        .push((text, failed));
}

fn files(
    path: &Path,
    base: &Path,
    into: &mut Vec<GiftFile>,
) -> std::io::Result<()> {
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
                std::io::Error::other(
                    "The gift needs a UTF-8 file name.",
                )
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
        return Err(std::io::Error::other(
            "A gift can contain only regular files and folders, \
             without links.",
        ));
    }
    Ok(())
}

fn letter(world: &mut World, agent: Entity, paths: &[PathBuf]) {
    let Some(to) = agents::address_of(world, agent) else {
        return;
    };
    if !supported(world, "give") {
        return;
    }
    let mut gift = Gift {
        agent,
        files: Vec::new(),
        upload: None,
        skill: false,
    };
    for path in paths {
        let base = path.parent().unwrap_or(Path::new(""));
        if let Err(error) = files(path, base, &mut gift.files) {
            report(
                world,
                format!("Could not read the gift: {error}"),
                true,
            );
            return;
        }
    }
    let files = gift
        .files
        .iter()
        .map(|file| v0::GiveFilesFiles {
            path: file.path.clone(),
            size: file.size,
        })
        .collect();
    queue(
        world,
        v0::ClientMessage::Give(v0::Give::Files {
            id: String::new(),
            to,
            files,
        }),
        gift,
    );
}

fn queue(
    world: &mut World,
    mut message: v0::ClientMessage,
    gift: Gift,
) {
    if serde_json::to_vec(&message)
        .map_or(true, |bytes| bytes.len() + 36 >= 1024 * 1024)
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
            v0::ClientMessage::Give(
                v0::Give::Files { id, .. }
                | v0::Give::Skill { id, .. },
            ) => *id = request,
            v0::ClientMessage::GivePart(part) => part.id = request,
            _ => unreachable!(),
        }
        message
    });
    world.get_resource_or_init::<Gifts>().0.insert(id, gift);
}

fn open_gift(file: &GiftFile) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::fd::{AsRawFd, FromRawFd};

        let base = if file.base.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &file.base
        };
        let mut opened = File::open(base)?;
        let mut parts = file.path.split('/').peekable();
        while let Some(part) = parts.next() {
            let part =
                CString::new(part).map_err(std::io::Error::other)?;
            let mut flags = libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_CLOEXEC;
            if parts.peek().is_some() {
                flags |= libc::O_DIRECTORY;
            }
            let fd = unsafe {
                libc::openat(opened.as_raw_fd(), part.as_ptr(), flags)
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            // The successful openat hands this file a new descriptor.
            opened = unsafe { File::from_raw_fd(fd) };
        }
        Ok(opened)
    }
    #[cfg(not(unix))]
    File::open(file.base.join(&file.path))
}

fn answered(world: &mut World, response: Response) {
    let Some(mut gift) =
        world.resource_mut::<Gifts>().0.remove(&response.to)
    else {
        return;
    };
    let reply = match response.message {
        v0::ServerMessage::Reply(reply) => reply,
        v0::ServerMessage::Fault(fault) => {
            report(world, fault.text, true);
            return;
        }
        _ => return,
    };
    if reply.kind != v0::ReplyKind::Notice {
        report(world, reply.text, true);
        return;
    }
    if let Some(upload) = reply.upload {
        gift.upload = Some(upload);
    }
    gift.files.retain(|file| file.offset < file.size);
    if gift.files.is_empty() {
        if gift.skill {
            hand::gifted(world, gift.agent);
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
    let file = &mut gift.files[0];
    let mut bytes =
        vec![0; (file.size - file.offset).min(512 * 1024) as usize];
    let read = (|| {
        if file.file.is_none() {
            let opened = open_gift(file)?;
            let metadata = opened.metadata()?;
            if !metadata.is_file() || metadata.len() != file.size {
                return Err(std::io::Error::other(
                    "The gift's file changed after it was offered.",
                ));
            }
            file.file = Some(opened);
        }
        file.file.as_mut().unwrap().read_exact(&mut bytes)
    })();
    if let Err(error) = read {
        report(
            world,
            format!("Could not read the gift: {error}"),
            true,
        );
        return;
    }
    let offset = file.offset;
    file.offset += bytes.len() as u64;
    if file.offset == file.size {
        file.file = None;
    }
    let path = file.path.clone();
    let data =
        base64::engine::general_purpose::STANDARD.encode(bytes);
    queue(
        world,
        v0::ClientMessage::GivePart(v0::GivePart {
            id: String::new(),
            upload,
            path,
            offset,
            data,
        }),
        gift,
    );
}

/// Takes the town of `hamlet` in hand, to be set down with a click
/// on the ground.
pub fn carry(world: &mut World, hamlet: Entity) {
    if !supported(world, "move-hamlet") {
        return;
    }
    if let Some(town) = town_of(world, hamlet) {
        moving::pick_up(world, Place::Town(town));
    }
}

/// Records the place a chief set something down. The world moves it
/// when the record says so.
fn set_down(world: &mut World, set: SetDown) {
    match set.place {
        Place::Town(town) => {
            if !supported(world, "move-hamlet") {
                return;
            }
            let Some(hamlet) = hamlet_of(world, town) else {
                return;
            };
            let Some(name) = world
                .get::<Settlement>(hamlet)
                .map(|settlement| settlement.name.clone())
            else {
                return;
            };
            let (x, z) = (set.to.x, set.to.y);
            world.trigger(Decree {
                text: format!("/move-hamlet {name} {x:.1} {z:.1}"),
                hamlet: name,
            });
            world.flush();
        }
        Place::Chief => {
            let player = world
                .query_filtered::<Entity, With<Player>>()
                .iter(world)
                .next();
            if let Some(chief) = player {
                world.entity_mut(chief).insert(Stands(set.to));
            }
        }
        Place::Library => {
            if let Some(view) = world.get_resource::<ViewDir>() {
                let file = Stood::file(&view.0);
                let _ = Stands(set.to).write_at(&file);
            }
            world.resource_mut::<Stood>().0 = Some(set.to);
        }
    }
}

/// Carries out everything the hand asked for.
fn grant(
    mut set: MessageReader<SetDown>,
    mut wishes: MessageReader<Wish>,
    mut commands: Commands,
) {
    for set in set.read().copied() {
        commands.queue(move |world: &mut World| set_down(world, set));
    }
    let mut letters = Vec::<(Entity, Vec<PathBuf>)>::new();
    for wish in wishes.read().cloned() {
        match wish {
            Wish::Road { a, b } => commands
                .queue(move |world: &mut World| road(world, a, b)),
            Wish::Letter { to, path } => {
                if let Some((_, paths)) =
                    letters.iter_mut().find(|(agent, _)| *agent == to)
                {
                    paths.push(path);
                } else {
                    letters.push((to, vec![path]));
                }
            }
        }
    }
    for (to, paths) in letters {
        commands.queue(move |world: &mut World| {
            letter(world, to, &paths)
        });
    }
}

#[cfg(test)]
mod tests {
    use hamlet_client::ClientPlugin;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::v0;
    use hamlet_testing::{Scratch, realm};

    use super::*;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None })
            .add_message::<Wish>()
            .add_message::<SetDown>()
            .add_plugins(plugin);
        let mut state = realm::state();
        state.deeds = hamlet_testing::realm::deeds(&[
            "give",
            "road",
            "move-hamlet",
        ]);
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        let founder = agents::mayor(app.world_mut()).unwrap();
        (app, founder)
    }

    fn sent(app: &mut App) -> Vec<v0::ClientMessage> {
        app.world_mut().resource_mut::<Outbox>().drain().collect()
    }

    #[test]
    fn a_file_gift_begins_with_its_manifest_and_waits_for_acceptance()
    {
        let dir = Scratch::new("world-gift-manifest");
        let path = dir.join("list.txt");
        std::fs::write(&path, "Dune").unwrap();
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [
            v0::ClientMessage::Give(v0::Give::Files {
                files,
                to,
                ..
            }),
        ] = &messages[..]
        else {
            panic!("one manifest: {messages:?}");
        };
        assert_eq!(
            to,
            &v0::Address {
                hamlet: HOME.into(),
                agent: "founder".into()
            }
        );
        assert_eq!(
            files,
            &[v0::GiveFilesFiles {
                path: "list.txt".into(),
                size: 4
            }]
        );
        app.update();
        assert!(sent(&mut app).is_empty());
    }
    fn reply(
        app: &mut App,
        to: String,
        kind: v0::ReplyKind,
        text: &str,
        upload: Option<&str>,
    ) {
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Reply(v0::Reply {
                to,
                kind,
                text: text.into(),
                upload: upload.map(str::to_string),
            }),
        );
        app.update();
    }

    #[test]
    fn an_accepted_gift_sends_ordered_bounded_parts_and_shows_the_receipt()
     {
        use base64::Engine;
        let dir = Scratch::new("world-gift-parts");
        let path = dir.join("big.bin");
        let bytes = (0..1_100_003)
            .map(|n| (n % 251) as u8)
            .collect::<Vec<_>>();
        std::fs::write(&path, &bytes).unwrap();
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [v0::ClientMessage::Give(v0::Give::Files { id, .. })] =
            &messages[..]
        else {
            panic!("manifest");
        };
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Notice,
            "Ready",
            Some("upload-one"),
        );
        let mut got = Vec::new();
        loop {
            let messages = sent(&mut app);
            let [v0::ClientMessage::GivePart(part)] = &messages[..]
            else {
                panic!("one part at a time: {messages:?}");
            };
            assert!(
                serde_json::to_vec(&messages[0]).unwrap().len()
                    < 1024 * 1024
            );
            assert_eq!(part.path, "big.bin");
            assert_eq!(part.upload, "upload-one");
            assert_eq!(part.offset as usize, got.len());
            got.extend(
                base64::engine::general_purpose::STANDARD
                    .decode(&part.data)
                    .unwrap(),
            );
            let done = got.len() == bytes.len();
            reply(
                &mut app,
                part.id.clone(),
                v0::ReplyKind::Notice,
                if done {
                    "Handed over `inbox/big.bin`."
                } else {
                    "Received the gift part."
                },
                None,
            );
            if done {
                break;
            }
        }
        assert_eq!(got, bytes);
        assert!(sent(&mut app).is_empty());
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [("Handed over `inbox/big.bin`.".into(), false)]
        );
    }

    #[test]
    fn a_refused_folder_shows_the_servers_words_and_sends_no_parts() {
        let dir = Scratch::new("world-gift-folder");
        let path = dir.join("books");
        std::fs::create_dir_all(path.join("notes")).unwrap();
        std::fs::write(path.join("notes/list.txt"), "Dune").unwrap();
        std::fs::write(path.join("empty.txt"), "").unwrap();
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [
            v0::ClientMessage::Give(v0::Give::Files {
                id,
                files,
                ..
            }),
        ] = &messages[..]
        else {
            panic!("manifest");
        };
        assert_eq!(
            files,
            &[
                v0::GiveFilesFiles {
                    path: "books/empty.txt".into(),
                    size: 0
                },
                v0::GiveFilesFiles {
                    path: "books/notes/list.txt".into(),
                    size: 4
                },
            ]
        );
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Error,
            "The inbox already holds books.",
            None,
        );
        assert!(sent(&mut app).is_empty());
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [("The inbox already holds books.".into(), true)]
        );
    }

    #[test]
    fn setting_down_the_library_keeps_its_place_in_the_view_folder() {
        let dir = Scratch::new("world-library-view");
        let (mut app, _) = app();
        app.insert_resource(ViewDir(dir.to_path_buf()))
            .init_resource::<Stood>();
        app.world_mut().write_message(SetDown {
            place: Place::Library,
            to: Vec2::new(12.5, -40.0),
        });
        app.update();
        assert_eq!(
            Stands::read_at(&Stood::file(&dir)).unwrap().0,
            Vec2::new(12.5, -40.0)
        );
    }

    #[test]
    fn skill_offers_use_received_starters_and_other_houses_without_local_files()
     {
        let (mut app, founder) = app();
        let mut state = realm::state();
        state.deeds = hamlet_testing::realm::deeds(&["give"]);
        state.starters = Some(vec![v0::Skill {
            name: "research".into(),
            description: "Find sources".into(),
        }]);
        state.agents[0].skills = Some(vec!["own".into()]);
        let mut scout = realm::agent("home", 1, "scout");
        scout.skills = Some(vec!["maps".into(), "own".into()]);
        state.agents.push(scout.clone());
        let message = v0::ServerMessage::State(state);
        hamlet_client::receive(app.world_mut(), message);
        let offered = offers(app.world(), founder);
        assert_eq!(
            offered
                .iter()
                .map(|offer| (
                    offer.skill.as_str(),
                    offer.from.as_deref()
                ))
                .collect::<Vec<_>>(),
            [("research", None), ("maps", Some("scout"))]
        );
        for (index, expected) in [
            v0::GiveSkillSkill::Starter {
                starter: "research".into(),
            },
            v0::GiveSkillSkill::From {
                from: v0::Address {
                    hamlet: HOME.into(),
                    agent: "scout".into(),
                },
                name: "maps".into(),
            },
        ]
        .into_iter()
        .enumerate()
        {
            give(app.world_mut(), founder, &offered[index]);
            let messages = sent(&mut app);
            let [
                v0::ClientMessage::Give(v0::Give::Skill {
                    skill,
                    to,
                    ..
                }),
            ] = &messages[..]
            else {
                panic!("skill gift: {messages:?}");
            };
            assert_eq!(skill, &expected);
            assert_eq!(to.agent, "founder");
        }
        scout.skills = Some(vec!["compass".into()]);
        let message = v0::ServerMessage::Change(v0::Changes {
            epoch: 1,
            revision: 1,
            changes: vec![v0::Update::Agent { agent: scout }],
        });
        hamlet_client::receive(app.world_mut(), message);
        assert_eq!(offers(app.world(), founder)[1].skill, "compass");
    }
    #[test]
    fn a_folder_upload_keeps_paths_and_skips_empty_files() {
        let dir = Scratch::new("world-gift-folder-parts");
        let path = dir.join("books");
        std::fs::create_dir_all(path.join("notes")).unwrap();
        std::fs::write(path.join("empty.txt"), "").unwrap();
        std::fs::write(path.join("notes/list.txt"), "Dune").unwrap();
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [v0::ClientMessage::Give(v0::Give::Files { id, .. })] =
            &messages[..]
        else {
            panic!("manifest");
        };
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Notice,
            "Ready",
            Some("folder-upload"),
        );
        let messages = sent(&mut app);
        let [v0::ClientMessage::GivePart(part)] = &messages[..]
        else {
            panic!("one nonempty file");
        };
        assert_eq!(part.path, "books/notes/list.txt");
        assert_eq!(part.offset, 0);
        assert_eq!(part.data, "RHVuZQ==");
        reply(
            &mut app,
            part.id.clone(),
            v0::ReplyKind::Notice,
            "Handed over `inbox/books`.",
            None,
        );
        assert!(sent(&mut app).is_empty());
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [("Handed over `inbox/books`.".into(), false)]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_of_three_hundred_files_uploads_under_the_desktop_file_limit()
     {
        if std::env::var_os("HAMLET_TEST_GIFT_FILE_LIMIT").is_none() {
            let output = std::process::Command::new("/bin/sh")
                .args(["-c", "ulimit -n 256; exec \"$@\"", "gift-test"])
                .arg(std::env::current_exe().unwrap())
                .args(["--exact", "orders::tests::a_folder_of_three_hundred_files_uploads_under_the_desktop_file_limit"])
                .env("HAMLET_TEST_GIFT_FILE_LIMIT", "1")
                .output().unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let dir = Scratch::new("world-gift-many-files");
        let path = dir.join("books");
        std::fs::create_dir(&path).unwrap();
        for n in 0..300 {
            std::fs::write(path.join(format!("{n:03}.txt")), "x")
                .unwrap();
        }
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [
            v0::ClientMessage::Give(v0::Give::Files {
                id,
                files,
                ..
            }),
        ] = &messages[..]
        else {
            panic!(
                "one manifest, reports: {:?}",
                app.world().resource::<GiftReports>().0
            );
        };
        assert_eq!(files.len(), 300);
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Notice,
            "Ready",
            Some("many-files"),
        );
        for n in 0..300 {
            let messages = sent(&mut app);
            let [v0::ClientMessage::GivePart(part)] = &messages[..]
            else {
                panic!("part {n}: {messages:?}");
            };
            assert_eq!(part.path, format!("books/{n:03}.txt"));
            assert_eq!(part.data, "eA==");
            reply(
                &mut app,
                part.id.clone(),
                v0::ReplyKind::Notice,
                "Received",
                None,
            );
        }
        assert!(sent(&mut app).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_gift_refuses_a_link_inside_a_folder_or_as_its_dropped_root()
    {
        let dir = Scratch::new("world-gift-links");
        let folder = dir.join("books");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(dir.join("secret.txt"), "secret").unwrap();
        std::fs::write(folder.join("ok.txt"), "ok").unwrap();
        std::os::unix::fs::symlink(
            "../secret.txt",
            folder.join("link.txt"),
        )
        .unwrap();
        for path in [folder.clone(), folder.join("link.txt")] {
            let (mut app, founder) = app();
            app.world_mut()
                .write_message(Wish::Letter { to: founder, path });
            app.update();
            assert!(
                sent(&mut app).is_empty(),
                "no manifest for links"
            );
            assert_eq!(
                app.world().resource::<GiftReports>().0,
                [(
                    "Could not read the gift: A gift can contain \
                     only regular files and folders, without links."
                        .into(),
                    true
                )]
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_accepted_gift_refuses_a_file_replaced_by_a_link_or_changed_in_size()
     {
        let dir = Scratch::new("world-gift-swapped-link");
        std::fs::write(dir.join("secret.txt"), "secret").unwrap();
        for link in [true, false] {
            let path = dir.join(if link {
                "link.txt"
            } else {
                "changed.txt"
            });
            std::fs::write(&path, "public").unwrap();
            let (mut app, founder) = app();
            app.world_mut().write_message(Wish::Letter {
                to: founder,
                path: path.clone(),
            });
            app.update();
            let messages = sent(&mut app);
            let [
                v0::ClientMessage::Give(v0::Give::Files {
                    id, ..
                }),
            ] = &messages[..]
            else {
                panic!("manifest");
            };
            if link {
                std::fs::remove_file(&path).unwrap();
                std::os::unix::fs::symlink(
                    dir.join("secret.txt"),
                    &path,
                )
                .unwrap();
            } else {
                std::fs::write(&path, "longer than public").unwrap();
            }
            reply(
                &mut app,
                id.clone(),
                v0::ReplyKind::Notice,
                "Ready",
                Some("swapped"),
            );
            assert!(sent(&mut app).is_empty());
            let reports = &app.world().resource::<GiftReports>().0;
            assert_eq!(reports.len(), 1);
            assert!(
                reports[0].1
                    && reports[0]
                        .0
                        .starts_with("Could not read the gift:")
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_accepted_gift_refuses_a_parent_folder_swapped_for_a_link() {
        let dir = Scratch::new("world-gift-swapped-folder");
        let path = dir.join("books");
        let outside = dir.join("outside");
        std::fs::create_dir(&path).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(path.join("list.txt"), "public").unwrap();
        std::fs::write(outside.join("list.txt"), "secret").unwrap();
        let (mut app, founder) = app();
        app.world_mut().write_message(Wish::Letter {
            to: founder,
            path: path.clone(),
        });
        app.update();
        let messages = sent(&mut app);
        let [v0::ClientMessage::Give(v0::Give::Files { id, .. })] =
            &messages[..]
        else {
            panic!("manifest");
        };
        std::fs::rename(&path, dir.join("original")).unwrap();
        std::os::unix::fs::symlink(&outside, &path).unwrap();
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Notice,
            "Ready",
            Some("swapped-parent"),
        );
        assert!(
            sent(&mut app).is_empty(),
            "no bytes through a replaced parent link"
        );
        assert!(app.world().resource::<GiftReports>().0[0].1);
    }

    #[test]
    fn gifts_queued_offline_survive_reconnection_and_their_replies_are_kept()
     {
        let dir = Scratch::new("world-gift-offline");
        let path = dir.join("list.txt");
        std::fs::write(&path, "Dune").unwrap();
        let (mut app, founder) = app();
        give(
            app.world_mut(),
            founder,
            &Offer {
                skill: "research".into(),
                from: None,
                source: v0::GiveSkillSkill::Starter {
                    starter: "research".into(),
                },
            },
        );
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        app.world_mut().trigger(Reconnected);
        app.world_mut().flush();
        assert!(app.world().resource::<GiftReports>().0.is_empty());
        let messages = sent(&mut app);
        let [
            v0::ClientMessage::Give(v0::Give::Skill {
                id: skill,
                ..
            }),
            v0::ClientMessage::Give(v0::Give::Files {
                id: files,
                ..
            }),
        ] = &messages[..]
        else {
            panic!("queued gifts: {messages:?}");
        };
        reply(
            &mut app,
            skill.clone(),
            v0::ReplyKind::Notice,
            "Gave research.",
            None,
        );
        reply(
            &mut app,
            files.clone(),
            v0::ReplyKind::Notice,
            "Ready",
            Some("offline-files"),
        );
        assert!(matches!(
            sent(&mut app).as_slice(),
            [v0::ClientMessage::GivePart(_)]
        ));
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [("Gave research.".into(), false)]
        );
        app.world_mut().trigger(Reconnected);
        app.world_mut().flush();
        assert!(
            app.world().resource::<GiftReports>().0.last().unwrap().1
        );
    }

    #[test]
    fn a_gift_already_sent_is_reported_lost_on_reconnection() {
        let (mut app, founder) = app();
        give(
            app.world_mut(),
            founder,
            &Offer {
                skill: "research".into(),
                from: None,
                source: v0::GiveSkillSkill::Starter {
                    starter: "research".into(),
                },
            },
        );
        sent(&mut app);
        app.world_mut().trigger(Reconnected);
        app.world_mut().flush();
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [(
                "The gift's connection was lost. Check with the \
                 agent before giving it again."
                    .into(),
                true
            )]
        );
    }

    #[test]
    fn an_empty_gift_completes_with_the_manifest_reply() {
        let dir = Scratch::new("world-gift-empty");
        let path = dir.join("empty.txt");
        std::fs::write(&path, "").unwrap();
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [
            v0::ClientMessage::Give(v0::Give::Files {
                id,
                files,
                ..
            }),
        ] = &messages[..]
        else {
            panic!("manifest");
        };
        assert_eq!(files[0].size, 0);
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Notice,
            "Handed over `inbox/empty.txt`.",
            None,
        );
        assert!(sent(&mut app).is_empty());
        assert_eq!(
            app.world().resource::<GiftReports>().0[0].0,
            "Handed over `inbox/empty.txt`."
        );
    }

    #[test]
    fn a_failed_part_ends_the_upload_and_shows_the_refusal() {
        let dir = Scratch::new("world-gift-part-refused");
        let path = dir.join("large.bin");
        std::fs::write(&path, vec![1; 1024 * 1024]).unwrap();
        let (mut app, founder) = app();
        app.world_mut()
            .write_message(Wish::Letter { to: founder, path });
        app.update();
        let messages = sent(&mut app);
        let [v0::ClientMessage::Give(v0::Give::Files { id, .. })] =
            &messages[..]
        else {
            panic!("manifest");
        };
        reply(
            &mut app,
            id.clone(),
            v0::ReplyKind::Notice,
            "Ready",
            Some("refused-upload"),
        );
        let messages = sent(&mut app);
        let [v0::ClientMessage::GivePart(part)] = &messages[..]
        else {
            panic!("first part");
        };
        reply(
            &mut app,
            part.id.clone(),
            v0::ReplyKind::Error,
            "The gift expired.",
            None,
        );
        assert!(sent(&mut app).is_empty());
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [("The gift expired.".into(), true)]
        );
    }
}
