//! The hamlet, the talks and the records a scene shows.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use bevy::asset::RenderAssetUsages;
use bevy::ecs::entity::Entity;
use bevy::ecs::name::Name;
use bevy::ecs::world::World;
use bevy::image::Image;
use bevy::math::Rect;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat,
};
use hamlet_core::agents::{
    Account, Busy, Citizen, Left, LivesIn, Mayor, Purpose, Settlement,
};
use hamlet_core::core::HomeDir;
use hamlet_core::oversight::danger::Danger;
use hamlet_core::oversight::{Asks, Kind, Waiting};
use hamlet_core::session::archive::Past;
use hamlet_core::session::{
    ASKED, MAIL, SessionKey, TalksTo, Transcript,
};
use hamlet_home::Home;
use hamlet_realm::chief;
use hamlet_realm::pick::{Also, Selected, Thing};
use hamlet_rules::budget::{AgentBudget, Amount};
use hamlet_rules::chief::asking::Pick;
use hamlet_rules::chief::rulings::Choice;
use hamlet_ui::history::{History, Reading};
use hamlet_ui::links::{Aimed, Roots};
use hamlet_ui::{
    Me, asking, chat, history, inspector, menu, overview, welcome,
};
use serde_json::{Value, json};

use super::Scene;

/// The fonts, the dock's icons, and a hamlet of one agent in talk
/// with the chief.
pub(super) fn setup(world: &mut World) {
    let scene = *world.resource::<Scene>();
    // Under where its title stands in the top bar.
    let open = match scene {
        Scene::Menu => Some((menu::Menu::App, 8.0, 95.0)),
        Scene::ViewMenu => Some((menu::Menu::View, 104.0, 176.0)),
        _ => None,
    };
    if let Some((which, left, right)) = open {
        let title = Rect::new(left, 0.0, right, 34.0);
        world.insert_resource(menu::OpenMenu::under(which, title));
    }
    let agent = world
        .spawn((
            Name::new("Juniper"),
            Citizen(5),
            Purpose("Keeps the orchard's notes in order.".into()),
            Account(AgentBudget {
                name: "Juniper".into(),
                model: "small".into(),
                spent: Amount {
                    micros: 100_000,
                    ..Amount::default()
                },
            }),
        ))
        .id();
    let talk = match scene {
        Scene::Chat => long_talk(),
        Scene::Group => group_talk(world, agent),
        Scene::Links => linked_talk(world),
        Scene::Edits => edited_talk(world),
        Scene::Pictures => pictured_talk(world),
        Scene::Rewind => second_talk(),
        Scene::Quiet => Transcript::default(),
        _ => Transcript::from_log(&[
            said(
                0,
                json!({"type": "user", "user": "chief",
                "content": "Tidy the orchard notes and tell me what \
                            is missing."}),
            ),
            said(
                1,
                json!({"type": "assistant", "message": {
                "content": "The notes are in order. Two things are \
                            missing: the date the pears were \
                            grafted, and who keeps the key to the \
                            press."}}),
            ),
        ]),
    };
    let open = talk.ids().into_iter().rev().find(|id| {
        matches!(
            talk.get(*id),
            Some(hamlet_core::session::Entry::Tool { .. })
        )
    });
    let session = world.spawn(talk).id();
    if let (Scene::Chat, Some(last)) = (scene, open) {
        // The last turn's fold begins three steps before its end.
        world
            .resource_mut::<chat::Unfolded>()
            .0
            .push((session, last - 2));
    }
    world.insert_resource(Me {
        session,
        key: SessionKey {
            channel: "app".into(),
            chat: "chief".into(),
            thread: "main".into(),
        },
        user: "chief".into(),
        mayor: session,
        agent,
        with: Some("Juniper".into()),
    });
    world.init_resource::<Also>();
    world.insert_resource(Selected(Some(Thing::Resident(agent))));
    if scene == Scene::Plugins {
        world.insert_resource(hamlet_ui::settings::Pane::Plugins);
    }
    #[cfg(feature = "wasm")]
    if scene == Scene::PluginRoster {
        super::roster::put(world);
    }
    if matches!(
        scene,
        Scene::Settings | Scene::Charter | Scene::Plugins
    ) {
        let mut config = hamlet_model::config::Config::default();
        config.endpoints.push(hamlet_model::config::Endpoint {
            name: "far".into(),
            base_url: "https://models.example/api/v1".into(),
            chatgpt: false,
            api_key: Some("key".into()),
        });
        world.insert_resource(
            hamlet_core::catalogue::ModelCatalogue::of(shelf()),
        );
        world.init_resource::<hamlet_ui::models::Shelf>();
        world.init_resource::<hamlet_core::core::Listing>();
        world.insert_resource(hamlet_core::core::Settings {
            config,
            error: None,
        });
        world.init_resource::<welcome::Committing>();
        world.init_resource::<hamlet_desktop::awake::KeepAwake>();
        world.init_resource::<hamlet_realm::sky::Quality>();
    }
    if scene == Scene::Charter {
        let home = house();
        // One of its own, so that the way back to the default shows.
        std::fs::write(
            hamlet_home::charter::file(&home.root),
            hamlet_home::charter::DEFAULT,
        )
        .expect("a charter");
        world.insert_resource(HomeDir(home));
        world.insert_resource(hamlet_ui::settings::Pane::Charter);
    }
    if matches!(
        scene,
        Scene::Notes | Scene::Character | Scene::Prompt
    ) {
        keep_notes(world);
    }
    if scene == Scene::Character {
        world.insert_resource(inspector::Section::Character);
    }
    if matches!(scene, Scene::Skills | Scene::SkillsAlone) {
        let skill =
            |name: &str, description: &str| hamlet_home::Skill {
                name: name.into(),
                description: description.into(),
                usage: None,
                path: PathBuf::new(),
            };
        if scene == Scene::Skills {
            // No home here to read them from.
            world.insert_resource(hamlet_sdk::convo::AgentSelected(
                true,
            ));
            world.insert_resource(hamlet_sdk::papers::Papers {
                skills: vec![
                    (
                        "press".into(),
                        "Press the late fruit, rows 4, 7 and 9, \
                         before dusk."
                            .into(),
                    ),
                    (
                        "frost-watch".into(),
                        "Say which rows want straw when the night \
                         turns cold."
                            .into(),
                    ),
                ],
                ..Default::default()
            });
        }
        world.insert_resource(hamlet_core::core::Skills(vec![
            skill("graft", "Graft a scion onto a named stock."),
            skill(
                "label-rows",
                "Name each tree by its row and place.",
            ),
        ]));
    }
    if scene == Scene::Prompt {
        let agent = world.resource::<Me>().agent;
        let text = world.resource::<HomeDir>().0.system_prompt(
            hamlet_home::charter::DEFAULT,
            "Sandbox: seatbelt. Network for tools: off.",
            None,
        );
        world.insert_resource(inspector::Briefing {
            agent: Some(agent),
            text,
        });
        world.insert_resource(inspector::Section::Prompt);
    }
    if matches!(scene, Scene::History | Scene::Reading) {
        world.insert_resource(inspector::Section::History);
        past_sessions(world, scene == Scene::Reading);
    }
    if matches!(
        scene,
        Scene::Overview
            | Scene::Hamlet
            | Scene::Rules
            | Scene::SkillsAlone
    ) {
        let places = [
            ("home", "Pear Tree Hollow", "founder", 3),
            ("mill", "mill", "miller", 2),
            ("quay", "The Quay", "harbourmaster", 1),
        ];
        let mut first = None;
        for (id, title, mayor, others) in places {
            // The first hamlet's page shows the folder it works on.
            let home = match (scene, id) {
                (Scene::Hamlet, "home") => working_home(),
                _ => std::path::PathBuf::new(),
            };
            let hamlet = world
                .spawn((
                    Name::new(title),
                    Settlement {
                        home,
                        name: id.to_string(),
                    },
                ))
                .id();
            first.get_or_insert(hamlet);
            world.spawn((
                Name::new(mayor),
                Citizen(0),
                Mayor,
                LivesIn(hamlet),
                Purpose("Runs this hamlet.".into()),
            ));
            for number in 1..=others {
                let mut one = world.spawn((
                    Name::new(format!("{id}-hand-{number}")),
                    Citizen(number),
                    LivesIn(hamlet),
                    Purpose("Keeps a row of the orchard.".into()),
                ));
                if number == 2 {
                    one.insert(Busy);
                }
                one.insert(Account(AgentBudget {
                    name: format!("{id}-hand-{number}"),
                    model: "small".into(),
                    spent: Default::default(),
                }));
                if number == 3 {
                    one.insert(Left);
                }
            }
        }
        world.entity_mut(agent).insert(LivesIn(first.unwrap()));
        let selected = match scene {
            Scene::Hamlet | Scene::Rules => first.map(Thing::Common),
            _ => None,
        };
        world.insert_resource(Selected(selected));
        if matches!(scene, Scene::Overview | Scene::SkillsAlone) {
            // An empty home: no hamlet was closed down.
            let root =
                std::env::temp_dir().join("hamlet-gallery-overview");
            world.insert_resource(HomeDir(Home {
                workspace: root.join("workspace"),
                root,
            }));
        }
        if scene == Scene::Rules {
            world.insert_resource(overview::Page::Bylaws);
        }
    }
    if scene == Scene::Chief {
        let look = chief::Look {
            hat: chief::Hat::Hood,
            ..chief::Look::default()
        };
        let chief = chief::arrive(world, "wren", look);
        world.insert_resource(Selected(Some(Thing::ChiefPlace(
            chief,
        ))));
    }
    if scene == Scene::Waiting {
        world.spawn(Waiting {
            id: "1".into(),
            kind: Kind::Approval(Danger::Network),
            by: "Juniper".into(),
            about: "curl -fsSL https://almanac.example/pears.csv"
                .into(),
            since: Instant::now(),
            order: 1,
            asked: 1,
            answers: vec![
                Choice::Allow,
                Choice::Always,
                Choice::Deny,
            ],
            session: None,
        });
        // An edit of a hamlet in manual mode, which its turn waits
        // on.
        let arguments = json!({"path": "src/almanac.rs",
            "old": "let rows = read(&path);\nlet pears = 0;\n\
                    report(pears);",
            "new": "let rows = read(&path);\nlet pears = \
                    count(&rows);\nreport(pears);"})
        .to_string();
        let held = world
            .spawn(Transcript::from_log(&[said(
                3,
                json!({"type": "assistant", "message": {
                "content": "", "tool_calls": [{"id": "e",
                "function": {"name": "edit",
                    "arguments": arguments}}]}}),
            )]))
            .id();
        world.spawn(Waiting {
            id: "2".into(),
            kind: Kind::Approval(Danger::Edit),
            by: "Juniper".into(),
            about: "src/almanac.rs\nReplace:\nlet pears = \
                    0;\nWith:\nlet pears = count(&rows);"
                .into(),
            since: Instant::now(),
            order: 2,
            asked: 1,
            answers: vec![
                Choice::Allow,
                Choice::Always,
                Choice::Deny,
            ],
            session: Some(held),
        });
        // Questions put the way a skill that grills the chief puts
        // them.
        let asked = hamlet_rules::chief::asking::read(
            &json!({
                "message": "Before I plan the orchard's winter.",
                "questions": [
                    {"question": "Which trees come first?",
                     "description": "The frost reaches the low rows \
                                     a week early.",
                     "options": [
                        {"title": "Pears",
                         "description": "Bruise in the cold"},
                        {"title": "Apples"},
                        {"title": "Both",
                         "description": "Takes two more days"}
                     ]},
                    {"question": "How much may the straw cost?",
                     "description": "Per bale."}
                ]
            })
            .to_string(),
        )
        .expect("questions");
        // Asked in the open chat, so the chat shows the card too.
        let open = world.get_resource::<Me>().map(|me| me.session);
        let card = world
            .spawn((
                Waiting {
                    id: "3".into(),
                    kind: Kind::Question,
                    by: "Juniper".into(),
                    about: asked.about(),
                    since: Instant::now(),
                    order: 0,
                    asked: 1,
                    answers: vec![Choice::Reply, Choice::Dismiss],
                    session: open,
                },
                Asks(asked),
            ))
            .id();
        // The first answered, and shown again to see it picked.
        asking::choose(world, card, 2, 0, Some(Pick::Offer(0)));
        asking::turn(world, card, 2, 0);
    }
}

