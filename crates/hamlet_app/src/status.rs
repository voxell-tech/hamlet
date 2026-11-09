//! The line at the foot of the window, and the words it is made
//! of.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::app::{App, PreUpdate};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ui::{AlignItems, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, TooltipExt, column, frame, icon, label, row,
};
use bevy_fynix::{Prop, ScopedExt, ViewExt, keyed, resource};
use hamlet_client::Connection;
use hamlet_client::chats::{Status, Usage};
use hamlet_client::connection::Contact;
use hamlet_client::realm::{
    Confinement, LivesIn, Settings, Treasury,
};
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{hairline, note, small_rules};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, View, built, with_cx};

use crate::PanelSystems;
use crate::convo::Convo;
use crate::links::Aimed;
use crate::petition::Queue;
use crate::standing::{Standing, need_you};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Bar>().add_systems(
        PreUpdate,
        update_status.in_set(PanelSystems::Bind),
    );
}

/// The words of the status bar.
#[derive(Resource, Default, PartialEq)]
pub struct Bar {
    /// The open chat's agent has a turn running.
    pub busy: bool,
    pub running: String,
    pub running_tone: Tone,
    pub model: String,
    pub link: String,
    pub link_tone: Tone,
    /// The connection's words, under the pointer on the link.
    pub connection: String,
    /// Tokens and cost (or model time) of the current session.
    pub usage: String,
    /// The spend against the budget of the hamlet the chat's agent
    /// lives in.
    pub budget: String,
}

pub(crate) fn update_status(
    convo: Res<Convo>,
    sessions: Query<(Ref<Status>, Ref<Usage>)>,
    settings: Res<Settings>,
    connection: Res<Connection>,
    lives: Query<&LivesIn>,
    treasury: Query<Ref<Treasury>>,
    mut bar: ResMut<Bar>,
) {
    let session = sessions.get(convo.session).ok();
    // The Hand is of no hamlet, and has no budget to show here.
    let spend = lives
        .get(convo.agent)
        .ok()
        .and_then(|lives| treasury.get(lives.0).ok());
    let moved = convo.is_changed()
        || settings.is_changed()
        || connection.is_changed()
        || session.as_ref().is_some_and(|(status, usage)| {
            status.is_changed() || usage.is_changed()
        })
        || spend.as_ref().is_some_and(DetectChanges::is_changed);
    if !moved {
        return;
    }
    let busy = session.as_ref().is_some_and(|(s, _)| s.is_working());
    let next = Bar {
        busy,
        running: if busy { "Working" } else { "Idle" }.into(),
        running_tone: if busy { Tone::Warning } else { Tone::Dim },
        model: crate::models::model_name(&settings.0.model)
            .to_string(),
        link: if connection.state == Contact::Connected {
            settings.0.link.words.clone().unwrap_or_default()
        } else {
            connection.words.clone()
        },
        link_tone: match connection.state {
            Contact::Lost | Contact::Refused => Tone::Critical,
            Contact::Connected
                if settings.0.link.state
                    == protocol::LinkState::Failed =>
            {
                Tone::Critical
            }
            _ => Tone::Dim,
        },
        connection: connection.words.clone(),
        usage: session.map(|(_, u)| u.summary()).unwrap_or_default(),
        budget: spend
            .map(|t| t.0.summary.clone())
            .unwrap_or_default(),
    };
    if *bar != next {
        *bar = next;
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// An icon and the words it stands before.
fn pair(
    mark: impl View + 'static,
    words: impl View + 'static,
) -> impl View {
    with_cx(move |cx| {
        row((mark, words))
            .gap(cx.theme().space.status_icon_gap)
            .align(AlignItems::Center)
    })
}

/// The icon `which` of the bar's size, in `tone`.
fn mark(
    which: Glyph,
    tone: impl Into<Prop<Tone>> + 'static,
) -> impl View {
    with_cx(move |cx| {
        glyph(which, cx.theme().space.status_icon).tone(tone)
    })
}

/// Words of the small size in `tone`, as the bar tells what is on.
fn told(
    text: impl Into<Prop<String>> + 'static,
    tone: impl Into<Prop<Tone>> + 'static,
) -> impl View {
    label(text).tone(tone).rules(small_rules)
}

/// [`told`] in bold, for what needs the chief.
fn alarm(
    text: impl Into<Prop<String>> + 'static,
    tone: impl Into<Prop<Tone>> + 'static,
) -> impl View {
    label(text).bold(true).tone(tone).rules(small_rules)
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
                mark(which, Tone::Dim),
                told(resource::<Bar, _>(text), Tone::Dim),
            )
            .tooltip(move || note(tip))
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
    .boxed()
}

/// What confines the agents' commands. In insecure mode nothing
/// does, and the bar says so in the critical tone.
fn confinement() -> AnyView {
    keyed(
        resource::<Confinement, _>(|c| c.0.clone()),
        |confinement| match confinement {
            protocol::Confinement::Sandbox { name } => {
                let tip = format!("Agents' commands run in {name}");
                pair(
                    mark(Glyph::Shield, Tone::Dim),
                    told("Sandboxed", Tone::Dim),
                )
                .tooltip(move || note(tip.clone()))
                .boxed()
            }
            protocol::Confinement::Insecure => pair(
                mark(Glyph::Shield, Tone::Critical),
                alarm("Insecure mode", Tone::Critical),
            )
            .tooltip(|| {
                note(
                    "Agents' commands run without a sandbox, with \
                     this account's full reach.",
                )
            })
            .boxed(),
            protocol::Confinement::Remote => pair(
                mark(Glyph::Shield, Tone::Dim),
                told("Remote", Tone::Dim),
            )
            .tooltip(|| {
                note("Agents' commands run on the remote machine")
            })
            .boxed(),
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
            mark(Standing::Waits.glyph(), Standing::Waits.tone()),
            alarm(
                format!("{n} {}", need_you(*n)),
                Standing::Waits.words(),
            ),
        )
        .boxed(),
    })
    .within(row(()).gap(0.0))
}

