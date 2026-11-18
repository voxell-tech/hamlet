//! The card of a request an agent sends the chief with
//! `request_chief`, one question at a time.

use std::collections::HashMap;

use bevy::app::{App, PreUpdate};
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::RemovedComponents;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, JustifyContent, UiRect, percent, px};
use bevy_fynix::tokens::{MotionTokens, SurfaceTokens, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, button, column, frame, label, row,
    text_field,
};
use bevy_fynix::{
    ScopedExt, ViewExt, component, keyed, resource, style,
};
use hamlet_client::messages::{Answer, Answered};
use hamlet_client::waits::{Choice, Refused, Waiting};
use hamlet_protocol::client::{Offer, Pick, Questions, RequestId};
use hamlet_ui::styles::{
    Fills, Looks, bare, marked, note, note_rules, small_rules,
    text_input, transparent,
};
use hamlet_ui::theme::HamletTheme;
use hamlet_ui::widgets::effects::Variant;
use hamlet_ui::widgets::icons::{Glyph, glyph};
use hamlet_ui::{AnyView, Cx, View, with_cx};

use crate::PanelSystems;

pub fn plugin(app: &mut App) {
    app.init_resource::<Answering>()
        .add_systems(PreUpdate, forget.in_set(PanelSystems::Follow));
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

    fn sheet(&mut self, card: Entity, count: usize) -> &mut Sheet {
        self.0.entry(card).or_insert_with(|| Sheet {
            at: 0,
            picks: vec![None; count],
        })
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

/// Answers question `at` of `card` with `pick`.
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

/// Words typed for question `at`; empty words keep the option picked.
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

/// Sends `card`'s answers, or shows the first question left open.
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
                request: RequestId::new(),
                picks: picks.into_iter().flatten().collect(),
            });
            world.flush();
        }
    }
}

fn dismiss(world: &mut World, card: Entity) {
    world.trigger(Answer {
        wait: card,
        request: RequestId::new(),
        choice: Choice::Dismiss,
        text: None,
    });
    world.flush();
}

/// An option at rest: no plate, the wood showing under the pointer.
fn option(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(transparent(t.palette.button_hover))
                .border(t.space.border)
                .border_color(Color::NONE)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.fill(t.palette.button_hover)))
        .pressed(|s| s.frame(|f, t| f.fill(t.pressed())))
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// The option picked: the selected fill in a gold border.
fn picked(cx: &mut Cx) {
    style::<HamletTheme>()
        .frame(|f, t| {
            f.fill(t.palette.selected)
                .border(t.space.border)
                .border_color(t.palette.gold_4)
                .radius(t.space.round)
        })
        .hovered(|s| s.frame(|f, t| f.border_color(t.palette.accent)))
        .transition(HamletTheme::interact)
        .apply(cx);
}

/// The options of question `at`; pressing the one picked takes it
/// back.
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

/// The way between `count` pages turned by `to`, at page `at`.
pub(crate) fn pager(
    at: usize,
    count: usize,
    to: impl Fn(&mut World, usize) + Clone + Send + Sync + 'static,
) -> impl View {
    with_cx(move |cx| {
        let space = &cx.theme().space;
        let (body, gap) = (cx.theme().text.body, space.gap);
        let step = |which: &'static str, page: usize| {
            let to = to.clone();
            button(label(which).size(body).wrap(false))
                .padding(UiRect::axes(px(gap / 2.0), px(0.0)))
                .variant(Variant::Text)
                .on_activate(move |world| to(world, page))
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
        head.push(
            pager(at, count, move |world, to| {
                turn(world, card, count, to);
            })
            .boxed(),
        );
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
            true => "Response",
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
        true => marked(Glyph::Into, "Next")
            .variant(Variant::Default)
            .on_activate(move |world| {
                turn(world, card, count, at + 1);
            })
            .boxed(),
        false => marked(Glyph::Check, "Submit")
            .variant(Variant::Primary)
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
                marked(Glyph::Cross, "Dismiss")
                    .variant(Variant::Text)
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

#[cfg(test)]
mod tests {
    use bevy::ecs::query::With;
    use bevy::text::EditableText;
    use hamlet_client::chats::Chat;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;
    use serde_json::json;

    use super::*;
    use crate::convo::Convo;
    use crate::testing::{panel_app, press, words};

    #[test]
    fn the_chat_shows_its_oldest_request_and_sends_its_response() {
        let mut app = panel_app();
        app.add_plugins(crate::petition::plugin);
        let shown = app.world().resource::<Convo>().chat;
        let chat = app.world().get::<Chat>(shown).unwrap().0.clone();
        let question = |id: &str, since, chat: &protocol::ChatRef| {
            serde_json::from_value::<protocol::Wait>(json!({
                "id": id, "agent": null, "by": "ada",
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
        other.with = protocol::AgentId(3);
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
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            crate::petition::in_chat(),
        );
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
        assert!(
            words(&mut app)
                .iter()
                .any(|text| text.as_str() == "2 of 2 answered")
        );
        press(&mut app, "Submit");
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
