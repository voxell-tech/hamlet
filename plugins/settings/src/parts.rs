//! What the pages are made of: words of each kind, the problem of a
//! page, and the footer that sets up again.

pub(crate) use agents_ui::{name, note, part, setting};
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::ui::{
    AnyView, FrameProps, Size, Space, Tone, UiExt, ViewExt, column,
    icon_button, keyed, label, resource, row,
};

use crate::{Known, Page, SettingsWorldExt};

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
