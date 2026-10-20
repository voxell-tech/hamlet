//! Process-crash controls at society's record/file boundaries.

use hamlet_acceptance::{Script, call, words};
use hamlet_server::record::{
    Actor, Admit, Change, FoundRecord, RecordOperation,
};
use hamlet_server::{
    OperationId, Outcome, Reply, RuntimeInstance, Store,
};

use super::*;

type Socket = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

fn seed(home: &Scratch) {
    let mut store = Store::open(&home.join("realm.sqlite")).unwrap();
    let identity = |key: &str| OperationId {
        runtime: RuntimeInstance::new("seed").unwrap(),
        key: key.into(),
    };
    assert!(matches!(
        store
            .run(
                &identity("home"),
                &FoundRecord {
                    hamlet: "home".into(),
                    name: "founder".into(),
                    purpose: "Lead".into(),
                    chief: "chief".into(),
                    at: 0
                }
            )
            .unwrap(),
        Reply::Outcome(Outcome::Done(_))
    ));
    for name in ["kept", "gone", "third"] {
        assert!(matches!(
            store
                .run(
                    &identity(name),
                    &RecordOperation {
                        hamlet: "home".into(),
                        actor: Actor::Chief("chief".into()),
                        at: 1,
                        reason: "Seed".into(),
                        task: None,
                        change: Change::Admit(Admit {
                            name: name.into(),
                            purpose: "Work".into(),
                            model: None,
                            character: String::new(),
                            roads: Vec::new()
                        }),
                    }
                )
                .unwrap(),
            Reply::Outcome(Outcome::Done(_))
        ));
    }
}

async fn send(socket: &mut Socket, mut message: Value) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    message["id"] = id.clone().into();
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
    id
}

async fn until(
    socket: &mut Socket,
    matches: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let message = socket.next().await.unwrap().unwrap();
            let value = serde_json::from_str::<Value>(
                message.to_text().unwrap(),
            )
            .unwrap();
            if matches(&value) {
                break value;
            }
        }
    })
    .await
    .expect("the server did not send the expected result")
}

async fn say(socket: &mut Socket, text: &str) {
    let id = send(socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":text})).await;
    let reply = until(socket, |value| {
        value["type"] == "reply" && value["to"] == id
    })
    .await;
    assert_eq!(reply["kind"], "notice", "{reply}");
}

async fn paused(side: &Scratch) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !side.join("server-paused").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the server did not reach the crash point");
}

#[tokio::test]
async fn merge_recovers_before_files_between_files_and_after_notes() {
    for point in [
        "after_merge_record",
        "during_merge_files",
        "after_merge_notes",
    ] {
        let home = Scratch::new(point);
        let side = Scratch::new("merge-crash-control");
        seed(&home);
        let script = side.join("faux.json");
        Script::new().write(&script);
        let env = [
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_FAULT", point),
            ("HAMLET_FAULT_DIR", side.to_str().unwrap()),
        ];
        let mut server =
            Server::start_with_script(&home, &env, Some(&script));
        let (mut socket, _) = server.connect().await;
        std::fs::write(
            home.join("houses/home/1/NOTES.md"),
            "# Notes\nKept fact.",
        )
        .unwrap();
        std::fs::write(
            home.join("houses/home/2/NOTES.md"),
            "# Notes\nOther fact.",
        )
        .unwrap();
        std::fs::write(home.join("houses/home/1/clash.txt"), "mine")
            .unwrap();
        std::fs::write(
            home.join("houses/home/2/clash.txt"),
            "theirs",
        )
        .unwrap();
        std::fs::write(home.join("houses/home/2/new.txt"), "new")
            .unwrap();
        send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/merge gone into kept"})).await;
        paused(&side).await;
        server.child.kill().unwrap();
        server.child.wait().unwrap();
        let server = Server::start_with_script(
            &home,
            &[("HAMLET_REFLECT_EVERY", "0")],
            Some(&script),
        );
        let (mut socket, _) = server.connect().await;
        let kept = home.join("houses/home/1");
        assert_eq!(
            std::fs::read_to_string(kept.join("clash.txt")).unwrap(),
            "mine"
        );
        assert_eq!(
            std::fs::read_to_string(
                kept.join("merged/gone/clash.txt")
            )
            .unwrap(),
            "theirs"
        );
        assert_eq!(
            std::fs::read_to_string(kept.join("new.txt")).unwrap(),
            "new"
        );
        let notes =
            std::fs::read_to_string(kept.join("NOTES.md")).unwrap();
        assert_eq!(notes.matches("Other fact.").count(), 1);
        assert!(notes.contains("Kept fact."));
        std::fs::write(kept.join("new.txt"), "changed after merge")
            .unwrap();
        say(&mut socket, "/restore gone").await;
        assert_eq!(
            std::fs::read_to_string(kept.join("new.txt")).unwrap(),
            "changed after merge"
        );
        assert_eq!(
            std::fs::read_to_string(kept.join("NOTES.md")).unwrap(),
            "# Notes\nKept fact."
        );
        assert_eq!(
            std::fs::read_to_string(
                home.join("houses/home/2/clash.txt")
            )
            .unwrap(),
            "theirs"
        );
    }
}

