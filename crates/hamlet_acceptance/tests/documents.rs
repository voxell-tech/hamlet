//! The chief's documents: reads and edits of the charter, characters,
//! notes, limits, settings and the catalogue, and the hamlet's
//! folder.

use hamlet_acceptance::*;
use hamlet_protocol::client::*;
use hamlet_testing::{Script, words};
use serde_json::json;

fn kind_text(reply: &Reply) -> (&ReplyKind, &str) {
    (&reply.kind, reply.text.as_str())
}

async fn edit(
    c: &mut Client,
    make: impl FnOnce(RequestId) -> Edit,
) -> Reply {
    c.request(|id| ClientMessage::Edit(make(id))).await
}

#[tokio::test]
async fn edits_and_reads_the_charter_character_and_notes() {
    let mut s =
        Session::start(Script::new().responses([words("Done.")]))
            .await;
    let reply = edit(&mut s.chief, |id| Edit::Charter {
        id,
        text: Some("Be brief.".into()),
    })
    .await;
    assert_eq!(kind_text(&reply), (&ReplyKind::Notice, "Saved."));
    let reply = edit(&mut s.chief, |id| Edit::Character {
        id,
        agent: mayor(),
        text: "Calm.".into(),
    })
    .await;
    assert_eq!(kind_text(&reply), (&ReplyKind::Notice, "Saved."));
    let reply = edit(&mut s.chief, |id| Edit::Notes {
        id,
        agent: mayor(),
        text: "# Notes\nTea.".into(),
    })
    .await;
    assert_eq!(kind_text(&reply), (&ReplyKind::Notice, "Saved."));
    let papers = s.chief.papers(mayor()).await;
    assert_eq!(
        papers,
        Papers {
            character: "Calm.".into(),
            notes: "# Notes\nTea.".into(),
            skills: vec![]
        }
    );
    assert_eq!(
        std::fs::read_to_string(
            s.home.file("houses/home/0/NOTES.md")
        )
        .unwrap(),
        "# Notes\nTea."
    );
    let charter = s.chief.read(|id| Read::Charter { id }).await;
    assert!(
        matches!(&charter, ServerMessage::Read(ReadResult::Charter { charter, .. })
            if charter == &Charter { own: true, text: "Be brief.".into() }),
        "{charter:?}"
    );
    let prompt = s.chief.prompt(mayor()).await;
    assert!(prompt.starts_with("Be brief."), "{prompt}");
    assert!(prompt.contains("# Your character\nCalm."), "{prompt}");
    assert!(prompt.contains("# NOTES.md\n# Notes\nTea."), "{prompt}");
    // The built-in charter comes back when the chief's is taken away.
    edit(&mut s.chief, |id| Edit::Charter { id, text: None }).await;
    let ServerMessage::Read(ReadResult::Charter { charter, .. }) =
        s.chief.read(|id| Read::Charter { id }).await
    else {
        panic!("a charter")
    };
    assert!(!charter.own);
    assert!(charter.text.contains("`bash`"), "{}", charter.text);
}

