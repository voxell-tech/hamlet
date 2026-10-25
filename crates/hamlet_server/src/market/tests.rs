use hamlet_protocol::client::{
    Address, Bought, PackageKind, ReplyKind, RequestId,
};
use hamlet_testing::Scratch;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::package::{
    Agent, CHARACTER, Contents, File, Hamlet, MANIFEST, NOTES,
    Package, Tool, Tools,
};
use super::*;
use crate::{Home, Overrides};

fn file(path: &str, text: &str) -> File {
    File {
        path: path.into(),
        data: text.as_bytes().to_vec(),
        executable: false,
    }
}

fn scribe(character: &str, skill: &str) -> Agent {
    Agent {
        name: "scribe".into(),
        purpose: "Writes letters.".into(),
        model: None,
        files: vec![
            file(CHARACTER, character),
            file(NOTES, "# Notes\nthe seller's notes\n"),
            file("workspace/skills/ink/SKILL.md", skill),
            File {
                executable: true,
                ..file(
                    "workspace/skills/ink/scripts/run.sh",
                    "echo\n",
                )
            },
        ],
    }
}

fn bytes(contents: Contents) -> Vec<u8> {
    Package::new(contents).bytes().unwrap()
}

fn settle_of(
    listing: &str,
    version: &str,
    into: Option<&str>,
    bytes: &[u8],
) -> Settle {
    Settle {
        id: RequestId::new(),
        into: into.map(Into::into),
        listing: listing.into(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        size: bytes.len() as u64,
        title: listing.to_uppercase(),
        version: version.into(),
    }
}

fn open(scratch: &Scratch) -> (Service, Harnesses) {
    let mut service = Service::new(
        Home::open(scratch).unwrap(),
        Overrides::none(true),
    )
    .unwrap();
    let layout = service.home.layout().clone();
    let (harnesses, _) = Harnesses::start(
        &layout,
        service.home.store(),
        crate::testing::config(Overrides::none(true)),
    )
    .unwrap();
    (service, harnesses)
}

/// Sends `bytes` under `settle` as a window does: the settle, then
/// its parts of `size` bytes. The last answer, as words or a refusal.
async fn deliver(
    service: &mut Service,
    harnesses: &mut Harnesses,
    settle: &Settle,
    bytes: &[u8],
    size: usize,
) -> Result<String, String> {
    let answer = |message: ServerMessage| match message {
        ServerMessage::Reply(reply) => reply,
        other => panic!("{other:?}"),
    };
    let begun = answer(begin(service, 1, settle).unwrap());
    let Some(upload) = begun.upload else {
        return match begun.kind {
            ReplyKind::Error => Err(begun.text),
            _ => Ok(begun.text),
        };
    };
    let mut last = None;
    for (index, chunk) in bytes.chunks(size).enumerate() {
        let sent = SettlePart {
            data: STANDARD.encode(chunk),
            id: RequestId::new(),
            offset: (index * size) as u64,
            upload: upload.clone(),
        };
        last = Some(answer(
            part(service, harnesses, 1, &sent).await.unwrap(),
        ));
    }
    let last = last.unwrap();
    match last.kind {
        ReplyKind::Error => Err(last.text),
        _ => Ok(last.text),
    }
}

fn read_bought(service: &Service) -> Vec<Bought> {
    match bought(service, &RequestId::new()).unwrap() {
        ServerMessage::Read(ReadResult::Bought {
            bought, ..
        }) => bought,
        other => panic!("{other:?}"),
    }
}

/// The package `packing` comes to, gathered from its parts.
async fn unpack(service: &mut Service, packing: &Packing) -> Package {
    let parts =
        package(service, &RequestId::new(), packing).await.unwrap();
    let mut bytes = Vec::new();
    for message in parts {
        let ServerMessage::Read(ReadResult::Package {
            package, ..
        }) = message
        else {
            panic!("{message:?}");
        };
        assert_eq!(package.offset, bytes.len() as u64);
        bytes.extend(STANDARD.decode(package.data).unwrap());
    }
    Package::read(&bytes).unwrap()
}

#[tokio::test]
async fn an_agent_bought_moves_in_and_is_brought_up_keeping_its_notes()
 {
    let scratch = Scratch::new("market-agent");
    let (mut service, mut harnesses) = open(&scratch);
    let first = bytes(Contents::Agent {
        agent: scribe("Careful.", "---\nname: ink\n---\nv1\n"),
    });
    let settle = settle_of("l_scribe", "1.0.0", Some("home"), &first);

    // An agent needs a hamlet to move into.
    let homeless = settle_of("l_scribe", "1.0.0", None, &first);
    let refused =
        deliver(&mut service, &mut harnesses, &homeless, &first, 100)
            .await
            .unwrap_err();
    assert!(refused.contains("choose the hamlet"), "{refused}");

    let words =
        deliver(&mut service, &mut harnesses, &settle, &first, 100)
            .await
            .unwrap();
    assert_eq!(words, "scribe moved into home.");
    let record =
        service.home.store().record("home").unwrap().unwrap();
    let agent =
        record.agents.iter().find(|a| a.name == "scribe").unwrap();
    assert_eq!(agent.character, "Careful.");
    let number = agent.number;
    let house = service.houses.house("home", number).unwrap();
    let script = house.read_file("skills/ink/scripts/run.sh", 64);
    assert_eq!(script.await.unwrap(), b"echo\n");
    let notes = service.houses.read_notes("home", number).await;
    assert_eq!(notes.unwrap(), "# Notes\nthe seller's notes\n");

    // The same settle again is the same answer, without its parts;
    // another under its id is refused.
    let again =
        deliver(&mut service, &mut harnesses, &settle, &first, 100)
            .await
            .unwrap();
    assert_eq!(again, words);
    let reused = Settle {
        version: "9.0.0".into(),
        ..settle.clone()
    };
    let refused =
        deliver(&mut service, &mut harnesses, &reused, &first, 100)
            .await
            .unwrap_err();
    assert!(refused.contains("already used"), "{refused}");

    // A new version brings up its character and skills, where the
    // agent lives; its notes are its own now.
    service
        .houses
        .write_notes("home", number, "# Notes\nmine now\n")
        .await
        .unwrap();
    let second = bytes(Contents::Agent {
        agent: scribe("Bolder.", "---\nname: ink\n---\nv2\n"),
    });
    let update =
        settle_of("l_scribe", "1.1.0", Some("mill"), &second);
    let words =
        deliver(&mut service, &mut harnesses, &update, &second, 4096)
            .await
            .unwrap();
    assert_eq!(words, "scribe is up to version 1.1.0.");
    let record =
        service.home.store().record("home").unwrap().unwrap();
    let scribes = record
        .agents
        .iter()
        .filter(|a| a.name.starts_with("scribe"));
    assert_eq!(scribes.count(), 1);
    let agent = record.agents.iter().find(|a| a.number == number);
    assert_eq!(agent.unwrap().character, "Bolder.");
    let skill = house.read_file("skills/ink/SKILL.md", 64).await;
    assert!(
        String::from_utf8(skill.unwrap()).unwrap().contains("v2")
    );
    let notes = service.houses.read_notes("home", number).await;
    assert_eq!(notes.unwrap(), "# Notes\nmine now\n");
    let kept = read_bought(&service);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].kind, PackageKind::Agent);
    assert_eq!(kept[0].version, "1.1.0");
    assert_eq!(kept[0].names, ["scribe"]);
    assert_eq!(kept[0].hamlet.as_deref(), Some("home"));

    // Bought again under another listing, it takes another name.
    let other = settle_of("l_other", "1.0.0", Some("home"), &first);
    let words =
        deliver(&mut service, &mut harnesses, &other, &first, 4096)
            .await
            .unwrap();
    assert_eq!(words, "scribe-ii moved into home.");

    // Packed back, it is what was bought, with its own notes.
    let packing = Packing::Agent {
        agent: Address {
            agent: "scribe".into(),
            hamlet: "home".into(),
        },
        notes: true,
    };
    let Contents::Agent { agent } =
        unpack(&mut service, &packing).await.contents
    else {
        panic!("not an agent");
    };
    assert_eq!(agent.text(CHARACTER).unwrap(), "Bolder.");
    assert_eq!(agent.text(NOTES).unwrap(), "# Notes\nmine now\n");
    let run = agent
        .files
        .iter()
        .find(|f| f.path == "workspace/skills/ink/scripts/run.sh")
        .unwrap();
    assert!(run.executable);
}

