//! The library's skills, as a list of cards.

use bevy::ecs::resource::Resource;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, frame, label};
use bevy_fynix::{ViewExt, each, keyed, resource};
use hamlet_core::core::Skills;
pub use hamlet_sdk::cards::{card, item};

use crate::View;
use crate::dock::show_tab;
use crate::icons::Glyph;
use crate::looks::{action, bad_action};
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
}