#[tokio::test]
async fn edits_limits_one_key_or_whole_and_refuses_bad_values() {
    let mut s = Session::start(Script::new()).await;
    let reply =
        s.chief.limit("home", "budget", "cost", json!(2)).await;
    assert_eq!(kind_text(&reply), (&ReplyKind::Notice, "Saved."));
    s.chief
        .until("the new cost", |c| {
            let home = &c.realm().hamlets[0];
            home.limits.budget.cost == Some(2.0)
                && home.budget.summary
                    == "Hamlet today: $0.00 of $2.00"
                && home.budget.caps.micros == Some(2_000_000)
        })
        .await;
    let reply = edit(&mut s.chief, |id| {
        Edit::Limits(EditLimits::Whole {
            hamlet: "home".into(),
            id,
            reason: None,
            whole: [
                ("mostAgents".to_string(), json!(4)),
                (
                    "budget".to_string(),
                    json!({"cost": 3, "minutes": 10}),
                ),
            ]
            .into(),
        })
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("the whole limits", |c| {
            let home = &c.realm().hamlets[0];
            home.limits.most_agents == 4
                && home.limits.budget.cost == Some(3.0)
                && home.limits.budget.minutes == Some(10)
                && home.limits.task.cost == Some(1.0)
                && home.budget.caps.ms == Some(600_000)
        })
        .await;
    let before = s.chief.messages().len();
    let reply =
        s.chief.limit("home", "budget", "cost", json!(-1)).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "budget.cost: a cost is a number of zero or more"
        )
    );
    assert!(
        !s.chief.messages()[before..]
            .iter()
            .any(|m| matches!(m, ServerMessage::Change(_))),
        "a refused edit changes nothing"
    );
    let reply =
        s.chief.limit("home", "budget", "mystery", json!(1)).await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Error, "unknown field `mystery` under budget")
    );
    let reply = edit(&mut s.chief, |id| {
        Edit::Limits(EditLimits::Whole {
            hamlet: "home".into(),
            id,
            reason: None,
            whole: [(
                "permissions".to_string(),
                json!({"network": false}),
            )]
            .into(),
        })
    })
    .await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "permissions.network: expected a list of names"
        )
    );
    let reply =
        s.chief.limit("home", "votes", "passYes", json!(9)).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "votes.passYes: cannot exceed votes.passOf"
        )
    );
    let reply = s
        .chief
        .limit("home", "budget", "minutes", json!(null))
        .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief.limit("home", "mayor", "toldDays", json!(2)).await;
    s.chief
        .limit("home", "mayor", "toldDays", json!(null))
        .await;
    s.chief
        .until("the defaults back", |c| {
            let limits = &c.realm().hamlets[0].limits;
            limits.budget.minutes.is_none()
                && limits.mayor.told_days == 7
        })
        .await;
}

#[tokio::test]
async fn edits_settings_and_refuses_bad_ones() {
    let mut s = Session::start(Script::new()).await;
    let reply = edit(&mut s.chief, |id| Edit::Settings {
        id,
        set: EditSettingsSet {
            reflect_every: Some(0),
            allow_net: Some(true),
            ..set()
        },
    })
    .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
    s.chief
        .until("net allowed", |c| c.realm().settings.allow_net)
        .await;
    let reply = edit(&mut s.chief, |id| Edit::Settings {
        id,
        set: EditSettingsSet {
            reflect_every: Some(-1),
            ..set()
        },
    })
    .await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Error, "reflectEvery must be a whole number")
    );
    let reply = edit(&mut s.chief, |id| Edit::Settings {
        id,
        set: EditSettingsSet {
            endpoints: Some(vec![Endpoint {
                name: "private".into(),
                base_url: "http://u:p@host/v1".into(),
            }]),
            ..set()
        },
    })
    .await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "Keep credentials out of an endpoint's address."
        )
    );
    // A new connection gets the whole state as it is now.
    let late = s.join().await;
    assert!(late.realm().settings.allow_net);
}

fn set() -> EditSettingsSet {
    EditSettingsSet {
        model: None,
        reflect_every: None,
        allow_net: None,
        endpoints: None,
        workspace: None,
    }
}

