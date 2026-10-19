//! The line at the foot of the window, and the words it is made
//! of.

use bevy::app::{App, Update};
use bevy::ecs::change_detection::{DetectChanges, Ref};
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ui::{AlignItems, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    FrameProps, TooltipExt, column, frame, icon, label, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_agents_api::waits::Waits;
use hamlet_core::agents::{Settlement, Treasury};
use hamlet_core::core::{Link, Sandboxing, Settings};
use hamlet_core::session::{Status, Usage};
use hamlet_rules::hamlet::HOME;
pub use hamlet_sdk::bar::Bar;

use crate::icons::{Glyph, glyph};
use crate::links::Aimed;
use crate::looks::{hairline, note};
use crate::me::Me;
use crate::{AnyView, Cx, View};

/// The side of an icon in the bar.
const MARK: f32 = 12.0;

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Update, update_status);
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
    keyed(resource::<Waits, _>(|w| w.0), |waiting| match waiting {
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
        let sandbox = cx.world.resource::<Sandboxing>().0.name();
        let palette = &cx.theme().palette;
        cx.build(
            column((
                hairline(palette.seam),
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
                .fill(palette.black),
            ))
            .gap(0.0),
        )
    })
}
