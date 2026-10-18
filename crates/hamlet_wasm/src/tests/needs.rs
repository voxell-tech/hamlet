use bevy::prelude::*;
use bevy_fynix::dock::DockRegistry;
use hamlet_core::oversight::Waiting;
use hamlet_sdk::capability::AskToRemove;
use hamlet_sdk::plugin::{
    Blocked, Disable, Enable, PluginInfo, Status,
};
use hamlet_sdk::theme::HamletTheme;

use super::tree::items;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::install::{self, Why};

fn plugin(id: &'static str) -> Plugin {
    let wasm = fixture::component(&fixture::drawing(
        &items("a"),
        &items("b"),
    ));
    Plugin::new(id, wasm)
}

fn user() -> Plugin {
    plugin("user").needing(&[("base", ">=0.1, <1")])
}

/// The status of the plugin `id` and why it is held back.
fn standing(app: &mut App, id: &str) -> (Status, Option<String>) {
    let mut plugins =
        app.world_mut()
            .query::<(&PluginInfo, &Status, Option<&Blocked>)>();
    plugins
        .iter(app.world())
        .find(|(info, ..)| info.id == id)
        .map(|(_, status, blocked)| {
            (*status, blocked.map(|blocked| blocked.0.clone()))
        })
        .unwrap()
}

fn frames(app: &mut App) {
    (0..3).for_each(|_| app.update());
}

fn listed(app: &App, id: &str) -> bool {
    app.world()
        .resource::<DockRegistry<HamletTheme>>()
        .get(id)
        .is_some()
}

#[test]
fn a_plugin_starts_when_what_it_needs_runs_and_goes_off_and_on_with_it()
 {
    let scratch = Scratch::new("needs_missing");
    let mut app = home::app(&scratch, &[user()]);
    frames(&mut app);
    let (status, why) = standing(&mut app, "user");
    assert_eq!(status, Status::Disabled);
    assert_eq!(
        why.as_deref(),
        Some("needs `base`, which is not here")
    );
    assert!(!listed(&app, "user/main"));

    let scratch = Scratch::new("needs_both");
    let mut app = home::app(&scratch, &[plugin("base"), user()]);
    frames(&mut app);
    assert_eq!(standing(&mut app, "user"), (Status::Running, None));
    assert!(listed(&app, "user/main"));

    app.world_mut().trigger(Disable("base".into()));
    frames(&mut app);
    let (status, why) = standing(&mut app, "user");
    assert_eq!(status, Status::Disabled);
    assert_eq!(
        why.as_deref(),
        Some("needs `base`, which is switched off")
    );
    assert!(!listed(&app, "user/main"));

    app.world_mut().trigger(Enable("base".into()));
    frames(&mut app);
    assert_eq!(standing(&mut app, "user"), (Status::Running, None));
    assert!(listed(&app, "user/main"));
}

#[test]
fn needs_in_a_circle_or_in_the_wrong_version_are_turned_down_or_held_back()
 {
    let scratch = Scratch::new("needs_circle");
    home::put(
        &scratch,
        &[
            plugin("a").needing(&[("b", "*")]),
            plugin("b").needing(&[("a", "*")]),
            plugin("c"),
        ],
    );
    let found = install::scan(&scratch.plugins());
    let circles = found
        .iter()
        .filter_map(|found| found.as_ref().err())
        .map(|rejected| rejected.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        circles,
        [
            "plugins/a: its needs go in a circle: a -> b -> a",
            "plugins/b: its needs go in a circle: b -> a -> b"
        ]
    );
    assert!(matches!(
        found[0].as_ref().unwrap_err().why,
        Why::Cycle(_)
    ));

    let scratch = Scratch::new("needs_version");
    let fussy = plugin("fussy").needing(&[("base", ">=2")]);
    let mut app = home::app(&scratch, &[plugin("base"), fussy]);
    frames(&mut app);
    assert_eq!(
        standing(&mut app, "fussy").1.as_deref(),
        Some("needs `base` >=2, but 0.1.0 is installed")
    );
}

#[test]
fn the_petitions_to_install_and_to_remove_name_the_plugins_they_touch()
 {
    let scratch = Scratch::new("needs_petitions");
    let mut app = home::app(&scratch, &[user().uninstalled()]);
    frames(&mut app);
    let about = |app: &mut App| {
        let mut waiting = app.world_mut().query::<&Waiting>();
        waiting
            .iter(app.world())
            .map(|waiting| waiting.about.clone())
            .collect::<Vec<_>>()
    };
    let asked = about(&mut app);
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].contains("It needs `base`, which is not here."),
        "{}",
        asked[0]
    );

    let scratch = Scratch::new("needs_remove");
    let mut app = home::app(&scratch, &[plugin("base"), user()]);
    frames(&mut app);
    app.world_mut().trigger(AskToRemove {
        plugin: "base".into(),
    });
    app.world_mut().flush();
    let asked = about(&mut app);
    assert!(
        asked[0].contains("Switched off with it: user."),
        "{}",
        asked[0]
    );
}
