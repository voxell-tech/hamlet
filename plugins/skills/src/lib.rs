//! The Skills panel: the skills the agent of the open conversation
//! wrote and keeps in its house, over the library's, which every
//! agent has, each to be taken out.

use agents_ui::{
    AgentsWorldExt, CONFIRM, Focused, Heard, confirmation,
    library_list, start,
};
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Papers,
};
use hamlet_engine_sdk::ui::{
    AnyView, Face, FrameProps, Size, Space, Tone, ViewExt, World,
    column, keyed, label, list, resource, scroll,
};
use hamlet_engine_sdk::{Entry, Message, Plugin};

#[derive(Default)]
struct Skills;

/// The open session the papers were read for.
#[derive(Default)]
struct Read(Option<String>);

fn prepare(world: &mut World) {
    world.init_agents();
    world.init_resource::<Papers>();
    world.init_resource::<Read>();
}

/// Reads the papers of the agent of the open conversation again.
fn follow(world: &mut World) {
    let agent = world.resource::<Focused>().agent();
    // Another's papers are not shown meanwhile.
    if world.resource::<Read>().0 != agent {
        world.insert(Papers::default());
        world.insert(Read(agent.clone()));
    }
    if agent.is_some() {
        world.ask(AgentsRequest::Papers);
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
        let whose =
            agents_ui::name(resource::<Focused, _>(|focused| {
                format!("{}: ITS OWN SKILLS", focused.whose())
            }));
        // One scroll for both lists, so that a long one of its own
        // does not push the library out of reach.
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

    fn receive(&mut self, message: Message, world: &mut World) {
        prepare(world);
        let Some(told) = world.hear(message) else {
            return;
        };
        match told {
            Heard::Reply(_, AgentsReply::Papers(papers))
                if world.resource::<Read>().0.is_some() =>
            {
                world.keep(papers);
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
