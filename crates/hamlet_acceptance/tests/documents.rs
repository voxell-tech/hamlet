//! The chief's documents: reads and edits of the charter, characters,
//! memory, limits, settings and the catalogue, and what an agent's
//! prompt makes of them.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use serde_json::json;

/// A reply to the edit `make` builds. An edit of a scholar's
/// repository can fail with a fault while the server is still working
/// on it, so a fault is answered with the same edit again.
async fn edit(
    client: &mut Client,
    make: impl Fn(RequestId) -> Edit,
) -> Reply {
    for _ in 0..100 {
        match client.ask(|id| ClientMessage::Edit(make(id))).await {
            ServerMessage::Reply(reply) => return reply,
            ServerMessage::Fault(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(
                    50,
                ))
                .await;
            }
            other => panic!("an edit got {other:?}"),
        }
    }
    panic!("the edit never settled");
}

fn memory(description: &str, body: &str) -> String {
    format!("---\ndescription: {description}\n---\n{body}\n")
}

fn settings() -> EditSettingsSet {
    EditSettingsSet {
        allow_net: None,
        effort: None,
        endpoints: None,
        model: None,
        reflect_every: None,
        workspace: None,
    }
}

#[tokio::test]
async fn edits_the_charter_character_and_memory_and_the_prompt_follows()
 {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let scholar = s.chief.scholar("ada").unwrap().id;
    let repository =
        s.home.file(&format!("scholars/{}.git", scholar.0));
    while !repository.exists() {
        tokio::time::sleep(std::time::Duration::from_millis(20))
            .await;
    }

    let reply = edit(&mut s.chief, |id| Edit::Charter {
        id,
        text: Some("Be brief.".into()),
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let reply = edit(&mut s.chief, |id| Edit::Character {
        id,
        scholar,
        text: "Calm.".into(),
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let tea = memory("Tea", "Earl grey.");
    let reply = edit(&mut s.chief, |id| Edit::Memory {
        id,
        of: Keeper::Scholar(scholar),
        path: "tea.md".into(),
        text: tea.clone(),
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    // A hire's own memory is its lodge's, which it alone has learned.
    let own = memory("Own", "A hire's note.");
    edit(&mut s.chief, |id| Edit::Memory {
        id,
        of: Keeper::Agent(ada),
        path: "own.md".into(),
        text: own.clone(),
    })
    .await;

    let papers = s.chief.papers(Keeper::Scholar(scholar)).await;
    assert_eq!(papers.character, "Calm.");
    assert_eq!(papers.memory.len(), 1);
    assert_eq!(papers.memory[0].path, "tea.md");
    assert_eq!(papers.memory[0].description, "Tea");
    assert!(papers.learned.is_empty());
    let papers = s.chief.papers(Keeper::Agent(ada)).await;
    assert_eq!(papers.character, "Calm.");
    let learned = papers
        .learned
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert!(learned.contains(&"memory/own.md"), "{learned:?}");
    assert!(papers.memory.iter().any(|m| m.path == "own.md"));

    let prompt = s.chief.prompt(ada).await;
    assert!(prompt.starts_with("Be brief."), "{prompt}");
    assert!(prompt.contains("# Your character\nCalm."), "{prompt}");
    assert!(prompt.contains("You are `ada@docs`"), "{prompt}");

    let ServerMessage::Read(ReadResult::Charter { charter, .. }) =
        s.chief.read(|id| Read::Charter { id }).await
    else {
        panic!("a charter");
    };
    assert_eq!(
        (charter.own, charter.text.as_str()),
        (true, "Be brief.")
    );
    // The built-in charter comes back when the chief's is taken away.
    edit(&mut s.chief, |id| Edit::Charter { id, text: None }).await;
    let ServerMessage::Read(ReadResult::Charter { charter, .. }) =
        s.chief.read(|id| Read::Charter { id }).await
    else {
        panic!("a charter");
    };
    assert!(!charter.own);
    assert!(charter.text.contains("`bash`"), "{}", charter.text);

    // An empty text removes a memory file; a path outside memory/ is
    // refused.
    edit(&mut s.chief, |id| Edit::Memory {
        id,
        of: Keeper::Scholar(scholar),
        path: "tea.md".into(),
        text: String::new(),
    })
    .await;
    assert!(
        s.chief
            .papers(Keeper::Scholar(scholar))
            .await
            .memory
            .is_empty()
    );
    let reply = edit(&mut s.chief, |id| Edit::Memory {
        id,
        of: Keeper::Scholar(scholar),
        path: "../CHARACTER.md".into(),
        text: "x".into(),
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Error);
}

#[tokio::test]
async fn edits_limits_one_key_or_whole_and_refuses_misshapen_ones() {
    let mut s = Session::start(Faux::new()).await;
    s.found("docs", "ada", "keep the docs").await;
    let reply = s
        .chief
        .limit(Some("docs"), "budget", "cost", json!(2))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    s.chief
        .until("the new cost", |c| {
            let docs = c.hamlet("docs").unwrap();
            docs.limits.budget.cost == Some(2.0)
                && docs.budget.summary
                    == "Hamlet today: $0.00 of $2.00"
                && docs.budget.caps.micros == Some(2_000_000)
        })
        .await;

    let mut whole =
        serde_json::to_value(s.chief.hamlet("docs").unwrap().limits)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
    whole.insert("depth".into(), json!(2));
    whole.insert(
        "budget".into(),
        json!({"cost": 3, "minutes": 10, "period": "day"}),
    );
    let reply = edit(&mut s.chief, |id| {
        Edit::Limits(EditLimits::Whole {
            hamlet: Some("docs".into()),
            id,
            reason: Some("a longer day".into()),
            whole: whole.clone().into_iter().collect(),
        })
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    s.chief
        .until("the whole limits", |c| {
            let docs = c.hamlet("docs").unwrap();
            docs.limits.depth == 2
                && docs.limits.budget.cost == Some(3.0)
                && docs.limits.budget.minutes == Some(10)
                && docs.limits.task.cost == Some(1.0)
                && docs.budget.caps.ms == Some(600_000)
        })
        .await;

    let before = s.chief.messages().len();
    let reply = s
        .chief
        .limit(Some("docs"), "budget", "mystery", json!(1))
        .await;
    assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
    let reply = s
        .chief
        .limit(Some("docs"), "road", "most", json!("many"))
        .await;
    assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
    let reply = s
        .chief
        .limit(Some("nowhere"), "budget", "cost", json!(2))
        .await;
    assert_eq!(reply.kind, ReplyKind::Error, "{}", reply.text);
    assert!(
        !s.chief.messages()[before..]
            .iter()
            .any(|m| matches!(m, ServerMessage::Change(_))),
        "a refused edit changes nothing"
    );

    // The realm has limits of its own.
    let reply = s.chief.limit(None, "budget", "cost", json!(9)).await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    s.chief
        .until("the realm's budget", |c| {
            c.realm().limits.budget.cost == Some(9.0)
        })
        .await;
    // A null puts a limit back to its default.
    s.chief
        .limit(Some("docs"), "budget", "minutes", json!(null))
        .await;
    s.chief
        .until("the default back", |c| {
            c.hamlet("docs").unwrap().limits.budget.minutes.is_none()
        })
        .await;
}

#[tokio::test]
async fn edits_settings_and_refuses_bad_ones() {
    let mut s = Session::start(Faux::new()).await;
    let reply = edit(&mut s.chief, |id| Edit::Settings {
        id,
        set: EditSettingsSet {
            reflect_every: Some(0),
            allow_net: Some(true),
            ..settings()
        },
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    s.chief
        .until("net allowed", |c| c.realm().settings.allow_net)
        .await;
    // A new connection gets the whole state as it is now.
    let late = s.join().await;
    assert!(late.realm().settings.allow_net);
}

#[tokio::test]
async fn prices_allows_and_notes_a_model_in_the_catalogue() {
    let mut s = Session::start(Faux::new()).await;
    s.chief
        .until("the faux provider", |c| {
            !c.realm().providers.is_empty()
        })
        .await;
    let reply = edit(&mut s.chief, |id| Edit::Catalogue {
        id,
        model: "faux::faux-1".into(),
        set: EditCatalogueSet {
            allowed: Some(true),
            price: Some(Some(Price {
                input: 0.12,
                output: 0.34,
            })),
            note: Some("Preferred.".into()),
        },
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{}", reply.text);
    let ServerMessage::Read(ReadResult::Models { models, .. }) =
        s.chief.read(|id| Read::Models { id, refresh: None }).await
    else {
        panic!("a model list");
    };
    let faux =
        models.iter().find(|m| m.name == "faux::faux-1").unwrap();
    assert_eq!(
        faux.price,
        Some(Price {
            input: 0.12,
            output: 0.34
        })
    );
    assert_eq!(faux.priced, ListedPriced::Yours);
    assert!(faux.allowed);
    assert_eq!(faux.note, "Preferred.");
    let reply = edit(&mut s.chief, |id| Edit::Catalogue {
        id,
        model: "missing::model".into(),
        set: EditCatalogueSet {
            allowed: Some(true),
            price: None,
            note: None,
        },
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Error);
    assert_eq!(reply.text, "no model is named `missing::model`");
}

#[tokio::test]
async fn a_hamlets_folder_and_mode_reach_the_prompt() {
    let mut s = Session::start(Faux::new()).await;
    let ada = s.found("docs", "ada", "keep the docs").await;
    let work = s.home.side().join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("AGENTS.md"), "Use tabs.").unwrap();
    let folder = work.display().to_string();
    s.chief.command(ada, &format!("/folder {folder}")).await;
    let prompt = s.chief.prompt(ada).await;
    assert!(prompt.contains("Use tabs."), "{prompt}");
    assert!(prompt.contains(&folder), "{prompt}");
    s.chief.command(ada, "/folder none").await;
    let prompt = s.chief.prompt(ada).await;
    assert!(!prompt.contains("Use tabs."));
    s.chief
        .until("folders remembered", |c| {
            c.hamlet("docs").unwrap().before == [folder.clone()]
        })
        .await;
}
