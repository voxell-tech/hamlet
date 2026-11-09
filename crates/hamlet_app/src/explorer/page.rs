//! The file open beside the tree, as drawn: its path and the way to
//! close it over its text, or a word on why there is no text.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, Overflow};
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, label, row,
};
use bevy_fynix::{ViewExt, component, derived, keyed};
use hamlet_ui::styles::{Fills, filling, note, scroll_area};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::MonoText;
use hamlet_ui::{AnyView, View};

use super::editor::{self, Opened, Page};
use super::view::tool;
use super::{Browsing, shown};
use crate::roll::{either, hamlets_moved, moved};

/// The size of the text, in the monospace face.
const TEXT: f32 = 13.0;

/// The open file of the hamlet chosen.
pub(super) fn pane() -> impl View {
    let hamlet = derived(|world: &World| shown(world))
        .when(either(moved::<Browsing>(), hamlets_moved()));
    keyed(hamlet, |hamlet| match hamlet {
        Some(hamlet) => opened(*hamlet).boxed(),
        None => column(()).boxed(),
    })
    .within(filling())
}

fn opened(hamlet: Entity) -> impl View {
    let name = component::<Opened, _>(hamlet, |opened| {
        opened.map(|opened| opened.name.clone())
    });
    let page = component::<Opened, _>(hamlet, |opened| {
        opened.map(|opened| opened.page.clone())
    });
    column((
        keyed(name, move |name| head(hamlet, name.as_deref()))
            .within(column(())),
        keyed(page, |page| body(page.as_ref())).within(filling()),
    ))
    .gap(4.0)
    .fills_height()
}

/// The path of the open file, and the button that closes it.
fn head(hamlet: Entity, name: Option<&str>) -> AnyView {
    let Some(name) = name else {
        return column(()).boxed();
    };
    row((
        row((label(name.to_string())
            .size(12.0)
            .tone(Tone::Faint)
            .wrap(false),))
        .overflow(Overflow::clip_x())
        .fills_width(),
        tool(Glyph::Cross, "Close", move |world| {
            editor::close(world, hamlet);
        }),
    ))
    .gap(2.0)
    .align(AlignItems::Center)
    .boxed()
}

/// The text of `page`, or why there is none.
fn body(page: Option<&Page>) -> AnyView {
    let said = |words: String| column((note(words),)).boxed();
    match page {
        None => said("Select a file to see it here.".into()),
        Some(Page::Reading) => column(()).boxed(),
        Some(Page::Binary) => said("This file is binary.".into()),
        Some(Page::TooBig(size)) => said(format!(
            "This file is too big to show: {} KB, past the 500 KB \
             shown.",
            size / 1024
        )),
        Some(Page::Gone(why)) => {
            said(format!("This file cannot be read: {why}"))
        }
        Some(Page::Text(text)) => scroll_area((lines(text),)).boxed(),
    }
}

/// `text` with the number of each line beside it, unwrapped.
fn lines(text: &str) -> impl View {
    let count = text.lines().count().max(1);
    let numbers = (1..=count)
        .map(|number| number.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let mono = |words: String, tone: Tone| {
        label(words)
            .size(TEXT)
            .tone(tone)
            .wrap(false)
            .tagged(MonoText)
    };
    row((
        column((mono(numbers, Tone::Dim),))
            .align(AlignItems::FlexEnd)
            .shrink(0.0),
        mono(text.to_string(), Tone::Body),
    ))
    .gap(16.0)
    .shrink(0.0)
}