#[tokio::test]
async fn chooses_a_remote_workspace_in_settings() {
    let mut s = Session::start(Script::new()).await;
    let workspace = s.chief.realm().settings.workspace;
    assert_eq!((workspace.url, workspace.running), (None, None));
    let reply = s
        .chief
        .request(|id| ClientMessage::MoveHouses(MoveHouses { id }))
        .await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "Hamlet already works in the workspace chosen in \
             Settings, so there are no houses to move."
        )
    );
    let choose = |url: &str, token: &str| EditSettingsSet {
        workspace: Some(Some(EditSettingsSetWorkspace {
            url: url.into(),
            token: token.into(),
        })),
        ..set()
    };
    let wrong =
        choose("http://workspace.example", "a-long-enough-token");
    let reply =
        edit(&mut s.chief, |id| Edit::Settings { id, set: wrong })
            .await;
    assert_eq!(reply.kind, ReplyKind::Error);
    assert!(
        reply.text.contains(
            "The workspace address must start with https://"
        ),
        "{reply:?}"
    );
    let short = choose("https://workspace.example", "short");
    let reply =
        edit(&mut s.chief, |id| Edit::Settings { id, set: short })
            .await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "The workspace token needs at least 16 characters."
        )
    );
    let good =
        choose("https://workspace.example", "a-long-enough-token");
    let reply =
        edit(&mut s.chief, |id| Edit::Settings { id, set: good })
            .await;
    assert_eq!(reply.kind, ReplyKind::Notice, "{reply:?}");
    s.chief
        .until("the chosen workspace", |c| {
            let workspace = c.realm().settings.workspace;
            workspace.url.as_deref()
                == Some("https://workspace.example")
                && workspace.running.is_none()
        })
        .await;
    let text = serde_json::to_string(s.chief.messages()).unwrap();
    assert!(!text.contains("a-long-enough-token"));
    let local = EditSettingsSet {
        workspace: Some(None),
        ..set()
    };
    let reply =
        edit(&mut s.chief, |id| Edit::Settings { id, set: local })
            .await;
    assert_eq!(reply.kind, ReplyKind::Notice);
}

#[tokio::test]
async fn prices_allows_and_notes_a_model_in_the_catalogue() {
    let mut s = Session::start(Script::new()).await;
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
    assert_eq!(reply.kind, ReplyKind::Notice);
    let ServerMessage::Read(ReadResult::Models { models, .. }) =
        s.chief.read(|id| Read::Models { id, refresh: None }).await
    else {
        panic!("a model list")
    };
    let faux =
        models.iter().find(|m| m.name == "faux::faux-1").unwrap();
    assert_eq!(
        (&faux.price, &faux.priced, faux.allowed, faux.note.as_str()),
        (
            &Some(Price {
                input: 0.12,
                output: 0.34
            }),
            &ListedPriced::Yours,
            true,
            "Preferred."
        )
    );
    let reply = edit(&mut s.chief, |id| Edit::Catalogue {
        id,
        model: "faux::faux-1".into(),
        set: EditCatalogueSet {
            allowed: None,
            price: Some(Some(Price {
                input: -1.0,
                output: 3.0,
            })),
            note: None,
        },
    })
    .await;
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Error, "a cost is a number of zero or more")
    );
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
    assert_eq!(
        kind_text(&reply),
        (&ReplyKind::Error, "no model named `missing::model`")
    );
}

#[tokio::test]
async fn points_the_hamlet_at_a_folder_and_switches_its_mode() {
    let mut s = Session::start(Script::new()).await;
    let a = s.home.side().join("a");
    let b = s.home.side().join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let (a, b) = (a.display().to_string(), b.display().to_string());
    let reply = s.chief.say(mayor(), "/folder").await;
    assert_eq!(
        reply.text,
        "This hamlet works on no folder. `/folder <folder>` points \
         it at one."
    );
    let reply = s.chief.say(mayor(), &format!("/folder {a}")).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Notice,
            format!(
                "This hamlet works on `{a}` now. Agents ask you \
                 before each edit and command."
            )
            .as_str()
        )
    );
    let reply = s.chief.say(mayor(), "/mode auto").await;
    assert_eq!(
        reply.text,
        "Agents edit and run commands in the folder without asking."
    );
    s.chief.say(mayor(), &format!("/folder {b}")).await;
    s.chief
        .until("the second folder", |c| {
            let home = &c.realm().hamlets[0];
            home.folder.as_deref() == Some(b.as_str())
                && home.mode == HamletMode::Auto
                && home.before == [a.clone()]
        })
        .await;
    let reply = s.chief.say(mayor(), "/folder none").await;
    assert_eq!(
        reply.text,
        "This hamlet works on no folder now. Its agents are back in \
         their own workspaces."
    );
    s.chief
        .until("no folder", |c| {
            let home = &c.realm().hamlets[0];
            home.folder.is_none()
                && home.before == [b.clone(), a.clone()]
        })
        .await;
    let reply = s.chief.say(mayor(), "/folder relative").await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            "give the folder's full path, such as \
             /Users/you/code/project"
        )
    );
    let missing = s.home.side().join("missing").display().to_string();
    let reply =
        s.chief.say(mayor(), &format!("/folder {missing}")).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            format!("`{missing}` is not a folder").as_str()
        )
    );
    let home = s.home.path().display().to_string();
    let reply =
        s.chief.say(mayor(), &format!("/folder {home}")).await;
    assert_eq!(
        kind_text(&reply),
        (
            &ReplyKind::Error,
            format!(
                "a hamlet cannot work on Hamlet's own folder \
                 ({home}), or on one that holds it"
            )
            .as_str()
        )
    );
    // The project shows in the prompt once a folder is set.
    s.chief.say(mayor(), &format!("/folder {a}")).await;
    let prompt = s.chief.prompt(mayor()).await;
    assert!(
        prompt.contains(&format!(
            "# The project\nYour hamlet works on the folder {a}."
        )),
        "{prompt}"
    );
}

