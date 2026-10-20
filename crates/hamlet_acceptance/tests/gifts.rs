//! Gifts: files handed over in parts, and skills from the starters or
//! from another agent.

use std::os::unix::fs::PermissionsExt;

use hamlet_acceptance::*;
use hamlet_protocol::v0::*;
use serde_json::json;

fn kind_text(reply: &Reply) -> (&ReplyKind, &str) {
    (&reply.kind, reply.text.as_str())
}

fn inbox(s: &Session) -> Vec<String> {
    let mut names =
        std::fs::read_dir(s.home.file("houses/home/0/inbox"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect::<Vec<_>>();
    names.sort();
    names
}

async fn manifest(c: &mut Client, files: &[(&str, u64)]) -> Reply {
    let files = files
        .iter()
        .map(|(path, size)| GiveFilesFiles {
            path: path.to_string(),
            size: *size,
        })
        .collect();
    c.request(|id| {
        ClientMessage::Give(Give::Files {
            files,
            id,
            to: mayor(),
        })
    })
    .await
}

async fn starter(c: &mut Client, to: Address, name: &str) -> Reply {
    c.request(|id| {
        ClientMessage::Give(Give::Skill {
            id,
            skill: GiveSkillSkill::Starter {
                starter: name.into(),
            },
            to,
        })
    })
    .await
}

fn scout() -> Script {
    Script::new().agent(
        "home:0",
        [
            call(
                "create_agent",
                json!({"name": "scout", "purpose": "Maps", "reason": "Explore"}),
            ),
            words("Scout made"),
        ],
    )
}

async fn make_scout(s: &mut Session) {
    s.chief.say(mayor(), "Make a scout").await;
    s.chief
        .until("scout created", |c| {
            c.agent("home", "scout").is_some()
        })
        .await;
    s.chief.idle("home", "founder").await;
}

#[tokio::test]
async fn hands_over_a_file_in_parts_with_its_bytes() {
    let mut s =
        Session::start(Script::new().responses([words("A list.")]))
            .await;
    let bytes = [0u8, 255, 1, 128, 10];
    let begun = manifest(&mut s.chief, &[("list.bin", 5)]).await;
    assert_eq!(
        kind_text(&begun),
        (&ReplyKind::Notice, "Ready for the gift's file parts.")
    );
    let upload = begun.upload.unwrap();
    let reply =
        s.chief.part(&upload, "list.bin", 0, &bytes[..2]).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Received the gift part.")
    );
    assert_eq!(inbox(&s), Vec::<String>::new());
    let reply =
        s.chief.part(&upload, "list.bin", 2, &bytes[2..]).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Handed over `inbox/list.bin`.")
    );
    assert_eq!(
        std::fs::read(s.home.file("houses/home/0/inbox/list.bin"))
            .unwrap(),
        bytes
    );
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.shows(&mayor_chat, "A list.").await;
}

#[tokio::test]
async fn keeps_a_folders_shape_and_earlier_gifts_and_tells_the_agent()
{
    let mut s = Session::start(
        Script::new().responses([words("One."), words("Two.")]),
    )
    .await;
    let files: &[(&str, &[u8])] =
        &[("folder/deep/a.md", b"A"), ("folder/b.md", b"B")];
    let reply = s.chief.give(mayor(), files).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Handed over `inbox/folder`.")
    );
    s.chief.idle("home", "founder").await;
    let reply = s.chief.give(mayor(), files).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Handed over `inbox/folder-2`.")
    );
    for folder in ["folder", "folder-2"] {
        let read =
            |rel: &str| {
                std::fs::read_to_string(s.home.file(&format!(
                    "houses/home/0/inbox/{folder}/{rel}"
                )))
                .unwrap()
            };
        assert_eq!(
            (read("deep/a.md"), read("b.md")),
            ("A".into(), "B".into())
        );
    }
    let mayor_chat = chat(mayor());
    s.chief.open(&mayor_chat).await;
    s.chief.shows(&mayor_chat, "Two.").await;
    let users = s
        .chief
        .items(&mayor_chat)
        .into_iter()
        .filter(|i| i.kind == ItemKind::User)
        .map(|i| i.text)
        .collect::<Vec<_>>();
    assert_eq!(
        users,
        ["folder", "folder-2"].map(|f| format!(
            "I handed you `inbox/{f}` in your workspace. Read it \
             and do what it asks. If it asks for nothing, tell me \
             in a few lines what it is."
        ))
    );
}

