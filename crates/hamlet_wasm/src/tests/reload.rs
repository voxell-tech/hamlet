use bevy::ecs::message::Messages;
use bevy::prelude::*;
use hamlet_agents_api::oversight::Choice as Answer;
use hamlet_sdk::capability::{Capability, Subject};
use hamlet_sdk::plugin::Status;
use hamlet_sdk::reload::{Reload, Rollback};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};

use super::permissions::{answer, granted, petitions};
use super::tree::{PANEL, items, tree};
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::interface::VERSION;
use crate::reload::{Modified, Polling};

fn component(count: &'static str) -> Vec<u8> {
    fixture::component(&fixture::drawing(
        &items(count),
        &items(count),
    ))
}

fn opened(scratch: &Scratch, plugin: Plugin) -> App {
    let mut app = home::app(scratch, &[plugin]);
    app.world_mut().resource_mut::<Polling>().poll_ms = 0;
    app.update();
    app
}

fn shows(app: &App, count: &str) -> bool {
    app.world()
        .resource::<PluginTrees>()
        .shows(PANEL, &Drawn::Tree(tree(count, false)))
}

fn reload(app: &mut App) -> Reload {
    let mut reloads = app.world_mut().query::<&Reload>();
    reloads.single(app.world()).unwrap().clone()
}

fn settle(app: &mut App) {
    app.update();
    app.update();
}

#[test]
fn a_reload_takes_a_good_version_and_keeps_the_old_one_when_the_new_does_not_start()
 {
    let scratch = Scratch::new("reload");
    let file = scratch.plugins().join("probe/plugin.wasm");
    let mut app =
        opened(&scratch, Plugin::new("probe", component("first")));
    assert!(shows(&app, "first"));

    std::fs::write(&file, component("second")).unwrap();
    settle(&mut app);
    assert!(shows(&app, "second"));
    assert_eq!(reload(&mut app), Reload::default());

    std::fs::write(&file, b"not a component").unwrap();
    settle(&mut app);
    assert!(shows(&app, "second"));
    let mut status = app.world_mut().query::<&Status>();
    assert_eq!(
        status.single(app.world()).ok(),
        Some(&Status::Running)
    );
    let held = reload(&mut app);
    assert!(
        held.note.is_some_and(|why| why.contains("could not load"))
    );
    assert!(held.rollback);

    app.world_mut().trigger(Rollback {
        plugin: "probe".into(),
    });
    app.world_mut().flush();
    settle(&mut app);
    assert_eq!(std::fs::read(&file).unwrap(), component("second"));
    assert!(shows(&app, "second"));
    assert_eq!(reload(&mut app), Reload::default());
}

#[test]
fn a_file_saved_again_with_the_same_contents_is_not_reloaded() {
    let scratch = Scratch::new("reload_same");
    let file = scratch.plugins().join("probe/plugin.wasm");
    let mut app =
        opened(&scratch, Plugin::new("probe", component("first")));
    let mut cursor = app
        .world()
        .resource::<Messages<Modified>>()
        .get_cursor_current();
    let mut looked = |app: &mut App| {
        settle(app);
        let messages = app.world().resource::<Messages<Modified>>();
        cursor.read(messages).count()
    };

    std::fs::write(&file, component("first")).unwrap();
    assert_eq!(looked(&mut app), 0);

    std::fs::write(&file, component("second")).unwrap();
    assert_eq!(looked(&mut app), 1);
    assert!(shows(&app, "second"));
}

#[test]
fn a_new_version_that_asks_for_more_waits_for_the_answer_before_it_runs()
 {
    let scratch = Scratch::new("reload_asks");
    let folder = scratch.plugins().join("probe");
    let mut app =
        opened(&scratch, Plugin::new("probe", component("first")));
    assert!(shows(&app, "first"));

    std::fs::write(
        folder.join("manifest.toml"),
        format!(
            "id = \"probe\"\nname = \"probe\"\nversion = \
             \"0.2.0\"\ninterface = \
             \"{VERSION}\"\n[[asks]]\ncapability = \
             \"read.agents\"\nreason = \"a test\"\n"
        ),
    )
    .unwrap();
    std::fs::write(folder.join("plugin.wasm"), component("second"))
        .unwrap();
    settle(&mut app);

    assert!(shows(&app, "first"));
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(_, Subject::Update { capabilities, .. })]
            if capabilities == &[Capability::ReadAgents]
    ));
    assert!(reload(&mut app).note.is_some());

    answer(&mut app, asked[0].0, Answer::Allow);
    settle(&mut app);
    assert!(shows(&app, "second"));
    assert!(granted(&mut app).holds(Capability::ReadAgents));
    assert_eq!(reload(&mut app), Reload::default());
}
