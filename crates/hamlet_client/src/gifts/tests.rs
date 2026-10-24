use std::sync::LazyLock;
use std::time::{Duration, Instant};

use base64::Engine;
use hamlet_protocol::client as protocol;
use hamlet_testing::{Scratch, realm};

use super::*;
use crate::ClientPlugin;
use crate::realm::HOME;

/// The runtime every test's gifts are read on.
static RUNTIME: LazyLock<tokio::runtime::Runtime> =
    LazyLock::new(|| tokio::runtime::Runtime::new().unwrap());

fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(ClientPlugin { server: None })
        .insert_resource(Tokio(RUNTIME.handle().clone()));
    let mut state = realm::state();
    state.deeds = realm::deeds(&["give"]);
    crate::receive(
        app.world_mut(),
        protocol::ServerMessage::State(state),
    );
    let founder = crate::realm::mayor(app.world_mut()).unwrap();
    (app, founder)
}

/// Updates until no gift's files are being walked or read.
fn settle(app: &mut App) {
    let until = Instant::now() + Duration::from_secs(10);
    app.update();
    while !app.world().resource::<Reading>().0.is_empty() {
        assert!(Instant::now() < until, "the gift is still read");
        std::thread::sleep(Duration::from_millis(1));
        app.update();
    }
}

fn send(app: &mut App, agent: Entity, path: PathBuf) {
    letter(app.world_mut(), agent, vec![path]);
    settle(app);
}

fn sent(app: &mut App) -> Vec<protocol::ClientMessage> {
    app.world_mut().resource_mut::<Outbox>().drain().collect()
}

fn reply(
    app: &mut App,
    to: RequestId,
    kind: protocol::ReplyKind,
    text: &str,
    upload: Option<&str>,
) {
    crate::receive(
        app.world_mut(),
        protocol::ServerMessage::Reply(protocol::Reply {
            to,
            kind,
            text: text.into(),
            upload: upload.map(str::to_string),
        }),
    );
    settle(app);
}

fn research() -> Offer {
    Offer {
        skill: "research".into(),
        from: None,
        source: protocol::GiveSkillSource::Starter {
            starter: "research".into(),
        },
    }
}

#[test]
fn a_file_gift_begins_with_its_manifest_and_waits_for_acceptance() {
    let dir = Scratch::new("world-gift-manifest");
    let path = dir.join("list.txt");
    std::fs::write(&path, "Dune").unwrap();
    let (mut app, founder) = app();
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
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
        &protocol::Address {
            hamlet: HOME.into(),
            agent: "founder".into()
        }
    );
    assert_eq!(
        files,
        &[protocol::GiveFile {
            path: "list.txt".into(),
            size: 4
        }]
    );
    app.update();
    assert!(sent(&mut app).is_empty());
}