#[tokio::test]
async fn hands_over_empty_files_once_every_file_is_in() {
    let mut s =
        Session::start(Script::new().responses([words("Ok.")])).await;
    let reply = s
        .chief
        .give(mayor(), &[("folder/empty", b""), ("folder/a", b"A")])
        .await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Handed over `inbox/folder`.")
    );
    let empty = s.home.file("houses/home/0/inbox/folder/empty");
    assert_eq!(std::fs::read(empty).unwrap(), b"");
}

#[tokio::test]
async fn refuses_bad_paths_and_manifests_before_writing_anything() {
    let mut s = Session::start(Script::new()).await;
    for path in [
        "/absolute.txt",
        "../outside.txt",
        "folder/../outside.txt",
        "C:\\outside.txt",
        "folder\\..\\outside.txt",
        "",
        ".",
        "folder//a",
        "nul\0.txt",
    ] {
        let reply = s
            .chief
            .give(mayor(), &[("innocent.txt", b"A"), (path, b"B")])
            .await;
        assert_eq!(
            reply.kind,
            ReplyKind::Error,
            "{path:?}: {reply:?}"
        );
        assert!(
            reply.text.contains("path")
                || reply.text.contains("inbox"),
            "{path:?}: {reply:?}"
        );
        assert!(reply.upload.is_none());
    }
    for files in [
        &[][..],
        &[("a", 1), ("a", 1)][..],
        &[("a/b", 1), ("a", 1)][..],
    ] {
        let reply = manifest(&mut s.chief, files).await;
        assert_eq!(
            reply.kind,
            ReplyKind::Error,
            "{files:?}: {reply:?}"
        );
    }
    for (a, b) in [
        ("folder/a.txt", "folder/A.txt"),
        ("folder/caf\u{e9}.txt", "folder/cafe\u{301}.txt"),
        ("folder/stra\u{df}e.txt", "folder/strasse.txt"),
        ("FOLDER", "folder/a.txt"),
    ] {
        let reply = manifest(&mut s.chief, &[(a, 1), (b, 1)]).await;
        assert_eq!(reply.kind, ReplyKind::Error, "{reply:?}");
        assert!(
            reply.text.contains("repeats")
                || reply.text.contains("both"),
            "{a} {b}: {reply:?}"
        );
        assert!(
            !reply.text.contains(&*s.home.path().to_string_lossy())
        );
    }
    let reply =
        manifest(&mut s.chief, &[("large.bin", 40 * 1024 * 1024)])
            .await;
    assert_eq!(reply.kind, ReplyKind::Error);
    assert!(reply.text.contains("32 MB"), "{reply:?}");
    assert_eq!(inbox(&s), Vec::<String>::new());
    // None of it reached the mayor.
    let page = s.chief.open(&chat(mayor())).await;
    assert_eq!(page.items, []);
}

