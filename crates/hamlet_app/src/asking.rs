//! The questions an agent asks the chief, drawn as one card: a
//! question at a time with its options as a list and a line for words
//! of the chief's own, and the answers sent once each question has
//! one. Needs You and the chat show the same card, so an answer begun
//! in one stands in the other.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate};
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
    text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, component, keyed, resource, style,
};
use hamlet_client::chats::Chat;
use hamlet_client::messages::{Answer, Answered};
use hamlet_client::waits::{
    Asks, Choice, RaisedBy, Refused, Waiting,
};
use hamlet_protocol::client::{Offer, Pick, Questions};
use hamlet_ui::styles::{
    self, Fills, back_link, bare, marked, note, note_rules,
    primary_button, small_rules, text_input,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::PanelSystems;
use crate::convo::Convo;

pub fn plugin(app: &mut App) {
    app.init_resource::<Answering>()
        .init_resource::<OpenQuestion>()
        .add_systems(
            PreUpdate,
            (
                forget.in_set(PanelSystems::Follow),
                follow.in_set(PanelSystems::Bind),
            ),
        );
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
    convo: Res<Convo>,
    chats: Query<&Chat>,
    waiting: Query<(Entity, &Waiting, &Asks, Option<&RaisedBy>)>,
    mut open: ResMut<OpenQuestion>,
) {
    let now = waiting
        .iter()
        .filter(|(_, waits, _, by)| {
            waits.chat.as_ref().is_some_and(|reference| {
                chats
                    .get(convo.session)
                    .is_ok_and(|chat| &chat.0 == reference)
            }) || by.is_some_and(|by| by.0 == convo.agent)
        })
        .min_by_key(|(_, waits, ..)| (waits.since, waits.id.as_str()))
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
        matches!(pick, Some(Pick::Offer { .. })) && at + 1 < count;
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
        (false, _) => Some(Pick::Words { text }),
        (true, Some(offer @ Pick::Offer { .. })) => Some(offer),
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
            world.trigger(Answered {
                entity: card,
                picks: picks.into_iter().flatten().collect(),
            });
            world.flush();
        }
    }
}

fn dismiss(world: &mut World, card: Entity) {
    world.trigger(Answer {
        wait: card,
        choice: Choice::Dismiss,
        text: None,
    });
    world.flush();
}

/// An option at rest: no plate, the wood showing under the pointer.
fn option(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(Color::NONE)
                .border(t.space.border)
                .border_color(Color::NONE)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button_hover)))
        .pressed(|s| s.frame(|f, t| f.fill(t.palette.input)))
        .transition(Motion::Interact)
        .apply(cx);
}

/// The option picked: the primary fill in a gold border.
fn picked(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.primary_low)
                .border(t.space.border)
                .border_color(t.palette.gold_4)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.border_color(t.palette.glow)))
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
) -> impl View + use<> {
    let (offered, pick) = (offered.to_vec(), pick.clone());
    with_cx(move |cx| {
        let theme = cx.theme();
        let (small, heading) = (theme.text.small, theme.text.heading);
        let space = &theme.space;
        let all = offered
            .iter()
            .enumerate()
            .map(|(n, offer)| {
                let index = n as u64;
                let chosen = pick == Some(Pick::Offer { index });
                let mark = column((label(
                    char::from(b'a' + n as u8).to_string(),
                )
                .size(small)
                .bold(true)
                .wrap(false)
                .tone(if chosen {
                    Tone::Accent
                } else {
                    Tone::Faint
                }),))
                .width(px(small))
                .shrink(0.0);
                let tick = match chosen {
                    true => glyph(Glyph::Check, small)
                        .tone(Tone::Accent)
                        .boxed(),
                    false => frame().width(px(small)).boxed(),
                };
                let line = row((
                    mark,
                    label(offer.title.clone())
                        .size(heading)
                        .wrap(false),
                    column((note(offer.description.clone()),))
                        .fills_width(),
                    tick,
                ))
                .gap(space.section)
                .align(AlignItems::Center)
                .width(percent(100.0));
                let (rules, variant): (fn(&mut Cx), _) = match chosen
                {
                    true => (picked, Variant::Primary),
                    false => (option, Variant::Subtle),
                };
                let then = (!chosen).then_some(Pick::Offer { index });
                button(line)
                    .width(percent(100.0))
                    .justify(JustifyContent::FlexStart)
                    .padding(UiRect::axes(
                        px(space.gap),
                        px(space.gap / 2.0),
                    ))
                    .rules(rules)
                    .tagged(variant)
                    .on_activate(move |world| {
                        choose(world, card, count, at, then.clone());
                    })
                    .boxed()
            })
            .collect::<Vec<_>>();
        column(all).gap(space.gap / 2.0)
    })
}

