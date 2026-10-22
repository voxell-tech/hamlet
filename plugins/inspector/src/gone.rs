//! The page of an agent that is gone, and of the library.

use agents_ui::{
    AgentsWorldExt, Confirm, breadcrumb, library_list, moved, short,
};
use hamlet_engine_sdk::api::agents::{
    AgentsRequest, Roster, RosterAgent, Selected,
};
use hamlet_engine_sdk::ui::{
    AnyView, ButtonProps, ButtonStyle, FrameProps, Space, Tone,
    UiExt, ViewExt, World, column, derived, icon, icon_button, keyed,
    label, row, scroll,
};

use crate::{HAMLETS, PURPOSE, agent_named, select};

/// The page of the agent `agent`, which is gone, from the hamlet
/// titled `of` if that is still there.
fn page((agent, of): &(RosterAgent, Option<String>)) -> AnyView {
    let fate = match &agent.merged_into {
        Some(host) => format!("Merged into {host}"),
        None => "Removed".to_string(),
    };
    let (restored, erased, hamlet) =
        (agent.clone(), agent.clone(), agent.hamlet.clone());
    let mut trail = vec![HAMLETS.to_string()];
    trail.extend(of.clone());
    column((
        row((
            breadcrumb(
                trail,
                agent.name.clone(),
                move |world, at| {
                    let to = match at {
                        0 => Selected::Nothing,
                        _ => Selected::Hamlet(hamlet.clone()),
                    };
                    select(world, to);
                },
            ),
            icon("struck").tone(Tone::Critical),
            label(fate).tone(Tone::Critical),
        ))
        .space(Space::Gap),
        label(short(&agent.purpose, PURPOSE))
            .tone(Tone::Dim)
            .tip(agent.purpose.clone()),
        label(
            "Its house is kept in the archive. Deleting it frees \
             the space on disk.",
        )
        .tone(Tone::Dim),
        row((
            icon_button("restore", "RESTORE")
                .style(ButtonStyle::Primary)
                .on_click(move |world| {
                    world.decree(
                        &restored.hamlet,
                        format!("/restore {}", restored.name),
                    )
                }),
            icon_button("bin", "DELETE FOR GOOD")
                .style(ButtonStyle::Critical)
                .on_click(move |world| {
                    let name = erased.name.clone();
                    world.confirm(Confirm {
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
                    });
                }),
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
            let agent = agent_named(world, &name)?.clone();
            let of = world
                .resource::<Roster>()
                .hamlets
                .iter()
                .find(|hamlet| hamlet.id == agent.hamlet)
                .map(|hamlet| hamlet.title.clone());
            Some((agent, of))
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
        breadcrumb(
            vec![HAMLETS.into()],
            "THE LIBRARY",
            |world, _| select(world, Selected::Nothing),
        ),
        label(
            "Every agent shares these skills. An agent offers one \
             and the librarian decides whether to keep it.",
        )
        .tone(Tone::Dim),
        scroll((library_list(),)),
    ))
    .space(Space::Gap)
    .boxed()
}
