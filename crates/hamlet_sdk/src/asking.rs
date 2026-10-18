//! The questions an agent asks the chief, drawn as one card: a
//! question at a time with its options as a list and a line for words
//! of the chief's own, and the answers sent once each question has
//! one. Needs You and the chat show the same card, so an answer begun
//! in one stands in the other.

use std::collections::HashMap;

use bevy::app::{App, Update};
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
    text_field,
};
use bevy_fynix::{ScopedExt, ViewExt, keyed, resource, style};
use hamlet_core::oversight::{
    Answer, Answered, Asks, RaisedBy, Waiting,
};
use hamlet_rules::chief::asking::{Offer, Pick, Questions, letter};
use hamlet_rules::chief::rulings::Choice;

use crate::convo::Me;
use crate::feel::Mood;
use crate::icons::{Glyph, glyph};
use crate::looks::{
    Fills, bare, brass, lamp, marked, note, retrace, well,
};
use crate::theme::HamletTheme;
use crate::{AnyView, Cx, View};

pub fn plugin(app: &mut App) {
    app.init_resource::<Answering>()
        .init_resource::<OpenQuestion>()
        .add_systems(Update, (follow, forget));
}

/// Each card's question on show and the answers so far.
#[derive(Resource, Default)]
pub struct Answering(HashMap<Entity, Sheet>);

#[derive(Clone)]
struct Sheet {
    /// The question on show.
    at: usize,
    picks: Vec<Option<Pick>>,
}

impl Answering {
    fn page(&self, card: Entity) -> usize {
        self.0.get(&card).map_or(0, |sheet| sheet.at)
    }

    fn pick(&self, card: Entity, at: usize) -> Option<Pick> {
        self.0.get(&card)?.picks.get(at)?.clone()
    }

    fn answered(&self, card: Entity) -> usize {
        self.0
            .get(&card)
            .map_or(0, |sheet| sheet.picks.iter().flatten().count())
    }

    /// The sheet of `card`, which asks `count` questions.
    fn sheet(&mut self, card: Entity, count: usize) -> &mut Sheet {
        self.0.entry(card).or_insert_with(|| Sheet {
            at: 0,
            picks: vec![None; count],
        })
    }
}

/// The questions the agent of the open chat waits on, oldest first.
#[derive(Resource, Default, PartialEq)]
pub struct OpenQuestion(Option<(Entity, Questions)>);

/// Keeps [`OpenQuestion`] as the oldest card raised by the agent the
/// chat is with, or by a turn of the chat's own session.
fn follow(
    me: Res<Me>,
    waiting: Query<(Entity, &Waiting, &Asks, Option<&RaisedBy>)>,
    mut open: ResMut<OpenQuestion>,
) {
    let now = waiting
        .iter()
        .filter(|(_, waits, _, by)| {
            waits.session == Some(me.session)
                || by.is_some_and(|by| by.0 == me.agent)
        })
        .min_by_key(|(_, waits, ..)| waits.order)
        .map(|(card, _, asks, _)| (card, asks));
    let was = open.0.as_ref().map(|(card, _)| *card);
    if was != now.map(|(card, _)| card) {
        open.0 = now.map(|(card, asks)| (card, asks.0.clone()));
    }
}

/// Drops what was begun on a card that no longer waits.
fn forget(
    mut gone: RemovedComponents<Waiting>,
    mut answering: ResMut<Answering>,
) {
    for card in gone.read() {
        if answering.0.contains_key(&card) {
            answering.0.remove(&card);
        }
    }
}

/// Makes `pick` the answer to the question `at` of `card`, which asks
/// `count`. An option picked moves on to the next question.
pub fn choose(
    world: &mut World,
    card: Entity,
    count: usize,
    at: usize,
    pick: Option<Pick>,
) {
    let mut answering = world.resource_mut::<Answering>();
    let sheet = answering.sheet(card, count);
    let onward =
        matches!(pick, Some(Pick::Offer(_))) && at + 1 < count;
    sheet.picks[at] = pick;
    if onward {
        sheet.at = at + 1;
    }
}

