//! Gifts: files handed over in parts to an inbox, and skills given
//! to a scholar's `main`.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{call, words};
use serde_json::json;

fn kind_text(reply: &Reply) -> (&ReplyKind, &str) {
    (&reply.kind, reply.text.as_str())
}

#[tokio::test]
async fn hands_over_files_in_parts_and_tells_the_agent() {
    let mut s = Session::start(
        Faux::new().responses([words("A list."), words("Two.")]),
    )
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    let bytes = [0u8, 255, 1, 128, 10];
    let begun = s
        .chief
        .request(|id| {
            ClientMessage::Give(Give::Files {
                files: vec![GiveFile {
                    path: "list.bin".into(),
                    size: 5,
                }],
                id,
                to: hand,
            })
        })
        .await;
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
    assert!(!s.home.file("hand/inbox/list.bin").exists());
    let reply =
        s.chief.part(&upload, "list.bin", 2, &bytes[2..]).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Notice, "Handed over `inbox/list.bin`.")
    );
    assert_eq!(
        std::fs::read(s.home.file("hand/inbox/list.bin")).unwrap(),
        bytes
    );
    // The chief's line to the agent names the file.
    s.chief.shows(&now, "A list.").await;
    let said = s.chief.texts(&now);
    assert!(
        said.iter().any(|t| t.contains("`inbox/list.bin`")),
        "{said:?}"
    );

    // A folder keeps its shape and the earlier gift stays.
    let reply = s
        .chief
        .give(
            hand,
            &[("notes/a.txt", b"alpha"), ("notes/b/c.txt", b"gamma")],
            3,
        )
        .await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Notice,
            "Handed over `inbox/notes/a.txt`, `inbox/notes/b/c.txt`."
        )
    );
    assert_eq!(
        std::fs::read(s.home.file("hand/inbox/notes/b/c.txt"))
            .unwrap(),
        b"gamma"
    );
    assert!(s.home.file("hand/inbox/list.bin").exists());
    s.chief.shows(&now, "Two.").await;
}

#[tokio::test]
async fn refuses_files_that_would_leave_the_inbox_or_the_manifest() {
    let mut s = Session::start(Faux::new()).await;
    let hand = s.hand();
    for path in ["../out", "/abs", "a//b", "a\\b", ""] {
        let reply = s.chief.give(hand, &[(path, b"x")], 8).await;
        assert_eq!(reply.kind, ReplyKind::Error, "{path}: {reply:?}");
    }
    let reply = s.chief.give(hand, &[], 8).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Error, "Give at least one file.")
    );
    // More bytes than the manifest declared are refused.
    let begun = s
        .chief
        .request(|id| {
            ClientMessage::Give(Give::Files {
                files: vec![GiveFile {
                    path: "a".into(),
                    size: 1,
                }],
                id,
                to: hand,
            })
        })
        .await;
    let upload = begun.upload.unwrap();
    let reply = s.chief.part(&upload, "a", 0, b"too long").await;
    assert_eq!(reply.kind, ReplyKind::Error, "{reply:?}");
    assert!(!s.home.file("hand/inbox/a").exists());
}

#[tokio::test]
async fn gives_a_scholar_skills_from_an_agent_or_a_scholar() {
    let mut s = Session::start(Faux::new().responses([
        call(
            "bash",
            json!({"command": "mkdir -p skills/greet && printf -- \
                '---\\nname: greet\\ndescription: Says hello\\n---\\n\
                 Run it.\\n' > skills/greet/SKILL.md"}),
        ),
        words("Written."),
    ]))
    .await;
    let hand = s.hand();
    let now = chat(hand);
    s.chief.open(&now).await;
    s.chief.tell(hand, "Write a skill.").await;
    s.chief.shows(&now, "Written.").await;
    s.found("docs", "ada", "keep the docs").await;
    s.found("mill", "bo", "grind").await;
    let ada = s.chief.scholar("ada").unwrap().id;
    let bo = s.chief.scholar("bo").unwrap().id;
    let skills = |papers: Papers| {
        papers
            .skills
            .into_iter()
            .map(|s| s.name)
            .collect::<Vec<_>>()
    };

    let reply = s
        .chief
        .give_skill(
            ada,
            GiveSkillSource {
                from: Keeper::Agent(hand),
                name: "greet".into(),
            },
        )
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    assert!(
        reply.text.starts_with("Gave greet to scholar#2 (ada)"),
        "{}",
        reply.text
    );
    assert_eq!(
        skills(s.chief.papers(Keeper::Scholar(ada)).await),
        ["greet"]
    );

    let reply = s
        .chief
        .give_skill(
            bo,
            GiveSkillSource {
                from: Keeper::Scholar(ada),
                name: "greet".into(),
            },
        )
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    assert_eq!(
        skills(s.chief.papers(Keeper::Scholar(bo)).await),
        ["greet"]
    );

    let reply = s
        .chief
        .give_skill(
            bo,
            GiveSkillSource {
                from: Keeper::Scholar(ada),
                name: "nothing".into(),
            },
        )
        .await;
    assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
}
