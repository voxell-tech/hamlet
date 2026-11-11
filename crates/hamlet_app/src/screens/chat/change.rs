//! The change of an `edit` call drawn: its file, its count, and its
//! lines taken out and put in.

use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, percent, px};
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, ghost,
    label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, component, keyed, resource};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_protocol::client::{
    Change, ChangeLines as Line, ChangeLinesMark as Mark,
};
use hamlet_ui::kinds::inset;
use hamlet_ui::markdown::Area;
use hamlet_ui::styles::{bare, note, small_rules};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::bubble::inked;
use super::entry::{Underway, status};
use super::lines::INSET;

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
    shut: Vec<(Entity, String)>,
    whole: Vec<(Entity, String)>,
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
    fn extent(&self, change: &(Entity, String)) -> Extent {
        if self.shut.contains(change) {
            Extent::Shut
        } else if self.whole.contains(change) {
            Extent::Whole
        } else {
            Extent::Cut
        }
    }
}

/// Adds `one` to `all`, or takes it out if it is there.
fn flip(all: &mut Vec<(Entity, String)>, one: (Entity, String)) {
    match all.iter().position(|kept| *kept == one) {
        Some(at) => {
            all.remove(at);
        }
        None => all.push(one),
    }
}

/// The lines of a change, one row each, the lines taken out and put
/// in tinted across the whole plate. Their words are selected as one.
fn block(lines: &[Line]) -> AnyView {
    let lines = lines.to_vec();
    with_cx(move |cx| {
        let theme = cx.theme();
        let rows = lines
            .iter()
            .map(|line| {
                let (lead, ink, fill) = match line.mark {
                    Mark::Space => ("  ", None, Color::NONE),
                    Mark::Minus => (
                        "- ",
                        Some(theme.tone(Tone::Critical)),
                        theme.palette.danger,
                    ),
                    Mark::Plus => (
                        "+ ",
                        Some(theme.palette.green),
                        theme.palette.green_deep,
                    ),
                };
                let tone = match line.mark {
                    Mark::Space => Tone::Dim,
                    Mark::Minus | Mark::Plus => Tone::Body,
                };
                let lead = match ink {
                    Some(ink) => inked(lead.to_string(), ink, SIZE),
                    None => {
                        label(lead).size(SIZE).wrap(false).boxed()
                    }
                };
                row((
                    lead.tagged(MonoText),
                    label(line.text.clone())
                        .size(SIZE)
                        .tone(tone)
                        .tagged(MonoText),
                ))
                .min_width(px(0.0))
                .padding(UiRect::horizontal(px(PAD)))
                .fill(fill)
                .boxed()
            })
            .collect::<Vec<_>>();
        column(rows).min_width(px(0.0)).tagged(Area)
    })
    .boxed()
}

/// The size of a change's lines.
const SIZE: f32 = 15.0;

/// The room between a change's plate and its words, across and down.
const PAD: f32 = 12.0;
const DOWN: f32 = 8.0;

/// The lines of `change` on their plate, the first `most` of them,
/// with a count of the rest.
fn plate(
    change: &Change,
    most: usize,
    more: Option<AnyView>,
) -> impl View {
    let shown = &change.lines[..most.min(change.lines.len())];
    let mut after = Vec::from_iter(more);
    // Past what a transcript keeps of a change.
    let lost =
        (change.total as usize).saturating_sub(change.lines.len());
    if most >= change.lines.len() && lost > 0 {
        after.push(note(format!("... {lost} more lines")).boxed());
    }
    let after = column(after).padding(UiRect::horizontal(px(PAD)));
    // Its lines run to its sides, so their tints do too.
    inset((block(shown), after)).padding(UiRect::vertical(px(DOWN)))
}

/// The lines a change adds in green and takes out in red, its count
/// in words under the pointer; the words alone when it changed none.
fn counted(change: &Change) -> AnyView {
    let words = change.count.clone();
    if change.added == 0 && change.removed == 0 {
        return note(words).boxed();
    }
    let (added, removed) = (change.added, change.removed);
    with_cx(move |cx| {
        let theme = cx.theme();
        let size = theme.text.small;
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(inked(
                format!("+{added}"),
                theme.palette.green,
                size,
            ));
        }
        if removed > 0 {
            parts.push(inked(
                format!("-{removed}"),
                theme.tone(Tone::Critical),
                size,
            ));
        }
        let words = words.clone();
        row(parts)
            .gap(6.0)
            .shrink(0.0)
            .tooltip(move || note(words.clone()))
    })
    .boxed()
}

