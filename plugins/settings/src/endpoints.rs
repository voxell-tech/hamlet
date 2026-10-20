//! The Endpoints page: each place models come from, whether it has a
//! key, and the way to add another.

use hamlet_engine_sdk::api::agents::{AgentsRequest, Endpoint};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, UiExt,
    ViewExt, button, column, icon_button, keyed, label, resource,
    row,
};

use crate::parts::{note, problem};
use crate::{Known, Page, SettingsWorldExt};

pub(crate) fn page() -> AnyView {
    let known = keyed(
        resource::<Known, _>(|known| {
            known
                .settings
                .as_ref()
                .map(|settings| settings.endpoints.clone())
                .unwrap_or_default()
        }),
        |endpoints| {
            column(endpoints.iter().map(endpoint).collect::<Vec<_>>())
                .boxed()
        },
    );
    column((
        note(
            "Any server that speaks the OpenAI chat protocol works. \
             A model is named `endpoint::model`.",
        ),
        known,
        problem(Page::Endpoints),
        row((icon_button("plus", "ADD A PLACE FOR MODELS")
            .style(ButtonStyle::Primary)
            .on_click(|world| {
                world.ask(
                    Page::Endpoints,
                    AgentsRequest::BeginSetup { adding: true },
                );
            }),)),
    ))
    .boxed()
}

/// One endpoint: its name, whether it has a key, what it offers to
/// do, and its address under them.
fn endpoint(endpoint: &Endpoint) -> AnyView {
    let key = match (endpoint.chatgpt, endpoint.has_key) {
        (true, _) => "Using ChatGPT plan",
        (false, true) => "Key set",
        (false, false) => "No key",
    };
    let mut parts = vec![
        label(endpoint.name.clone()).bold(true).boxed(),
        note(key).boxed(),
    ];
    if endpoint.chatgpt {
        parts.push(
            button("MANAGE USAGE")
                .on_click(|world| {
                    world.ask(
                        Page::Endpoints,
                        AgentsRequest::OpenUsage,
                    );
                })
                .boxed(),
        );
    }
    let gone = endpoint.name.clone();
    parts.push(
        button("REMOVE")
            .style(ButtonStyle::Critical)
            .on_click(move |world| {
                world.ask(
                    Page::Endpoints,
                    AgentsRequest::RemoveEndpoint(gone.clone()),
                );
            })
            .boxed(),
    );
    column((row(parts), note(endpoint.url.clone())))
        .space(Space::Half)
        .boxed()
}