/// A home whose first hamlet works on a folder, after two others.
fn working_home() -> std::path::PathBuf {
    let root = std::env::temp_dir().join("hamlet-gallery-works");
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).expect("a home");
    let hamlet = hamlet_rules::hamlet::Hamlet::new(home.clone());
    for name in ["pear-press", "cider-ledger", "orchard-almanac"] {
        let folder = root.join("code").join(name);
        std::fs::create_dir_all(&folder).expect("a folder");
        hamlet.work_on(Some(&folder), "chief").expect("a record");
    }
    home
}

/// A line of a log, said `minute` minutes into the gallery's
/// afternoon.
fn said(minute: u64, mut line: Value) -> Value {
    const NOON: u64 = 1_791_374_400_000;
    line["ts"] = json!(NOON + minute * 60_000);
    line
}

/// A call of the shell and its end, as a log has them.
fn step(minute: u64, id: &str, command: &str) -> [Value; 2] {
    let arguments = json!({ "command": command }).to_string();
    [
        said(
            minute,
            json!({"type": "assistant", "message": {
            "content": "",
            "tool_calls": [{"id": id, "function": {
                "name": "bash", "arguments": arguments}}]}}),
        ),
        said(minute, json!({"type": "tool", "exit_code": 0})),
    ]
}

