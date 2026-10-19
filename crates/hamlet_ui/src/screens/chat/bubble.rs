//! The chat drawn as a group chat: a bubble for each message, each
//! speaker's picture and name by its run of them, and who is in it.

use bevy::color::{Color, Oklcha};
use bevy::ecs::entity::Entity;
use bevy::picking::hover::Hovered;
use bevy::text::TextColor;
use bevy::ui::{
    AlignItems, BorderRadius, Display, FlexWrap, JustifyContent,
    Node, Overflow, PositionType, UiRect, Val, percent, px,
};
use bevy_fynix::patch::Paint;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, TooltipExt, button, column, frame,
    label, row,
};
use bevy_fynix::{ScopedExt, ViewExt, component, keyed, resource};
use hamlet_client::chats::{Entry, Transcript};

use super::entry::entry_view;
use super::lines::{Lines, Row, fold_view};
use super::quote;
use super::reply::{self, Replies};
use super::rewind::{self, Rewinds};
use super::timeline::{Kind, Line, Who};
use crate::looks::{Fills, bare, hairline};
use crate::me::Me;
use crate::platform::DisplayText;
use crate::select::{self, Area, Quotable};
use crate::theme::{Palette, contrast};
use crate::world_menu::{self, Deed};
use crate::{AnyView, Cx, View, markdown, with_cx};

/// The room at the left of the chat that a speaker's picture stands
/// in, and the width of the picture.
const GUTTER: f32 = 30.0;
const FACE: f32 = 24.0;

/// The room a bubble leaves at the far side, so that whose it is
/// shows from where it sits.
const FAR: f32 = 26.0;

/// The room over a run, above the room between any two rows.
const RUN: f32 = 9.0;

/// The corner of a bubble, and the one by the picture of the last
/// bubble of a run.
const ROUND: f32 = 10.0;
const TAIL: f32 = 3.0;

/// The room the hour takes at the end of a message's last line.
const RESERVE: &str =
    "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}";

/// `colour` made lighter, its hue kept, until it reads on
/// `surface`, or `parchment` if it never does.
fn legible(colour: Color, surface: Color, parchment: Color) -> Color {
    let paint = Oklcha::from(colour);
    (0..=12)
        .map(|step| {
            let lightness =
                (paint.lightness + 0.05 * step as f32).min(1.0);
            Color::from(Oklcha { lightness, ..paint })
        })
        .find(|lighter| contrast(*lighter, surface) >= 4.5)
        .unwrap_or(parchment)
}

