//! The Skills panel: the skills the agent of the open conversation
//! wrote and keeps in its house, over the library's, which every
//! agent has, each to be taken out.

use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Papers,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, Face, FrameProps, Space, Tone, ViewExt, column,
    description, keyed, label, list, resource, scroll,
};

use crate::shared::{
    AgentsWorldExt, Focused, Heard, library_list, start,
};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<AgentPapers>()
        .init_resource::<Read>()
        .add_panel("skills", "Skills", skills)
        .add_systems(
            Declare,
            start(&[AgentsRequest::Focus, AgentsRequest::Skills]),
        )
        .add_systems(Update, receive);
}

/// The papers of the agent of the open conversation.
#[derive(Resource, Default, PartialEq)]
struct AgentPapers(Papers);

/// The open session the papers were read for.
#[derive(Resource, Default)]
struct Read(Option<String>);

/// Reads the papers of the agent of the open conversation again.
fn follow(world: &mut World) {
    let agent = world.resource::<Focused>().agent();
    // Another's papers are not shown meanwhile.
    if world.resource::<Read>().0 != agent {
        world.insert_resource(AgentPapers::default());
        world.insert_resource(Read(agent.clone()));
    }
    if agent.is_some() {
        world.ask(AgentsRequest::Papers);
    }
}

/// The agent's own skills.
fn own() -> AnyView {
    keyed(
        resource::<AgentPapers, _>(|papers| papers.0.skills.clone()),
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
                            description(skill.description.clone()),
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

fn skills(world: &mut World) -> AnyView {
    follow(world);
    let whose =
        crate::shared::name(resource::<Focused, _>(|focused| {
            format!("{}: ITS OWN SKILLS", focused.whose())
        }));
    // One scroll for both lists, so that a long one of its own does
    // not push the library out of reach.
    scroll((
        whose,
        own(),
        label("THE LIBRARY, SHARED BY ALL")
            .bold(true)
            .face(Face::Display)
            .tone(Tone::Faint),
        library_list(),
    ))
    .boxed()
}

fn receive(
    world: &mut World,
    mut cursor: Local<MessageCursor<Heard>>,
) {
    for told in world.heard(&mut cursor) {
        match told {
            Heard::Reply(_, AgentsReply::Papers(papers))
                if world.resource::<Read>().0.is_some() =>
            {
                world.keep(AgentPapers(papers));
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
            _ => {}
        }
    }
}