#[tokio::test]
async fn composes_project_instructions_skills_and_unicode_cuts() {
    let mut s = Session::start_with(
        Script::new(),
        &[("HAMLET_REFLECT_EVERY", "0"), ("HAMLET_NET", "1")],
    )
    .await;
    let work = s.home.side().join("work");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    std::fs::write(work.join("CLAUDE.md"), "Use tabs.").unwrap();
    let house = s.home.file("houses/home/0");
    std::fs::create_dir_all(house.join("skills/forecast")).unwrap();
    std::fs::write(
        house.join("skills/forecast/SKILL.md"),
        "---\nname: forecast\ndescription: Get the forecast\nusage: \
         sh forecast.sh\n---\n",
    )
    .unwrap();
    s.chief
        .say(mayor(), &format!("/folder {}", work.display()))
        .await;
    let prompt = s.chief.prompt(mayor()).await;
    for text in [
        if cfg!(target_os = "macos") {
            "System: macOS, whose shell tools are the BSD ones."
        } else {
            "System: Linux."
        },
        "Network for tools: on.",
        "git worktree",
        "The chief approves each edit",
        "# The project's CLAUDE.md\nUse tabs.",
        "forecast: Get the forecast",
    ] {
        assert!(prompt.contains(text), "{text}: {prompt}");
    }
    assert!(prompt.contains(&format!(
        "cd '{}' && sh forecast.sh",
        house.display()
    )));
    std::fs::write(work.join("AGENTS.md"), "🌻".repeat(8001))
        .unwrap();
    edit(&mut s.chief, |id| Edit::Character {
        id,
        agent: mayor(),
        text: "🌻".repeat(4001),
    })
    .await;
    std::fs::write(house.join("NOTES.md"), "🌻".repeat(8001))
        .unwrap();
    let prompt = s.chief.prompt(mayor()).await;
    assert!(!prompt.contains("Use tabs."));
    for (heading, count, ending) in [
        (
            "# The project's AGENTS.md",
            8000,
            "[AGENTS.md is longer than 8000 characters and is cut \
             here.]",
        ),
        (
            "# Your character",
            4000,
            "[The character is longer than 4000 characters and is \
             cut here.]",
        ),
        (
            "# NOTES.md",
            8000,
            "[NOTES.md is longer than 8000 characters and is cut \
             here. Shorten it so that all of it is shown.]",
        ),
    ] {
        assert!(
            prompt.contains(&format!(
                "{heading}\n{}\n{ending}",
                "🌻".repeat(count)
            )),
            "missing {heading}"
        );
    }
    std::fs::write(house.join("NOTES.md"), [255, 254, 65]).unwrap();
    let prompt = s.chief.prompt(mayor()).await;
    assert!(prompt.contains("# NOTES.md\n\n# "));
    assert!(!prompt.contains("[NOTES.md is longer"));
}
