use bevy::ecs::message::Messages;
use bevy::prelude::*;
use hamlet_sdk::agents::SelectedAgent;
use hamlet_sdk::capability::{Capability, Subject};
use hamlet_sdk::plugin::{Command, Disable};

use super::permissions::petitions;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::limits::Limits;
use crate::run::Said;

const IMPORTS: &str = "[imports]\ncommands = [\"target/add\"]\n";
const EXPORTS: &str = "[exports]\ncommands = [\"add\"]\n";

fn caller() -> Plugin {
    let module = fixture::commanding("target", "add", "ping");
    Plugin::new("caller", fixture::component(&module)).saying(IMPORTS)
}

fn target(module: String) -> Plugin {
    Plugin::new("target", fixture::component(&module)).saying(EXPORTS)
}

/// What the plugins logged in `frames` frames, each as the plugin and
/// the line.
pub(super) fn said(
    app: &mut App,
    frames: usize,
) -> Vec<(String, String)> {
    let mut cursor =
        app.world().resource::<Messages<Said>>().get_cursor();
    let mut lines = Vec::new();
    for _ in 0..frames {
        app.update();
        let messages = app.world().resource::<Messages<Said>>();
        lines.extend(
            cursor
                .read(messages)
                .map(|said| (said.plugin.clone(), said.text.clone())),
        );
    }
    lines
}

fn line(plugin: &str, text: &str) -> (String, String) {
    (plugin.into(), text.into())
}

#[test]
fn a_command_runs_in_the_plugin_that_exports_it_and_its_answer_comes_back()
 {
    let scratch = Scratch::new("interop_round");
    let mut app = home::app(
        &scratch,
        &[
            caller().allowed_for(&[("plugin.use", "target")]),
            target(fixture::answering()),
        ],
    );

    let lines = said(&mut app, 8);

    assert!(lines.contains(&line("caller", "ping")), "{lines:?}");
    let mut commands = app.world_mut().query::<&Command>();
    let declared = commands
        .iter(app.world())
        .map(|command| (command.id.clone(), command.title.clone()))
        .collect::<Vec<_>>();
    assert_eq!(declared, [("add".to_string(), "Add".to_string())]);
}

#[test]
fn a_command_is_refused_without_the_grant_and_the_chief_is_asked() {
    let scratch = Scratch::new("interop_denied");
    let mut app = home::app(
        &scratch,
        &[
            caller().asking(&["plugin.use"]),
            target(fixture::answering()),
        ],
    );

    let lines = said(&mut app, 8);

    assert!(
        lines.contains(&line("caller", "plugin.use not granted")),
        "{lines:?}"
    );
    assert!(petitions(&mut app).iter().any(|(_, subject)| {
        matches!(
            subject,
            Subject::Use {
                capability: Capability::PluginUse,
                target: Some(target),
                ..
            } if target == "target"
        )
    }));
}

const TICKS: &str = "[exports]\nevents = [\"ticked\"]\n";
const LISTENS: &str = "[imports]\nevents = [\"clock/ticked\"]\n";

fn listener(id: &'static str) -> Plugin {
    Plugin::new(id, fixture::component(&fixture::listening()))
        .saying(LISTENS)
}

#[test]
fn an_event_goes_to_each_subscriber_that_may_use_the_publisher() {
    let scratch = Scratch::new("interop_events");
    let module = fixture::publishing("ticked", "noon");
    let mut app = home::app(
        &scratch,
        &[
            Plugin::new("clock", fixture::component(&module))
                .saying(TICKS),
            listener("first").allowed_for(&[("plugin.use", "clock")]),
            listener("second")
                .allowed_for(&[("plugin.use", "clock")]),
            listener("third").asking(&["plugin.use"]),
        ],
    );

    let lines = said(&mut app, 6);

    assert!(lines.contains(&line("first", "noon")), "{lines:?}");
    assert!(lines.contains(&line("second", "noon")), "{lines:?}");
    assert!(!lines.iter().any(|(plugin, _)| plugin == "third"));
    assert!(petitions(&mut app).iter().any(|(_, subject)| matches!(
        subject,
        Subject::Use { plugin, target: Some(target), .. }
            if plugin == "third" && target == "clock"
    )));
}

#[test]
fn an_event_of_the_host_reaches_the_subscriber_that_holds_its_capability()
 {
    let scratch = Scratch::new("interop_host");
    let hears = |id: &'static str| {
        Plugin::new(id, fixture::component(&fixture::listening()))
            .saying(
                "[imports]\nevents = [\"host/selection-changed\"]\n",
            )
            .asking(&["read.agents"])
    };
    let mut app = home::app(
        &scratch,
        &[
            hears("granted").granted(&["read.agents"]),
            hears("refused"),
        ],
    );
    app.update();
    app.world_mut().resource_mut::<SelectedAgent>().0 =
        Some("Juniper".into());

    let lines = said(&mut app, 4);

    assert!(lines.contains(&line("granted", "Juniper")), "{lines:?}");
    assert!(!lines.iter().any(|(plugin, _)| plugin == "refused"));
}

#[test]
fn a_command_to_a_plugin_that_is_off_or_silent_is_answered_with_an_error()
 {
    let scratch = Scratch::new("interop_off");
    let mut app = home::app(
        &scratch,
        &[
            caller().allowed_for(&[("plugin.use", "target")]),
            target(fixture::answering()),
        ],
    );
    app.update();
    app.world_mut().trigger(Disable("target".into()));
    app.world_mut().flush();
    let lines = said(&mut app, 6);
    assert!(
        lines.contains(&line("caller", "`target` is switched off")),
        "{lines:?}"
    );

    let scratch = Scratch::new("interop_silent");
    let mut app = home::app(
        &scratch,
        &[
            caller().allowed_for(&[("plugin.use", "target")]),
            target(fixture::quiet()),
        ],
    );
    app.world_mut().resource_mut::<Limits>().call_frames = 5;
    let lines = said(&mut app, 12);
    assert!(
        lines.contains(&line(
            "caller",
            "`target` did not answer in 5 frames"
        )),
        "{lines:?}"
    );
}