/// The way between the questions of a card, and where it is.
fn pager(card: Entity, count: usize, at: usize) -> impl View {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let (body, gap) = (cx.theme().text.body, space.gap);
        let step = |which: &'static str, to: usize| {
            button(label(which).size(body).wrap(false))
                .padding(UiRect::axes(px(gap / 2.0), px(0.0)))
                .rules(back_link)
                .tagged(Variant::Text)
                .on_activate(move |world| {
                    turn(world, card, count, to);
                })
        };
        row((
            step("\u{2039}", at.saturating_sub(1)),
            label(format!("{} of {count}", at + 1))
                .wrap(false)
                .rules(note_rules),
            step("\u{203a}", at + 1),
        ))
        .gap(gap / 4.0)
        .align(AlignItems::Center)
        .shrink(0.0)
    })
}

/// The question `at` of `card`, which asks `asked`.
fn page(card: Entity, asked: &Questions, at: usize) -> AnyView {
    let count = asked.questions.len();
    let Some(one) = asked.questions.get(at) else {
        return frame().boxed();
    };
    let question = one.question.clone();
    let mut head = vec![
        with_cx(move |cx| {
            column((label(question)
                .size(cx.theme().text.heading)
                .bold(true),))
            .fills_width()
        })
        .boxed(),
    ];
    if count > 1 {
        head.push(pager(card, count, at).boxed());
    }
    let mut parts = vec![
        with_cx(move |cx| {
            row(head)
                .gap(cx.theme().space.gap)
                .align(AlignItems::FlexStart)
        })
        .boxed(),
    ];
    if !one.description.is_empty() {
        parts.push(note(one.description.clone()).boxed());
    }
    if !one.options.is_empty() {
        let offered = one.options.clone();
        parts.push(
            keyed(
                resource::<Answering, _>(move |a| a.pick(card, at)),
                move |pick| {
                    options(card, count, at, &offered, pick).boxed()
                },
            )
            .within(bare())
            .boxed(),
        );
    }
    if one.text_input {
        let typed = resource::<Answering, _>(move |a| {
            match a.pick(card, at) {
                Some(Pick::Words { text }) => text,
                _ => String::new(),
            }
        });
        let name = match one.options.is_empty() {
            true => "Answer",
            false => "Other",
        };
        parts.push(
            with_cx(move |cx| {
                let space = &cx.theme().space;
                row((
                    label(name).wrap(false).rules(note_rules),
                    text_field(typed, move |world, text| {
                        write(world, card, count, at, text);
                    })
                    .fills_width()
                    .rules(text_input)
                    .tagged(Variant::Input),
                ))
                .gap(space.section)
                .align(AlignItems::Center)
                .padding(UiRect::horizontal(px(space.gap)))
            })
            .boxed(),
        );
    }
    let next = match at + 1 < count {
        true => marked(Glyph::Into, "NEXT")
            .rules(styles::button)
            .tagged(Variant::Default)
            .on_activate(move |world| {
                turn(world, card, count, at + 1);
            })
            .boxed(),
        false => marked(Glyph::Check, "SUBMIT")
            .rules(primary_button)
            .tagged(Variant::Primary)
            .on_activate(move |world| submit(world, card, count))
            .boxed(),
    };
    let done = note(resource::<Answering, _>(move |a| {
        format!("{} of {count} answered", a.answered(card))
    }));
    parts.push(
        with_cx(move |cx| {
            let gap = cx.theme().space.gap;
            row((
                column((done,)).fills_width(),
                marked(Glyph::Cross, "DISMISS")
                    .rules(back_link)
                    .tagged(Variant::Text)
                    .on_activate(move |world| dismiss(world, card)),
                next,
            ))
            .gap(gap)
            .align(AlignItems::Center)
            .padding(UiRect::top(px(gap / 4.0)))
        })
        .boxed(),
    );
    with_cx(move |cx| column(parts).gap(cx.theme().space.gap)).boxed()
}

/// The card of the waiting `card`, which asks `asked`.
pub fn card(card: Entity, asked: Questions) -> impl View {
    let message = match asked.message.is_empty() {
        true => frame().boxed(),
        false => {
            label(asked.message.clone()).rules(small_rules).boxed()
        }
    };
    let pages = keyed(
        resource::<Answering, _>(move |a| a.page(card)),
        move |at| page(card, &asked, *at),
    )
    .within(bare());
    // The server checks the answers; its refusal stays until the
    // next.
    #[expect(
        clippy::redundant_closure_for_method_calls,
        reason = "Option::cloned is not general over the lifetime"
    )]
    let refused = keyed(
        component::<Refused, _>(card, |refused| refused.cloned()),
        |refused| match refused {
            Some(Refused(text)) => label(text.clone())
                .tone(Tone::Critical)
                .rules(small_rules)
                .boxed(),
            None => frame().boxed(),
        },
    )
    .within(bare());
    with_cx(move |cx| {
        column((message, pages, refused)).gap(cx.theme().space.gap)
    })
}

