//! The questions an agent asks the chief, as one card: a question at
//! a time, its options as a list and a line for words of the chief's
//! own, and the answers sent once each question has one. What is
//! picked is kept by the agents service, so every panel showing the
//! card shows the same.

use hamlet_engine_bevy::AgentsRequest;
use hamlet_engine_bevy::api::agents::{Choice, Need, Pick};
use hamlet_engine_bevy::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Size, Space, Tone,
    UiExt, ViewExt, button, column, description, field, icon, label,
    pressable, row,
};

use crate::shared::client::AgentsWorldExt;

/// The card of the questions `need` asks, at the question its sheet
/// shows.
pub fn card(need: &Need) -> AnyView {
    let Some(asked) = &need.questions else {
        return column(()).boxed();
    };
    let id = need.id;
    let count = asked.questions.len();
    let at = (need.sheet.at as usize).min(count.saturating_sub(1));
    let Some(one) = asked.questions.get(at) else {
        return column(()).boxed();
    };
    let question = u32::try_from(at).unwrap_or(u32::MAX);
    let pick = need.sheet.picks.get(at).cloned().flatten();
    let mut parts = Vec::new();
    if !asked.message.is_empty() {
        parts.push(
            label(asked.message.clone()).size(Size::Small).boxed(),
        );
    }
    let mut top =
        vec![label(one.question.clone()).bold(true).boxed()];
    if count > 1 {
        top.push(pager(id, count, at));
    }
    parts.push(row(top).space(Space::Gap).boxed());
    if !one.description.is_empty() {
        parts.push(description(one.description.clone()).boxed());
    }
    let options = one
        .options
        .iter()
        .enumerate()
        .map(|(n, offer)| {
            let offered = u32::try_from(n).unwrap_or(u32::MAX);
            let chosen = pick == Some(Pick::Offer(offered));
            let then = (!chosen).then_some(Pick::Offer(offered));
            let letter = char::from(b'a' + (n as u8).min(25));
            let tone =
                if chosen { Tone::Accent } else { Tone::Faint };
            let mut line = vec![
                label(letter.to_string())
                    .size(Size::Small)
                    .bold(true)
                    .tone(tone)
                    .boxed(),
                label(offer.title.clone()).boxed(),
                description(offer.description.clone()).boxed(),
            ];
            if chosen {
                line.push(icon("check").tone(Tone::Accent).boxed());
            }
            pressable((row(line).space(Space::Gap),))
                .style(if chosen {
                    ButtonStyle::Plain
                } else {
                    ButtonStyle::Quiet
                })
                .on_click(move |world| {
                    world.ask(AgentsRequest::Pick {
                        need: id,
                        question,
                        pick: then.clone(),
                    })
                })
                .boxed()
        })
        .collect::<Vec<_>>();
    if !options.is_empty() {
        parts.push(column(options).space(Space::None).boxed());
    }
    if one.text_input {
        let typed = match &pick {
            Some(Pick::Words(words)) => words.clone(),
            _ => String::new(),
        };
        let name = if one.options.is_empty() {
            "Answer"
        } else {
            "Other"
        };
        parts.push(
            row((
                label(name).size(Size::Small).tone(Tone::Dim),
                field(typed).on_commit(move |world, text| {
                    world.ask(AgentsRequest::Pick {
                        need: id,
                        question,
                        pick: Some(Pick::Words(text)),
                    })
                }),
            ))
            .space(Space::Gap)
            .boxed(),
        );
    }
    let answered = need.sheet.picks.iter().flatten().count();
    let next = if at + 1 < count {
        button("NEXT")
            .on_click(move |world| {
                world.ask(AgentsRequest::Turn {
                    need: id,
                    to: question + 1,
                })
            })
            .boxed()
    } else {
        button("SUBMIT")
            .style(ButtonStyle::Primary)
            .on_click(move |world| {
                world.ask(AgentsRequest::Submit(id))
            })
            .boxed()
    };
    parts.push(
        row((
            label(format!("{answered} of {count} answered"))
                .size(Size::Small)
                .tone(Tone::Dim),
            button("DISMISS").style(ButtonStyle::Quiet).on_click(
                move |world| {
                    world.ask(AgentsRequest::Answer {
                        need: id,
                        choice: Choice::Dismiss,
                        text: String::new(),
                    })
                },
            ),
            next,
        ))
        .space(Space::Gap)
        .boxed(),
    );
    column(parts).space(Space::Half).boxed()
}

/// The way between the questions of a card, and where it is.
fn pager(need: u64, count: usize, at: usize) -> AnyView {
    let step = |which: &'static str, to: usize| {
        let to = u32::try_from(to.min(count - 1)).unwrap_or(u32::MAX);
        button(which).style(ButtonStyle::Quiet).on_click(
            move |world| world.ask(AgentsRequest::Turn { need, to }),
        )
    };
    row((
        step("<", at.saturating_sub(1)),
        label(format!("{} of {count}", at + 1))
            .size(Size::Small)
            .tone(Tone::Dim),
        step(">", at + 1),
    ))
    .space(Space::Half)
    .boxed()
}
