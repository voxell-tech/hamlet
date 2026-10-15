//! The line at the foot of the window, and the words it is made
//! of.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Query, Res, ResMut};
use bevy::state::condition::in_state;
use bevy::ui::{AlignItems, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, TooltipExt, column, frame, icon, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::Connection;
use hamlet_client::chats::{Status, Usage};
use hamlet_client::connection::State as ConnectionState;
use hamlet_client::realm::{
    HOME, Sandbox, Settings, Settlement, Treasury,
};
use hamlet_protocol::v0;

use crate::icons::{Glyph, glyph};
use crate::links::Aimed;
use crate::looks::{hairline, note};
use crate::me::Me;
use crate::needs::Queue;
use crate::window::Layout;
use crate::{AnyView, Cx, View};

/// The side of an icon in the bar.
const MARK: f32 = 12.0;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Bar>()
        .init_resource::<Lately>()
        .add_systems(
            Update,
            (
                update_status,
                update_lately.run_if(in_state(Layout::Minimal)),
            ),
        );
}

/// The words of the status bar and the minimal window.
#[derive(Resource, Default, PartialEq)]
pub struct Bar {
    /// The open chat's agent has a turn running.
    pub busy: bool,
    pub running: String,
    pub running_tone: Tone,
    pub model: String,
    pub link: String,
    pub link_tone: Tone,
    /// Tokens and cost (or model time) of the current session.
    pub usage: String,
    /// The hamlet's spend against its budget.
    pub budget: String,
}

pub(crate) fn update_status(
    me: Res<Me>,
    sessions: Query<(Ref<Status>, Ref<Usage>)>,
    settings: Res<Settings>,
    connection: Res<Connection>,
    treasury: Query<(&Settlement, Ref<Treasury>)>,
    mut bar: ResMut<Bar>,
) {
    let session = sessions.get(me.session).ok();
    let home = treasury
        .iter()
        .find(|(settlement, _)| settlement.name == HOME)
        .map(|(_, treasury)| treasury);
    let moved = me.is_changed()
        || settings.is_changed()
        || connection.is_changed()
        || session.as_ref().is_some_and(|(status, usage)| {
            status.is_changed() || usage.is_changed()
        })
        || home.as_ref().is_some_and(|spent| spent.is_changed());
    if !moved {
        return;
    }
    let busy = session.as_ref().is_some_and(|(s, _)| s.working());
    let next = Bar {
        busy,
        running: if busy {
            "Working".into()
        } else {
            "Running, idle".into()
        },
        running_tone: if busy { Tone::Accent } else { Tone::Dim },
        model: settings.0.model.clone(),
        link: if connection.state == ConnectionState::Connected {
            settings.0.link.words.clone().unwrap_or_default()
        } else {
            connection.words.clone()
        },
        link_tone: match connection.state {
            ConnectionState::Lost | ConnectionState::Refused => {
                Tone::Critical
            }
            ConnectionState::Connected
                if settings.0.link.state == v0::LinkState::Failed =>
            {
                Tone::Critical
            }
            _ => Tone::Dim,
        },
        usage: session.map(|(_, u)| u.summary()).unwrap_or_default(),
        budget: home.map(|t| t.0.summary.clone()).unwrap_or_default(),
    };
    if *bar != next {
        *bar = next;
    }
}

/// The last thing the open chat's agent did and how long ago, as the
/// minimal window says it.
#[derive(Resource, Default, PartialEq)]
pub(crate) struct Lately(pub String);

