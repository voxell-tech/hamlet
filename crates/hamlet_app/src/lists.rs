//! The library's skills, and the card a row of a list is.

use bevy::ecs::resource::Resource;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::{Motion, Tone};
use bevy_fynix::views::{FrameProps, column, frame, label, row};
use bevy_fynix::{
    ScopedExt, StateExt, ViewExt, each, keyed, resource,
};
use hamlet_client::realm::Skills;
use hamlet_ui::kinds::tile;
use hamlet_ui::styles::{
    Fills, action, bad_action, description, hidden,
};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{AnyView, View, built, with_cx};

use crate::dock::{HUB, show_tab};
use crate::world_menu::{self, Deed};

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
) -> impl View {
    keyed(resource::<R, _>(is_empty), move |empty| {
        if *empty {
            with_cx(move |cx| {
                let section = cx.theme().space.section;
                column((
                    description(hint),
                    action(Glyph::Talk, "Open chat", |w| {
                        show_tab(w, HUB);
                    }),
                ))
                .gap(section)
                .padding(UiRect::bottom(px(section)))
                .align(AlignItems::FlexStart)
            })
            .boxed()
        } else {
            frame().boxed()
        }
    })
    .within(column(()))
}

/// The library's skills, each with the press that takes it out.
/// Whoever shows them scrolls them.
pub(crate) fn skills() -> impl View {
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
                    "Take out",
                    move |world| {
                        world_menu::ask(
                            world,
                            Deed::Unshelve(gone.clone()),
                        );
                    },
                );
                card(
                    r.name.clone(),
                    r.description.clone(),
                    Tone::Body,
                    Some(out),
                )
                .boxed()
            },
        )
        .within(column(()).gap(8.0)),
    ))
    .gap(0.0)
}

/// One card: a title over a dim line.
pub fn item(title: String, detail: String, tone: Tone) -> impl View {
    card(title, detail, tone, None)
}

/// One card: a title, with `beside` it at the card's right when
/// there is something to press, over a dim line the card's whole
/// width.
pub fn card(
    title: String,
    detail: String,
    tone: Tone,
    beside: Option<AnyView>,
) -> impl View {
    built(move |cx| {
        let space = &cx.theme().space;
        let name = row((label(title)
            .size(cx.theme().text.heading)
            .bold(true)
            .tone(tone),))
        .fills_width()
        .boxed();
        let head =
            std::iter::once(name).chain(beside).collect::<Vec<_>>();
        cx.build(
            tile((
                row(head).gap(space.gap).align(AlignItems::Center),
                description(detail),
            ))
            .appear(hidden)
            .transition(Motion::Expand),
        )
    })
}
