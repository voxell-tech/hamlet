//! The library's skills, and the card a row of a list is.

use bevy::ecs::resource::Resource;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use bevy_fynix::{
    Bevy, ScopedExt, StateExt, View, ViewExt, each, keyed, resource,
};
use hamlet_core::core::Skills;

use crate::dock::show_tab;
use crate::icons::Glyph;
use crate::looks::{Fills, action, away, bad_action};
use crate::platform::DisplayText;
use crate::theme::{HamletTheme, paint, themed};
use crate::world_menu::{self, Deed};
use crate::{Cx, Ui};

#[derive(Clone, PartialEq)]
pub(crate) struct SkillRow {
    name: String,
    description: String,
}

fn skill_rows(skills: &Skills) -> Vec<SkillRow> {
    skills
        .0
        .iter()
        .map(|s| SkillRow {
            name: s.name.clone(),
            description: s.description.clone(),
        })
        .collect()
}

/// The words of a list while it is empty, and a way to the chat.
fn empty_state<R: Resource>(
    is_empty: fn(&R) -> bool,
    hint: &'static str,
) -> impl View<Bevy, HamletTheme> {
    keyed(resource::<R, _>(is_empty), move |empty| {
        if *empty {
            column((
                label(hint).tone(Tone::Dim),
                action(Glyph::Talk, "OPEN CHAT", |w| {
                    show_tab(w, "agent")
                }),
            ))
            .gap(12.0)
            .padding(UiRect::bottom(px(12.0)))
            .align(AlignItems::FlexStart)
            .boxed()
        } else {
            frame().boxed()
        }
    })
    .within(column(()))
}

/// The library's skills, each with the press that takes it out.
/// Whoever shows them scrolls them.
pub(crate) fn skills() -> Ui {
    column((
        empty_state(
            |l: &Skills| l.0.is_empty(),
            "The library is empty. Agents offer skills and the \
             librarian decides which to keep.",
        ),
        each(
            resource::<Skills, _>(skill_rows),
            |r: &SkillRow| format!("{}|{}", r.name, r.description),
            |r| {
                // The press that takes it out is in its card,
                // beside its name, so that what it does has the
                // card's whole width.
                let gone = r.name.clone();
                let out = bad_action(
                    Glyph::Delete,
                    "TAKE OUT",
                    move |world| {
                        world_menu::ask(
                            world,
                            Deed::Unshelve(gone.clone()),
                        )
                    },
                );
                card(
                    r.name.clone(),
                    r.description.clone(),
                    Tone::Body,
                    Some(out),
                )
            },
        )
        .within(column(()).gap(8.0)),
    ))
    .gap(0.0)
    .boxed()
}

/// One card: a title over a dim line.
pub fn item(title: String, detail: String, tone: Tone) -> Ui {
    card(title, detail, tone, None)
}

/// One card: a title, with `beside` it at the card's right when
/// there is something to press, over a dim line the card's whole
/// width.
pub fn card(
    title: String,
    detail: String,
    tone: Tone,
    beside: Option<Ui>,
) -> Ui {
    Ui::new(move |cx: &mut Cx| {
        let name = row((label(title)
            .size(13.0)
            .bold(true)
            .tone(tone)
            .tagged(DisplayText),))
        .fills_width()
        .boxed();
        let head =
            std::iter::once(name).chain(beside).collect::<Vec<_>>();
        cx.build(
            column((
                row(head).gap(8.0).align(AlignItems::Center),
                label(detail).size(13.0).tone(Tone::Dim),
            ))
            .gap(4.0)
            .padding(UiRect::axes(px(14.0), px(10.0)))
            .border(1.0)
            .border_color(paint(|p| p.brass_6))
            .radius(themed(|t| t.space.round))
            .fill(paint(|p| p.card))
            .appear(away)
            .transition(Motion::Expand),
        )
    })
}