#[tokio::test]
async fn a_hamlet_bought_is_founded_with_all_its_agents_know() {
    let scratch = Scratch::new("market-hamlet");
    let (mut service, mut harnesses) = open(&scratch);
    let agent = |name: &str| Agent {
        name: name.into(),
        ..scribe("Works.", "---\nname: ink\n---\n")
    };
    let mill = Hamlet {
        name: "mill".into(),
        title: "The Mill".into(),
        kind: "Workshop".into(),
        purpose: "Grind.".into(),
        seed: "7".into(),
        mayor: agent("mill-lead"),
        agents: vec![agent("miller")],
    };
    let sold = bytes(Contents::Hamlet {
        hamlet: mill.clone(),
    });
    let settle = settle_of("l_mill", "1.0.0", None, &sold);
    let words =
        deliver(&mut service, &mut harnesses, &settle, &sold, 512)
            .await
            .unwrap();
    assert!(
        words.starts_with("The Mill stands in your realm"),
        "{words}"
    );
    let record =
        service.home.store().record("mill").unwrap().unwrap();
    assert_eq!(record.agents.len(), 2);
    for agent in &record.agents {
        let notes = service.houses.read_notes("mill", agent.number);
        assert!(notes.await.unwrap().contains("the seller's notes"));
    }

    // Another listing of a hamlet by that name is refused.
    let other = settle_of("l_other", "1.0.0", None, &sold);
    let refused =
        deliver(&mut service, &mut harnesses, &other, &sold, 4096)
            .await
            .unwrap_err();
    assert!(
        refused.contains("has a hamlet named `mill`"),
        "{refused}"
    );

    // A new version brings a new agent in.
    let newer = bytes(Contents::Hamlet {
        hamlet: Hamlet {
            agents: vec![agent("miller"), agent("carter")],
            ..mill
        },
    });
    let update = settle_of("l_mill", "2.0.0", None, &newer);
    deliver(&mut service, &mut harnesses, &update, &newer, 4096)
        .await
        .unwrap();
    let record =
        service.home.store().record("mill").unwrap().unwrap();
    assert_eq!(record.agents.len(), 3);

    let packing = Packing::Hamlet {
        hamlet: "mill".into(),
        kind: "Workshop".into(),
        notes: false,
    };
    let Contents::Hamlet { hamlet } =
        unpack(&mut service, &packing).await.contents
    else {
        panic!("not a hamlet");
    };
    assert_eq!(hamlet.title, "The Mill");
    assert_eq!(hamlet.mayor.name, "mill-lead");
    assert_eq!(hamlet.agents.len(), 2);
    assert!(hamlet.mayor.text(NOTES).is_none());
}

