use std::path::PathBuf;

use bevy::prelude::*;
use hamlet_core::agents::{Citizen, Left, LivesIn, Settlement};
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::capability::{Capability, Granted, Verdict};
use hamlet_sdk::papers::Papers;

use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::host::Logged;
use crate::interface::exports::hamlet::plugin::plugin::Intent;
use crate::interface::hamlet::plugin::host::Level;
use crate::limits::Limits;

fn agent(name: &str, hamlet: &str) -> Agent {
    Agent {
        name: name.into(),
        hamlet: hamlet.into(),
    }
}

#[test]
fn a_read_needs_its_grant_and_a_change_of_grant_applies_at_once() {
    let limits = Limits::default();
    let mut plugin = super::load(&fixture::reading()).unwrap();
    let agents =
        AgentList(vec![agent("ada", "home"), agent("bo", "home")]);
    let granted = Granted::allowing(&[Capability::ReadAgents]);
    let lines = |plugin: &mut crate::instance::Instance| {
        let out = plugin.step(&limits, &[]).unwrap();
        (out.intents, out.logs)
    };
    let denied = (
        vec![Intent::LogLine(String::new())],
        vec![Logged {
            level: Level::Error,
            message: "read.agents not granted".into(),
        }],
    );

    let papers = Papers::default();
    plugin.see(&Granted::default(), &agents, &papers);
    assert_eq!(lines(&mut plugin), denied);

    plugin.see(&granted, &agents, &papers);
    assert_eq!(
        lines(&mut plugin),
        (vec![Intent::LogLine("he".into())], Vec::new())
    );

    plugin.see(&Granted::default(), &agents, &papers);
    assert_eq!(lines(&mut plugin), denied);
}

#[test]
fn the_papers_are_read_with_their_own_grant_and_only_as_a_copy() {
    let limits = Limits::default();
    let mut plugin = super::load(&fixture::reading_papers()).unwrap();
    let papers = Papers {
        notes: "tide table".into(),
        ..Papers::default()
    };
    let agents = AgentList::default();

    plugin.see(
        &Granted::allowing(&[Capability::ReadAgents]),
        &agents,
        &papers,
    );
    let out = plugin.step(&limits, &[]).unwrap();
    assert_eq!(
        out.logs,
        [Logged {
            level: Level::Error,
            message: "read.papers not granted".into(),
        }]
    );

    plugin.see(
        &Granted::allowing(&[Capability::ReadPapers]),
        &agents,
        &papers,
    );
    let out = plugin.step(&limits, &[]).unwrap();
    assert_eq!(out.intents, [Intent::LogLine("hello worl".into())]);
}

#[test]
fn the_agent_list_follows_the_core_and_grants_stop_at_what_was_asked()
{
    let scratch = Scratch::new("reads");
    let mut app = home::app(
        &scratch,
        &[Plugin::new(
            "reader",
            fixture::component(&fixture::reading()),
        )
        .asking(&["read.agents"])
        .granted(&["read.agents", "net"])],
    );
    app.update();
    let mut held = app.world_mut().query::<&Granted>();
    let held = held.single(app.world()).unwrap();
    assert_eq!(
        held.all(),
        [(Capability::ReadAgents, Verdict::Allowed)]
    );

    let hamlet = app
        .world_mut()
        .spawn((
            Name::new("Pear Tree"),
            Settlement {
                home: PathBuf::new(),
                name: "home".into(),
            },
        ))
        .id();
    let ada = app
        .world_mut()
        .spawn((Name::new("ada"), Citizen(1), LivesIn(hamlet)))
        .id();
    app.update();
    assert_eq!(
        app.world().resource::<AgentList>().0,
        [agent("ada", "Pear Tree")]
    );

    app.world_mut().entity_mut(ada).insert(Left);
    app.update();
    assert!(app.world().resource::<AgentList>().0.is_empty());
}
