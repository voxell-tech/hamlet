//! What the pages are made of: words of each kind, the problem of a
//! page, and the footer that sets up again.

use hamlet_engine_sdk::World;
use hamlet_engine_sdk::api::agents::AgentsRequest;
use hamlet_engine_sdk::ui::{
    AnyView, Face, FrameProps, Label, Prop, Size, Space, Tone, UiExt,
    ViewExt, column, icon, icon_button, keyed, label, resource, row,
};

use crate::{Known, Page, SettingsWorldExt};

/// Small words that stand back.
pub(crate) fn note(text: impl Into<Prop<World, String>>) -> Label {
    label(text).size(Size::Small).tone(Tone::Dim)
}

/// The name of a setting, over its control.
pub(crate) fn name(text: &str) -> Label {
    label(text.to_string())
        .size(Size::Small)
        .bold(true)
        .tone(Tone::Faint)
        .face(Face::Display)
}

/// The head of a part of a page: its icon and its name.
pub(crate) fn part(which: &str, text: &str) -> AnyView {
    row((icon(which.to_string()).tone(Tone::Faint), name(text)))
        .space(Space::Half)
        .boxed()
}

/// A setting: its name, its control, and words under it if any.
pub(crate) fn setting(
    text: &str,
    control: AnyView,
    help: Option<&str>,
) -> AnyView {
    let mut parts = vec![name(text).boxed(), control];
    if let Some(help) = help {
        parts.push(note(help).boxed());
    }
    column(parts).space(Space::Half).boxed()
}

/// Why the last change on `page` did not go through, in red, or
/// nothing.
pub(crate) fn problem(page: Page) -> AnyView {
    keyed(
        resource::<Known, _>(move |known| {
            known.problems.get(&page).cloned()
        }),
        |problem| match problem {
            Some(why) => label(why.clone())
                .size(Size::Small)
                .tone(Tone::Critical)
                .boxed(),
            None => column(()).boxed(),
        },
    )
    .boxed()
}

/// The way back to setting up where the model comes from, under the
/// pages.
pub(crate) fn again() -> AnyView {
    column((
        note("Choose again where your model comes from."),
        row((icon_button("restore", "SET UP AGAIN").on_click(
            |world| {
                world.ask(
                    Page::Endpoints,
                    AgentsRequest::BeginSetup { adding: false },
                );
            },
        ),)),
    ))
    .space(Space::Half)
    .boxed()
}
