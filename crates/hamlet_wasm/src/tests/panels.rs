use bevy::prelude::*;
use bevy_fynix::dock::DockRegistry;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::plugin::{Disable, Enable};
use hamlet_sdk::theme::HamletTheme;

use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};

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
        &[Plugin {
            id: "probe",
            wasm: fixture::component(&fixture::declaring()),
        }],
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
