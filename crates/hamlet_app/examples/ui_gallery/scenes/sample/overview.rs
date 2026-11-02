//! The overview and hamlet gallery scenes.

use std::path::PathBuf;

use bevy::ecs::world::World;
use hamlet_client::realm::HamletId;
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Selected, Thing};

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(
        scene,
        Scene::Overview
            | Scene::Hamlet
            | Scene::Bylaws
            | Scene::Explorer
    ) {
        return;
    }
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    state.deeds = hamlet_testing::realm::deeds(&[
        "record",
        "overturn",
        "restore-hamlet",
        "erase-hamlet",
        "found",
    ]);
    state.closed = vec![protocol::ClosedHamlet {
        id: "press".into(),
        name: "The Old Press".into(),
        at: 1000,
        by: "chief".into(),
        reason: "The harvest is finished. Its houses are kept."
            .into(),
    }];
    for (id, title, mayor, others) in [
        ("home", "Pear Tree Hollow", "founder", 3),
        ("mill", "mill", "miller", 2),
        ("quay", "The Quay", "harbourmaster", 1),
    ] {
        let mut hamlet = hamlet_testing::realm::hamlet(id);
        hamlet.name = title.into();
        if id == "home" {
            hamlet.votes = vec![protocol::VoteView {
                id: 3,
                motion: "scout for mayor".into(),
                by: "chief".into(),
                open: false,
                overturnable: true,
                report: "Vote 3: scout for mayor. Two yes of three; \
                         passed."
                    .into(),
                voters: vec![],
            }];
            hamlet.folder = Some(match scene {
                Scene::Explorer => project().display().to_string(),
                _ => "/projects/orchard-almanac".into(),
            });
            hamlet.mode_words = "Agents ask before each edit and \
                                 command on the server."
                .into();
            hamlet.budget.lines = vec![
                "Today: $1.25 of $5.00".into(),
                "Model time: 4 minutes".into(),
            ];
            hamlet.before = vec![
                "/projects/pear-press".into(),
                "/projects/cider-ledger".into(),
            ];
        }
        if id == "home" {
            state.hamlets[0] = hamlet;
        } else {
            state.hamlets.push(hamlet);
        }
        if id != "home" {
            state
                .agents
                .push(hamlet_testing::realm::agent(id, 0, mayor));
        }
        for number in 1..=others {
            let mut agent = hamlet_testing::realm::agent(
                id,
                number,
                &format!("{id}-hand-{number}"),
            );
            agent.purpose = "Keeps a row of the orchard.".into();
            agent.busy = number == 2;
            agent.activity.doing_words =
                "Reading the orchard's notes".into();
            state.agents.push(agent);
        }
    }
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
    let first = world
        .query::<(bevy::ecs::entity::Entity, &HamletId)>()
        .iter(world)
        .find(|(_, hamlet)| hamlet.0 == "home")
        .map(|(entity, _)| entity)
        .unwrap();
    world.insert_resource(Selected(match scene {
        Scene::Hamlet | Scene::Bylaws | Scene::Explorer => {
            Some(Thing::Common(first))
        }
        _ => None,
    }));
    world.insert_resource(hamlet_app::overview::Annals::received(
        first,
        "The mayor created scout to read the orchard's records. The \
         chief restored smith, whose house and notes returned."
            .into(),
    ));
    if scene == Scene::Hamlet {
        world.insert_resource(hamlet_app::overview::Page::Record);
    }
    if scene == Scene::Bylaws {
        world.insert_resource(hamlet_app::overview::Page::Bylaws);
    }
    if scene == Scene::Explorer {
        let apples = project().join("src/press/apples.rs");
        hamlet_app::explorer::reveal(world, first, &apples);
    }
}

/// A small project on the disk, with a folder and a file git
/// ignores, for the Explorer to show.
fn project() -> PathBuf {
    let folder = std::env::temp_dir().join("hamlet-gallery-orchard");
    if folder.join("Cargo.toml").exists() {
        return folder;
    }
    for inside in [".git", "src/press", "assets", "target/debug"] {
        std::fs::create_dir_all(folder.join(inside))
            .expect("a folder");
    }
    let files = [
        (".gitignore", "target/\n*.log\n"),
        ("Cargo.toml", ""),
        ("README.md", ""),
        ("press.log", ""),
        ("src/main.rs", ""),
        ("src/ledger.rs", ""),
        ("src/press/mod.rs", ""),
        ("src/press/apples.rs", ""),
        ("src/press/pears.rs", ""),
        ("src/press/cider10.rs", ""),
        ("src/press/cider9.rs", ""),
    ];
    for (file, text) in files {
        std::fs::write(folder.join(file), text).expect("a file");
    }
    folder
}
