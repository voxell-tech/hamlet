//! The chat drawn as a group chat: a bubble for each message, each
//! speaker's picture and name by its run of them, and who is in it.

use std::sync::Arc;

use bevy::color::Color;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::picking::hover::Hovered;
use bevy::text::TextColor;
use bevy::ui::{
    AlignItems, BorderRadius, JustifyContent, Overflow, PositionType,
    UiRect, Val, percent, px,
};
use bevy_fynix::patch::Paint;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, component};
use hamlet_client::chats::{Chat, Entry, Transcript};
use hamlet_ui::markdown::{Area, Block, Kind as Shape, Span, Style};
use hamlet_ui::styles::small_rules;
use hamlet_ui::theme::Palette;
use hamlet_ui::theme::contrast::{self, TEXT};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::entry_view;
use super::lines::{Board, Row, fold_view};
use super::quote;
use super::reply::{self, Replies};
use super::rewind::{self, Rewinds};
use super::timeline::{Kind, Line, Who};
use crate::convo::Convo;
use crate::markdown;
use crate::select::{self, Quotable};

/// The room at the left of the chat that a speaker's picture stands
/// in, and the width of the picture.
const GUTTER: f32 = 30.0;
const FACE: f32 = 24.0;

/// The room a bubble leaves at the far side, so that whose it is
/// shows from where it sits.
const FAR: f32 = 26.0;

/// The room over a run, above the room between any two rows.
const RUN: f32 = 9.0;

/// The corner of a bubble, and the one by the picture of the first
/// bubble of a run.
const ROUND: f32 = 10.0;
const TAIL: f32 = 3.0;

/// The room the hour takes at the end of a message's last line.
const RESERVE: &str =
    "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}";

/// A line of text in a colour that is not one of the theme's tones.
pub(super) fn inked(
    text: String,
    colour: Color,
    size: f32,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node =
            cx.build(label(text).size(size).bold(true).wrap(false));
        let mut entity = cx.world.entity_mut(node);
        if let Some(mut paint) = entity.get_mut::<Paint>() {
            paint.ink = colour;
        }
        if let Some(mut ink) = entity.get_mut::<TextColor>() {
            ink.0 = colour;
        }
        node
    })
}

/// A speaker's picture: its first letter on its robot's paint. The
/// mayor's is ringed in gold, and a chief's is square.
fn face(cx: &Cx, who: &Who, size: f32) -> impl View + use<> {
    let theme = cx.theme();
    let letter = who
        .name
        .chars()
        .next()
        .map(|first| first.to_uppercase().collect::<String>())
        .unwrap_or_default();
    let ink = contrast::clearer(
        theme.palette.gold_1,
        theme.palette.surface,
        who.paint,
    );
    let face = row((inked(letter, ink, size * 0.5),))
        .width(px(size))
        .height(px(size))
        .shrink(0.0)
        .justify(JustifyContent::Center)
        .align(AlignItems::Center)
        .fill(who.paint);
    match who.kind {
        Kind::Agent => face.radius(size / 2.0),
        Kind::Mayor | Kind::Hand => face
            .radius(size / 2.0)
            .border(1.5)
            .border_color(theme.palette.gold_3),
        Kind::Me | Kind::Chief => face
            .radius(theme.space.round)
            .border(1.0)
            .border_color(theme.palette.gold_2),
    }
}

/// `blocks` with room for the hour at the end of the last line: on
/// the last paragraph, or on a line of its own after any other block.
fn kept_room(
    mut blocks: Vec<Arc<Block>>,
    reserve: &'static str,
) -> Vec<Arc<Block>> {
    if reserve.is_empty() {
        return blocks;
    }
    let room = Span {
        text: reserve.to_string(),
        style: Style::default(),
        link: None,
    };
    match blocks.last_mut().map(Arc::make_mut) {
        Some(Block {
            kind: Shape::Paragraph(spans),
            ..
        }) => spans.push(room),
        _ => blocks.push(Arc::new(Block {
            quote: 0,
            indent: 0,
            marker: None,
            kind: Shape::Paragraph(vec![room]),
        })),
    }
    blocks
}

