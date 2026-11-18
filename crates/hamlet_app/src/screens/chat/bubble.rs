//! The chat drawn as a group chat, a bubble for each message.

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
    BehaviorExt, FrameProps, button, column, frame, label, row,
};
use bevy_fynix::{
    ScopedExt, ViewExt, Visual, component, keyed, resource,
};
use hamlet_client::chats::{Entry, Transcript};
use hamlet_protocol::client::ItemMessage;
use hamlet_ui::markdown::{Area, Block, Kind as Shape, Span, Style};
use hamlet_ui::styles::{note, small_rules, text_button};
use hamlet_ui::theme::contrast::{self, TEXT};
use hamlet_ui::theme::{Palette, StatusKind};
use hamlet_ui::widgets::ellipsis::{cut, one_line};
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use super::entry::{OpenTools, entry_view, toggle};
use super::lines::{Board, INSET, Row, fold_view};
use super::reply::{self, Replies};
use super::rewind::{self, Rewinds};
use super::timeline::{Kind, Line, Who};
use super::{attach, card, edits, quote};
use crate::select::Quotable;
use crate::{convo, markdown};

/// The room at the left a speaker's picture stands in, and its width.
const GUTTER: f32 = 30.0;
const FACE: f32 = 24.0;

/// The room a bubble leaves at the far side.
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

/// A speaker's picture.
fn face(cx: &Cx, who: &Who, size: f32) -> impl View + use<> {
    let theme = cx.theme();
    let mark = match who.kind {
        Kind::Agent => Glyph::Robot,
        Kind::Lead => Glyph::Lead,
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
        Kind::Lead | Kind::Hand => face
            .radius(size / 2.0)
            .border(1.5)
            .border_color(theme.palette.gold_3),
        Kind::Me | Kind::Chief => face
            .radius(theme.space.round)
            .border(theme.space.border)
            .border_color(theme.palette.gold_2),
    }
}

/// `blocks` with room for the hour at the end of the last line.
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

#[derive(Component)]
pub(super) struct Bubble;

/// The words of a message, by how they are set.
#[derive(Clone, Copy, PartialEq)]
enum Words {
    Plain,
    Markdown,
}

/// One message: the chief's on the right, others' by their picture.
fn said<B: Board>(
    hub: Entity,
    line: Line,
    chat: Entity,
    id: String,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let cast = cx
            .world
            .get::<B>(hub)
            .map(|board| board.lines().cast())
            .unwrap_or_default();
        let who = cast.get(line.by).cloned().unwrap_or_default();
        // Past two speakers, a picture alone does not say who speaks.
        let group = cast.len() > 2;
        // Words to the chat's agent or the chief name no one.
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
        let (words, typed) = match entry {
            Some(Entry::Chief { text, .. }) => {
                (Words::Plain, text.as_str())
            }
            Some(Entry::Answered(said)) => {
                (Words::Plain, said.as_str())
            }
            _ => (Words::Markdown, ""),
        };
        // Attachment places count the reply line's bytes too.
        let head = typed.len() - reply::rest(typed).len();
        let (answers, selected, typed) = match reply::unreplied(typed)
        {
            Some((answers, _)) => {
                let (selected, own) = reply::own_words(typed);
                (Some(answers), selected, own)
            }
            None => (None, None, typed.to_string()),
        };
        // A merged conversation is only read.
        let own = chat == convo::chat_of(cx.world, hub);
        let replies =
            own && !queued && entry.is_some_and(reply::can_reply_to);
        let mine = who.kind == Kind::Me;
        let chief_line =
            mine && own && matches!(entry, Some(Entry::Chief { .. }));
        let back = chief_line && !queued;
        let withdraws = chief_line && queued;
        let palette = &cx.theme().palette;
        let border = cx.theme().space.border;
        let corner = cx.theme().space.round_card;
        let (gap, padding_x) =
            (cx.theme().space.gap, cx.theme().space.card_padding_x);
        let fill: fn(&Palette) -> Color =
            if mine { |p| p.gold_6 } else { |p| p.card };
        // A queued line has no hour yet.
        let time = match queued {
            true => String::new(),
            false => hour(line.at),
        };
        let ink = contrast::reach(who.paint, &[fill(palette)], TEXT);

        let mut inside = Vec::new();
        if line.named && !mine && group {
            let mut head = vec![inked(who.name.clone(), ink, 12.0)];
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
            // The chat's own agent is tagged in the header.
            let tag = if line.by == 0 { "" } else { &who.tag };
            let tag = label(tag.to_string()).wrap(false);
            head.push(match who.kind {
                Kind::Lead | Kind::Hand => tag
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
        let selectable = |said: AnyView| {
            let words = column((said,))
                .min_width(px(0.0))
                .max_width(percent(100.0));
            match replies {
                true => words.tagged(Area).boxed(),
                false => words.boxed(),
            }
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
        // Outside the words, so it is never selected with them.
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
                if !images.is_empty() {
                    inside.push(attach::tiles(images).boxed());
                }
                // Places count the words drawn, unless a quote was
                // taken out.
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
            }
            Words::Markdown => {
                let blocks = markdown::entry(chat, id.clone())
                    .map(move |blocks| kept_room(blocks, reserve));
                inside.push(selectable(
                    hamlet_ui::markdown::document(blocks, Tone::Body)
                        .boxed(),
                ));
            }
        }
        inside.extend(hour);
        let bubble = column(inside)
            .gap(gap / 4.0)
            .padding(UiRect::axes(px(padding_x), px(6.0)))
            .min_width(px(0.0))
            .max_width(percent(100.0))
            .fill(fill(palette))
            .border(border)
            .border_color(rewind::ringed(
                hub,
                chat,
                id.clone(),
                palette.gold_6,
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
        let above = px(if line.lead { gap } else { 0.0 });
        let (replying, reply_room) = match replies {
            true => (
                reply::mark(hub, chat, id.clone(), who.name.clone())
                    .boxed(),
                reply::ROOM,
            ),
            false => (frame().boxed(), 0.0),
        };
        let view =
            if mine {
                let (mark, room) = match (back, withdraws) {
                    (true, _) => (
                        rewind::mark(hub, chat, id.clone()).boxed(),
                        reply::ROOM,
                    ),
                    (_, true) => (
                        rewind::withdraw(chat, id.clone()).boxed(),
                        reply::ROOM,
                    ),
                    _ => (frame().boxed(), 0.0),
                };
                let marks = row((replying, mark))
                    .rules(rewind::hiding(hub, chat, id.clone()));
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
                    true => with_cx(move |cx| face(cx, &who, FACE))
                        .boxed(),
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
                hub,
                chat,
                entry: id.clone(),
                at: line.at,
            });
        }
        if replies {
            built.insert((
                Replies,
                Quotable {
                    hub,
                    chat,
                    entry: id,
                },
            ));
        }
        node
    })
}

