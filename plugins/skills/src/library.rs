//! The library every agent shares.

use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{FrameProps, column, frame, label};
use bevy_fynix::{ViewExt, each, keyed, resource};
use hamlet_core::core::Skills;
use hamlet_sdk::View;
use hamlet_sdk::dock::show_tab;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::looks::action;

use crate::row::{SkillRow, rows, skill};

/// The library's skills, each with the press that takes it out.
pub(crate) fn library() -> impl View {
    column((
        empty(),
        each(
            resource::<Skills, _>(rows),
            |r: &SkillRow| format!("{}|{}", r.name, r.description),
            skill,
        )
        .within(column(()).gap(8.0)),
    ))
    .gap(0.0)
}

/// The words of the library while it is empty, and a way to the
/// chat.
fn empty() -> impl View {
    keyed(resource::<Skills, _>(|l| l.0.is_empty()), |empty| {
        if !*empty {
            return frame().boxed();
        }
        column((
            label(
                "The library is empty. Agents offer skills and the \
                 librarian decides which to keep.",
            )
            .tone(Tone::Dim),
            action(Glyph::Talk, "OPEN CHAT", |w| {
                show_tab(w, "agent")
            }),
        ))
        .gap(12.0)
        .padding(UiRect::bottom(px(12.0)))
        .align(AlignItems::FlexStart)
        .boxed()
    })
    .within(column(()))
}