/// How the agents reach their models: what confines their commands,
/// the default model and the link.
fn setup() -> impl View {
    with_cx(|cx| {
        row((
            confinement(),
            fact(
                Glyph::Chip,
                |bar| {
                    if bar.model.is_empty() {
                        String::new()
                    } else {
                        format!("Default model: {}", bar.model)
                    }
                },
                "The model agents use unless given another",
            ),
            pair(
                mark(
                    Glyph::Signal,
                    resource::<Bar, _>(|bar| bar.link_tone),
                ),
                keyed(
                    resource::<Bar, _>(|bar| bar.link_tone),
                    |tone| {
                        let words =
                            resource::<Bar, _>(|s| s.link.clone());
                        match tone {
                            Tone::Critical => {
                                alarm(words, Tone::Critical).boxed()
                            }
                            _ => told(words, *tone).boxed(),
                        }
                    },
                )
                .within(row(()).gap(0.0)),
            )
            .tooltip(|| {
                note(resource::<Bar, _>(|bar| bar.connection.clone()))
            }),
        ))
        .gap(cx.theme().space.status_gap)
        .align(AlignItems::Center)
    })
}

/// What the current session used, after a short upright line that
/// sets it apart from [`setup`]. Nothing while there is no session.
fn session() -> impl View {
    keyed(
        resource::<Bar, _>(|bar| {
            bar.usage.is_empty() && bar.budget.is_empty()
        }),
        |none| {
            if *none {
                return frame().boxed();
            }
            with_cx(|cx| {
                let space = &cx.theme().space;
                row((
                    frame()
                        .width(px(1.0))
                        .height(px(space.status_icon))
                        .fill(cx.theme().palette.border),
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
                ))
                .gap(space.status_gap)
                .align(AlignItems::Center)
            })
            .boxed()
        },
    )
    .within(row(()).gap(0.0))
}

/// The place the link under the pointer leads, in the room between
/// the bar's two ends.
fn aimed() -> impl View {
    row((label(resource::<Aimed, _>(|aimed| aimed.address.clone()))
        .wrap(false)
        .tone(Tone::Accent)
        .rules(small_rules),))
    .grow(1.0)
    .min_width(px(0.0))
    .overflow(Overflow::clip())
}

/// The bar along the foot of the window.
pub fn status_bar() -> impl View {
    built(|cx| {
        let theme = cx.theme();
        let (palette, space) = (&theme.palette, &theme.space);
        cx.build(
            column((
                hairline(palette.border),
                row((
                    setup(),
                    session(),
                    aimed(),
                    waiting(),
                    pair(
                        with_cx(|cx| {
                            let side = cx.theme().space.status_icon;
                            icon(resource::<Bar, _>(move |bar| {
                                let which = if bar.busy {
                                    Standing::Works
                                } else {
                                    Standing::Rests
                                };
                                which.glyph().image(side)
                            }))
                            .size(side)
                            .tone(
                                resource::<Bar, _>(|s| {
                                    s.running_tone
                                }),
                            )
                        }),
                        told(
                            resource::<Bar, _>(|s| s.running.clone()),
                            resource::<Bar, _>(|s| {
                                if s.busy {
                                    Tone::Body
                                } else {
                                    Tone::Dim
                                }
                            }),
                        ),
                    ),
                ))
                .gap(space.status_gap)
                .align(AlignItems::Center)
                .padding(UiRect::axes(
                    px(space.status_padding_x),
                    px(space.status_padding_y),
                ))
                .fill(palette.surface),
            ))
            .gap(0.0),
        )
    })
}
