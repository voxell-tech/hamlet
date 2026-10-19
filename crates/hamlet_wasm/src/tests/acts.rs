use bevy::prelude::*;
use hamlet_agents_api::oversight::Choice as Answer;
use hamlet_realm_api::pick::{Gesture, Named, PointAt};
use hamlet_realm_api::survey::{Survey, SurveyedAgent};
use hamlet_sdk::capability::{Capability, Subject};
use hamlet_sdk::command::{Notice, RunCommand};
use hamlet_sdk::plugin::PluginInfo;

use super::permissions::{answer, petitions};
use super::tree::items;
use crate::fixture;
use crate::fixture::home::{self, Plugin, Scratch};
use crate::interface::exports::hamlet::plugin::plugin::{
    ChatMessage, Intent, WorldOrder, WorldTarget,
};
use crate::run::Done;

/// The commands that were run as the chief, and the notices told.
#[derive(Resource, Default)]
struct Seen {
    ran: Vec<String>,
    told: Vec<String>,
}

pub(super) fn probe() -> Plugin {
    Plugin::new(
        "probe",
        fixture::component(&fixture::drawing(
            &items("a"),
            &items("b"),
        )),
    )
}

pub(super) fn open(scratch: &Scratch, plugin: Plugin) -> App {
    let mut app = home::app(scratch, &[plugin]);
    app.init_resource::<Seen>()
        .add_observer(
            |run: On<RunCommand>, mut seen: ResMut<Seen>| {
                seen.ran.push(run.id.clone());
            },
        )
        .add_observer(|note: On<Notice>, mut seen: ResMut<Seen>| {
            seen.told.push(note.0.clone());
        });
    app.update();
    app
}

/// The plugin does `intents` in a frame.
pub(super) fn does(app: &mut App, intents: Vec<Intent>) {
    let plugin = app
        .world_mut()
        .query::<(Entity, &PluginInfo)>()
        .iter(app.world())
        .find(|(_, info)| info.id == "probe")
        .map(|(plugin, _)| plugin)
        .unwrap();
    app.world_mut().write_message(Done {
        plugin,
        intents,
        changes: Vec::new(),
    });
    app.update();
}

fn run(command: &str) -> Vec<Intent> {
    vec![Intent::RunAsChief(command.into())]
}

fn ran(app: &App) -> Vec<String> {
    app.world().resource::<Seen>().ran.clone()
}

#[test]
fn an_act_as_the_chief_waits_for_the_chief_and_is_noted_with_the_plugin()
 {
    let scratch = Scratch::new("act_asked");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["commands.run"])
            .asking_next_time(&["commands.run"])
            .limited_to(&[("commands.run", &["notes/new"])]),
    );

    does(&mut app, run("notes/new"));
    assert!(ran(&app).is_empty());
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(
            _,
            Subject::Use {
                capability: Capability::CommandsRun,
                ..
            }
        )]
    ));

    answer(&mut app, asked[0].0, Answer::Allow);
    does(&mut app, run("notes/new"));
    assert_eq!(ran(&app), ["notes/new"]);
    let told = app.world().resource::<Seen>().told.clone();
    assert_eq!(told[0], "probe (probe) ran notes/new");
}

#[test]
fn acts_are_limited_to_the_scope_of_the_ask_and_to_so_many_a_minute()
{
    let scratch = Scratch::new("act_limited");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["commands.run"])
            .granted(&["commands.run"])
            .limited_to(&[("commands.run", &["notes/new"])]),
    );

    does(&mut app, run("other/delete"));
    assert!(ran(&app).is_empty());
    assert!(petitions(&mut app).is_empty());

    for _ in 0..10 {
        does(&mut app, run("notes/new"));
    }
    let most =
        crate::limits::Limits::default().acts_per_minute as usize;
    assert_eq!(ran(&app).len(), most);
}

#[test]
fn an_ask_with_no_scope_reaches_nothing() {
    let scratch = Scratch::new("act_unscoped");
    let mut app = open(
        &scratch,
        probe().asking(&["commands.run"]).granted(&["commands.run"]),
    );

    does(&mut app, run("notes/new"));
    assert!(ran(&app).is_empty());
}

#[test]
fn a_plugin_that_does_not_ask_cannot_act_and_is_not_asked_about() {
    let scratch = Scratch::new("act_unnamed");
    let mut app = open(&scratch, probe());

    does(&mut app, run("notes/new"));
    does(
        &mut app,
        vec![Intent::SendChat(ChatMessage {
            agent: "scout".into(),
            text: "hello".into(),
        })],
    );

    assert!(ran(&app).is_empty());
    assert!(app.world().resource::<Seen>().told.is_empty());
    assert!(petitions(&mut app).is_empty());
}

#[test]
fn a_click_on_the_world_waits_for_the_chief_and_reaches_only_the_hamlets_asked_for()
 {
    let scratch = Scratch::new("act_world");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["world.act"])
            .asking_next_time(&["world.act"])
            .limited_to(&[("world.act", &["home"])]),
    );
    let agent = |name: &str, hamlet: &str| SurveyedAgent {
        name: name.into(),
        hamlet: hamlet.into(),
        at: None,
        busy: false,
    };
    app.insert_resource(Survey {
        agents: vec![agent("Juniper", "home"), agent("Hawk", "far")],
        ..Survey::default()
    })
    .init_resource::<Pointed>()
    .add_observer(
        |point: On<PointAt>, mut seen: ResMut<Pointed>| {
            seen.0.push(point.event().clone());
        },
    );
    let focus = |name: &str| {
        vec![Intent::WorldAct(WorldOrder::Focus(WorldTarget::Agent(
            name.into(),
        )))]
    };
    let pointed =
        |app: &App| app.world().resource::<Pointed>().0.clone();

    does(&mut app, focus("Juniper"));
    assert!(pointed(&app).is_empty());
    let asked = petitions(&mut app);
    assert!(matches!(
        &asked[..],
        [(
            _,
            Subject::Use {
                capability: Capability::WorldAct,
                ..
            }
        )]
    ));

    answer(&mut app, asked[0].0, Answer::Allow);
    does(&mut app, focus("Juniper"));
    does(&mut app, focus("Hawk"));
    assert_eq!(
        pointed(&app),
        [PointAt {
            named: Named::Agent("Juniper".into()),
            gesture: Gesture::Focus,
        }]
    );
    let told = app.world().resource::<Seen>().told.clone();
    assert_eq!(told, ["probe (probe) focused on Juniper"]);
}

/// What the plugin pointed the realm at.
#[derive(Resource, Default)]
struct Pointed(Vec<PointAt>);

#[test]
fn a_message_over_the_cap_is_not_sent() {
    let scratch = Scratch::new("act_long");
    let mut app = open(
        &scratch,
        probe()
            .asking(&["chat.send"])
            .granted(&["chat.send"])
            .limited_to(&[("chat.send", &["scout"])]),
    );
    let message = |text: String| {
        vec![Intent::SendChat(ChatMessage {
            agent: "scout".into(),
            text,
        })]
    };

    does(&mut app, message("x".repeat(5000)));
    assert!(app.world().resource::<Seen>().told.is_empty());

    // Within the cap it goes on to the chat, which is not there to
    // take it in this app.
    does(&mut app, message("hello".into()));
    let told = app.world().resource::<Seen>().told.clone();
    assert_eq!(told.len(), 1);
    assert!(
        told[0].starts_with("probe (probe) sent nothing"),
        "{told:?}"
    );
}
