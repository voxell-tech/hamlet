//! The library's skills, shared by every agent, each to be taken
//! out.

use hamlet_engine_sdk::AgentsRequest;
use hamlet_engine_sdk::api::agents::Skill;
use hamlet_engine_sdk::ui::{
    AnyView, ButtonStyle, FrameProps, Size, Space, Tone, UiExt,
    ViewExt, World, button, column, keyed, label, list, resource,
    row,
};

use crate::client::{Focused, Shelves};
use crate::confirm::{Confirm, confirm};

const EMPTY: &str = "The library is empty. Agents offer skills and \
                     the librarian decides which to keep.";

/// Asks before `skill` is deleted from the library.
fn take_out(world: &mut World, skill: &str) {
    let Some(to) = world
        .resource::<Focused>()
        .0
        .as_ref()
        .map(|focus| focus.with.clone())
    else {
        return;
    };
    confirm(
        world,
        Confirm {
            question: format!(
                "TAKE {} OUT OF THE LIBRARY?",
                skill.to_uppercase()
            ),
            outcome: format!(
                "{skill} is deleted from the library for every \
                 hamlet. This cannot be undone."
            ),
            working: Vec::new(),
            verb: "TAKE OUT".into(),
            then: vec![AgentsRequest::Send {
                to,
                text: format!("/unshelve {skill}"),
            }],
        },
    );
}

fn card(skill: &Skill) -> AnyView {
    let name = skill.name.clone();
    row((
        column((
            label(skill.name.clone()).bold(true),
            label(skill.description.clone())
                .size(Size::Small)
                .tone(Tone::Dim),
        ))
        .space(Space::None),
        button("TAKE OUT")
            .style(ButtonStyle::Critical)
            .on_click(move |world| take_out(world, &name)),
    ))
    .space(Space::Gap)
    .boxed()
}

/// The library's skills, a card each, or `empty` when there is
/// none.
pub fn library_list(empty: fn() -> AnyView) -> AnyView {
    keyed(
        resource::<Shelves, _>(|shelves| shelves.0.clone()),
        move |skills| {
            if skills.is_empty() {
                return empty();
            }
            list(skills.iter().map(card).collect::<Vec<_>>()).boxed()
        },
    )
    .boxed()
}

/// What an empty library says.
pub fn library_empty() -> AnyView {
    label(EMPTY).tone(Tone::Dim).boxed()
}