/// Words typed for the question `at`. Empty words leave an option
/// picked as it was.
fn write(
    world: &mut World,
    card: Entity,
    count: usize,
    at: usize,
    text: String,
) {
    let text = text.trim().to_string();
    let was = world.resource::<Answering>().pick(card, at);
    let pick = match (text.is_empty(), was) {
        (false, _) => Some(Pick::Words(text)),
        (true, Some(Pick::Offer(offer))) => Some(Pick::Offer(offer)),
        (true, _) => None,
    };
    world.resource_mut::<Answering>().sheet(card, count).picks[at] =
        pick;
}

/// Shows the question `to` of `card`, which asks `count`.
pub fn turn(
    world: &mut World,
    card: Entity,
    count: usize,
    to: usize,
) {
    world.resource_mut::<Answering>().sheet(card, count).at =
        to.min(count - 1);
}

/// Sends the answers of `card` once each question has one, and shows
/// the first without one otherwise.
fn submit(world: &mut World, card: Entity, count: usize) {
    let picks = world
        .resource_mut::<Answering>()
        .sheet(card, count)
        .picks
        .clone();
    match picks.iter().position(Option::is_none) {
        Some(open) => turn(world, card, count, open),
        None => {
            let chief = world.resource::<Me>().user.clone();
            world.trigger(Answered {
                entity: card,
                picks: picks.into_iter().flatten().collect(),
                chief,
            });
            world.flush();
        }
    }
}

fn dismiss(world: &mut World, card: Entity) {
    let chief = world.resource::<Me>().user.clone();
    world.trigger(Answer {
        entity: card,
        choice: Choice::Dismiss,
        text: String::new(),
        chief,
    });
    world.flush();
}

/// An option at rest: no plate, the wood showing under the pointer.
fn option(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .border(1.0)
                .border_color(Color::NONE)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.wood_hover)))
        .pressed(|s| s.frame(|f, t| f.fill(t.palette.well)))
        .transition(Motion::Interact)
        .apply(cx);
}

/// The option picked: lamp glass in a brass frame.
fn picked(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.lamp_low)
                .border(1.0)
                .border_color(t.palette.brass_4)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.border_color(t.palette.lamp)))
        .transition(Motion::Interact)
        .apply(cx);
}

/// The options of the question `at`, as a list, the one picked lit.
/// A click on the one picked takes it back.
fn options(
    card: Entity,
    count: usize,
    at: usize,
    offered: &[Offer],
    pick: &Option<Pick>,
) -> AnyView {
    let all = offered
        .iter()
        .enumerate()
        .map(|(n, offer)| {
            let chosen = *pick == Some(Pick::Offer(n));
            let mark = column((label(letter(n).to_string())
                .size(12.0)
                .bold(true)
                .wrap(false)
                .tone(if chosen {
                    Tone::Accent
                } else {
                    Tone::Faint
                }),))
            .width(px(12.0))
            .shrink(0.0);
            let tick = match chosen {
                true => glyph(Glyph::Check, 12.0)
                    .tone(Tone::Accent)
                    .boxed(),
                false => frame().width(px(12.0)).boxed(),
            };
            let line = row((
                mark,
                label(offer.title.clone()).size(13.0).wrap(false),
                column((label(offer.description.clone())
                    .size(12.0)
                    .tone(Tone::Dim),))
                .fills_width(),
                tick,
            ))
            .gap(10.0)
            .align(AlignItems::Center)
            .width(percent(100.0));
            let (rules, mood): (fn(&mut Cx), _) = match chosen {
                true => (picked, Mood::Good),
                false => (option, Mood::Quiet),
            };
            let then = (!chosen).then_some(Pick::Offer(n));
            button(line)
                .width(percent(100.0))
                .justify(JustifyContent::FlexStart)
                .padding(UiRect::axes(px(8.0), px(5.0)))
                .rules(rules)
                .tagged(mood)
                .on_activate(move |world| {
                    choose(world, card, count, at, then.clone())
                })
                .boxed()
        })
        .collect::<Vec<_>>();
    column(all).gap(2.0).boxed()
}

