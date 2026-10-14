//! The line at the foot of the window, and the words it is made
//! of.

use std::time::Instant;

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
use bevy_fynix::{Bevy, View, ViewExt, keyed, resource};
use hamlet_core::agents::{Settlement, Treasury};
use hamlet_core::core::{Link, Sandboxing, Settings};
use hamlet_core::session::{Status, Usage};
use hamlet_rules::hamlet::HOME;

use crate::icons::{Glyph, glyph};
use crate::links::Aimed;
use crate::looks::{hairline, note};
use crate::me::Me;
use crate::needs::Queue;
use crate::theme::{BLACK, Lamplight, SEAM};
use crate::window::Layout;
use crate::{Build, Ui};

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
    /// Tokens and cost (or model time) of the current session.
    pub usage: String,
    /// The hamlet's spend against its budget.
    pub budget: String,
}

pub(crate) fn update_status(
    me: Res<Me>,
    sessions: Query<(Ref<Status>, Ref<Usage>)>,
    settings: Res<Settings>,
    link: Res<Link>,
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
        || link.is_changed()
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
        model: settings.config.model.clone(),
        link: link_text(&link),
        usage: session.map(|(_, u)| u.summary()).unwrap_or_default(),
        budget: home.map(|t| t.0.summary()).unwrap_or_default(),
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
    mut seen: Local<Option<(Instant, u64)>>,
) {
    let last = sessions
        .get(me.session)
        .ok()
        .and_then(|status| status.last_activity.as_ref());
    // The words change with the second at the soonest.
    let now = last.map(|(at, _)| (*at, at.elapsed().as_secs()));
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

/// The state of the connection in a few words.
pub(crate) fn link_text(link: &Link) -> String {
    match link {
        Link::Unknown => "Connection not tested".into(),
        Link::Testing => "Testing connection...".into(),
        Link::Ok { models } => format!(
            "Connected, {} {}",
            models.len(),
            if models.len() == 1 { "model" } else { "models" }
        ),
        Link::Failed(_) => "Not connected".into(),
    }
}

pub(crate) fn link_tone(link: &Link) -> Tone {
    match link {
        Link::Failed(_) => Tone::Critical,
        _ => Tone::Dim,
    }
}

pub(crate) fn ago(since: Instant) -> String {
    let s = since.elapsed().as_secs();
    match s {
        0..=4 => "just now".into(),
        5..=59 => format!("{s}s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

/// The side of an icon in the bar.
const MARK: f32 = 12.0;

/// An icon and the words it stands before.
fn pair(
    mark: impl View<Bevy, Lamplight> + 'static,
    words: impl View<Bevy, Lamplight> + 'static,
) -> impl View<Bevy, Lamplight> + 'static {
    row((mark, words)).gap(5.0).align(AlignItems::Center)
}

/// One of the bar's facts: an icon and what `text` reads, with `tip`
/// to name it under the pointer. Nothing while there is nothing to
/// say.
fn fact(
    which: Glyph,
    text: fn(&Bar) -> String,
    tip: &'static str,
) -> Ui {
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
fn waiting() -> Ui {
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
    .boxed()
}

/// The place the link under the pointer leads, in the room between
/// the bar's two ends.
fn aimed() -> Ui {
    row((label(resource::<Aimed, _>(|aimed| aimed.address.clone()))
        .size(12.0)
        .wrap(false)
        .tone(Tone::Accent),))
    .grow(1.0)
    .min_width(px(0.0))
    .overflow(Overflow::clip())
    .boxed()
}

/// The bar along the foot of the window.
pub fn status_bar() -> Ui {
    Ui::new(|cx: &mut Build| {
        let sandbox = cx.world.resource::<Sandboxing>().0.name();
        cx.build(
            column((
                hairline(SEAM),
                row((
                    pair(
                        glyph(Glyph::Shield, MARK).tone(Tone::Dim),
                        note(sandbox),
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
                            Link,
                            _,
                        >(
                            link_tone
                        )),
                        label(resource::<Bar, _>(|s| s.link.clone()))
                            .size(12.0)
                            .tone(resource::<Link, _>(link_tone)),
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
                .fill(BLACK),
            ))
            .gap(0.0),
        )
    })
}