#[tokio::test]
async fn library_publication_recovers_without_rejudging_or_partial_files()
 {
    let home = Scratch::new("library-crash");
    let side = Scratch::new("library-crash-control");
    seed(&home);
    let script = side.join("faux.json");
    Script::new()
        .agent(
            "home:0",
            [
                call(
                    "submit_skill",
                    json!({"skill":"artifact","reason":"Useful"}),
                ),
                words("Shelved"),
            ],
        )
        .errands([words("ACCEPT Useful.")])
        .write(&script);
    let env = [
        ("HAMLET_REFLECT_EVERY", "0"),
        ("HAMLET_FAULT", "after_library_publish"),
        ("HAMLET_FAULT_DIR", side.to_str().unwrap()),
    ];
    let mut server =
        Server::start_with_script(&home, &env, Some(&script));
    let source = home.join("houses/home/0/skills/artifact");
    std::fs::create_dir_all(source.join("scripts")).unwrap();
    let paper = "---\nname: artifact\ndescription: Make \
                 artifacts\n---\nRead this.";
    std::fs::write(source.join("SKILL.md"), paper).unwrap();
    std::fs::write(
        source.join("scripts/run.py"),
        "print('artifact')",
    )
    .unwrap();
    let (mut socket, _) = server.connect().await;
    send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Share the skill"})).await;
    paused(&side).await;
    assert_eq!(
        std::fs::read_to_string(
            home.join("library/artifact/SKILL.md")
        )
        .unwrap(),
        paper
    );
    assert_eq!(
        std::fs::read_to_string(
            home.join("library/artifact/scripts/run.py")
        )
        .unwrap(),
        "print('artifact')"
    );
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    Script::new()
        .agent("home:0", [words("Shelved")])
        .errands([hamlet_acceptance::failure(
            "Must not consult twice",
        )])
        .write(&script);
    let _server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    tokio::time::timeout(Duration::from_secs(20), async {
        while source.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("recovered publication did not finish");
    assert_eq!(
        std::fs::read_to_string(
            home.join("library/artifact/SKILL.md")
        )
        .unwrap(),
        paper
    );
    assert!(!home.join(".library-staging/artifact").exists());
}

#[tokio::test]
async fn erased_record_precedes_file_deletion_and_finishes_on_restart()
 {
    let home = Scratch::new("erase-crash");
    let side = Scratch::new("erase-crash-control");
    seed(&home);
    let script = side.join("faux.json");
    Script::new().write(&script);
    let env = [
        ("HAMLET_REFLECT_EVERY", "0"),
        ("HAMLET_FAULT", "after_agent_erasure"),
        ("HAMLET_FAULT_DIR", side.to_str().unwrap()),
    ];
    let mut server =
        Server::start_with_script(&home, &env, Some(&script));
    let (mut socket, _) = server.connect().await;
    say(&mut socket, "/remove gone").await;
    send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/erase gone"})).await;
    paused(&side).await;
    assert!(home.join("houses/home/2").exists());
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    let server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    assert!(!home.join("houses/home/2").exists());
    let id = send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/restore gone"})).await;
    let reply = until(&mut socket, |value| value["to"] == id).await;
    assert_eq!(reply["kind"], "error");
    assert!(
        reply["text"]
            .as_str()
            .unwrap()
            .contains("nothing is archived")
    );
}

async fn state_until(
    server: &Server,
    matches: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let (_, state) = server.connect().await;
            if matches(&state) {
                break state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the expected state did not arrive")
}

#[tokio::test]
async fn withdrawal_recovers_after_ruling_before_cancellation() {
    let home = Scratch::new("withdraw-crash");
    let side = Scratch::new("withdraw-crash-control");
    seed(&home);
    let script = side.join("faux.json");
    Script::new()
        .agent(
            "home:1",
            [
                call("object", json!({"reason":"Keep the copy"})),
                call(
                    "write",
                    json!({"path":"lost.txt","content":"wrong"}),
                ),
                words("Done"),
            ],
        )
        .write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_FAULT", "after_instruction_withdrawal"),
            ("HAMLET_FAULT_DIR", side.to_str().unwrap()),
        ],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"kept"},"text":"Remove it"})).await;
    let state = state_until(&server, |s| {
        s["waits"].as_array().is_some_and(|waits| {
            waits.iter().any(|w| w["detail"]["kind"] == "objection")
        })
    })
    .await;
    let wait = state["waits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["detail"]["kind"] == "objection")
        .unwrap();
    send(&mut socket, json!({"type":"answer","wait":wait["id"],"choice":"withdraw","text":"Keep it"})).await;
    paused(&side).await;
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    let server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    send(&mut socket, json!({"type":"open","chat":{"with":{"hamlet":"home","agent":"kept"}}})).await;
    until(&mut socket, |value| {
        matches!(value["type"].as_str(), Some("transcript" | "turn"))
            && value.to_string().contains(
                "The chief withdrew the instruction: Keep it",
            )
    })
    .await;
    assert!(!home.join("houses/home/1/lost.txt").exists());
    let (_, state) = server.connect().await;
    assert_eq!(state["waits"], json!([]));
}