/// A chat of many turns, each with steps of work and an answer in
/// several paragraphs.
fn long_talk() -> Transcript {
    let mut log = Vec::new();
    for turn in 0..24 {
        log.push(said(turn * 3, json!({"type": "user",
            "user": "chief", "content":
            format!("Look over row {turn} of the orchard and say what \
                     it needs before the frost.")})));
        for part in 0..3 {
            log.extend(step(
                turn * 3 + 1,
                &format!("{turn}-{part}"),
                &format!(
                    "grep -n row-{turn} notes/orchard/{part}.md"
                ),
            ));
        }
        log.push(said(
            turn * 3 + 2,
            json!({"type": "assistant",
            "message": {"content":
            format!("Row {turn} is sound.\n\nIt wants **three \
                     things** before the frost:\n\n- straw at the \
                     roots\n- the split stake by the gate replaced\n\
                     - the last pears off the high branches\n\n\
                     Nothing else there needs you.")}}),
        ));
    }
    Transcript::from_log(&log)
}

/// A talk of two turns, the second of which went the wrong way.
fn second_talk() -> Transcript {
    let mut log = vec![
        said(
            0,
            json!({"type": "user", "user": "chief",
            "content": "Tidy the orchard notes and tell me what is \
                        missing."}),
        ),
        said(
            1,
            json!({"type": "assistant", "message": {
            "content": "The notes are in order. Two things are \
                        missing: the date the pears were grafted, \
                        and who keeps the key to the press."}}),
        ),
        said(
            4,
            json!({"type": "user", "user": "chief",
            "content": "(Replying to Juniper: \"The notes are in order. \
                        Two things are missing: the date the pears were \
                        grafted, and who keeps the key to the \
                        press.\")\n\n> the date the pears were \
                        grafted\nWrite it into the almanac."}),
        ),
    ];
    log.extend(step(5, "g", "grep -rn grafted notes/orchard"));
    log.push(said(
        6,
        json!({"type": "assistant", "message": {
        "content": "No note says when the pears were grafted, so I \
                    wrote this spring into the almanac as a guess."}}),
    ));
    // Questions the chief answered on a card.
    let asked = json!({"questions": [
        {"question": "Is this spring right?",
         "options": ["Yes", "No"]},
        {"question": "Who keeps the key to the press?"}]})
    .to_string();
    log.push(said(
        7,
        json!({"type": "assistant", "message": {"content": "",
            "tool_calls": [{"id": "q", "function": {
                "name": "ask_chief", "arguments": asked}}]}}),
    ));
    log.push(said(
        8,
        json!({"type": "tool", "name": "ask_chief", "exit_code": 0,
            "output": "The chief answers:\n> Is this spring right?\n\
                       No\n\n> Who keeps the key to the press?\nWren, \
                       since the autumn fair"}),
    ));
    Transcript::from_log(&log)
}