/// A line of text in a colour that is not one of the theme's tones.
fn inked(text: String, colour: Color, size: f32) -> AnyView {
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
/// mayor's is ringed in brass, and a chief's is square.
fn face(cx: &Cx, who: &Who, size: f32) -> impl View + use<> {
    let theme = cx.theme();
    let letter = who
        .name
        .chars()
        .next()
        .map(|first| first.to_uppercase().collect::<String>())
        .unwrap_or_default();
    let ground = who.paint;
    let tone = if contrast(theme.palette.brass_1, ground) >= 3.0 {
        Tone::Body
    } else {
        Tone::OnAccent
    };
    let face = row((label(letter)
        .size(size * 0.5)
        .bold(true)
        .wrap(false)
        .tone(tone),))
    .width(px(size))
    .height(px(size))
    .shrink(0.0)
    .justify(JustifyContent::Center)
    .align(AlignItems::Center)
    .fill(who.paint);
    match who.kind {
        Kind::Agent => face.radius(size / 2.0),
        Kind::Mayor => face
            .radius(size / 2.0)
            .border(1.5)
            .border_color(theme.palette.brass_3),
        Kind::Me | Kind::Chief => face
            .radius(theme.space.round)
            .border(1.0)
            .border_color(theme.palette.brass_2),
    }
}

/// `first` and then `last` on one line where there is room for
/// both, and `last` on a line of its own under it where there is
/// not.
fn wrapping(first: AnyView, last: AnyView) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let node = cx.build(
            row((first, last)).gap(0.0).align(AlignItems::FlexEnd),
        );
        if let Some(mut node) = cx.world.get_mut::<Node>(node) {
            node.flex_wrap = FlexWrap::Wrap;
        }
        node
    })
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
fn said(line: Line, session: Entity, id: String) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let cast = cx.world.resource::<Lines>().cast();
        let who = cast.get(line.by).cloned().unwrap_or_default();
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
            Some(Entry::User { text, .. }) => {
                (Words::Plain, text.as_str())
            }
            Some(Entry::Answered(said)) => {
                (Words::Plain, said.as_str())
            }
            Some(Entry::Error(_)) => (Words::Fault, ""),
            _ => (Words::Markdown, ""),
        };
        // The message a reply answers is drawn on its own.
        let (answers, typed) = match reply::unreplied(typed) {
            Some((answers, words)) => (Some(answers), words),
            None => (None, typed),
        };
        let replies = words != Words::Fault
            && entry.is_some_and(reply::answerable);
        let mine = who.kind == Kind::Me;
        // The chief's own words in its own conversation with the
        // agent are what that conversation can be taken back to.
        let back = cx
            .world
            .get::<hamlet_client::chats::Chat>(session)
            .is_none_or(|chat| chat.0.from.is_none())
            && mine
            && matches!(entry, Some(Entry::User { .. }))
            && words == Words::Plain
            && session == cx.world.resource::<Me>().session;
        let palette = &cx.theme().palette;
        let fill: fn(&Palette) -> Color =
            if mine { |p| p.brass_6 } else { |p| p.card };
        let time = hour(line.at);
        let key = line.key();

        let mut inside = Vec::new();
        if line.named && !mine {
            let mut head = vec![inked(
                who.name.clone(),
                legible(who.paint, fill(palette), palette.brass_1),
                12.0,
            )];
            if let Some(to) = to {
                head.push(
                    label(format!("to {to}"))
                        .size(11.0)
                        .wrap(false)
                        .tone(Tone::Dim)
                        .boxed(),
                );
            }
            head.push(frame().grow(1.0).min_width(px(4.0)).boxed());
            // The panel over the chat says what its own agent is
            // for.
            let tag = if line.by == 0 { "" } else { &who.tag };
            let tag = label(tag.to_string()).size(10.0).wrap(false);
            head.push(match who.kind {
                Kind::Mayor => tag
                    .bold(true)
                    .tone(Tone::Faint)
                    .tagged(DisplayText)
                    .boxed(),
                _ => tag.tone(Tone::Dim).boxed(),
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
        let linked = match quote::quotes(typed) {
            true => {
                Some(quote::drawn(typed, reserve, palette.brass_4))
            }
            false => markdown::typed(typed, reserve),
        };
        let hour = label(time).size(10.0).wrap(false).tone(if mine {
            Tone::Faint
        } else {
            Tone::Dim
        });
        match words {
            // The hour sits in the room kept at the end of the
            // last line.
            Words::Plain => {
                if let Some(answers) = &answers {
                    inside.push(reply::answered(answers).boxed());
                }
                let said = linked.unwrap_or_else(|| {
                    label(component::<Transcript, _>(
                        session,
                        move |t| match t.and_then(|t| {
                            t.item(&typed_id).map(|item| &item.entry)
                        }) {
                            Some(Entry::User { text, .. }) => {
                                format!(
                                    "{}{reserve}",
                                    reply::rest(text)
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
                inside.push(
                    row((hour,))
                        .position(PositionType::Absolute)
                        .inset(UiRect::new(
                            Val::Auto,
                            px(9.0),
                            Val::Auto,
                            px(5.0),
                        ))
                        .boxed(),
                );
            }
            // The hour sits beside a reply of one short line, and
            // under any other.
            Words::Markdown | Words::Fault => {
                let body = match words {
                    Words::Fault => entry_view(session, id.clone()),
                    _ => markdown::document(
                        markdown::entry(session, id.clone()),
                        Tone::Body,
                    )
                    .boxed(),
                };
                // The hour stands outside the words, so it is never
                // selected with them.
                inside.push(wrapping(
                    selectable(body),
                    row((hour,))
                        .margin(UiRect::left(Val::Auto))
                        .padding(UiRect::left(px(10.0)))
                        .boxed(),
                ));
            }
        }
        let ends_key = key.clone();
        let ends = move |lines: &Lines| lines.ends_run(&ends_key);
        let corner_ends = ends.clone();
        let bubble = column(inside)
            .gap(2.0)
            .padding(UiRect::axes(px(10.0), px(6.0)))
            // No wider than its row, however long the names over
            // its words are: what does not fit there is cut.
            .min_width(px(0.0))
            .max_width(percent(100.0))
            .fill(fill(palette))
            .border(1.0)
            .border_color(if mine {
                palette.brass_6
            } else {
                palette.seam
            })
            .corners(resource::<Lines, _>(move |lines| {
                let tail =
                    px(if corner_ends(lines) { TAIL } else { ROUND });
                let round = BorderRadius::all(px(ROUND));
                if mine {
                    BorderRadius {
                        bottom_right: tail,
                        ..round
                    }
                } else {
                    BorderRadius {
                        bottom_left: tail,
                        ..round
                    }
                }
            }));
        let above = px(if line.lead { RUN } else { 0.0 });
        let (replying, reply_room) = match replies {
            true => (
                reply::mark(session, id.clone(), who.name.clone()),
                reply::ROOM,
            ),
            false => (frame().boxed(), 0.0),
        };
        let view = if mine {
            let (mark, room) = match back {
                true => {
                    (rewind::mark(session, id.clone()), rewind::ROOM)
                }
                false => (frame().boxed(), 0.0),
            };
            row((replying, mark, bubble))
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
            let picture =
                column((with_cx(move |cx| face(cx, &who, FACE)),))
                    .display(resource::<Lines, _>(move |lines| {
                        if ends(lines) {
                            Display::Flex
                        } else {
                            Display::None
                        }
                    }));
            row((
                column((picture,))
                    .width(px(GUTTER))
                    .shrink(0.0)
                    .justify(JustifyContent::FlexEnd),
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
                at: line.at,
            });
        }
        if replies {
            built.insert((Replies, Quotable { session, entry: id }));
        }
        node
    })
}

/// A row of the chat.
pub(super) fn line_view(line: &Line) -> AnyView {
    match &line.row {
        Row::Entry { session, id } => {
            said(line.clone(), *session, id.clone())
        }
        // A turn's steps are the agent's own, so they sit where its
        // bubbles do.
        Row::Fold { session, first, .. } => {
            row((fold_view(line.key(), *session, first.clone()),))
                .padding(UiRect::new(
                    px(GUTTER),
                    px(0.0),
                    px(if line.lead { RUN } else { 0.0 }),
                    px(0.0),
                ))
                .boxed()
        }
    }
}

/// The most pictures the head of the chat shows.
const FACES: usize = 6;

/// The names of everyone in `cast`, the chief at this window last.
fn names(cast: &[Who]) -> String {
    let mut names = cast
        .iter()
        .filter(|who| who.kind != Kind::Me)
        .map(|who| who.name.clone())
        .collect::<Vec<_>>();
    if cast.iter().any(|who| who.kind == Kind::Me) {
        names.push("you".to_string());
    }
    match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => {
            format!("{} and {last}", rest.join(", "))
        }
        _ => names.concat(),
    }
}

/// The way to a new session: a small mark with no plate at the chat's
/// head, which asks before it acts.
fn fresh() -> impl View {
    button(crate::looks::raised(crate::icons::glyph(
        crate::icons::Glyph::NewSession,
        crate::looks::ICON,
    )))
    .padding(UiRect::axes(px(8.0), px(0.0)))
    .min_height(px(crate::looks::CONTROL))
    .tooltip(|| {
        label("Start a new session. This one goes to the Archive.")
            .size(12.0)
    })
    .rules(crate::looks::retrace)
    .tagged(crate::feel::Mood::Ink)
    .on_activate(|world| world_menu::ask(world, Deed::NewSession))
}

/// The head of the chat: the house it is and everyone who speaks in
/// it.
pub(super) fn header() -> AnyView {
    keyed(
        resource::<Lines, _>(|lines| lines.cast().to_vec()),
        |cast| {
            let cast = cast.clone();
            with_cx(move |cx| head(cx, &cast))
        },
    )
    .within(bare())
    .boxed()
}

/// The house `cast` speaks in and everyone in it.
fn head(cx: &Cx, cast: &[Who]) -> AnyView {
    let Some(owner) = cast.first() else {
        return frame().boxed();
    };
    let title = match owner.kind {
        Kind::Mayor => "THE MAYOR'S HALL".to_string(),
        _ => format!("{}'S HOUSE", owner.name.to_uppercase()),
    };
    let me = |who: &&Who| who.kind == Kind::Me;
    let faces = cast
        .iter()
        .filter(|who| !me(who))
        .chain(cast.iter().filter(me))
        .take(FACES)
        .map(|who| face(cx, who, 20.0))
        .collect::<Vec<_>>();
    column((
        row((
            row(faces).gap(3.0).shrink(0.0),
            column((
                label(title)
                    .size(11.0)
                    .bold(true)
                    .wrap(false)
                    .tone(Tone::Faint)
                    .tagged(DisplayText),
                label(names(cast))
                    .size(12.0)
                    .wrap(false)
                    .tone(Tone::Dim),
            ))
            .gap(1.0)
            .fills_width(),
            fresh(),
        ))
        .gap(10.0)
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(0.0), px(7.0))),
        hairline(cx.theme().palette.seam),
    ))
    .gap(0.0)
    .boxed()
}