#[tokio::test]
async fn tools_bought_join_the_tools_added_by_hand() {
    let scratch = Scratch::new("market-tools");
    let (mut service, mut harnesses) = open(&scratch);
    let manifest = json!({
        "name": "cal", "description": "Dates.", "exec": ["sh", "run.sh"]
    });
    let sold = bytes(Contents::Tools {
        tools: Tools {
            tools: vec![Tool {
                name: "cal".into(),
                files: vec![
                    file(MANIFEST, &manifest.to_string()),
                    File {
                        executable: true,
                        ..file("run.sh", "date\n")
                    },
                ],
            }],
        },
    });
    let settle = settle_of("l_cal", "1.0.0", None, &sold);
    let words =
        deliver(&mut service, &mut harnesses, &settle, &sold, 4096)
            .await
            .unwrap();
    assert_eq!(words, "Every agent can use cal now.");
    let ServerMessage::Read(ReadResult::Tools {
        tools: listed, ..
    }) = tools(&service, &RequestId::new())
    else {
        panic!("not tools");
    };
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "cal");
    assert_eq!(listed[0].description, "Dates.");

    // A tool of the name from elsewhere is refused; the same listing
    // replaces its own.
    let other = settle_of("l_other", "1.0.0", None, &sold);
    let refused =
        deliver(&mut service, &mut harnesses, &other, &sold, 4096)
            .await
            .unwrap_err();
    assert!(refused.contains("has a tool named `cal`"), "{refused}");
    let update = settle_of("l_cal", "1.1.0", None, &sold);
    let words =
        deliver(&mut service, &mut harnesses, &update, &sold, 4096)
            .await
            .unwrap();
    assert_eq!(words, "cal is up to version 1.1.0.");

    let packing = Packing::Tools {
        tools: vec!["cal".into()],
    };
    let Contents::Tools { tools: packed } =
        unpack(&mut service, &packing).await.contents
    else {
        panic!("not tools");
    };
    let run = packed.tools[0]
        .files
        .iter()
        .find(|f| f.path == "run.sh")
        .unwrap();
    assert!(run.executable);
    let missing = Packing::Tools {
        tools: vec!["nope".into()],
    };
    let refused = package(&mut service, &RequestId::new(), &missing)
        .await
        .unwrap();
    assert!(matches!(
        &refused[..],
        [ServerMessage::Reply(reply)] if reply.kind == ReplyKind::Error
    ));
}

#[tokio::test]
async fn parts_that_do_not_make_the_package_announced_are_refused() {
    let scratch = Scratch::new("market-parts");
    let (mut service, mut harnesses) = open(&scratch);
    let sold = bytes(Contents::Agent {
        agent: scribe("Careful.", "---\nname: ink\n---\n"),
    });
    let mut wrong =
        settle_of("l_scribe", "1.0.0", Some("home"), &sold);
    wrong.sha256 = "0".repeat(64);
    let refused =
        deliver(&mut service, &mut harnesses, &wrong, &sold, 4096)
            .await
            .unwrap_err();
    assert!(refused.contains("not those"), "{refused}");
    let junk = br#"{"format": 1}"#.to_vec();
    let unreadable = settle_of("l_junk", "1.0.0", None, &junk);
    let refused = deliver(
        &mut service,
        &mut harnesses,
        &unreadable,
        &junk,
        4096,
    )
    .await
    .unwrap_err();
    assert!(refused.contains("cannot be read"), "{refused}");

    // A part out of order ends the upload.
    let settle = settle_of("l_scribe", "1.0.0", Some("home"), &sold);
    let ServerMessage::Reply(begun) =
        begin(&mut service, 1, &settle).unwrap()
    else {
        panic!("no reply");
    };
    let late = SettlePart {
        data: STANDARD.encode(&sold[..10]),
        id: RequestId::new(),
        offset: 10,
        upload: begun.upload.unwrap(),
    };
    let ServerMessage::Reply(answer) =
        part(&mut service, &mut harnesses, 1, &late).await.unwrap()
    else {
        panic!("no reply");
    };
    assert_eq!(answer.kind, ReplyKind::Error);
    assert!(read_bought(&service).is_empty());
}