#[tokio::test]
async fn drops_a_part_out_of_place_and_with_it_the_upload() {
    let mut s = Session::start(Script::new()).await;
    let begun = manifest(&mut s.chief, &[("folder/a", 2)]).await;
    let upload = begun.upload.unwrap();
    let reply = s.chief.part(&upload, "folder/a", 0, b"A").await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    assert_eq!(inbox(&s), Vec::<String>::new());
    let refused = s.chief.part(&upload, "folder/a", 0, b"A").await;
    assert_eq!(refused.kind, ReplyKind::Error);
    assert!(refused.text.contains("offset"), "{refused:?}");
    assert!(
        !refused.text.contains(&*s.home.path().to_string_lossy())
    );
    let late = s.chief.part(&upload, "folder/a", 1, b"A").await;
    assert_eq!(late.kind, ReplyKind::Error);
    assert!(late.text.contains("upload id"), "{late:?}");
    assert_eq!(inbox(&s), Vec::<String>::new());
}

#[tokio::test]
async fn keeps_uploads_to_their_connection_and_bounds_them() {
    let mut s = Session::start(Script::new()).await;
    let mut other = s.join().await;
    let begun = manifest(&mut other, &[("a", 1)]).await;
    let upload = begun.upload.unwrap();
    let reply = s.chief.part(&upload, "a", 0, b"A").await;
    assert_eq!(reply.kind, ReplyKind::Error);
    for _ in 0..2 {
        let reply = manifest(&mut other, &[("a", 1)]).await;
        assert!(reply.upload.is_some(), "{reply:?}");
    }
    let reply = manifest(&mut other, &[("a", 1)]).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "This connection already has three file gifts waiting. \
             Finish one before starting another."
        )
    );
    other.close().await;
    // The closed connection's places are freed, but not its bytes.
    let half = 16 * 1024 * 1024;
    let mut third = s.join().await;
    for client in [&mut s.chief, &mut third] {
        let reply = manifest(client, &[("a", half)]).await;
        assert!(reply.upload.is_some(), "{reply:?}");
    }
    let reply = manifest(&mut s.chief, &[("a", 1)]).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "This agent already has 32 MB of file gifts waiting. \
             Finish or discard one before starting another."
        )
    );
    third.close().await;
    let began = std::time::Instant::now();
    loop {
        let reply = manifest(&mut s.chief, &[("a", half)]).await;
        if reply.upload.is_some() {
            break;
        }
        assert!(began.elapsed() < client::PATIENCE, "{reply:?}");
        s.chief.settle(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(inbox(&s), Vec::<String>::new());
}

#[tokio::test]
async fn takes_exactly_32_mib_of_file_bytes() {
    let mut s =
        Session::start(Script::new().responses([words("Big.")]))
            .await;
    let bytes = vec![165u8; 32 * 1024 * 1024];
    let reply = s.chief.give(mayor(), &[("large.bin", &bytes)]).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Handed over `inbox/large.bin`.")
    );
    let got =
        std::fs::read(s.home.file("houses/home/0/inbox/large.bin"))
            .unwrap();
    assert!(got == bytes, "{} bytes arrived", got.len());
}

#[tokio::test]
async fn installs_a_starter_skill_and_shows_it_to_every_connection() {
    let mut s = Session::start(Script::new()).await;
    let mut other = s.join().await;
    let realm = s.chief.realm();
    let starters = realm.starters;
    let search = starters.iter().find(|s| s.name == "web-search");
    assert!(
        search.is_some_and(|s| !s.description.trim().is_empty()),
        "{starters:?}"
    );
    assert!(realm.agents[0].skills.is_empty());
    let reply = starter(&mut s.chief, mayor(), "web-search").await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Gave `mayor` the skill `web-search`.")
    );
    let skill =
        s.home.file("houses/home/0/skills/web-search/SKILL.md");
    assert!(skill.is_file());
    let shows = |c: &Client| {
        c.agent("home", "founder").unwrap().skills == ["web-search"]
    };
    s.chief.until("the skill in the state", shows).await;
    other
        .until("the skill on the other connection", shows)
        .await;
    assert!(shows(&s.join().await));
    for (reply, text) in [
        (
            starter(&mut s.chief, mayor(), "web-search").await,
            "`founder` already has a skill named `web-search`",
        ),
        (
            starter(&mut s.chief, mayor(), "nothing").await,
            "no starter skill named `nothing`. Say whose it is: \
             /give founder nothing from <agent>",
        ),
        (
            s.chief.say(mayor(), "/give mayor web-search").await,
            "`founder` already has a skill named `web-search`",
        ),
        (
            s.chief.say(mayor(), "/give mayor ../maps").await,
            "a skill is named in letters, digits and hyphens",
        ),
        (
            s.chief.say(mayor(), "/give nobody web-search").await,
            "no agent named `nobody`",
        ),
    ] {
        assert_eq!(kind_text(&reply), (&ReplyKind::Error, text));
    }
    // A gift of a skill is no turn.
    let page = s.chief.open(&chat(mayor())).await;
    assert_eq!(page.items, []);
}