/// The agent's own words across the chat with no bubble.
fn written(
    hub: Entity,
    chat: Entity,
    id: String,
    name: String,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let entry = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id))
            .map(|item| item.entry.clone());
        // A merged conversation is only read.
        let own = chat == convo::chat_of(cx.world, hub);
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
            false => entry_view(hub, chat, id.clone()),
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
            true => reply::mark(hub, chat, id.clone(), name).boxed(),
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
                Quotable {
                    hub,
                    chat,
                    entry: id,
                },
            ));
        }
        node
    })
}

/// The most letters of a message's first line a folded card lays out.
const PREVIEW: usize = 200;

/// The first line of `text` with words, without heading or quote
/// marks.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .map(|line| line.trim().trim_start_matches(['#', '>']).trim())
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    cut(line, PREVIEW)
}

/// A message from another agent, as a card a press opens.
fn message(hub: Entity, chat: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let item = cx
            .world
            .get::<Transcript>(chat)
            .and_then(|t| t.item(&id))
            .cloned();
        let Some(Entry::Agent {
            name,
            message,
            text,
            ..
        }) = item.as_ref().map(|item| &item.entry)
        else {
            return cx.build(frame());
        };
        let queued = item.as_ref().is_some_and(|item| item.queued);
        let own = chat == convo::chat_of(cx.world, hub);
        let replies = own
            && !queued
            && item
                .as_ref()
                .is_some_and(|i| reply::can_reply_to(&i.entry));
        let failed = *message == Some(ItemMessage::ErrorResponse);
        let heading = match message {
            Some(ItemMessage::Notification) => "Notification from",
            Some(ItemMessage::Request) => "Request from",
            Some(ItemMessage::Response) => "Response from",
            Some(ItemMessage::ErrorResponse) => "Error response from",
            None => "Message from",
        };
        let heading = format!("{heading} {name}");
        let head = hamlet_ui::markdown::typed(
            &heading,
            "",
            hamlet_ui::markdown::is_address_in(cx.world),
        )
        .unwrap_or_else(|| label(heading).bold(true).boxed());
        let key = (hub, chat, id.clone());
        let is_open = {
            let key = key.clone();
            move |open: &OpenTools| open.0.contains(&key)
        };
        let first = first_line(text);
        let preview =
            keyed(resource::<OpenTools, _>(is_open.clone()), {
                move |open| match open {
                    true => frame().boxed(),
                    false => one_line(label(first.clone()).tone(
                        match failed {
                            true => Tone::Critical,
                            false => Tone::Dim,
                        },
                    ))
                    .boxed(),
                }
            })
            .within(
                row(())
                    .grow(1.0)
                    .min_width(px(0.0))
                    .overflow(Overflow::clip_x()),
            );
        let gap = cx.theme().space.gap;
        let turned = is_open.clone();
        let head = button(
            row((
                glyph(Glyph::Chevron, 10.0).tone(Tone::Dim).rotation(
                    resource::<OpenTools, _>(move |open| {
                        if turned(open) { 180.0 } else { 90.0 }
                    }),
                ),
                row((head,)).shrink(0.0),
                preview,
            ))
            .gap(gap / 2.0)
            .align(AlignItems::Center)
            .width(percent(100.0))
            .min_width(px(0.0))
            .overflow(Overflow::clip_x())
            .rules(small_rules),
        )
        .width(percent(100.0))
        .min_width(px(0.0))
        .padding(UiRect::all(px(0.0)))
        .rules(text_button)
        .on_activate(move |world| toggle(world, key.clone()));
        let said = id.clone();
        let body =
            keyed(resource::<OpenTools, _>(is_open), move |open| {
                if !*open {
                    return frame().boxed();
                }
                let words = column((hamlet_ui::markdown::document(
                    markdown::entry(chat, said.clone()),
                    Tone::Body,
                ),))
                .min_width(px(0.0))
                .max_width(percent(100.0))
                .padding(UiRect::top(px(gap / 2.0)));
                match replies {
                    true => words.tagged(Area).boxed(),
                    false => words.boxed(),
                }
            })
            .within(
                column(()).width(percent(100.0)).min_width(px(0.0)),
            );
        let card = hamlet_ui::kinds::card((head, body))
            .gap(0.0)
            .grow(1.0)
            .min_width(px(0.0));
        let card = match failed {
            true => card.status(StatusKind::Danger).boxed(),
            false => card.boxed(),
        };
        let replying = match replies {
            true => reply::mark(hub, chat, id.clone(), name.clone())
                .boxed(),
            false => frame().boxed(),
        };
        let node = cx.build(
            row((card, replying))
                .width(percent(100.0))
                .min_width(px(0.0))
                .padding(UiRect::right(
                    px(FAR - reply_room(replies)),
                )),
        );
        if replies {
            cx.world.entity_mut(node).insert((
                Hovered::default(),
                Replies,
                Quotable {
                    hub,
                    chat,
                    entry: id,
                },
            ));
        }
        node
    })
}

