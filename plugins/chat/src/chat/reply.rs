//! A message that answers another: a line at its head naming the
//! message it answers, the mark on a message that replies to it, and
//! what is replied to shown over the field until the message is sent.

use bevy::app::{App, Update};
use bevy::camera::visibility::Visibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::picking::hover::Hovered;
use bevy::ui::{AlignItems, JustifyContent, Overflow, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource};
use hamlet_agents_api::session::{Entry, Transcript, reply_in};
use hamlet_agents_api::words::clip;

use super::{ChatSettings, Draft};
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{Fills, ICON, bare, retrace};
use crate::me::Me;
use crate::{AnyView, Cx, View, with_cx};

/// The room of a mark about its picture.
const AROUND: f32 = 4.0;

/// The room a mark takes beside its message.
pub(super) const ROOM: f32 = ICON + AROUND * 3.0;

/// The most characters of what is replied to shown on one line.
const SHOWN: usize = 160;

/// The head of a reply's first line.
const HEAD: &str = "(Replying to ";

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Replying>()
        .add_observer(reveal)
        .add_systems(Update, forget);
}

/// The message a reply answers: its speaker and its words.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Answers {
    pub by: String,
    pub text: String,
}

impl Answers {
    /// The words `text` of `by`, cut to at most `most` characters.
    pub fn of(by: &str, text: &str, most: usize) -> Self {
        let text = text.trim();
        let text = match text.char_indices().nth(most) {
            Some((cut, _)) => {
                format!("{}...", text[..cut].trim_end())
            }
            None => text.to_string(),
        };
        Answers {
            by: by.to_string(),
            text,
        }
    }
}

/// The message the next one sent in the chat of a session answers.
#[derive(Resource, Default)]
pub struct Replying(pub Option<(Entity, Answers)>);

/// `words` sent as a reply to `answers`: a first line that names it,
/// its words as a JSON string, then the words.
pub fn replied(answers: &Answers, words: &str) -> String {
    let said = serde_json::Value::from(answers.text.as_str());
    format!("{HEAD}{}: {said})\n\n{words}", answers.by)
}

/// What `text` replies to and the words after it. `None` when it is
/// no reply.
pub fn unreplied(text: &str) -> Option<(Answers, &str)> {
    let (head, words) = text.split_once("\n\n")?;
    let (by, said) = head
        .strip_prefix(HEAD)?
        .strip_suffix(')')?
        .split_once(": ")?;
    let said = serde_json::from_str::<String>(said).ok()?;
    let by = by.trim();
    if by.is_empty() {
        return None;
    }
    let answers = Answers {
        by: by.to_string(),
        text: said,
    };
    Some((answers, words))
}

/// The words of `text` after any reply line it opens with.
pub(super) fn rest(text: &str) -> &str {
    unreplied(text).map_or(text, |(_, words)| words)
}

/// The words of an entry a message can reply to or select.
pub(super) fn words(entry: &Entry) -> Option<&str> {
    match entry {
        Entry::User { user, text } => {
            Some(rest(reply_in(user, text).unwrap_or(text)))
        }
        Entry::Assistant(text)
        | Entry::Agent { text, .. }
        | Entry::Answered(text) => Some(text),
        _ => None,
    }
}

/// True when `entry` can be replied to.
pub(super) fn answerable(entry: &Entry) -> bool {
    words(entry).is_some_and(|text| !text.trim().is_empty())
}

/// Has the next message of the open chat reply to the entry `id` of
/// `session`, said by `by`, and puts the caret in the field.
fn take(world: &mut World, session: Entity, id: u64, by: &str) {
    let most = world.resource::<ChatSettings>().reply_chars;
    let answers = world
        .get::<Transcript>(session)
        .and_then(|said| said.get(id))
        .and_then(words)
        .map(|text| Answers::of(by, text, most));
    let Some(answers) = answers else {
        return;
    };
    let open = world.resource::<Me>().session;
    world.resource_mut::<Replying>().0 = Some((open, answers));
    // The field is focused one step on.
    world.resource_mut::<Draft>().step = 1;
}

/// `text` sent as a reply to the message waiting in the open chat,
/// which it uses up.
pub(super) fn attach(world: &mut World, text: String) -> String {
    if text.trim().is_empty() {
        return text;
    }
    let open = world.resource::<Me>().session;
    match world.resource_mut::<Replying>().0.take() {
        Some((session, answers)) if session == open => {
            replied(&answers, text.trim())
        }
        _ => text,
    }
}

/// Drops a reply begun in a chat that is no longer open.
fn forget(me: Res<Me>, mut replying: ResMut<Replying>) {
    if replying.0.as_ref().is_some_and(|(at, _)| *at != me.session) {
        replying.0 = None;
    }
}

/// On a row of the chat whose message can be replied to.
#[derive(Component, Clone, Copy, Debug)]
pub struct Replies;