/// The end of a tool call's entry, and the words for an edit that
/// changed nothing.
fn ended(entry: Option<&Entry>) -> (Option<i64>, Option<String>) {
    match entry {
        Some(Entry::Tool { exit, failure, .. }) => {
            let why = failure.as_ref();
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
/// count and how the call stands, over its lines.
pub(crate) fn change_view(
    session: Entity,
    id: String,
    change: Change,
    underway: Underway,
) -> AnyView {
    let key = (session, id.clone());
    let entry_id = id.clone();
    let entry =
        move |world: &World| {
            ended(world.get::<Transcript>(session).and_then(|t| {
                t.item(&entry_id).map(|item| &item.entry)
            }))
        };
    let verb = if change.created { "Create" } else { "Edit" };
    let fold_key = key.clone();
    let toggle_key = key.clone();
    let fold = button(
        row((
            glyph(Glyph::Chevron, 10.0).tone(Tone::Faint).rotation(
                resource::<Changes, _>(move |changes| match changes
                    .extent(&fold_key)
                {
                    Extent::Shut => 90.0,
                    _ => 180.0,
                }),
            ),
            label(verb)
                .wrap(false)
                .tone(Tone::Dim)
                .rules(small_rules),
        ))
        .gap(6.0)
        .align(AlignItems::Center),
    )
    .padding(UiRect::axes(px(INSET), px(4.0)))
    .rules(ghost)
    .on_activate(move |world| {
        flip(
            &mut world.resource_mut::<Changes>().shut,
            toggle_key.clone(),
        );
    });
    // The file is looked for again once the turn has ended, when a
    // new one is there.
    let path = change.path.clone();
    let file = keyed(
        component::<Transcript, _>(session, |t| {
            t.is_some_and(Transcript::is_answering)
        }),
        move |_| {
            hamlet_ui::markdown::typed(&path, "")
                .unwrap_or_else(|| label(path.clone()).boxed())
        },
    )
    .within(bare().shrink(1.0).min_width(px(0.0)));
    let mut parts =
        vec![fold.boxed(), file.boxed(), counted(&change)];
    // A held call's card says that it waits, so its head does not.
    if underway == Underway::Running {
        parts.push(status(session, id).boxed());
    }
    let head = row(parts).gap(8.0).align(AlignItems::Center);

    let mut moved = component::<Transcript, _>(session, |_| ());
    let mut turned = resource::<Changes, _>(|_| ());
    let mut seen = None;
    let read_key = key.clone();
    let check_key = key.clone();
    let read_entry = entry.clone();
    let drawn = bevy_fynix::Signal::new(
        move |world: &World| {
            let extent =
                world.resource::<Changes>().extent(&read_key);
            (extent, read_entry(world).1)
        },
        move |world: &World| {
            // Both are asked, so that neither is left unread.
            let stirred =
                moved.changed(world) | turned.changed(world);
            if !stirred {
                return false;
            }
            let extent =
                world.resource::<Changes>().extent(&check_key);
            let now = Some((extent, entry(world).1));
            let differs = seen != now;
            seen = now;
            differs
        },
    );
    let body = keyed(drawn, move |(extent, refused)| {
        let mut parts = Vec::new();
        if let Some(refused) = refused {
            parts.push(
                label(refused.clone())
                    .tone(Tone::Critical)
                    .rules(small_rules)
                    .boxed(),
            );
        }
        if *extent != Extent::Shut && !change.lines.is_empty() {
            parts.push(lines_view(key.clone(), &change, *extent));
        }
        column(parts).gap(4.0).boxed()
    })
    .within(bare());
    column((head, body)).gap(2.0).width(percent(100.0)).boxed()
}

/// The lines of `change` as far as `extent` goes, with the button
/// that shows the rest or puts it away.
fn lines_view(
    key: (Entity, String),
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
                (change.total as usize).saturating_sub(most)
            )
        };
        let more = (change.lines.len() > most).then(|| {
            button(label(words).tone(Tone::Dim).rules(small_rules))
                .rules(ghost)
                .on_activate(move |world| {
                    flip(
                        &mut world.resource_mut::<Changes>().whole,
                        key.clone(),
                    );
                })
                .boxed()
        });
        let shown = if whole { usize::MAX } else { most };
        let plate = plate(&change, shown, more);
        cx.build(plate)
    })
}