/// The way between the questions of a card, and where it is.
fn pager(card: Entity, count: usize, at: usize) -> AnyView {
    let step = |which: &'static str, to: usize| {
        button(label(which).size(14.0).wrap(false))
            .padding(UiRect::axes(px(5.0), px(0.0)))
            .rules(retrace)
            .tagged(Mood::Ink)
            .on_activate(move |world| turn(world, card, count, to))
    };
    row((
        step("\u{2039}", at.saturating_sub(1)),
        label(format!("{} of {count}", at + 1))
            .size(11.0)
            .wrap(false)
            .tone(Tone::Dim),
        step("\u{203a}", at + 1),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .shrink(0.0)
    .boxed()
}

/// The question `at` of `card`, which asks `asked`.
fn page(card: Entity, asked: &Questions, at: usize) -> AnyView {
    let count = asked.questions.len();
    let Some(one) = asked.questions.get(at) else {
        return frame().boxed();
    };
    let mut head = vec![
        column((label(one.question.clone()).size(13.0).bold(true),))
            .fills_width()
            .boxed(),
    ];
    if count > 1 {
        head.push(pager(card, count, at));
    }
    let mut parts =
        vec![row(head).gap(8.0).align(AlignItems::FlexStart).boxed()];
    if !one.description.is_empty() {
        parts.push(note(one.description.clone()).boxed());
    }
    if !one.options.is_empty() {
        let offered = one.options.clone();
        parts.push(
            keyed(
                resource::<Answering, _>(move |a| a.pick(card, at)),
                move |pick| options(card, count, at, &offered, pick),
            )
            .within(bare())
            .boxed(),
        );
    }
    if one.text_input {
        let typed = resource::<Answering, _>(move |a| {
            match a.pick(card, at) {
                Some(Pick::Words(words)) => words,
                _ => String::new(),
            }
        });
        let name = match one.options.is_empty() {
            true => "Answer",
            false => "Other",
        };
        parts.push(
            row((
                label(name).size(12.0).wrap(false).tone(Tone::Dim),
                text_field(typed, move |world, text| {
                    write(world, card, count, at, text)
                })
                .fills_width()
                .rules(well)
                .tagged(Mood::Field),
            ))
            .gap(10.0)
            .align(AlignItems::Center)
            .padding(UiRect::horizontal(px(8.0)))
            .boxed(),
        );
    }
    let next = match at + 1 < count {
        true => marked(Glyph::Into, "NEXT")
            .rules(brass)
            .tagged(Mood::Plain)
            .on_activate(move |world| {
                turn(world, card, count, at + 1)
            })
            .boxed(),
        false => marked(Glyph::Check, "SUBMIT")
            .rules(lamp)
            .tagged(Mood::Good)
            .on_activate(move |world| submit(world, card, count))
            .boxed(),
    };
    let done = label(resource::<Answering, _>(move |a| {
        format!("{} of {count} answered", a.answered(card))
    }))
    .size(11.0)
    .wrap(false)
    .tone(Tone::Dim);
    parts.push(
        row((
            column((done,)).fills_width(),
            marked(Glyph::Cross, "DISMISS")
                .rules(retrace)
                .tagged(Mood::Ink)
                .on_activate(move |world| dismiss(world, card)),
            next,
        ))
        .gap(8.0)
        .align(AlignItems::Center)
        .padding(UiRect::top(px(2.0)))
        .boxed(),
    );
    column(parts).gap(6.0).boxed()
}

/// The card of the waiting `card`, which asks `asked`.
pub fn card(card: Entity, asked: Questions) -> AnyView {
    let message = match asked.message.is_empty() {
        true => frame().boxed(),
        false => label(asked.message.clone()).size(12.0).boxed(),
    };
    let pages = keyed(
        resource::<Answering, _>(move |a| a.page(card)),
        move |at| page(card, &asked, *at),
    )
    .within(bare());
    column((message, pages)).gap(8.0).boxed()
}

/// The card the agent of the open chat waits on, over the field.
pub fn in_chat() -> impl View {
    keyed(resource::<OpenQuestion, _>(|o| o.0.clone()), |open| {
        let Some((entity, asked)) = open.clone() else {
            return frame().boxed();
        };
        crate::with_cx(move |cx| {
            let palette = &cx.theme().palette;
            column((
                row((
                    glyph(Glyph::Bell, 12.0).tone(Tone::Accent),
                    label("Waiting for your answer")
                        .size(11.0)
                        .wrap(false)
                        .tone(Tone::Accent),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
                card(entity, asked.clone()),
            ))
            .gap(8.0)
            .padding(UiRect::all(px(10.0)))
            .margin(UiRect::bottom(px(8.0)))
            .border(1.0)
            .border_color(palette.brass_5)
            .radius(cx.theme().space.round)
            .fill(palette.card)
            .boxed()
        })
    })
    .within(bare())
}
