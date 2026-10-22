//! The library's skills, shared by every agent, each to be taken
//! out.

use hamlet_engine_bevy::AgentsRequest;
use hamlet_engine_bevy::api::agents::Skill;
use hamlet_engine_bevy::prelude::World;
use hamlet_engine_bevy::ui::{
    AnyView, ButtonStyle, FrameProps, Size, Space, Tone, UiExt,
    ViewExt, button, column, icon_button, keyed, label, list,
    resource, row,
};

use crate::client::{AgentsWorldExt, Focused, Shelves, show_chat};
use crate::confirm::Confirm;

const EMPTY: &str = "The library is empty. Agents offer skills and \
                     the librarian decides which to keep.";

/// Asks before `skill` is deleted from the library.
fn take_out(world: &mut World, skill: &str) {
    let Some(to) = world.resource::<Focused>().with() else {
        return;
    };
    world.confirm(Confirm {
        question: format!(
            "TAKE {} OUT OF THE LIBRARY?",
            skill.to_uppercase()
        ),
        outcome: format!(
            "{skill} is deleted from the library for every hamlet. \
             This cannot be undone."
        ),
        working: Vec::new(),
        verb: "TAKE OUT".into(),
        then: vec![AgentsRequest::Send {
            to,
            text: format!("/unshelve {skill}"),
        }],
    });
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

/// The library's skills, a card each, or what an empty library says
/// with the way to the chat.
pub fn library_list() -> AnyView {
    keyed(
        resource::<Shelves, _>(|shelves| shelves.0.clone()),
        |skills| {
            if skills.is_empty() {
                return column((
                    label(EMPTY).tone(Tone::Dim),
                    row((icon_button("speech", "OPEN CHAT")
                        .on_click(|_| show_chat()),)),
                ))
                .space(Space::Half)
                .boxed();
            }
            list(skills.iter().map(card).collect::<Vec<_>>()).boxed()
        },
    )
    .boxed()
}
