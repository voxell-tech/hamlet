//! The change of an `edit` call drawn: its file, its count, and its
//! lines taken out and put in.

use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::text::{
    FontSize, FontSource, TextBackgroundColor, TextColor, TextFont,
    TextSpan,
};
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, ghost, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, component, keyed, resource};
use hamlet_agents_api::change::{Change, Line, Mark};
use hamlet_agents_api::session::{Entry, Transcript};

use super::entry::state;
use crate::looks::{Fills, bare, note};
use crate::platform::{Mono, MonoText};
use crate::{AnyView, Cx, View, markdown};

/// The tuning of the chat.
#[derive(Resource, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatSettings {
    /// The lines of a change shown before the rest is asked for.
    pub change_lines: usize,
    /// The widest a picture in a reply is drawn.
    pub picture_width: f32,
    /// The tallest a picture in a reply is drawn.
    pub picture_height: f32,
    /// The height of the plate that stands for a picture while it
    /// is read.
    pub picture_wait: f32,
    /// The largest file read as a picture, in bytes.
    pub picture_bytes: u64,
    /// The most bytes of pictures kept that no row shows.
    pub picture_memory: u64,
    /// The most characters of a message a reply carries.
    pub reply_chars: usize,
}

/// The changes a chief has shut, and those asked for whole, by
/// their conversation and entry.
#[derive(Resource, Default)]
pub struct Changes {
    shut: Vec<(Entity, u64)>,
    whole: Vec<(Entity, u64)>,
}

/// The part of a change that is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Extent {
    Shut,
    /// Its first lines.
    Cut,
    Whole,
}

impl Changes {
    fn extent(&self, change: (Entity, u64)) -> Extent {
        if self.shut.contains(&change) {
            Extent::Shut
        } else if self.whole.contains(&change) {
            Extent::Whole
        } else {
            Extent::Cut
        }
    }
}

/// Adds `one` to `all`, or takes it out if it is there.
fn flip(all: &mut Vec<(Entity, u64)>, one: (Entity, u64)) {
    match all.iter().position(|kept| *kept == one) {
        Some(at) => {
            all.remove(at);
        }
        None => all.push(one),
    }
}

/// The lines of a change as one block of text, each run of lines of
/// one kind a span of it.
fn block(lines: &[Line]) -> AnyView {
    let mut runs = Vec::<(Mark, String)>::new();
    for (at, line) in lines.iter().enumerate() {
        let lead = match line.mark {
            Mark::Same => "  ",
            Mark::Out => "- ",
            Mark::In => "+ ",
        };
        let end = if at + 1 == lines.len() { "" } else { "\n" };
        let text = format!("{lead}{}{end}", line.text);
        match runs.last_mut() {
            Some((mark, run)) if *mark == line.mark => {
                run.push_str(&text)
            }
            _ => runs.push((line.mark, text)),
        }
    }
    AnyView::new(move |cx: &mut Cx| {
        let root = cx.build(label("").size(SIZE));
        let theme = cx.theme();
        let mono =
            cx.world.get_resource::<Mono>().map(|m| m.0.clone());
        for (mark, text) in runs {
            let (ink, plate) = match mark {
                Mark::Same => (theme.tone(Tone::Dim), None),
                Mark::Out => (
                    theme.tone(Tone::Critical),
                    Some(theme.palette.red_deep),
                ),
                Mark::In => (
                    theme.palette.moss,
                    Some(theme.palette.moss_deep),
                ),
            };
            let font = TextFont {
                font: mono
                    .clone()
                    .map(FontSource::from)
                    .unwrap_or_default(),
                font_size: FontSize::Px(SIZE),
                ..TextFont::default()
            };
            let mut span = cx.world.spawn((
                TextSpan(text),
                font,
                TextColor(ink),
                MonoText,
                ChildOf(root),
            ));
            if let Some(plate) = plate {
                span.insert(TextBackgroundColor(plate));
            }
        }
        root
    })
}

/// The size of a change's lines.
const SIZE: f32 = 12.5;

/// The room at the left of what is under a change's head, which
/// sets it under the name of its file.
const INSET: f32 = 12.0;

/// The lines of `change` on their plate, the first `most` of them,
/// with a count of the rest.
fn plate(
    cx: &Cx,
    change: &Change,
    most: usize,
    more: Option<AnyView>,
) -> impl View {
    let theme = cx.theme();
    let shown = &change.lines[..most.min(change.lines.len())];
    let mut parts = vec![block(shown)];
    parts.extend(more);
    // Past what a transcript keeps of a change.
    let lost = change.total.saturating_sub(change.lines.len());
    if most >= change.lines.len() && lost > 0 {
        parts.push(note(format!("... {lost} more lines")).boxed());
    }
    column(parts)
        .gap(4.0)
        .padding(UiRect::axes(px(10.0), px(8.0)))
        .min_width(px(0.0))
        .fill(theme.palette.plate)
        .border(1.0)
        .border_color(theme.palette.brass_6)
        .radius(theme.space.round)
}