#[test]
fn an_accepted_gift_sends_ordered_bounded_parts_and_shows_the_receipt()
 {
    let dir = Scratch::new("world-gift-parts");
    let path = dir.join("big.bin");
    let bytes =
        (0..1_100_003).map(|n| (n % 251) as u8).collect::<Vec<_>>();
    std::fs::write(&path, &bytes).unwrap();
    let (mut app, founder) = app();
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            id,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("manifest");
    };
    reply(
        &mut app,
        id.clone(),
        protocol::ReplyKind::Notice,
        "Ready",
        Some("upload-one"),
    );
    let mut got = Vec::new();
    loop {
        let messages = sent(&mut app);
        let [protocol::ClientMessage::GivePart(part)] = &messages[..]
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
            protocol::ReplyKind::Notice,
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
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
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
            protocol::GiveFile {
                path: "books/empty.txt".into(),
                size: 0
            },
            protocol::GiveFile {
                path: "books/notes/list.txt".into(),
                size: 4
            },
        ]
    );
    reply(
        &mut app,
        id.clone(),
        protocol::ReplyKind::Error,
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
fn skill_offers_use_received_starters_and_other_houses_without_local_files()
 {
    let (mut app, founder) = app();
    let mut state = realm::state();
    state.deeds = realm::deeds(&["give"]);
    state.starters = vec![protocol::Skill {
        name: "research".into(),
        description: "Find sources".into(),
    }];
    state.agents[0].skills = vec!["own".into()];
    let mut scout = realm::agent("home", 1, "scout");
    scout.skills = vec!["maps".into(), "own".into()];
    state.agents.push(scout.clone());
    let message = protocol::ServerMessage::State(state);
    crate::receive(app.world_mut(), message);
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
        protocol::GiveSkillSource::Starter {
            starter: "research".into(),
        },
        protocol::GiveSkillSource::From {
            from: protocol::Address {
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
            protocol::ClientMessage::Give(protocol::Give::Skill {
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
    scout.skills = vec!["compass".into()];
    let message =
        protocol::ServerMessage::Change(protocol::Changes {
            epoch: 1,
            revision: 1,
            changes: vec![protocol::Update::Agent { agent: scout }],
        });
    crate::receive(app.world_mut(), message);
    assert_eq!(offers(app.world(), founder)[1].skill, "compass");
}

#[test]
fn a_given_skill_is_announced_on_its_agent() {
    #[derive(Resource, Default)]
    struct Given(Vec<Entity>);
    let (mut app, founder) = app();
    app.init_resource::<Given>().add_observer(
        |given: On<SkillGiven>, mut seen: ResMut<Given>| {
            seen.0.push(given.entity);
        },
    );
    give(app.world_mut(), founder, &research());
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Skill {
            id,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("skill gift: {messages:?}");
    };
    reply(
        &mut app,
        id.clone(),
        protocol::ReplyKind::Notice,
        "Gave research.",
        None,
    );
    assert_eq!(app.world().resource::<Given>().0, [founder]);
}

#[test]
fn a_folder_upload_keeps_paths_and_skips_empty_files() {
    let dir = Scratch::new("world-gift-folder-parts");
    let path = dir.join("books");
    std::fs::create_dir_all(path.join("notes")).unwrap();
    std::fs::write(path.join("empty.txt"), "").unwrap();
    std::fs::write(path.join("notes/list.txt"), "Dune").unwrap();
    let (mut app, founder) = app();
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            id,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("manifest");
    };
    reply(
        &mut app,
        id.clone(),
        protocol::ReplyKind::Notice,
        "Ready",
        Some("folder-upload"),
    );
    let messages = sent(&mut app);
    let [protocol::ClientMessage::GivePart(part)] = &messages[..]
    else {
        panic!("one nonempty file");
    };
    assert_eq!(part.path, "books/notes/list.txt");
    assert_eq!(part.offset, 0);
    assert_eq!(part.data, "RHVuZQ==");
    reply(
        &mut app,
        part.id.clone(),
        protocol::ReplyKind::Notice,
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
            .args(["--exact", "gifts::tests::a_folder_of_three_hundred_files_uploads_under_the_desktop_file_limit"])
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
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
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
        protocol::ReplyKind::Notice,
        "Ready",
        Some("many-files"),
    );
    for n in 0..300 {
        let messages = sent(&mut app);
        let [protocol::ClientMessage::GivePart(part)] = &messages[..]
        else {
            panic!("part {n}: {messages:?}");
        };
        assert_eq!(part.path, format!("books/{n:03}.txt"));
        assert_eq!(part.data, "eA==");
        reply(
            &mut app,
            part.id.clone(),
            protocol::ReplyKind::Notice,
            "Received",
            None,
        );
    }
    assert!(sent(&mut app).is_empty());
}

#[cfg(unix)]
#[test]
fn a_gift_refuses_a_link_inside_a_folder_or_as_its_dropped_root() {
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
        send(&mut app, founder, path);
        assert!(sent(&mut app).is_empty(), "no manifest for links");
        assert_eq!(
            app.world().resource::<GiftReports>().0,
            [(
                "Could not read the gift: A gift can contain only \
                 regular files and folders, without links."
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
        let path =
            dir.join(if link { "link.txt" } else { "changed.txt" });
        std::fs::write(&path, "public").unwrap();
        let (mut app, founder) = app();
        send(&mut app, founder, path.clone());
        let messages = sent(&mut app);
        let [
            protocol::ClientMessage::Give(protocol::Give::Files {
                id,
                ..
            }),
        ] = &messages[..]
        else {
            panic!("manifest");
        };
        if link {
            std::fs::remove_file(&path).unwrap();
            std::os::unix::fs::symlink(dir.join("secret.txt"), &path)
                .unwrap();
        } else {
            std::fs::write(&path, "longer than public").unwrap();
        }
        reply(
            &mut app,
            id.clone(),
            protocol::ReplyKind::Notice,
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
    send(&mut app, founder, path.clone());
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            id,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("manifest");
    };
    std::fs::rename(&path, dir.join("original")).unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    reply(
        &mut app,
        id.clone(),
        protocol::ReplyKind::Notice,
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
    give(app.world_mut(), founder, &research());
    send(&mut app, founder, path);
    app.world_mut().trigger(Reconnected);
    app.world_mut().flush();
    assert!(app.world().resource::<GiftReports>().0.is_empty());
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Skill {
            id: skill,
            ..
        }),
        protocol::ClientMessage::Give(protocol::Give::Files {
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
        protocol::ReplyKind::Notice,
        "Gave research.",
        None,
    );
    reply(
        &mut app,
        files.clone(),
        protocol::ReplyKind::Notice,
        "Ready",
        Some("offline-files"),
    );
    assert!(matches!(
        sent(&mut app).as_slice(),
        [protocol::ClientMessage::GivePart(_)]
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
    give(app.world_mut(), founder, &research());
    sent(&mut app);
    app.world_mut().trigger(Reconnected);
    app.world_mut().flush();
    assert_eq!(
        app.world().resource::<GiftReports>().0,
        [(
            "The gift's connection was lost. Check with the agent \
             before giving it again."
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
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
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
        protocol::ReplyKind::Notice,
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
    send(&mut app, founder, path);
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            id,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("manifest");
    };
    reply(
        &mut app,
        id.clone(),
        protocol::ReplyKind::Notice,
        "Ready",
        Some("refused-upload"),
    );
    let messages = sent(&mut app);
    let [protocol::ClientMessage::GivePart(part)] = &messages[..]
    else {
        panic!("first part");
    };
    reply(
        &mut app,
        part.id.clone(),
        protocol::ReplyKind::Error,
        "The gift expired.",
        None,
    );
    assert!(sent(&mut app).is_empty());
    assert_eq!(
        app.world().resource::<GiftReports>().0,
        [("The gift expired.".into(), true)]
    );
}

#[test]
fn a_walk_and_a_part_read_off_the_frame_each_wake_the_window() {
    let dir = Scratch::new("world-gift-wake");
    let path = dir.join("list.txt");
    std::fs::write(&path, "Dune").unwrap();
    let (mut app, founder) = app();
    let (woke, wakes) = std::sync::mpsc::channel();
    app.insert_resource(Wake::new(move || {
        let _ = woke.send(());
    }));
    letter(app.world_mut(), founder, vec![path]);
    // The walk wakes the window, and the one update collects it.
    wakes.recv_timeout(Duration::from_secs(5)).unwrap();
    app.update();
    let messages = sent(&mut app);
    let [
        protocol::ClientMessage::Give(protocol::Give::Files {
            id,
            ..
        }),
    ] = &messages[..]
    else {
        panic!("one manifest: {messages:?}");
    };
    crate::receive(
        app.world_mut(),
        protocol::ServerMessage::Reply(protocol::Reply {
            to: id.clone(),
            kind: protocol::ReplyKind::Notice,
            text: "Ready".into(),
            upload: Some("upload-one".into()),
        }),
    );
    // The reply starts the part's read, which wakes the window too.
    app.world_mut().flush();
    wakes.recv_timeout(Duration::from_secs(5)).unwrap();
    app.update();
    let messages = sent(&mut app);
    assert!(
        matches!(
            &messages[..],
            [protocol::ClientMessage::GivePart(_)]
        ),
        "one part: {messages:?}"
    );
    assert!(wakes.try_recv().is_err());
}