#[tokio::test]
async fn copies_another_agents_whole_skill() {
    let mut s = Session::start(scout()).await;
    make_scout(&mut s).await;
    let maps = s.home.file("houses/home/1/skills/maps");
    std::fs::create_dir_all(maps.join("scripts")).unwrap();
    std::fs::write(
        maps.join("SKILL.md"),
        "---\nname: maps\ndescription: Read maps.\n---\nA map.",
    )
    .unwrap();
    std::fs::write(maps.join("scripts/run.sh"), "echo maps").unwrap();
    std::fs::set_permissions(
        maps.join("scripts/run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let from = |name: &str| GiveSkillSkill::From {
        from: to("home", "scout"),
        name: name.into(),
    };
    let skill = from("maps");
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Give(Give::Skill {
                id,
                skill,
                to: mayor(),
            })
        })
        .await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Gave `mayor` the skill `maps`.")
    );
    let given = s.home.file("houses/home/0/skills/maps");
    for file in ["SKILL.md", "scripts/run.sh"] {
        assert_eq!(
            std::fs::read(given.join(file)).unwrap(),
            std::fs::read(maps.join(file)).unwrap()
        );
    }
    let mode = std::fs::metadata(given.join("scripts/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o7777, 0o755);
    // The giver keeps its own.
    assert!(maps.join("SKILL.md").is_file());
    s.chief
        .until("the skill in the state", |c| {
            c.agent("home", "founder").unwrap().skills == ["maps"]
        })
        .await;
    let skill = from("missing");
    let reply = s
        .chief
        .request(|id| {
            ClientMessage::Give(Give::Skill {
                id,
                skill,
                to: mayor(),
            })
        })
        .await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Error, "`scout` has no skill named `missing`")
    );
    let reply =
        s.chief.say(mayor(), "/give scout maps from mayor").await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "`scout` already has a skill named `maps`"
        )
    );
    std::fs::remove_dir_all(&maps).unwrap();
    let reply =
        s.chief.say(mayor(), "/give scout maps from mayor").await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Gave `scout` the skill `maps`.")
    );
    assert!(maps.join("scripts/run.sh").is_file());
}

#[tokio::test]
async fn answers_a_gift_to_a_departed_agent_with_its_farewell() {
    let mut s = Session::start(scout()).await;
    make_scout(&mut s).await;
    s.chief.say(mayor(), "/remove scout Work ended").await;
    let farewell = "`scout` was removed from the hamlet by the \
                    chief chief (\"Work ended\"). Its house is \
                    archived and its roads went with it; only a \
                    chief can restore it.";
    let typed = s.chief.say(mayor(), "/give scout weather").await;
    let skill =
        starter(&mut s.chief, to("home", "scout"), "weather").await;
    let files = s
        .chief
        .request(|id| {
            ClientMessage::Give(Give::Files {
                files: vec![GiveFilesFiles {
                    path: "note.txt".into(),
                    size: 0,
                }],
                id,
                to: to("home", "scout"),
            })
        })
        .await;
    for reply in [typed, skill, files] {
        assert_eq!(kind_text(&reply), (&ReplyKind::Error, farewell));
    }
}