/// The end of a tool call's entry, and the words for an edit that
/// changed nothing.
fn ended(entry: Option<&Entry>) -> (Option<i32>, Option<String>) {
    match entry {
        Some(Entry::Tool { exit, change, .. }) => {
            let why =
                change.as_ref().and_then(|c| c.failure.as_ref());
            let refused = match (exit, why) {
                (Some(0) | None, _) => None,
                (Some(_), Some(why)) => {
                    Some(format!("Not applied: {why}"))
                }
                (Some(_), None) => Some("Not applied".to_string()),
            };
            (*exit, refused)
        }
        _ => (None, None),
    }
}

/// The change of the edit at entry `id` of `session`: its file, its
/// count and how the call ended, over its lines.
pub(crate) fn change_view(
    session: Entity,
    id: u64,
    change: Change,
) -> AnyView {
    let key = (session, id);
    let entry = move |world: &World| {
        ended(
            world.get::<Transcript>(session).and_then(|t| t.get(id)),
        )
    };
    let verb = if change.created { "Create" } else { "Edit" };
    let fold = button(
        label(resource::<Changes, _>(move |changes| {
            let mark = match changes.extent(key) {
                Extent::Shut => "+",
                _ => "-",
            };
            format!("{mark}  {verb}")
        }))
        .size(12.0)
        .wrap(false)
        .tone(Tone::Dim),
    )
    .rules(ghost)
    .on_activate(move |world| {
        flip(&mut world.resource_mut::<Changes>().shut, key);
    });
    // The file is looked for again once the turn has ended, when a
    // new one is there.
    let path = change.path.clone();
    let file = keyed(
        component::<Transcript, _>(session, |t| {
            t.is_some_and(Transcript::answering)
        }),
        move |_| {
            markdown::typed(&path, "")
                .unwrap_or_else(|| label(path.clone()).boxed())
        },
    )
    .within(bare().fills_width().min_width(px(0.0)));
    let head = row((fold, file, state(session, id)))
        .gap(10.0)
        .align(AlignItems::Center);

    let mut moved = component::<Transcript, _>(session, |_| ());
    let mut turned = resource::<Changes, _>(|_| ());
    let mut seen = None;
    let drawn = bevy_fynix::Signal::new(
        move |world: &World| {
            let extent = world.resource::<Changes>().extent(key);
            (extent, entry(world).1)
        },
        move |world: &World| {
            // Both are asked, so that neither is left unread.
            let stirred =
                moved.changed(world) | turned.changed(world);
            if !stirred {
                return false;
            }
            let extent = world.resource::<Changes>().extent(key);
            let now = Some((extent, entry(world).1));
            let differs = seen != now;
            seen = now;
            differs
        },
    );
    let body = keyed(drawn, move |(extent, refused)| {
        let mut parts = vec![note(change.count()).boxed()];
        if let Some(refused) = refused {
            parts.push(
                label(refused.clone())
                    .size(12.0)
                    .tone(Tone::Critical)
                    .boxed(),
            );
        }
        if *extent != Extent::Shut && !change.lines.is_empty() {
            parts.push(lines_view(key, &change, *extent));
        }
        column(parts)
            .gap(4.0)
            .padding(UiRect::left(px(INSET)))
            .boxed()
    })
    .within(bare());
    column((head, body)).gap(2.0).width(percent(100.0)).boxed()
}

/// The lines of `change` as far as `extent` goes, with the button
/// that shows the rest or puts it away.
fn lines_view(
    key: (Entity, u64),
    change: &Change,
    extent: Extent,
) -> AnyView {
    let change = change.clone();
    AnyView::new(move |cx: &mut Cx| {
        let most = cx.world.resource::<ChatSettings>().change_lines;
        let whole = extent == Extent::Whole;
        let words = if whole {
            "Show fewer lines".to_string()
        } else {
            format!(
                "... {} more lines",
                change.total.saturating_sub(most)
            )
        };
        let more = (change.lines.len() > most).then(|| {
            button(label(words).size(12.0).tone(Tone::Dim))
                .rules(ghost)
                .on_activate(move |world| {
                    flip(
                        &mut world.resource_mut::<Changes>().whole,
                        key,
                    );
                })
                .boxed()
        });
        let shown = if whole { usize::MAX } else { most };
        let plate = plate(cx, &change, shown, more);
        cx.build(plate)
    })
}