/// A talk with links of every kind in it, in a hamlet whose folder
/// holds the files it names.
fn linked_talk(world: &mut World) -> Transcript {
    let folder = std::env::temp_dir().join("hamlet-gallery-links");
    let _ = std::fs::remove_dir_all(&folder);
    for file in ["src/almanac.rs", "notes/plan.md", "tools/press.sh"]
    {
        let file = folder.join(file);
        std::fs::create_dir_all(file.parent().expect("a folder"))
            .expect("a folder");
        std::fs::write(file, "").expect("a file");
    }
    world.insert_resource(Roots::new([folder]));
    let press = "https://orchard.example/press/how-it-works";
    world.insert_resource(Aimed::at(press));
    let answer = [
        "It is, in `src/almanac.rs:42`, and what is left to do is in \
         notes/plan.md.\n\n",
        &format!("- A link by name: [how the press works]({press})\n"),
        "- An address as it is: \
         https://orchard.example/press_(old).\n",
        "- A file by name: [the plan](notes/plan.md)\n",
        "- A script, which a click only shows in its folder: \
         tools/press.sh\n",
        "- No such file, so no link: `src/missing.rs`\n",
        "- Not the web, so no link: [a file \
         address](file:///etc/passwd)\n\n",
        "```sh\ncat src/almanac.rs\n```\n\n",
        "A long address runs over the end of its line: \
         https://orchard.example/almanac/rows/pears/before-the-frost\
         /straw-at-the-roots/the-split-stake-by-the-gate/the-last-\
         pears-off-the-high-branches",
    ]
    .concat();
    Transcript::from_log(&[
        said(
            0,
            json!({"type": "user", "user": "chief",
            "content": "Where is the almanac kept? I thought it was \
                        src/almanac.rs, as \
                        https://orchard.example/almanac says."}),
        ),
        said(
            1,
            json!({"type": "assistant",
            "message": {"content": answer}}),
        ),
    ])
}

