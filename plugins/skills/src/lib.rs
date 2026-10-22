//! The Skills panel: the skills the agent of the open conversation
//! wrote and keeps in its house, over the library's, which every
//! agent has, each to be taken out.

use agents_ui::{
    CONFIRM, Focused, Heard, ask, confirmation, hear, init,
    library_empty, library_list, show_chat, start,
};
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Papers, Recipient,
};
use hamlet_engine_sdk::ui::{
    AnyView, Face, FrameProps, Size, Space, Tone, UiExt, ViewExt,
    World, column, icon_button, keyed, label, list, resource, row,
    scroll,
};
use hamlet_engine_sdk::{Entry, Message, Plugin};

#[derive(Default)]
struct Skills;

/// The open session the papers were read for.
#[derive(Default)]
struct Read(Option<String>);

fn prepare(world: &mut World) {
    init(world);
    world.init_resource::<Papers>();
    world.init_resource::<Read>();
}

/// Reads the papers of the agent of the open conversation again.
fn follow(world: &mut World) {
    let agent = world
        .resource::<Focused>()
        .0
        .as_ref()
        .and_then(|focus| focus.open.agent.clone());
    world.resource_mut::<Read>().0.clone_from(&agent);
    if agent.is_some() {
        ask(world, AgentsRequest::Papers);
    }
}

/// The agent's own skills.
fn own() -> AnyView {
    keyed(
        resource::<Papers, _>(|papers| papers.skills.clone()),
        |skills| {
            if skills.is_empty() {
                return label("None of its own yet.")
                    .tone(Tone::Dim)
                    .boxed();
            }
            list(
                skills
                    .iter()
                    .map(|skill| {
                        column((
                            label(skill.name.clone()).bold(true),
                            label(skill.description.clone())
                                .size(Size::Small)
                                .tone(Tone::Dim),
                        ))
                        .space(Space::None)
                        .boxed()
                    })
                    .collect::<Vec<_>>(),
            )
            .boxed()
        },
    )
    .boxed()
}

/// An empty library, with the way to the chat.
fn empty() -> AnyView {
    column((
        library_empty(),
        row((icon_button("speech", "OPEN CHAT")
            .on_click(|_| show_chat()),)),
    ))
    .space(Space::Half)
    .boxed()
}

impl Plugin for Skills {
    fn declare(&self, entry: &mut Entry) {
        entry.panel("skills", "Skills").overlay(CONFIRM);
        start(&[AgentsRequest::Focus, AgentsRequest::Skills]);
    }

    fn mount(&mut self, name: &str, world: &mut World) -> AnyView {
        prepare(world);
        if name == CONFIRM {
            return confirmation();
        }
        follow(world);
        let whose = label(resource::<Focused, _>(|focused| {
            match focused.0.as_ref().map(|focus| &focus.with) {
                Some(Recipient::Agent(name)) => {
                    format!("{}: ITS OWN SKILLS", name.to_uppercase())
                }
                _ => "THE MAYOR: ITS OWN SKILLS".to_string(),
            }
        }))
        .size(Size::Small)
        .bold(true)
        .face(Face::Display)
        .tone(Tone::Faint);
        // One scroll for both lists, so that a long one of its own
        // does not push the library out of reach.
        scroll((
            whose,
            own(),
            label("THE LIBRARY, SHARED BY ALL")
                .bold(true)
                .face(Face::Display)
                .tone(Tone::Faint),
            library_list(empty),
        ))
        .boxed()
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        prepare(world);
        let Some(told) = hear(world, message) else {
            return;
        };
        match told {
            Heard::Reply(_, AgentsReply::Papers(papers)) => {
                if *world.resource::<Papers>() != papers {
                    world.insert(papers);
                }
            }
            Heard::Reply(_, AgentsReply::Focus(focus))
            | Heard::Event(AgentsEvent::Focus(focus))
                if world.resource::<Read>().0 != focus.open.agent =>
            {
                follow(world);
            }
            // A turn may have written a skill of its own.
            Heard::Event(AgentsEvent::Working {
                working: false,
                ..
            }) => {
                follow(world);
            }
            Heard::Failed(Some(request), why) => {
                hamlet_engine_sdk::warn(&format!(
                    "{request:?}: {why}"
                ));
            }
            _ => {}
        }
    }
}

hamlet_engine_sdk::export!(Skills);
