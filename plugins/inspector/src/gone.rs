//! The page of an agent that is gone, and of the library.

use agents_ui::{
    Confirm, confirm, library_empty, library_list, moved, short,
};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, Roster, RosterAgent, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, Tone,
    UiExt, ViewExt, World, column, derived, heading, icon,
    icon_button, keyed, label, row, scroll,
};

use crate::{agent_named, select};

/// The characters of a purpose shown.
const PURPOSE: usize = 96;

/// Gives the chief's command `text` to the hamlet of `agent`.
fn decree(world: &mut World, agent: &RosterAgent, text: String) {
    let hamlet = agent.hamlet.clone();
    agents_ui::ask(world, AgentsRequest::Decree { hamlet, text });
}

fn page(agent: &RosterAgent) -> AnyView {
    let fate = match &agent.merged_into {
        Some(host) => format!("Merged into {host}"),
        None => "Removed".to_string(),
    };
    let (restored, erased, hamlet) =
        (agent.clone(), agent.clone(), agent.hamlet.clone());
    column((
        row((
            heading(agent.name.to_uppercase()),
            icon("struck").tone(Tone::Critical),
            label(fate).tone(Tone::Critical),
        ))
        .space(Space::Gap),
        label(short(&agent.purpose, PURPOSE)).tone(Tone::Dim),
        label(
            "Its house is kept in the archive. Deleting it frees \
             the space on disk.",
        )
        .tone(Tone::Dim),
        row((
            icon_button("restore", "RESTORE")
                .style(ButtonStyle::Primary)
                .on_click(move |world| {
                    decree(
                        world,
                        &restored,
                        format!("/restore {}", restored.name),
                    )
                }),
            icon_button("bin", "DELETE FOR GOOD")
                .style(ButtonStyle::Critical)
                .on_click(move |world| {
                    let name = erased.name.clone();
                    confirm(
                        world,
                        Confirm {
                            question: format!(
                                "DELETE {} FOR GOOD?",
                                name.to_uppercase()
                            ),
                            outcome: format!(
                                "The archived house of {name} is \
                                 deleted from disk. This cannot be \
                                 undone."
                            ),
                            working: Vec::new(),
                            verb: "DELETE".into(),
                            then: vec![AgentsRequest::Decree {
                                hamlet: erased.hamlet.clone(),
                                text: format!("/erase {name}"),
                            }],
                        },
                    );
                }),
            icon_button("house", "ITS HAMLET").on_click(
                move |world| {
                    select(world, Selected::Hamlet(hamlet.clone()))
                },
            ),
        ))
        .space(Space::Half),
    ))
    .space(Space::Gap)
    .boxed()
}

/// The page of the agent `name`, which is gone.
pub(crate) fn gone(name: String) -> AnyView {
    keyed(
        derived(move |world: &World| {
            agent_named(world, &name).cloned()
        })
        .when(moved::<Roster>()),
        |agent| match agent {
            Some(agent) => page(agent),
            None => column(()).boxed(),
        },
    )
    .boxed()
}

/// The library: what it is, and its shelves.
pub(crate) fn library() -> AnyView {
    column((
        heading("THE LIBRARY"),
        row((icon_button("back", "ALL HAMLETS")
            .style(ButtonStyle::Quiet)
            .on_click(|world| select(world, Selected::Nothing)),)),
        label(
            "Every agent shares these skills. An agent offers one \
             and the librarian decides whether to keep it.",
        )
        .tone(Tone::Dim),
        scroll((library_list(library_empty),)),
    ))
    .space(Space::Gap)
    .boxed()
}
