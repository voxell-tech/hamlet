use bevy::prelude::*;
use bevy_fynix::dock::DockRegistry;
use hamlet_core::oversight::{Settled, Waiting};
use hamlet_rules::chief::rulings::Choice as Answer;
use hamlet_sdk::agents::AgentList;
use hamlet_sdk::capability::{
    Asked, Asks, Capability, Choice, Choices, Granted, Installation,
    Scope, Store, Subject, Verdict,
};
use hamlet_sdk::plugin::Status;
use hamlet_sdk::theme::HamletTheme;

use super::tree::items;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::host::Want;
use crate::inbox::Inbox;
use crate::instance::Instance;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Intent,
};
use crate::limits::Limits;

fn listed(app: &App, id: &str) -> bool {
    app.world()
        .resource::<DockRegistry<HamletTheme>>()
        .get(id)
        .is_some()
}

/// What waits for the chief about a plugin, with what it is about.
pub(super) fn petitions(app: &mut App) -> Vec<(Entity, Subject)> {
    app.world_mut()
        .query::<(Entity, &Waiting, &Subject)>()
        .iter(app.world())
        .map(|(entity, _, subject)| (entity, subject.clone()))
        .collect()
}

/// The chief answers what waits as `entity`, which the core then
/// takes out of the queue.
fn settle(app: &mut App, entity: Entity, choice: Answer) {
    app.world_mut().trigger(Settled { entity, choice });
    app.world_mut().flush();
    app.world_mut().despawn(entity);
}

pub(super) fn answer(app: &mut App, entity: Entity, choice: Answer) {
    settle(app, entity, choice);
    app.update();
}

pub(super) fn granted(app: &mut App) -> Granted {
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

fn asks(capabilities: &[Capability]) -> Asks {
    Asks(
        capabilities
            .iter()
            .map(|&capability| Asked {
                capability,
                reason: "a test".into(),
                scope: Scope::default(),
            })
            .collect(),
    )
}

#[test]
fn a_request_is_answered_by_the_choice_and_only_a_named_capability_can_be_asked_for()
 {
    let limits = Limits::default();
    let mut plugin =
        super::load(&fixture::requesting("read.agents")).unwrap();
    let mut asked = |asks: &Asks, choices: &Choices| {
        plugin
            .see(&Granted::of(asks, choices), &AgentList::default());
        let out = plugin.step(&limits, &[]).unwrap();
        (out.intents, out.wants)
    };
    let named = asks(&[Capability::ReadAgents]);
    let on = |choice| {
        Choices::default().decided(
            Capability::ReadAgents,
            choice,
            false,
        )
    };
    let line = |text: &str| vec![Intent::LogLine(text.into())];

    assert_eq!(
        asked(&named, &on(Choice::Allow)),
        (line(""), Vec::new())
    );
    assert_eq!(
        asked(&named, &on(Choice::Deny)),
        (line("h"), Vec::new())
    );
    assert_eq!(
        asked(&named, &Choices::default()),
        (
            line("he"),
            vec![Want {
                capability: Capability::ReadAgents,
                reason: "why".into()
            }]
        )
    );
    // Not in the manifest: no matter what the chief chose.
    assert_eq!(
        asked(
            &asks(&[]),
            &Choices::allowing(&[Capability::ReadAgents])
        ),
        (line("h"), Vec::new())
    );
}

#[test]
fn a_refused_call_asks_the_chief_once_and_ask_next_time_holds_for_the_run()
 {
    let scratch = Scratch::new("later");
    let reader = || {
        Plugin::new("reader", fixture::component(&fixture::reading()))
            .asking(&["read.agents"])
            .asking_next_time(&["read.agents"])
    };
    let mut app = home::app(&scratch, &[reader()]);
    for _ in 0..3 {
        app.update();
    }
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(
            _,
            Subject::Use {
                capability: Capability::ReadAgents,
                ..
            }
        )]
    ));
    assert!(!granted(&mut app).holds(Capability::ReadAgents));

    settle(&mut app, asked[0].0, Answer::AskNextTime);
    assert!(granted(&mut app).holds(Capability::ReadAgents));
    let mut inbox = app.world_mut().query::<&mut Inbox>();
    let mail = inbox.single_mut(app.world_mut()).unwrap().take();
    assert!(mail.contains(&Incoming::Granted("read.agents".into())));
    for _ in 0..3 {
        app.update();
    }
    assert!(petitions(&mut app).is_empty());
    let record = Store::open(&scratch.plugins())
        .get("reader")
        .cloned()
        .unwrap();
    assert_eq!(
        record.choices.get("read.agents"),
        Some(&Choice::AskNextTime)
    );

    // The next launch asks again.
    let mut again = home::open(&scratch);
    again.update();
    again.update();
    assert!(!granted(&mut again).holds(Capability::ReadAgents));
    assert_eq!(petitions(&mut again).len(), 1);
}