/// The hour and minute of `at`, in milliseconds since 1970.
fn hour(at: u64) -> String {
    chrono::DateTime::from_timestamp_millis(at as i64)
        .filter(|_| at > 0)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// The bubble of a message.
#[derive(Component)]
pub(super) struct Bubble;

/// The words of a message, by how they are set.
#[derive(Clone, Copy, PartialEq)]
enum Words {
    /// As they were typed.
    Plain,
    Markdown,
    Fault,
}

/// One message: a bubble at the right for the chief's own, and at
/// the left by its speaker's picture for anyone else's.
fn said<B: Board>(
    line: Line,
    session: Entity,
    id: String,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let cast = cx.world.resource::<B>().lines().cast();
        let who = cast.get(line.by).cloned().unwrap_or_default();
        // Beside the agent and the chief, a picture alone does not
        // say who speaks.
        let group = cast.len() > 2;
        // The agent the chat is with comes first, and what is said
        // to it or to the chief needs no saying.
        let to = cast
            .get(line.to)
            .filter(|to| line.to != 0 && to.kind != Kind::Me)
            .map(|to| to.name.clone());
        let entry = cx
            .world
            .get::<Transcript>(session)
            .and_then(|t| t.item(&id).map(|item| &item.entry));
        let (words, typed) = match entry {
            Some(Entry::Chief { text, .. }) => {
                (Words::Plain, text.as_str())
            }
            Some(Entry::Answered(said)) => {
                (Words::Plain, said.as_str())
            }
            Some(Entry::Error(_)) => (Words::Fault, ""),
            _ => (Words::Markdown, ""),
        };
        // The message a reply answers is drawn on its own, with what
        // the reply quotes of it.
        let (answers, selected, typed) = match reply::unreplied(typed)
        {
            Some((answers, _)) => {
                let (selected, own) = reply::own_words(typed);
                (Some(answers), selected, own)
            }
            None => (None, None, typed.to_string()),
        };
        // A past session is only read.
        let past = cx
            .world
            .get::<Chat>(session)
            .is_some_and(|chat| chat.0.past.is_some());
        let replies = words != Words::Fault
            && !past
            && entry.is_some_and(reply::can_reply_to);
        let mine = who.kind == Kind::Me;
        // The chief's own words in its own conversation with the
        // agent are what that conversation can be taken back to.
        let back = cx
            .world
            .get::<Chat>(session)
            .is_none_or(|chat| chat.0.from.is_none())
            && mine
            && matches!(entry, Some(Entry::Chief { .. }))
            && words == Words::Plain
            && session == cx.world.resource::<Convo>().session;
        let palette = &cx.theme().palette;
        let fill: fn(&Palette) -> Color =
            if mine { |p| p.gold_6 } else { |p| p.card };
        let time = hour(line.at);

        let mut inside = Vec::new();
        if line.named && !mine && group {
            let mut head = vec![inked(
                who.name.clone(),
                contrast::reach(who.paint, &[fill(palette)], TEXT),
                12.0,
            )];
            if let Some(to) = to {
                head.push(
                    label(format!("to {to}"))
                        .wrap(false)
                        .tone(Tone::Dim)
                        .rules(small_rules)
                        .boxed(),
                );
            }
            head.push(frame().grow(1.0).min_width(px(4.0)).boxed());
            // The panel over the chat says what its own agent is
            // for.
            let tag = if line.by == 0 { "" } else { &who.tag };
            let tag = label(tag.to_string()).wrap(false);
            head.push(match who.kind {
                Kind::Mayor | Kind::Hand => tag
                    .bold(true)
                    .tone(Tone::Faint)
                    .rules(small_rules)
                    .boxed(),
                _ => tag.tone(Tone::Dim).rules(small_rules).boxed(),
            });
            inside.push(
                row(head)
                    .gap(6.0)
                    .align(AlignItems::Center)
                    .overflow(Overflow::clip())
                    .boxed(),
            );
        }
        let typed_id = id.clone();
        let reserve = if time.is_empty() { "" } else { RESERVE };
        // The words of a message, selected as one, with the quote
        // button at the end of what is selected.
        let selectable = |said: AnyView| match replies {
            true => column((
                said,
                select::quote_button(session, id.clone()).boxed(),
            ))
            .min_width(px(0.0))
            .max_width(percent(100.0))
            .tagged(Area)
            .boxed(),
            false => column((said,))
                .min_width(px(0.0))
                .max_width(percent(100.0))
                .boxed(),
        };
        // Words with a link in them are set once, span by span.
        let linked = match quote::has_quote(&typed) {
            true => {
                Some(quote::drawn(&typed, reserve, palette.gold_4))
            }
            false => hamlet_ui::markdown::typed(&typed, reserve),
        };
        let hour = label(time)
            .wrap(false)
            .tone(if mine { Tone::Faint } else { Tone::Dim })
            .rules(small_rules);
        // The hour stands outside the words, so it is never selected
        // with them, in the room kept at the end of their last line.
        let hour = row((hour,))
            .position(PositionType::Absolute)
            .inset(UiRect::new(
                Val::Auto,
                px(9.0),
                Val::Auto,
                px(5.0),
            ))
            .boxed();
        match words {
            Words::Plain => {
                if let Some(answers) = &answers {
                    inside.push(
                        reply::answered(answers, selected.clone())
                            .boxed(),
                    );
                }
                let said = linked.unwrap_or_else(|| {
                    label(component::<Transcript, _>(
                        session,
                        move |t| match t.and_then(|t| {
                            t.item(&typed_id).map(|item| &item.entry)
                        }) {
                            Some(Entry::Chief { text, .. }) => {
                                format!(
                                    "{}{reserve}",
                                    reply::own_words(text).1
                                )
                            }
                            Some(Entry::Answered(said)) => {
                                format!("{said}{reserve}")
                            }
                            _ => String::new(),
                        },
                    ))
                    .boxed()
                });
                inside.push(selectable(said));
                inside.push(hour);
            }
            Words::Markdown => {
                let blocks = markdown::entry(session, id.clone())
                    .map(move |blocks| kept_room(blocks, reserve));
                inside.push(selectable(
                    hamlet_ui::markdown::document(blocks, Tone::Body)
                        .boxed(),
                ));
                inside.push(hour);
            }
            // Beside the button that tries again.
            Words::Fault => {
                inside.push(selectable(entry_view(
                    session,
                    id.clone(),
                )));
                inside.push(hour);
            }
        }
        let bubble = column(inside)
            .gap(2.0)
            .padding(UiRect::axes(px(10.0), px(6.0)))
            // No wider than its row, however long the names over
            // its words are: what does not fit there is cut.
            .min_width(px(0.0))
            .max_width(percent(100.0))
            .fill(fill(palette))
            .border(1.0)
            .border_color(rewind::ringed(
                session,
                id.clone(),
                if mine { palette.gold_6 } else { palette.border },
                palette.accent,
            ))
            .corners({
                let tail = px(if line.named { TAIL } else { ROUND });
                let round = BorderRadius::all(px(ROUND));
                if mine {
                    BorderRadius {
                        top_right: tail.into(),
                        ..round
                    }
                } else {
                    BorderRadius {
                        top_left: tail.into(),
                        ..round
                    }
                }
            })
            .tagged(Bubble);
        let above = px(if line.lead { RUN } else { 0.0 });
        let (replying, reply_room) = match replies {
            true => (
                reply::mark(session, id.clone(), who.name.clone())
                    .boxed(),
                reply::ROOM,
            ),
            false => (frame().boxed(), 0.0),
        };
        let view = if mine {
            let (mark, room) = match back {
                true => (
                    rewind::mark(session, id.clone()).boxed(),
                    rewind::ROOM,
                ),
                false => (frame().boxed(), 0.0),
            };
            let marks = row((replying, mark))
                .rules(rewind::hiding(session, id.clone()));
            row((marks, bubble))
                .gap(0.0)
                .justify(JustifyContent::FlexEnd)
                .padding(UiRect::new(
                    px(GUTTER + FAR - room - reply_room),
                    px(0.0),
                    above,
                    px(0.0),
                ))
                .boxed()
        } else {
            let picture = match line.named {
                true => {
                    with_cx(move |cx| face(cx, &who, FACE)).boxed()
                }
                false => frame().boxed(),
            };
            row((
                column((picture,))
                    .width(px(GUTTER))
                    .shrink(0.0)
                    .justify(JustifyContent::FlexStart),
                bubble,
                replying,
            ))
            .gap(0.0)
            .padding(UiRect::new(
                px(0.0),
                px(FAR - reply_room),
                above,
                px(0.0),
            ))
            .boxed()
        };
        let node = cx.build(view);
        let mut built = cx.world.entity_mut(node);
        if back || replies {
            built.insert(Hovered::default());
        }
        if back {
            built.insert(Rewinds {
                session,
                entry: id.clone(),
                at: line.at,
            });
        }
        if replies {
            built.insert((Replies, Quotable { session, entry: id }));
        }
        node
    })
}

/// A row of the chat, as board `B` holds it.
pub(super) fn line_view<B: Board>(line: &Line) -> AnyView {
    let (view, session) = match &line.row {
        Row::Entry { session, id } => {
            (said::<B>(line.clone(), *session, id.clone()), *session)
        }
        // A turn's steps are the agent's own, so they sit where its
        // bubbles do.
        Row::Fold { session, first, .. } => (
            row((fold_view::<B>(
                line.key(),
                *session,
                first.clone(),
            ),))
            .padding(UiRect::new(
                px(GUTTER),
                px(0.0),
                px(if line.lead { RUN } else { 0.0 }),
                px(0.0),
            ))
            .boxed(),
            *session,
        ),
    };
    view.rules(rewind::fading(session, line.at)).boxed()
}
