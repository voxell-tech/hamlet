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
use bevy_fynix::{ScopedExt, ViewExt, Visual, component};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_ui::markdown::{Area, Block, Kind as Shape, Span, Style};
use hamlet_ui::styles::{note, small_rules};
use hamlet_ui::theme::Palette;
use hamlet_ui::theme::contrast::{self, TEXT};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::entry_view;
use super::lines::{Board, INSET, Row, fold_view};
use super::reply::{self, Replies};
use super::rewind::{self, Rewinds};
use super::timeline::{Kind, Line, Who};
use super::{attach, card, edits, quote};
use crate::convo::Convo;
use crate::markdown;
use crate::select::Quotable;

/// The room at the left of the chat that a speaker's picture stands
/// in, and the width of the picture.
const GUTTER: f32 = 30.0;
const FACE: f32 = 24.0;

/// The room a bubble leaves at the far side, so that whose it is
/// shows from where it sits.
const FAR: f32 = 24.0;

/// The corner by the picture of the first bubble of a run.
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

/// A speaker's picture: the mark the Hub's tree gives it, on its
/// robot's paint. The mayor's is ringed in gold, and a chief's is
/// square.
fn face(cx: &Cx, who: &Who, size: f32) -> impl View + use<> {
    let theme = cx.theme();
    let mark = match who.kind {
        Kind::Agent => Glyph::Robot,
        Kind::Mayor => Glyph::Mayor,
        Kind::Hand => Glyph::Tower,
        Kind::Me | Kind::Chief => Glyph::Hearth,
    };
    let ink = contrast::clearer(
        theme.palette.gold_1,
        theme.palette.surface,
        who.paint,
    );
    let face =
        row((glyph(mark, (size * 0.6).round()).tint(Some(ink)),))
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
            .border(theme.space.border)
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
    jiff::Timestamp::from_millisecond(at as i64)
        .ok()
        .filter(|_| at > 0)
        .map(|at| {
            at.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M")
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
fn said<B: Board>(line: Line, chat: Entity, id: String) -> AnyView {
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
        let item = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id));
        let entry = item.map(|item| &item.entry);
        let queued = item.is_some_and(|item| item.queued);
        // Another agent's words, never drawn as the chief's.
        let peer = matches!(entry, Some(Entry::Agent { .. }));
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
        // The bytes the reply line puts before the words, which the
        // places of attachments count.
        let head = typed.len() - reply::rest(typed).len();
        let (answers, selected, typed) = match reply::unreplied(typed)
        {
            Some((answers, _)) => {
                let (selected, own) = reply::own_words(typed);
                (Some(answers), selected, own)
            }
            None => (None, None, typed.to_string()),
        };
        // A conversation merged in, drawn under its row, is only
        // read.
        let own = chat == cx.world.resource::<Convo>().chat;
        let replies = words != Words::Fault
            && own
            && !queued
            && entry.is_some_and(reply::can_reply_to);
        let mine = who.kind == Kind::Me;
        let chief_line = mine
            && own
            && matches!(entry, Some(Entry::Chief { .. }))
            && words == Words::Plain;
        // The chief's own words are what the conversation can be
        // taken back to, and those that wait their turn can be taken
        // back.
        let back = chief_line && !queued;
        let withdraws = chief_line && queued;
        let palette = &cx.theme().palette;
        let border = cx.theme().space.border;
        let corner = cx.theme().space.round_card;
        let (gap, padding_x) =
            (cx.theme().space.gap, cx.theme().space.card_padding_x);
        let fill: fn(&Palette) -> Color =
            if mine { |p| p.gold_6 } else { |p| p.card };
        // A line that waits its turn says so under the bubble, and
        // has no hour yet.
        let time = match queued {
            true => String::new(),
            false => hour(line.at),
        };
        let ink = contrast::reach(who.paint, &[fill(palette)], TEXT);
        // Another agent's message is edged in its paint.
        let edge = if peer { ink } else { palette.gold_6 };

        let mut inside = Vec::new();
        if line.named && !mine && (group || peer) {
            let mut head = vec![inked(who.name.clone(), ink, 12.0)];
            // Who another agent wrote to, by an arrow.
            let to = match peer {
                true => cast.get(line.to).map(|to| to.name.clone()),
                false => to,
            };
            if peer && let Some(to) = to {
                head.push(
                    glyph(Glyph::Back, 10.0)
                        .tone(Tone::Dim)
                        .rotation(180.0)
                        .boxed(),
                );
                head.push(
                    label(to)
                        .wrap(false)
                        .bold(true)
                        .tone(Tone::Dim)
                        .rules(small_rules)
                        .boxed(),
                );
            } else if let Some(to) = to {
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
        // The words of a message, selected as one.
        let selectable = |said: AnyView| match replies {
            true => column((said,))
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
            true => Some(quote::drawn(
                &typed,
                reserve,
                palette.gold_4,
                Vec::new(),
            )),
            false => hamlet_ui::markdown::typed(
                &typed,
                reserve,
                hamlet_ui::markdown::is_address_in(cx.world),
            ),
        };
        let shown = !time.is_empty();
        let hour = label(time)
            .wrap(false)
            .tone(if mine { Tone::Faint } else { Tone::Dim })
            .rules(small_rules);
        // The hour stands outside the words, so it is never selected
        // with them, in the room kept at the end of their last line.
        let hour = shown.then(|| {
            row((hour,))
                .position(PositionType::Absolute)
                .inset(UiRect::new(
                    Val::Auto,
                    px(9.0),
                    Val::Auto,
                    px(5.0),
                ))
                .boxed()
        });
        match words {
            Words::Plain => {
                if let Some(answers) = &answers {
                    inside.push(
                        reply::answered(answers, selected.clone())
                            .boxed(),
                    );
                }
                let (chips, images) = match entry {
                    Some(Entry::Chief { images, files, .. }) => (
                        attach::sent_chips(images, files),
                        images.as_slice(),
                    ),
                    _ => (Vec::new(), [].as_slice()),
                };
                // The images of the line over its words.
                if !images.is_empty() {
                    inside.push(attach::tiles(images).boxed());
                }
                // The words the places count are the ones drawn,
                // unless the reply's quote was taken out of them.
                let placed = selected.is_none()
                    && chips.iter().any(|(at, _)| at.is_some());
                let (said, after) = if placed {
                    let chips = chips
                        .into_iter()
                        .map(|(at, atom)| {
                            let at = at.and_then(|at| {
                                at.checked_sub(head as u64)
                            });
                            (at, atom)
                        })
                        .collect();
                    let (words, atoms) =
                        attach::marked(&typed, chips);
                    let said = quote::drawn(
                        &words,
                        reserve,
                        palette.gold_4,
                        atoms,
                    );
                    (said, None)
                } else {
                    let said = linked.unwrap_or_else(|| {
                        label(component::<Transcript, _>(
                            chat,
                            move |t| match t.and_then(|t| {
                                t.item(&typed_id)
                                    .map(|item| &item.entry)
                            }) {
                                Some(Entry::Chief {
                                    text, ..
                                }) => {
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
                    (said, Some(chips))
                };
                inside.push(selectable(said));
                // Attachments with no place follow the words.
                if let Some(chips) =
                    after.filter(|chips| !chips.is_empty())
                {
                    inside.push(attach::after_words(chips).boxed());
                }
                inside.extend(hour);
            }
            Words::Markdown => {
                let blocks = markdown::entry(chat, id.clone())
                    .map(move |blocks| kept_room(blocks, reserve));
                inside.push(selectable(
                    hamlet_ui::markdown::document(blocks, Tone::Body)
                        .boxed(),
                ));
                inside.extend(hour);
            }
            // Beside the button that tries again.
            Words::Fault => {
                inside.push(selectable(entry_view(chat, id.clone())));
                inside.extend(hour);
            }
        }
        let bubble = column(inside)
            .gap(gap / 4.0)
            .padding(UiRect::axes(px(padding_x), px(6.0)))
            // No wider than its row, however long the names over
            // its words are: what does not fit there is cut.
            .min_width(px(0.0))
            .max_width(percent(100.0))
            .fill(fill(palette))
            .border(border)
            .border_color(rewind::ringed(
                chat,
                id.clone(),
                edge,
                palette.accent,
            ))
            .corners({
                let tail = px(if line.named { TAIL } else { corner });
                let round = BorderRadius::all(px(corner));
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
        // A gap over the first bubble of a run.
        let above = px(if line.lead { gap } else { 0.0 });
        let (replying, reply_room) = match replies {
            true => (
                reply::mark(chat, id.clone(), who.name.clone())
                    .boxed(),
                reply::ROOM,
            ),
            false => (frame().boxed(), 0.0),
        };
        let view = if mine {
            let (mark, room) = match (back, withdraws) {
                (true, _) => (
                    rewind::mark(chat, id.clone()).boxed(),
                    rewind::ROOM,
                ),
                (_, true) => (
                    rewind::withdraw(chat, id.clone()).boxed(),
                    rewind::ROOM,
                ),
                _ => (frame().boxed(), 0.0),
            };
            let marks = row((replying, mark))
                .rules(rewind::hiding(chat, id.clone()));
            let said = row((marks, bubble))
                .gap(0.0)
                .justify(JustifyContent::FlexEnd)
                .padding(UiRect::new(
                    px(GUTTER + FAR - room - reply_room),
                    px(0.0),
                    above,
                    px(0.0),
                ));
            match queued {
                // Under the bubble's right edge, where it cannot
                // touch the words.
                true => column((
                    said,
                    row((note("Queued"),))
                        .justify(JustifyContent::FlexEnd),
                ))
                .boxed(),
                false => said.boxed(),
            }
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
                chat,
                entry: id.clone(),
                at: line.at,
            });
        }
        if replies {
            built.insert((Replies, Quotable { chat, entry: id }));
        }
        node
    })
}

/// The agent's own words across the chat with no bubble: its reply,
/// why a reply failed, or a notice, by the agent shown as `name`.
fn written(chat: Entity, id: String, name: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let entry = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id))
            .map(|item| item.entry.clone());
        // A conversation merged in, drawn under its row, is only
        // read.
        let own = chat == cx.world.resource::<Convo>().chat;
        let reply = matches!(entry, Some(Entry::Assistant(_)));
        let replies = own
            && reply
            && entry.as_ref().is_some_and(reply::can_reply_to);
        let words = match reply {
            true => hamlet_ui::markdown::document(
                markdown::entry(chat, id.clone()),
                Tone::Body,
            )
            .boxed(),
            false => entry_view(chat, id.clone()),
        };
        let words = column((words,))
            .grow(1.0)
            .min_width(px(0.0))
            .padding(UiRect::horizontal(px(INSET)));
        let words = match replies {
            true => words.tagged(Area).boxed(),
            false => words.boxed(),
        };
        let replying = match replies {
            true => reply::mark(chat, id.clone(), name).boxed(),
            false => frame().boxed(),
        };
        let node = cx.build(
            row((words, replying))
                .width(percent(100.0))
                .min_width(px(0.0)),
        );
        if replies {
            cx.world.entity_mut(node).insert((
                Hovered::default(),
                Replies,
                Quotable { chat, entry: id },
            ));
        }
        node
    })
}

/// How opaque a row that waits its turn is.
const WAITING: f32 = 0.55;

/// A row of the chat, as board `B` holds it.
pub(super) fn line_view<B: Board>(line: &Line) -> AnyView {
    let line = line.clone();
    AnyView::new(move |cx: &mut Cx| {
        // A turn, its reply and the cards are the agent's own, across
        // the chat, a gap over the first of a run.
        let run = cx.theme().space.gap;
        let aside = |view: AnyView| {
            row((view,))
                .width(percent(100.0))
                .min_width(px(0.0))
                .padding(UiRect::top(px(if line.lead {
                    run
                } else {
                    0.0
                })))
                .boxed()
        };
        let (view, chat, id) = match &line.row {
            Row::Entry { chat, id } => {
                let own = cx
                    .world
                    .get::<Transcript>(*chat)
                    .and_then(|t| t.item(id))
                    .is_some_and(|item| {
                        matches!(
                            item.entry,
                            Entry::Assistant(_)
                                | Entry::Error(_)
                                | Entry::Notice(_)
                        )
                    });
                let name = cx
                    .world
                    .resource::<B>()
                    .lines()
                    .cast()
                    .get(line.by)
                    .map(|who| who.name.clone())
                    .unwrap_or_default();
                let view = match own {
                    true => aside(written(*chat, id.clone(), name)),
                    false => {
                        said::<B>(line.clone(), *chat, id.clone())
                    }
                };
                (view, *chat, id.as_str())
            }
            Row::Changes {
                chat, first, edits, ..
            } => (
                aside(edits::view(
                    *chat,
                    first.clone(),
                    edits.clone(),
                )),
                *chat,
                first.as_str(),
            ),
            Row::Card { chat, id } => (
                aside(card::view(*chat, id.clone()).boxed()),
                *chat,
                id.as_str(),
            ),
            Row::Fold { chat, first, .. } => (
                aside(
                    fold_view::<B>(line.key(), *chat, first.clone())
                        .boxed(),
                ),
                *chat,
                first.as_str(),
            ),
        };
        let queued = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(id))
            .is_some_and(|item| item.queued);
        let view = match queued {
            true => view
                .rules(|cx: &mut Cx| {
                    cx.set::<Visual>(|visual, _| {
                        visual.opacity(WAITING)
                    });
                })
                .boxed(),
            false => {
                view.rules(rewind::fading(chat, line.at)).boxed()
            }
        };
        cx.build(view)
    })
}
