use bevy::prelude::*;
use bevy_fynix::dock::DockRegistry;
use hamlet_core::oversight::{Settled, Waiting};
use hamlet_rules::chief::rulings::Choice as Answer;
use hamlet_sdk::capability::{
    Capability, Granted, Installation, Store, Subject, Verdict,
};
use hamlet_sdk::plugin::Status;
use hamlet_sdk::theme::HamletTheme;

use super::tree::items;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::instance::Instance;

fn listed(app: &App, id: &str) -> bool {
    app.world()
        .resource::<DockRegistry<HamletTheme>>()
        .get(id)
        .is_some()
}

/// What waits for the chief about a plugin, with what it is about.
fn petitions(app: &mut App) -> Vec<(Entity, Subject)> {
    app.world_mut()
        .query::<(Entity, &Waiting, &Subject)>()
        .iter(app.world())
        .map(|(entity, _, subject)| (entity, subject.clone()))
        .collect()
}

/// The chief answers what waits as `entity`.
fn answer(app: &mut App, entity: Entity, choice: Answer) {
    app.world_mut().trigger(Settled { entity, choice });
    app.world_mut().flush();
    app.update();
}

fn granted(app: &mut App) -> Granted {
    let mut held = app.world_mut().query::<&Granted>();
    held.single(app.world()).unwrap().clone()
}

/// A plugin with one panel, `main`.
fn component() -> Vec<u8> {
    fixture::component(&fixture::drawing(&items("a"), &items("b")))
}

fn probe() -> Plugin {
    Plugin::new("probe", component()).asking(&["read.agents"])
}

#[test]
fn a_plugin_that_is_not_installed_waits_for_the_chief_before_anything_of_it_runs()
 {
    let scratch = Scratch::new("install");
    home::put(&scratch, &[probe().uninstalled()]);
    let mut app = home::open(&scratch);
    app.update();

    let asked = petitions(&mut app);
    assert_eq!(
        asked.iter().map(|(_, s)| s).collect::<Vec<_>>(),
        [&Subject::Install {
            plugin: "probe".into()
        }]
    );
    assert!(!listed(&app, "probe/main"));
    let mut running = app.world_mut().query::<&Instance>();
    assert_eq!(running.iter(app.world()).count(), 0);

    answer(&mut app, asked[0].0, Answer::Allow);
    assert!(listed(&app, "probe/main"));
    assert!(granted(&mut app).holds(Capability::ReadAgents));
    let store = Store::open(&scratch.plugins());
    assert_eq!(
        store.get("probe").map(|record| record.state),
        Some(Installation::Installed)
    );

    // Declined, it stays out and is not asked about again.
    let scratch = Scratch::new("declined");
    home::put(&scratch, &[probe().uninstalled()]);
    let mut app = home::open(&scratch);
    app.update();
    let asked = petitions(&mut app);
    answer(&mut app, asked[0].0, Answer::Deny);
    assert!(!listed(&app, "probe/main"));
    let mut again = home::open(&scratch);
    again.update();
    assert!(petitions(&mut again).is_empty());
}

#[test]
fn an_update_that_asks_for_more_runs_on_what_it_had_and_asks_again() {
    let scratch = Scratch::new("update");
    let plugin = Plugin::new("probe", component())
        .asking(&["read.agents", "net"])
        .granted(&["read.agents"]);
    let mut app = home::app(&scratch, &[plugin]);
    app.update();

    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(_, Subject::Update { capabilities, .. })]
            if capabilities == &[Capability::Net]
    ));
    let held = granted(&mut app);
    assert!(held.holds(Capability::ReadAgents));
    assert_eq!(held.verdict(Capability::Net), Some(Verdict::Ask));
    let mut status = app.world_mut().query::<&Status>();
    assert_eq!(
        status.single(app.world()).ok(),
        Some(&Status::Running)
    );

    answer(&mut app, asked[0].0, Answer::Deny);
    assert_eq!(
        granted(&mut app).verdict(Capability::Net),
        Some(Verdict::Denied)
    );
}

#[test]
fn a_manifest_that_asks_for_what_is_never_granted_is_turned_down_without_a_petition()
 {
    let scratch = Scratch::new("never");
    let mut app = home::app(
        &scratch,
        &[Plugin::new("thief", component())
            .asking(&["keys"])
            .uninstalled()],
    );
    app.update();

    assert!(petitions(&mut app).is_empty());
    let mut plugins = app.world_mut().query::<&Granted>();
    assert_eq!(plugins.iter(app.world()).count(), 0);
    assert!(!listed(&app, "thief/main"));
}
