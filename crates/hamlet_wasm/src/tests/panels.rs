use bevy::prelude::*;
use bevy_fynix::dock::{
    DockAreaStyle, DockLeaf, DockRegistry, DockTree, Edge,
};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::plugin::{Disable, Enable};
use hamlet_sdk::theme::HamletTheme;
use hamlet_sdk::tree::store::PluginTrees;

use super::tree::{PANEL, items};
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::limits::Limits;

fn listed(app: &App, id: &str) -> bool {
    app.world()
        .resource::<DockRegistry<HamletTheme>>()
        .get(id)
        .is_some()
}

#[test]
fn a_declared_panel_lives_as_long_as_its_plugin_is_enabled() {
    let scratch = Scratch::new("declared");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::declaring()),
        )],
    );
    app.update();
    assert!(listed(&app, "probe/notes"));
    assert!(listed(&app, "probe/again"));
    // The same id twice is one panel.
    let mut panels = app.world_mut().query::<&Panel>();
    assert_eq!(panels.iter(app.world()).count(), 2);
    assert!(!listed(&app, "notes"));

    app.world_mut().trigger(Disable("probe".into()));
    app.world_mut().flush();
    assert!(!listed(&app, "probe/notes"));

    app.world_mut().trigger(Enable("probe".into()));
    app.world_mut().flush();
    assert!(listed(&app, "probe/notes"));
}

/// Whether the dock holds the panel `probe/main` after a plugin
/// asked to open the panel `asked`.
fn opened(name: &str, asked: &str) -> bool {
    let scratch = Scratch::new(name);
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::opening(asked)),
        )],
    );
    let mut tree = DockTree::default();
    let centre = tree.set_root_leaf(
        DockLeaf::new("centre", DockAreaStyle::TabBar)
            .with_windows(vec!["world".into()]),
    );
    tree.split(centre, Edge::Right, "agent".into());
    app.insert_resource(tree);
    for _ in 0..4 {
        app.update();
    }
    let tree = app.world().resource::<DockTree>();
    tree.find_leaf_with_window("probe/main").is_some()
}

#[test]
fn a_plugin_opens_its_own_panel_and_no_other() {
    assert!(opened("opens_own", "main"));
    assert!(!opened("opens_other", "agent"));
}

#[test]
fn a_panel_is_drawn_in_a_frame_the_steps_spent() {
    let scratch = Scratch::new("starved");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::drawing(
                &items("0 refreshes"),
                &items("0 refreshes"),
            )),
        )],
    );
    app.insert_resource(Limits {
        frame_ms: 0,
        ..Limits::default().patient()
    });
    app.update();
    assert!(app.world().resource::<PluginTrees>().contains(PANEL));
}

#[test]
fn a_data_plugin_is_a_panel_drawn_from_its_file_and_enabled_like_any()
{
    let scratch = Scratch::new("data");
    let mut app = home::app(
        &scratch,
        &[Plugin::data("note", "root = Column([Label(\"Hi\")])")],
    );
    app.update();
    app.update();
    assert!(listed(&app, "note/main"));
    let trees = |app: &App| {
        app.world().resource::<PluginTrees>().contains("note/main")
    };
    assert!(trees(&app));

    app.world_mut().trigger(Disable("note".into()));
    app.world_mut().flush();
    assert!(!listed(&app, "note/main"));

    app.world_mut().trigger(Enable("note".into()));
    app.world_mut().flush();
    app.update();
    assert!(listed(&app, "note/main"));
    assert!(trees(&app));
}