/// Writes a picture of `wide` by `tall` pixels to `file`: squares
/// of a side of 30 over a fade, which stay square while its
/// proportions are kept.
fn paint(file: &std::path::Path, wide: u32, tall: u32) {
    let mut data = Vec::with_capacity((wide * tall * 4) as usize);
    for y in 0..tall {
        for x in 0..wide {
            let dark = (x / 30 + y / 30) % 2 == 0;
            let across = (x * 100 / wide) as u8;
            let down = (y * 80 / tall) as u8;
            let lit = if dark { 0 } else { 60 };
            data.extend([
                70 + across + lit,
                60 + down + lit,
                40 + lit / 2,
                255,
            ]);
        }
    }
    Image::new(
        Extent3d {
            width: wide,
            height: tall,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    )
    .try_into_dynamic()
    .expect("a picture")
    .save(file)
    .expect("a picture written");
}

/// A talk with pictures of every kind in it, in a hamlet whose
/// folder holds the ones it has.
fn pictured_talk(world: &mut World) -> Transcript {
    let folder = std::env::temp_dir().join("hamlet-gallery-pictures");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(folder.join("shots")).expect("a folder");
    paint(&folder.join("shots/press.png"), 480, 270);
    paint(&folder.join("shots/ladder.png"), 300, 1500);
    paint(&folder.join("shots/rows.png"), 2400, 300);
    std::fs::write(folder.join("shots/plan.svg"), "<svg/>")
        .expect("a file");
    std::fs::write(folder.join("shots/torn.png"), "torn")
        .expect("a file");
    world.insert_resource(Roots::new([folder]));
    let answer = [
        "Here is the press as it stands:\n\n",
        "![The press, from the gate](shots/press.png)\n\n",
        "The ladder is taller than the chat lets a picture be, so \
         it is made smaller: ![The ladder](shots/ladder.png) and \
         the rows are wider than the chat, so they are too.\n\n",
        "![The rows, end to end](shots/rows.png)\n\n",
        "- Never drawn, so its words alone: \
         ![the cellar](shots/cellar.png)\n",
        "- A drawing the chat cannot show, so a link: \
         ![the plan](shots/plan.svg)\n",
        "- A file that is no picture inside, so a link: \
         ![the torn page](shots/torn.png)\n\n",
        "And one from the web, which waits to be asked for:\n\n",
        "![The old press](https://orchard.example/press/old.png?\
         row=3)",
    ]
    .concat();
    Transcript::from_log(&[
        said(
            0,
            json!({"type": "user", "user": "chief",
            "content": "Show me the press and the rows."}),
        ),
        said(
            1,
            json!({"type": "assistant",
            "message": {"content": answer}}),
        ),
    ])
}

/// A call of `edit` and its end, as a log has them.
fn edit(
    minute: u64,
    path: &str,
    old: &str,
    new: &str,
    answer: Result<&str, &str>,
) -> [Value; 2] {
    let arguments =
        json!({"path": path, "old": old, "new": new}).to_string();
    let (exit, output) = match answer {
        Ok(output) => (0, output),
        Err(output) => (1, output),
    };
    [
        said(
            minute,
            json!({"type": "assistant", "message": {
            "content": "",
            "tool_calls": [{"id": path, "function": {
                "name": "edit", "arguments": arguments}}]}}),
        ),
        said(
            minute,
            json!({"type": "tool", "exit_code": exit,
                "output": output}),
        ),
    ]
}

/// A talk with a turn of edits in it: a small change, a new file, a
/// long change and one that failed.
fn edited_talk(world: &mut World) -> Transcript {
    let folder = std::env::temp_dir().join("hamlet-gallery-edits");
    let _ = std::fs::remove_dir_all(&folder);
    for file in ["src/almanac.rs", "src/rows.rs", "notes/frost.md"] {
        let file = folder.join(file);
        std::fs::create_dir_all(file.parent().expect("a folder"))
            .expect("a folder");
        std::fs::write(file, "").expect("a file");
    }
    world.insert_resource(Roots::new([folder]));
    let rows = (1..=40)
        .map(|row| format!("    Row::new({row}, \"pear\"),"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut log = vec![said(
        0,
        json!({"type": "user", "user": "chief",
        "content": "Count the pears from the rows, and note what \
                    the frost needs."}),
    )];
    log.extend(step(1, "s", "grep -n pears src/almanac.rs"));
    log.extend(edit(
        1,
        "src/almanac.rs",
        "fn pears(path: &Path) -> usize {\n    let rows = \
         read(path);\n    let pears = 0;\n    // Every row is \
         counted.\n    report(pears);\n    pears\n}",
        "fn pears(path: &Path) -> usize {\n    let rows = \
         read(path);\n    let pears = count(&rows);\n    \
         report(pears);\n    log(pears);\n    pears\n}",
        Ok("edited src/almanac.rs"),
    ));
    log.extend(edit(
        2,
        "notes/frost.md",
        "",
        "# Before the frost\n\n- straw at the roots\n- the split \
         stake by the gate",
        Ok("created notes/frost.md"),
    ));
    log.extend(edit(
        2,
        "src/rows.rs",
        "const ROWS: [Row; 0] = [\n];",
        &format!("const ROWS: [Row; 40] = [\n{rows}\n];"),
        Ok("edited src/rows.rs"),
    ));
    log.extend(edit(
        3,
        "src/press.rs",
        "let key = None;",
        "let key = Some(by_the_door());",
        Err("Error: `old` is not in src/press.rs. Copy the text to \
             replace from the file exactly. It begins:\nfn press()"),
    ));
    log.push(said(
        4,
        json!({"type": "assistant", "message": {"content":
        "The pears are counted from the rows now, and what the \
         frost needs is in notes/frost.md. I could not change the \
         press: its key is not where I thought."}}),
    ));
    Transcript::from_log(&log)
}

/// Juniper's talk with the chief, and beside it what the mayor and
/// Pip wrote it and it wrote them, each in a conversation of its
/// own.
fn group_talk(world: &mut World, juniper: Entity) -> Transcript {
    let user = |minute, from: &str, text: &str| {
        said(
            minute,
            json!({"type": "user", "user": from,
            "content": text}),
        )
    };
    let reply = |minute, text: &str| {
        said(
            minute,
            json!({"type": "assistant",
            "message": {"content": text}}),
        )
    };
    world.spawn((
        Name::new("Bramble"),
        Citizen(0),
        Mayor,
        Purpose("Runs this hamlet.".into()),
    ));
    let pip = world
        .spawn((
            Name::new("Pip"),
            Citizen(4),
            Purpose("Keeps the press and its key.".into()),
        ))
        .id();

    // Juniper's question to Pip, which it waited on.
    world.spawn((
        SessionKey::with_agent(ASKED, "Juniper", "Pip"),
        TalksTo(pip),
        Transcript::from_log(&[
            user(4, "Juniper", "Who keeps the key to the press?"),
            reply(5, "I do. It hangs by the mill door."),
        ]),
    ));
    // Letters from Pip, and what Juniper wrote back.
    world.spawn((
        SessionKey::with_agent(MAIL, "Pip", "Juniper"),
        TalksTo(juniper),
        Transcript::from_log(&[
            user(8, "Pip", "The key is back on its hook."),
            user(9, "Pip", "Row 4 wants straw."),
            user(9, "Pip", "And the stake by the gate is split."),
            reply(10, "Noted. All three are in the notes now."),
        ]),
    ));
    // A letter from the mayor, answered after some work.
    let mut asked = vec![user(
        12,
        "mayor",
        "Send me the frost plan for the orchard before dusk.",
    )];
    for (part, file) in ["rows", "frost", "press"].iter().enumerate()
    {
        asked.extend(step(
            13,
            &format!("m-{part}"),
            &format!("grep -n frost notes/orchard/{file}.md"),
        ));
    }
    asked.push(reply(
        15,
        "## Frost plan\n\nThe orchard is ready but for **three \
         rows**. In order:\n\n1. Row 4: straw at the roots\n2. Row \
         7: the split stake replaced\n3. Row 9: the last pears off \
         the high branches\n\nPip has the key to the press, so the \
         late fruit can go in tonight:\n\n```sh\npress --rows 4,7,9 \
         --before dusk\n```\n\n| Row | Needs | By \
         |\n|---|---|---|\n| 4 | straw | Pip |\n| 7 | a stake | me \
         |\n\nNothing else there needs you.",
    ));
    let from_mayor = Transcript::from_log(&asked);
    let work = from_mayor.ids()[1];
    let letter = world
        .spawn((
            SessionKey::with_agent(MAIL, "mayor", "Juniper"),
            TalksTo(juniper),
            from_mayor,
        ))
        .id();
    world
        .resource_mut::<chat::Unfolded>()
        .0
        .push((letter, work));

    let mut log = vec![user(
        0,
        "chief",
        "Tidy the orchard notes and tell me what is missing.",
    )];
    for part in 0..3 {
        log.extend(step(
            1,
            &format!("c-{part}"),
            &format!("grep -n missing notes/orchard/{part}.md"),
        ));
    }
    log.extend([
        reply(
            2,
            "The notes are in order. Two things are missing: the \
             date the pears were grafted, and who keeps the key to \
             the press.",
        ),
        user(3, "chief", "Find out who has the key."),
    ]);
    log.extend(step(4, "c-ask", "ask Pip"));
    log.extend([
        reply(6, "Pip keeps it, by the mill door."),
        user(16, "chief", "Good."),
        user(16, "chief", "Keep me posted on the frost."),
        reply(17, "I will."),
    ]);
    Transcript::from_log(&log)
}

/// Sample sessions for the History page, one open if `reading`.
fn past_sessions(world: &mut World, reading: bool) {
    let told = [
        ("Tidy the orchard notes and tell me what is missing.", 0, 4),
        ("Which rows were grafted this spring?", 2 * 3600, 12),
        ("Draft the letter to the cooper about the casks.", 86400, 7),
        ("Count the hives and say which are weak.", 3 * 86400, 22),
        ("Where is the key to the press kept?", 9 * 86400, 2),
    ];
    let pasts = told
        .iter()
        .enumerate()
        .map(|(i, (title, ago, messages))| Past {
            path: PathBuf::from(format!("{i}.jsonl")),
            title: title.to_string(),
            by: "chief".into(),
            modified: SystemTime::now() - Duration::from_secs(*ago),
            number: (i > 0).then(|| format!("17000000{i}")),
            ours: i > 0,
            messages: *messages,
            tokens: 1800 * *messages as u64,
            said: title.to_lowercase(),
        })
        .collect::<Vec<_>>();
    let reading = reading.then(|| {
        let transcript = Transcript::from_log(&[
            json!({"type": "user", "user": "chief",
                "content": told[1].0}),
            json!({"type": "assistant", "message": {
                "content": "Three rows, all on the south wall:\n\n\
                    - the **Conference** pears, grafted in March\n\
                    - the two rows of Comice beside them\n\n\
                    The quince stock for a fourth row came too late."}}),
            json!({"type": "user", "user": "chief",
                "content": "Note the quince for next year."}),
            json!({"type": "assistant", "message": {
                "content": "Noted, under next spring's work."}}),
        ]);
        Reading {
            path: pasts[1].path.clone(),
            said: history::said(&transcript),
            holder: world.spawn(transcript).id(),
            reach: history::PAGE,
        }
    });
    world.insert_resource(History {
        pasts,
        reading,
        fresh: true,
        ..Default::default()
    });
}

/// The house the gallery's agent keeps its papers in.
fn house() -> Home {
    let root = std::env::temp_dir().join("hamlet-gallery");
    let home = Home {
        workspace: root.join("workspace"),
        root,
    };
    std::fs::create_dir_all(&home.workspace).expect("a workspace");
    home
}

/// A house with notes and a character in it, and the panel turned to
/// the notes.
fn keep_notes(world: &mut World) {
    let home = house();
    std::fs::write(
        home.character(),
        "You are **Juniper**, the orchard's keeper of notes.\n\n- \
         Speak plainly, in short sentences.\n- Say what you do not \
         know.\n- Name the row and the tree, never \"that one\".\n",
    )
    .expect("the character");
    std::fs::write(
        home.notes(),
        "# Orchard\n\nWhat I know of the rows, **newest \
         first**.\n\n## Before the frost\n\n- Row 4 wants straw at \
         the roots\n- The stake by the gate is split\n- [x] The \
         last pears are off the high branches\n\n## The \
         press\n\nPip keeps the key, by the mill door. To press the \
         late fruit:\n\n```sh\npress --rows 4,7,9 --before \
         dusk\n```\n\n| Row | Grafted |\n|---|---|\n| Conference | \
         March |\n| Comice | April |\n\n> The quince stock came too \
         late for a fourth row.\n",
    )
    .expect("the notes");
    world.insert_resource(HomeDir(home));
    world.insert_resource(inspector::Section::Notes);
}

/// The first-run screen as `scene` shows it: closed, or at a step
/// with what a provider or this computer might have answered.
pub(super) fn first_run(scene: Scene) -> welcome::Welcome {
    let Scene::Welcome(step) = scene else {
        return welcome::Welcome::default();
    };
    let models = [
        "maker/large",
        "maker/large-fast",
        "maker/small",
        "other/open-weights-70b",
        "other/open-weights-8b",
    ];
    let mut shown = welcome::Welcome::default();
    shown.open = true;
    shown.step = step;
    shown.from = "OpenRouter".into();
    shown.models = models.map(String::from).to_vec();
    shown.problem = match step {
        welcome::Step::Key => Some(
            "That key was not accepted. Check that you copied all \
             of it and that it is still active, then try again."
                .into(),
        ),
        welcome::Step::Local => Some(
            "No model server answered on this computer. Install \
             Ollama from ollama.com, open it and download a model. \
             Then choose Look Again."
                .into(),
        ),
        _ => None,
    };
    shown
}

/// A few models, as a home might keep them.
fn shelf() -> hamlet_model::catalogue::Catalogue {
    use hamlet_model::catalogue::{Catalogue, Listed, Price};
    let mut all = Catalogue::default();
    let listed = |id: &str, price: Option<(f64, f64)>| Listed {
        id: id.into(),
        price: price.map(|(input, output)| Price { input, output }),
        context: price.map(|_| 200_000),
    };
    all.take_in(
        "far",
        &[
            listed("maker/large", Some((3.0, 15.0))),
            listed("maker/small", Some((0.25, 1.25))),
        ],
        false,
    );
    all.take_in("own", &[listed("qwen3.6:35b-a3b", None)], true);
    let large = all.get_mut("far::maker/large").expect("just listed");
    large.allowed = true;
    large.note = "For hard questions and long plans.".into();
    all
}
