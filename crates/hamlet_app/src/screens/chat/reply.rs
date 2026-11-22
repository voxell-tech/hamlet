//! A message that answers another, and the mark that starts one.

use std::collections::HashMap;

use bevy::app::App;
use bevy::camera::visibility::Visibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::lifecycle::Insert;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::system::Query;
use bevy::ecs::world::World;
use bevy::picking::hover::Hovered;
use bevy::ui::{AlignItems, JustifyContent, Overflow, UiRect, px};
use bevy_fynix::tokens::{TextTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, Span, button, column, frame, label,
    rich, row,
};
use bevy_fynix::{ScopedExt, ViewExt, derived, every_frame, keyed};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_ui::styles::{
    Fills, ICON, Looks, Tipped, bare, close_button, small_rules,
};
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::ellipsis::{cut, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, built, with_cx};

use super::ChatSettings;
use crate::convo;

/// The room of a mark about its picture.
pub(super) const AROUND: f32 = 4.0;

/// The room a mark takes beside its message.
pub(super) const ROOM: f32 = ICON + AROUND * 3.0;

/// The most characters of what is replied to shown on one line.
const SHOWN: usize = 160;

/// The head of a reply's first line.
const HEAD: &str = "(Replying to ";

pub(super) fn plugin(app: &mut App) {
    app.add_observer(reveal);
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

/// The message the next line in each chat of a Hub answers, kept
/// with the chat's draft while another chat is shown.
#[derive(Component, Default)]
pub struct Replying(HashMap<Entity, Answers>);

impl Replying {
    /// Forgets the reply begun in `chat`, which is gone.
    pub(super) fn forget(&mut self, chat: Entity) {
        self.0.remove(&chat);
    }
}

/// Makes the next line `hub` sends in `chat` answer `answers`, or
/// none.
pub(super) fn reply_in(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    answers: Option<Answers>,
) {
    let Some(mut replying) = world.get_mut::<Replying>(hub) else {
        return;
    };
    match answers {
        Some(answers) => replying.0.insert(chat, answers),
        None => replying.0.remove(&chat),
    };
}

/// Whether the next line `hub` sends in `chat` answers a message.
pub(super) fn replies_in(
    world: &World,
    hub: Entity,
    chat: Entity,
) -> bool {
    world
        .get::<Replying>(hub)
        .is_some_and(|replying| replying.0.contains_key(&chat))
}

/// The message the next line in the chat `hub` shows answers.
fn answers_in(world: &World, hub: Entity) -> Option<&Answers> {
    let open = convo::chat_of(world, hub);
    world.get::<Replying>(hub)?.0.get(&open)
}

/// `words` sent as a reply to `answers`, under a line quoting it.
pub fn replied(answers: &Answers, words: &str) -> String {
    let said = serde_json::Value::from(answers.text.as_str());
    format!("{HEAD}{}: {said})\n\n{words}", answers.by)
}

/// What `text` replies to and the words after it.
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

/// The words `text` quotes of the message it replies to, and the
/// rest.
pub fn own_words(text: &str) -> (Option<String>, String) {
    let Some((answers, words)) = unreplied(text) else {
        return (None, text.to_string());
    };
    let parts = super::quote::parts(words);
    let quoted = parts
        .iter()
        .filter(|(quoted, _)| *quoted)
        .map(|(_, said)| squashed(said))
        .collect::<Vec<_>>();
    match quoted.as_slice() {
        [said] if squashed(&answers.text).contains(said.as_str()) => {
            let own = parts
                .into_iter()
                .filter(|(quoted, _)| !quoted)
                .map(|(_, words)| words)
                .collect::<Vec<_>>()
                .join("\n\n");
            (Some(said.clone()), own)
        }
        _ => (None, words.to_string()),
    }
}

fn squashed(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of an entry a message can reply to or select.
pub(super) fn words(entry: &Entry) -> Option<&str> {
    match entry {
        Entry::Chief { text, .. } => Some(rest(text)),
        Entry::Assistant(text)
        | Entry::Agent { text, .. }
        | Entry::Answered(text) => Some(text),
        _ => None,
    }
}

pub(super) fn can_reply_to(entry: &Entry) -> bool {
    words(entry).is_some_and(|text| !text.trim().is_empty())
}

/// Makes the next message in `hub` reply to entry `id` of `chat`,
/// said by `by`.
fn take(
    world: &mut World,
    hub: Entity,
    chat: Entity,
    id: &str,
    by: &str,
) {
    // A row left behind by a chat the Hub has gone from replies to
    // nothing.
    if convo::chat_of(world, hub) != chat {
        return;
    }
    let most = world.resource::<ChatSettings>().reply_chars;
    let answers = world
        .get::<Transcript>(chat)
        .and_then(|said| said.item(id).map(|item| &item.entry))
        .and_then(words)
        .map(|text| Answers::of(by, text, most));
    let Some(answers) = answers else {
        return;
    };
    reply_in(world, hub, chat, Some(answers));
    super::focus(world, hub);
}

/// `text` as a reply in `hub`, if one waits, and the bytes put
/// before it.
pub(super) fn attach(
    world: &World,
    hub: Entity,
    text: String,
) -> (String, usize) {
    if text.trim().is_empty() {
        return (text, 0);
    }
    match answers_in(world, hub) {
        Some(answers) => {
            let words = text.trim();
            let said = replied(answers, words);
            let shift = said.len() - words.len();
            (said, shift)
        }
        None => (text, 0),
    }
}

/// On a row of the chat whose message can be replied to.
#[derive(Component, Clone, Copy, Debug)]
pub struct Replies;

/// The mark of a row, which replies to its message.
#[derive(Component)]
struct Mark;

/// The mark beside message `id` of `chat` in `hub`, shown on hover.
pub(super) fn mark(
    hub: Entity,
    chat: Entity,
    id: String,
    by: String,
) -> impl View {
    built(move |cx| {
        let reply = button(glyph(Glyph::Talk, ICON))
            .padding(UiRect::all(px(AROUND)))
            .tip("Reply")
            .variant(Variant::Text)
            .on_activate(move |world| {
                take(world, hub, chat, &id, &by);
            });
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
    hover: On<Insert<Hovered>>,
    rows: Query<&Hovered, With<Replies>>,
    under: Query<&Children>,
    mut marks: Query<&mut Visibility, With<Mark>>,
) {
    let row = hover.entity;
    let Ok(hovered) = rows.get(row) else {
        return;
    };
    show_marks(row, hovered.get(), &under, &mut marks);
}

/// Shows or hides the marks `M` under `row`.
pub(super) fn show_marks<M: Component>(
    row: Entity,
    shown: bool,
    under: &Query<&Children>,
    marks: &mut Query<&mut Visibility, With<M>>,
) {
    let want = match shown {
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

/// The message a reply answers, `selected` words highlighted.
pub(super) fn answered(
    answers: &Answers,
    selected: Option<String>,
) -> impl View + use<> {
    let answers = answers.clone();
    with_cx(move |cx| {
        let theme = cx.theme();
        let said = squashed(&answers.text);
        let lit = selected
            .as_deref()
            .and_then(|lit| Some((said.find(lit)?, lit.len())));
        let spans = match lit {
            Some((at, len)) => vec![
                Span::new(&said[..at]),
                Span::new(&said[at..at + len])
                    .color(theme.tone(Tone::Body))
                    .background(theme.palette.text_selection),
                Span::new(&said[at + len..]),
            ],
            None => vec![Span::new(said.clone())],
        };
        row((
            frame()
                .width(px(3.0))
                .shrink(0.0)
                .fill(theme.palette.accent_fill),
            column((
                label(format!("Replying to {}", answers.by))
                    .bold(true)
                    .wrap(false)
                    .tone(Tone::Faint)
                    .rules(small_rules),
                rich(spans).tone(Tone::Dim).size(theme.text.small),
            ))
            .gap(2.0)
            .fills_width(),
        ))
        .gap(8.0)
        .align(AlignItems::Stretch)
        .padding(UiRect::bottom(px(4.0)))
    })
}

/// The message the next one in `hub` answers, over its field.
pub(super) fn preview(hub: Entity) -> impl View {
    keyed(
        derived(move |world: &World| answers_in(world, hub).cloned())
            .when(every_frame()),
        move |answers| match answers.clone() {
            Some(answers) => {
                with_cx(move |cx| shown(cx, hub, &answers)).boxed()
            }
            None => frame().boxed(),
        },
    )
    .within(bare())
}

fn shown(cx: &Cx, hub: Entity, answers: &Answers) -> AnyView {
    let drop = close_button(move |world| {
        let chat = convo::chat_of(world, hub);
        reply_in(world, hub, chat, None);
    })
    .tip("Do not reply");
    row((
        frame()
            .width(px(3.0))
            .shrink(0.0)
            .fill(cx.theme().palette.mark),
        column((
            label(format!("Replying to {}", answers.by))
                .bold(true)
                .wrap(false)
                .tone(Tone::Faint)
                .rules(small_rules),
            one_line(
                label(cut(&answers.text, SHOWN)).tone(Tone::Dim),
            )
            .rules(small_rules),
        ))
        .gap(2.0)
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
            "Ada",
            "  Paper or \"ebook\"?\n\n> Both ship: soon.)  \n",
            100,
        );
        let sent = replied(&answers, "Paper.\n\n> quoted\nThanks.");
        assert_eq!(
            sent,
            "(Replying to Ada: \"Paper or \\\"ebook\\\"?\\n\\n> \
             Both ship: soon.)\")\n\nPaper.\n\n> quoted\nThanks."
        );
        assert_eq!(
            unreplied(&sent),
            Some((answers, "Paper.\n\n> quoted\nThanks."))
        );
    }
}