/// The mark of a row, which replies to its message.
#[derive(Component)]
struct Mark;

/// The mark beside the message `id` of `session`, said by `by`, out
/// of sight until the pointer is on its row.
pub(super) fn mark(session: Entity, id: u64, by: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let reply = button(glyph(Glyph::Talk, ICON))
            .padding(UiRect::all(px(AROUND)))
            .tooltip(|| label("Reply").size(12.0))
            .rules(retrace)
            .tagged(Mood::Ink)
            .on_activate(move |world| take(world, session, id, &by));
        let node = cx.build(
            column((reply,))
                .justify(JustifyContent::Center)
                .padding(UiRect::horizontal(px(AROUND / 2.0)))
                .shrink(0.0)
                .tagged(Mark),
        );
        cx.world.entity_mut(node).insert(Visibility::Hidden);
        node
    })
}

/// Shows the mark of a row while the pointer is on the row.
fn reveal(
    hover: On<Insert, Hovered>,
    rows: Query<&Hovered, With<Replies>>,
    under: Query<&Children>,
    mut marks: Query<&mut Visibility, With<Mark>>,
) {
    let row = hover.entity;
    let Ok(hovered) = rows.get(row) else {
        return;
    };
    let want = match hovered.get() {
        true => Visibility::Inherited,
        false => Visibility::Hidden,
    };
    for node in under.iter_descendants(row) {
        if let Ok(mut shown) = marks.get_mut(node)
            && *shown != want
        {
            *shown = want;
        }
    }
}

/// The message a reply answers, as one line at the head of the
/// reply: who said it and the start of its words.
pub(super) fn answered(answers: &Answers) -> impl View + use<> {
    row((
        glyph(Glyph::Talk, 11.0).tone(Tone::Dim),
        label(answers.by.clone())
            .size(11.0)
            .bold(true)
            .wrap(false)
            .tone(Tone::Dim),
        label(clip(&answers.text, SHOWN))
            .size(11.0)
            .wrap(false)
            .tone(Tone::Dim),
    ))
    .gap(5.0)
    .align(AlignItems::Center)
    .overflow(Overflow::clip())
    .padding(UiRect::bottom(px(2.0)))
}

/// The message the next one answers, over the field, and the way to
/// let it go.
pub(super) fn preview() -> impl View {
    keyed(
        resource::<Replying, _>(|r| {
            r.0.as_ref().map(|(_, a)| a.clone())
        }),
        |answers| match answers.clone() {
            Some(answers) => with_cx(move |cx| shown(cx, &answers)),
            None => frame().boxed(),
        },
    )
    .within(bare())
}

fn shown(cx: &Cx, answers: &Answers) -> AnyView {
    let drop = button(glyph(Glyph::Cross, ICON))
        .padding(UiRect::all(px(AROUND)))
        .tooltip(|| label("Do not reply").size(12.0))
        .rules(retrace)
        .tagged(Mood::Ink)
        .on_activate(|world| {
            world.resource_mut::<Replying>().0 = None
        });
    row((
        frame()
            .width(px(3.0))
            .shrink(0.0)
            .fill(cx.theme().palette.brass_5),
        column((
            label(format!("Replying to {}", answers.by))
                .size(11.0)
                .bold(true)
                .wrap(false)
                .tone(Tone::Faint),
            label(clip(&answers.text, SHOWN))
                .size(12.0)
                .wrap(false)
                .tone(Tone::Dim),
        ))
        .gap(1.0)
        .fills_width()
        .overflow(Overflow::clip()),
        column((drop,)).justify(JustifyContent::Center),
    ))
    .gap(8.0)
    .align(AlignItems::Stretch)
    .padding(UiRect::bottom(px(6.0)))
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_reads_back_as_it_was_written() {
        let answers = Answers::of(
            "Mayor",
            "  Paper or \"ebook\"?\n\n> Both ship: soon.)  \n",
            100,
        );
        let sent = replied(&answers, "Paper.\n\n> quoted\nThanks.");
        assert_eq!(
            sent,
            "(Replying to Mayor: \"Paper or \\\"ebook\\\"?\\n\\n> \
             Both ship: soon.)\")\n\nPaper.\n\n> quoted\nThanks."
        );
        assert_eq!(
            unreplied(&sent),
            Some((answers, "Paper.\n\n> quoted\nThanks."))
        );
    }

    #[test]
    fn a_long_reply_is_cut_and_other_words_are_no_reply() {
        assert_eq!(Answers::of("wren", "abcdef", 3).text, "abc...");
        assert_eq!(unreplied("> wren: a quote\n\nwords"), None);
        assert_eq!(
            unreplied("(Replying to wren: not json)\n\nw"),
            None
        );
        assert_eq!(unreplied("(Replying to wren: \"a\")"), None);
        assert_eq!(rest("hello"), "hello");
    }
}