/// The card the agent of the open chat waits on, over the field.
pub fn in_chat() -> impl View {
    keyed(resource::<OpenQuestion, _>(|o| o.0.clone()), |open| {
        let Some((entity, asked)) = open.clone() else {
            return frame().boxed();
        };
        with_cx(move |cx| {
            let theme = cx.theme();
            let (palette, space) = (&theme.palette, &theme.space);
            column((
                row((
                    glyph(Glyph::Bell, theme.text.small)
                        .tone(Tone::Accent),
                    label("Waiting for your answer")
                        .size(theme.text.caption)
                        .wrap(false)
                        .tone(Tone::Accent),
                ))
                .gap(6.0)
                .align(AlignItems::Center),
                card(entity, asked.clone()),
            ))
            .gap(space.gap)
            .padding(UiRect::axes(
                px(space.card_padding_x),
                px(space.card_padding_y),
            ))
            .margin(UiRect::bottom(px(space.gap)))
            .border(space.border)
            .border_color(palette.gold_5)
            .radius(space.round)
            .fill(palette.card)
            .boxed()
        })
        .boxed()
    })
    .within(bare())
}

#[cfg(test)]
mod tests {
    use bevy::ecs::query::With;
    use bevy::text::EditableText;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;
    use crate::testing::{panel_app, press, words};

    #[test]
    fn received_questions_share_answers_between_chat_and_petition() {
        let mut app = panel_app();
        app.add_plugins(crate::petition::plugin);
        let session = app.world().resource::<Convo>().session;
        let chat =
            app.world().get::<Chat>(session).unwrap().0.clone();
        let question = |id: &str, since, chat: &protocol::ChatRef| {
            serde_json::from_value::<protocol::Wait>(json!({
                "id": id, "agent": null, "by": "founder",
                "about": "Choose", "subject": "Choose",
                "change": null, "answers": ["reply", "dismiss"],
                "since": since, "asked": 1, "chat": chat,
                "detail": {"kind": "question", "questions": {"message": id, "questions": [
                    {"question": "Which format?", "description": "",
                     "options": [{"title": "Paper", "description": "Printed"},
                                 {"title": "Digital", "description": "Download"}],
                     "text_input": false},
                    {"question": "Where?", "description": "",
                     "options": [], "text_input": true}
                ]}}
            })).unwrap()
        };
        let mut other = chat.clone();
        other.with.agent = "scout".into();
        let mut state = hamlet_testing::realm::state();
        state.waits = vec![
            question("later", 20, &chat),
            question("unrelated", 1, &other),
            question("oldest", 10, &chat),
        ];
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::State(state),
        );
        app.update();
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), in_chat());
        app.update();
        let shown = words(&mut app);
        assert!(shown.iter().any(|text| text == "oldest"));
        assert!(
            !shown
                .iter()
                .any(|text| text == "later" || text == "unrelated")
        );
        press(&mut app, "Digital");
        app.update();
        let field = app
            .world_mut()
            .query_filtered::<Entity, With<EditableText>>()
            .single(app.world())
            .unwrap();
        app.world_mut().trigger(bevy::input_focus::FocusGained {
            entity: field,
            cause: bevy::input_focus::FocusCause::Pressed,
        });
        app.world_mut()
            .get_mut::<EditableText>(field)
            .unwrap()
            .editor_mut()
            .set_text("  Library desk  ");
        app.world_mut()
            .trigger(bevy::input_focus::FocusLost { entity: field });
        app.update();
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            crate::petition::screen(),
        );
        app.update();
        let shown = words(&mut app);
        assert_eq!(
            shown
                .iter()
                .filter(|text| text.as_str() == "2 of 2 answered")
                .count(),
            2
        );
        press(&mut app, "SUBMIT");
        let answer = app
            .world()
            .resource::<Outbox>()
            .iter()
            .find_map(|message| match message {
                protocol::ClientMessage::Answer(answer) => {
                    Some(answer)
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(answer.wait, "oldest");
        assert_eq!(answer.choice, Choice::Reply);
        assert_eq!(
            serde_json::to_value(&answer.picks).unwrap(),
            json!([
                {"kind": "offer", "index": 1}, {"kind": "words", "text": "Library desk"}
            ])
        );
    }
}
