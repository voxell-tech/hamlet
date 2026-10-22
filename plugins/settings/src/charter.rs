//! The Charter page: the words every agent's prompt opens with, the
//! way to edit them, and the way back to the ones built in.

use agents_ui::markdown;
use hamlet_engine_bevy::api::agents::AgentsRequest;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, Face, FrameProps, Space, Tone, UiExt,
    ViewExt, button, column, description, divider, icon_button,
    keyed, label, resource, row,
};

use crate::parts::problem;
use crate::{Known, Page, SettingsWorldExt};

pub(crate) fn page() -> AnyView {
    let actions = keyed(
        resource::<Known, _>(|known| {
            let own = known
                .charter
                .as_ref()
                .is_some_and(|charter| charter.own);
            (own, own && known.confirming)
        }),
        |&(own, confirming)| {
            if confirming {
                return confirm();
            }
            let mut actions = vec![
                icon_button("pencil", "EDIT")
                    .on_click(|world| {
                        world.ask(
                            Page::Charter,
                            AgentsRequest::EditCharter,
                        );
                    })
                    .tip("Opens the file in your editor")
                    .boxed(),
            ];
            if own {
                actions.push(
                    icon_button("restore", "BACK TO THE DEFAULT")
                        .on_click(|world| {
                            world
                                .resource_mut::<Known>()
                                .confirming = true;
                        })
                        .boxed(),
                );
            }
            row(actions).boxed()
        },
    );
    let shown = keyed(
        resource::<Known, _>(|known| {
            known
                .charter
                .as_ref()
                .map(|charter| charter.text.clone())
                .unwrap_or_default()
        }),
        |text| markdown(text, Tone::Body),
    );
    column((
        description(
            "Every agent reads this at the start of each turn.",
        ),
        actions,
        problem(Page::Charter),
        divider(),
        shown,
    ))
    .boxed()
}

/// The question before the home's own charter is deleted.
fn confirm() -> AnyView {
    column((
        label("GO BACK TO THE DEFAULT CHARTER?")
            .bold(true)
            .face(Face::Display),
        description(
            "Your own charter is deleted from disk. This cannot be \
             undone.",
        ),
        row((
            button("USE THE DEFAULT")
                .style(ButtonStyle::Critical)
                .on_click(|world| {
                    world.resource_mut::<Known>().confirming = false;
                    world.ask(
                        Page::Charter,
                        AgentsRequest::ResetCharter,
                    );
                }),
            button("CANCEL").style(ButtonStyle::Quiet).on_click(
                |world| {
                    world.resource_mut::<Known>().confirming = false;
                },
            ),
        )),
    ))
    .space(Space::Half)
    .boxed()
}