fn reply_room(replies: bool) -> f32 {
    if replies { reply::ROOM } else { 0.0 }
}

/// How opaque a row that waits its turn is.
const WAITING: f32 = 0.55;

/// A row of `hub`'s chat, as its board `B` holds it.
pub(super) fn line_view<B: Board>(
    hub: Entity,
    line: &Line,
) -> AnyView {
    let line = line.clone();
    AnyView::new(move |cx: &mut Cx| {
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
                let entry = cx
                    .world
                    .get::<Transcript>(*chat)
                    .and_then(|t| t.item(id))
                    .map(|item| &item.entry);
                let own = matches!(
                    entry,
                    Some(
                        Entry::Assistant(_)
                            | Entry::Error(_)
                            | Entry::Notice(_)
                    )
                );
                let sent = matches!(entry, Some(Entry::Agent { .. }));
                let name = cx
                    .world
                    .get::<B>(hub)
                    .and_then(|board| {
                        board.lines().cast().get(line.by)
                    })
                    .map(|who| who.name.clone())
                    .unwrap_or_default();
                let view = match (own, sent) {
                    (true, _) => {
                        aside(written(hub, *chat, id.clone(), name))
                    }
                    (_, true) => {
                        aside(message(hub, *chat, id.clone()))
                    }
                    _ => said::<B>(
                        hub,
                        line.clone(),
                        *chat,
                        id.clone(),
                    ),
                };
                (view, *chat, id.as_str())
            }
            Row::Changes {
                chat, first, edits, ..
            } => (
                aside(edits::view(
                    hub,
                    *chat,
                    first.clone(),
                    edits.clone(),
                )),
                *chat,
                first.as_str(),
            ),
            Row::Card { chat, id } => (
                aside(card::view(hub, *chat, id.clone()).boxed()),
                *chat,
                id.as_str(),
            ),
            Row::Fold { chat, first, .. } => (
                aside(
                    fold_view::<B>(
                        hub,
                        line.key(),
                        *chat,
                        first.clone(),
                    )
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
                view.rules(rewind::fading(hub, chat, line.at)).boxed()
            }
        };
        cx.build(view)
    })
}