#[tokio::test]
async fn overturn_commits_the_office_and_vote_together() {
    let home = Scratch::new("overturn-crash");
    let side = Scratch::new("overturn-crash-control");
    seed(&home);
    let script = side.join("faux.json");
    Script::new().errands([
        words("A new direction"),
        call("cast_ballot", json!({"choice":"yes","reason":"A new direction","evidence":""})),
        call("cast_ballot", json!({"choice":"yes","reason":"A new direction","evidence":""})),
        call("cast_ballot", json!({"choice":"yes","reason":"A new direction","evidence":""})),
    ]).write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_FAULT", "after_vote_overturn"),
            ("HAMLET_FAULT_DIR", side.to_str().unwrap()),
        ],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    say(&mut socket, "/vote kept A new direction").await;
    state_until(&server, |s| {
        s["agents"].as_array().is_some_and(|agents| {
            agents
                .iter()
                .any(|a| a["name"] == "kept" && a["mayor"] == true)
        })
    })
    .await;
    send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/overturn 1 Back"})).await;
    paused(&side).await;
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    let server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (mut socket, state) = server.connect().await;
    assert!(
        state["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "founder" && a["mayor"] == true),
        "{state}"
    );
    let id = send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/overturn 1 Back"})).await;
    let reply = until(&mut socket, |v| v["to"] == id).await;
    assert!(
        reply["text"]
            .as_str()
            .unwrap()
            .contains("already overturned"),
        "{reply}"
    );
}

#[tokio::test]
async fn an_unowned_snapshot_is_removed_after_a_crash_without_following_links()
 {
    let home = Scratch::new("snapshot-crash");
    let side = Scratch::new("snapshot-crash-control");
    seed(&home);
    let script = side.join("faux.json");
    Script::new().write(&script);
    let mut server = Server::start_with_script(
        &home,
        &[
            ("HAMLET_REFLECT_EVERY", "0"),
            ("HAMLET_FAULT", "after_society_snapshot"),
            ("HAMLET_FAULT_DIR", side.to_str().unwrap()),
        ],
        Some(&script),
    );
    let (mut socket, _) = server.connect().await;
    std::fs::write(home.join("houses/home/2/copy.txt"), "original")
        .unwrap();
    send(&mut socket, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"/merge gone into kept"})).await;
    paused(&side).await;
    let snapshots = home.join(".society-files");
    assert_eq!(std::fs::read_dir(&snapshots).unwrap().count(), 1);
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    std::fs::write(side.join("untouched"), "outside").unwrap();
    std::os::unix::fs::symlink(
        &*side,
        snapshots.join("outside-link"),
    )
    .unwrap();
    let server = Server::start_with_script(
        &home,
        &[("HAMLET_REFLECT_EVERY", "0")],
        Some(&script),
    );
    let (_, state) = server.connect().await;
    assert_eq!(std::fs::read_dir(snapshots).unwrap().count(), 0);
    assert_eq!(
        std::fs::read_to_string(side.join("untouched")).unwrap(),
        "outside"
    );
    assert_eq!(
        std::fs::read_to_string(home.join("houses/home/2/copy.txt"))
            .unwrap(),
        "original"
    );
    assert!(!home.join("houses/home/1/copy.txt").exists());
    assert!(
        state["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "gone" && a["left"].is_null())
    );
}