pub(crate) fn update_lately(
    me: Res<Me>,
    sessions: Query<&Status>,
    mut lately: ResMut<Lately>,
    mut seen: Local<Option<(u64, u64)>>,
) {
    let last = sessions
        .get(me.session)
        .ok()
        .and_then(|status| status.last.as_ref());
    // The words change with the second at the soonest.
    let now = last
        .map(|(at, _)| (*at, now_ms().saturating_sub(*at) / 1000));
    if *seen == now && !lately.0.is_empty() {
        return;
    }
    *seen = now;
    let next = match last {
        Some((at, what)) => format!("{what}, {}", ago(*at)),
        None => "None yet".into(),
    };
    if lately.0 != next {
        lately.0 = next;
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ago(since: u64) -> String {
    let s = now_ms().saturating_sub(since) / 1000;
    match s {
        0..=4 => "just now".into(),
        5..=59 => format!("{s}s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

/// An icon and the words it stands before.
fn pair(
    mark: impl View + 'static,
    words: impl View + 'static,
) -> impl View + 'static {
    row((mark, words)).gap(5.0).align(AlignItems::Center)
}

/// One of the bar's facts: an icon and what `text` reads, with `tip`
/// to name it under the pointer. Nothing while there is nothing to
/// say.
fn fact(
    which: Glyph,
    text: fn(&Bar) -> String,
    tip: &'static str,
) -> AnyView {
    keyed(
        resource::<Bar, _>(move |bar| text(bar).is_empty()),
        move |empty| {
            if *empty {
                return frame().boxed();
            }
            pair(
                glyph(which, MARK).tone(Tone::Dim),
                note(resource::<Bar, _>(text)),
            )
            .tooltip(move || label(tip).size(12.0))
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

/// The number that wait for the chief, behind a bell. Nothing while
/// none does.
fn waiting() -> impl View {
    keyed(resource::<Queue, _>(Queue::len), |waiting| match waiting {
        0 => frame().boxed(),
        n => pair(
            glyph(Glyph::Bell, MARK).tone(Tone::Accent),
            label(format!("{n} waiting for you"))
                .size(12.0)
                .bold(true)
                .tone(Tone::Accent),
        )
        .boxed(),
    })
    .within(row(()).gap(0.0))
}

/// The place the link under the pointer leads, in the room between
/// the bar's two ends.
fn aimed() -> impl View {
    row((label(resource::<Aimed, _>(|aimed| aimed.address.clone()))
        .size(12.0)
        .wrap(false)
        .tone(Tone::Accent),))
    .grow(1.0)
    .min_width(px(0.0))
    .overflow(Overflow::clip())
}

/// The bar along the foot of the window.
pub fn status_bar() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let palette = &cx.theme().palette;
        cx.build(
            column((
                hairline(palette.seam),
                row((
                    pair(
                        glyph(Glyph::Shield, MARK).tone(Tone::Dim),
                        note(resource::<Sandbox, _>(|s| s.0.clone())),
                    )
                    .tooltip(|| {
                        label("The sandbox agents' commands run in")
                            .size(12.0)
                    }),
                    fact(
                        Glyph::Chip,
                        |bar| bar.model.clone(),
                        "Model",
                    ),
                    pair(
                        glyph(Glyph::Signal, MARK).tone(resource::<
                            Bar,
                            _,
                        >(
                            |bar| bar.link_tone,
                        )),
                        label(resource::<Bar, _>(|s| s.link.clone()))
                            .size(12.0)
                            .tone(resource::<Bar, _>(|bar| {
                                bar.link_tone
                            })),
                    ),
                    fact(
                        Glyph::Coins,
                        |bar| bar.usage.clone(),
                        "Used by this session",
                    ),
                    fact(
                        Glyph::Meter,
                        |bar| bar.budget.clone(),
                        "Spent of the hamlet's budget",
                    ),
                    aimed(),
                    waiting(),
                    pair(
                        icon(resource::<Bar, _>(|bar| {
                            let which = if bar.busy {
                                Glyph::Bolt
                            } else {
                                Glyph::Moon
                            };
                            which.image(MARK)
                        }))
                        .size(MARK)
                        .tone(resource::<
                            Bar,
                            _,
                        >(
                            |s| s.running_tone,
                        )),
                        label(resource::<Bar, _>(|s| {
                            s.running.clone()
                        }))
                        .size(12.0)
                        .bold(true)
                        .tone(resource::<
                            Bar,
                            _,
                        >(
                            |s| s.running_tone,
                        )),
                    ),
                ))
                .gap(18.0)
                .align(AlignItems::Center)
                .padding(UiRect::axes(px(16.0), px(5.0)))
                .fill(palette.black),
            ))
            .gap(0.0),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::me::tests::{panel_app, words};

    #[test]
    fn the_minimal_window_shows_the_chats_seconds_ago() {
        let mut app = panel_app();
        app.init_resource::<Bar>()
            .init_resource::<Lately>()
            .add_systems(Update, update_lately);
        let chat = app
            .world()
            .get::<hamlet_client::chats::Chat>(
                app.world().resource::<Me>().session,
            )
            .unwrap()
            .0
            .clone();
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Turn(v0::Turn {
                chat,
                number: Some(1),
                ops: vec![v0::Op::Status {
                    working: false,
                    pending: 0,
                    last: Some(v0::OpStatusLast {
                        at: (now_ms() - 12_000) as i64,
                        what: "Finished the plan".into(),
                    }),
                }],
            }),
        );
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            crate::window::minimal(),
        );
        for _ in 0..3 {
            app.update();
        }
        let shown = words(&mut app);
        let ago = shown
            .iter()
            .find_map(|text| {
                text.strip_prefix(
                    "Last activity: Finished the plan, ",
                )
            })
            .unwrap_or_else(|| panic!("{shown:?}"));
        let seconds = ago
            .strip_suffix("s ago")
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!((12..=17).contains(&seconds), "{ago}");
    }

    #[test]
    fn the_minimal_window_shows_recorded_live_late_and_reset_activity()
     {
        for recording in [
            include_str!("../../../server/test/frames/paging.jsonl"),
            include_str!("../../../server/test/frames/turn.jsonl"),
            include_str!(
                "../../../server/test/frames/new-and-resume.jsonl"
            ),
            include_str!("../../../server/test/frames/rewind.jsonl"),
        ] {
            let mut app = panel_app();
            app.init_resource::<Bar>()
                .init_resource::<Lately>()
                .add_systems(Update, update_lately);
            bevy_fynix::mount::<crate::theme::HamletTheme>(
                app.world_mut(),
                crate::window::minimal(),
            );
            for _ in 0..3 {
                app.update();
            }
            assert!(
                words(&mut app)
                    .iter()
                    .any(|text| text == "Last activity: None yet")
            );
            for line in recording.lines() {
                let frame =
                    serde_json::from_str::<serde_json::Value>(line)
                        .unwrap();
                if frame["from"] != "server" {
                    continue;
                }
                let message =
                    serde_json::from_value::<v0::ServerMessage>(
                        frame["message"].clone(),
                    )
                    .unwrap();
                let expected = match &message {
                    v0::ServerMessage::Transcript(page) => {
                        page.last.as_ref().map(|last| {
                            last.as_ref()
                                .map(|last| last.what.clone())
                        })
                    }
                    v0::ServerMessage::Turn(turn) => turn
                        .ops
                        .iter()
                        .filter_map(|op| match op {
                            v0::Op::Status { last, .. } => Some(
                                last.as_ref()
                                    .map(|last| last.what.clone()),
                            ),
                            _ => None,
                        })
                        .next_back(),
                    _ => None,
                };
                hamlet_client::receive(app.world_mut(), message);
                for _ in 0..3 {
                    app.update();
                }
                if let Some(expected) = expected {
                    let expected = expected.map_or_else(
                        || "Last activity: None yet".into(),
                        |what| format!("Last activity: {what}, "),
                    );
                    let shown = words(&mut app);
                    assert!(
                        shown
                            .iter()
                            .any(|text| text.starts_with(&expected)),
                        "{shown:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_mounted_bar_follows_received_sandbox_changes() {
        use bevy_fynix::FynixPlugin;
        use hamlet_assets::Tuning;

        use crate::theme::HamletTheme;

        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            hamlet_client::ClientPlugin { server: None },
            FynixPlugin::<HamletTheme>::default(),
            crate::me::plugin,
        ));
        crate::theme::insert_theme(
            app.world_mut(),
            crate::theme::HamletTheme::designed(),
        );
        app.init_resource::<Bar>()
            .init_resource::<Queue>()
            .init_resource::<crate::links::Aimed>()
            .add_systems(Update, update_status);
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            status_bar(),
        );
        app.update();
        for sandbox in ["seatbelt", "bubblewrap"] {
            let mut state = hamlet_testing::realm::state();
            state.sandbox = sandbox.into();
            hamlet_client::receive(
                app.world_mut(),
                v0::ServerMessage::State(state),
            );
            for _ in 0..3 {
                app.update();
            }
            let shown = words(&mut app);
            assert!(
                shown.iter().any(|text| text == sandbox),
                "{shown:?}"
            );
        }
    }

    #[test]
    fn the_bar_shows_received_budget_usage_model_link_and_waits() {
        let mut app = panel_app();
        app.insert_resource(Connection {
            state: ConnectionState::Connected,
            words: String::new(),
        });
        let mut state = hamlet_testing::realm::state();
        state.hamlets[0].budget.summary =
            "Orchard today: $1.20 of $20.00".into();
        state.settings.model = "server::orchard".into();
        state.settings.link.words =
            Some("A received link status".into());
        state.waits.push(hamlet_testing::realm::wait("approval"));
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::State(state),
        );
        let usage = serde_json::json!({
            "calls": 1,
            "unreported": 0,
            "input": 10,
            "cached": 0,
            "output": 2,
            "reasoning": 0,
            "ms": 1,
            "micros": 5,
            "lastInput": 10,
            "summary": "A received usage summary",
        });
        let usage = serde_json::from_value::<v0::UsageView>(usage)
            .expect("received usage");
        let session = app.world().resource::<Me>().session;
        app.world_mut().entity_mut(session).insert(Usage(usage));
        app.init_resource::<Bar>()
            .init_resource::<crate::links::Aimed>()
            .add_plugins(crate::needs::plugin)
            .add_systems(Update, update_status);
        app.update();
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            status_bar(),
        );
        app.update();
        let shown = words(&mut app);
        for text in [
            "Orchard today: $1.20 of $20.00",
            "A received usage summary",
            "server::orchard",
            "A received link status",
            "1 waiting for you",
        ] {
            assert!(
                shown.iter().any(|shown| shown == text),
                "missing {text}: {shown:?}"
            );
        }
    }
    #[test]
    fn the_bar_follows_link_changes_and_shows_connection_words_until_connected()
     {
        use hamlet_client::Connection;
        use hamlet_client::connection::State;
        let mut app = panel_app();
        app.init_resource::<Bar>()
            .init_resource::<Queue>()
            .init_resource::<crate::links::Aimed>()
            .add_systems(Update, update_status);
        app.insert_resource(Connection {
            state: State::Connected,
            words: "Connected to the realm.".into(),
        });
        bevy_fynix::mount::<crate::theme::HamletTheme>(
            app.world_mut(),
            status_bar(),
        );
        let mut settings = hamlet_testing::realm::state().settings;
        settings.model = "server::orchard".into();
        settings.link = v0::Link {
            state: v0::LinkState::Connected,
            models: Some(2),
            words: Some("Reached two models on the server.".into()),
        };
        hamlet_client::receive(
            app.world_mut(),
            v0::ServerMessage::Change(v0::Changes {
                epoch: 1,
                revision: 2,
                changes: vec![v0::Update::Settings { settings }],
            }),
        );
        for _ in 0..3 {
            app.update();
        }
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "server::orchard"));
        assert!(
            shown
                .iter()
                .any(|text| text
                    == "Reached two models on the server.")
        );
        for state in [State::Lost, State::Refused, State::Connecting]
        {
            let text = format!("The connection is {state:?}.");
            app.insert_resource(Connection {
                state,
                words: text.clone(),
            });
            for _ in 0..3 {
                app.update();
            }
            let shown = words(&mut app);
            assert!(
                shown.iter().any(|shown| shown == &text),
                "{shown:?}"
            );
            assert!(
                !shown.iter().any(|text| text
                    == "Reached two models on the server.")
            );
        }
        app.insert_resource(Connection {
            state: State::Connected,
            words: "Connected again.".into(),
        });
        for _ in 0..3 {
            app.update();
        }
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text
                    == "Reached two models on the server.")
        );
    }
}
