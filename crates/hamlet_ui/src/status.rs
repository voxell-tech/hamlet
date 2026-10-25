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
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::Connection;
use hamlet_client::chats::{Status, Usage};
use hamlet_client::connection::Contact;
use hamlet_client::realm::{
    HOME, HamletId, Sandbox, Settings, Treasury,
};
use hamlet_protocol::client as protocol;

use crate::icons::{Glyph, glyph};
use crate::links::Aimed;
use crate::looks::{hairline, note};
use crate::me::Me;
use crate::petition::Queue;
use crate::{AnyView, Cx, PanelSystems, View};

/// The side of an icon in the bar.
const MARK: f32 = 12.0;

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
    /// The hamlet's spend against its budget.
    pub budget: String,
}

pub(crate) fn update_status(
    me: Res<Me>,
    sessions: Query<(Ref<Status>, Ref<Usage>)>,
    settings: Res<Settings>,
    connection: Res<Connection>,
    treasury: Query<(&HamletId, Ref<Treasury>)>,
    mut bar: ResMut<Bar>,
) {
    let session = sessions.get(me.session).ok();
    let home = treasury
        .iter()
        .find(|(id, _)| id.0 == HOME)
        .map(|(_, treasury)| treasury);
    let moved = me.is_changed()
        || settings.is_changed()
        || connection.is_changed()
        || session.as_ref().is_some_and(|(status, usage)| {
            status.is_changed() || usage.is_changed()
        })
        || home.as_ref().is_some_and(DetectChanges::is_changed);
    if !moved {
        return;
    }
    let busy = session.as_ref().is_some_and(|(s, _)| s.is_working());
    let next = Bar {
        busy,
        running: if busy {
            "Working".into()
        } else {
            "Running, idle".into()
        },
        running_tone: if busy { Tone::Accent } else { Tone::Dim },
        model: settings.0.model.clone(),
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
        budget: home.map(|t| t.0.summary.clone()).unwrap_or_default(),
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
                    )
                    .tooltip(|| {
                        label(resource::<Bar, _>(|bar| {
                            bar.connection.clone()
                        }))
                        .size(12.0)
                    }),
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
