use bevy::prelude::*;
use bevy_fynix::mount;
use hamlet_sdk::plugin::{Enable, Status};
use hamlet_sdk::theme::HamletTheme;

use super::tree::{PANEL, items};
use crate::event::{Act, Interaction};
use crate::fixture::home::{self, Plugin, Scratch};
use crate::trees::{PluginTrees, Shape};
use crate::{fixture, view};

fn says(app: &mut App, said: &str) -> bool {
    let world = app.world_mut();
    world
        .query::<&bevy::ui::widget::Text>()
        .iter(world)
        .any(|text| text.0 == said)
}

#[test]
fn a_failed_plugin_shows_a_placeholder_that_restarts_it() {
    let scratch = Scratch::new("stopped");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "probe",
            fixture::component(&fixture::failing(&items(
                "0 refreshes",
            ))),
        )],
    );
    home::views(&mut app);
    app.update();
    mount::<HamletTheme>(app.world_mut(), view::panel(PANEL));
    app.update();
    app.update();
    assert!(says(&mut app, "Refresh"));

    Interaction::send(app.world_mut(), PANEL, "go", Act::Click);
    app.update();
    app.update();
    app.update();

    let shape = app.world().resource::<PluginTrees>().shape(PANEL);
    let Shape::Stopped(why) = shape else {
        panic!("the panel shows {shape:?}");
    };
    assert!(why.contains("trapped"), "{why}");
    assert!(says(&mut app, "This plugin stopped"));
    assert!(!says(&mut app, "Refresh"));

    app.world_mut().trigger(Enable("probe".into()));
    app.world_mut().flush();
    app.update();
    app.update();
    let mut plugins = app.world_mut().query::<&Status>();
    assert_eq!(
        plugins.single(app.world()).ok(),
        Some(&Status::Running)
    );
    assert!(says(&mut app, "Refresh"));
    assert!(!says(&mut app, "This plugin stopped"));
}
