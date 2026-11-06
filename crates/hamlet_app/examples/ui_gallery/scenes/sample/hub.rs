//! The memory, character and prompt gallery scenes.

use bevy::ecs::world::World;
use hamlet_app::Convo;
use hamlet_app::hub::{self, Showing};
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if matches!(
        scene,
        Scene::Memory | Scene::Character | Scene::Prompt
    ) {
        keep_notes(world);
        // The thread with its details open beside it.
        world.insert_resource(Showing::Thread);
        world.insert_resource(hub::Sheet::Details);
    }
    if matches!(
        scene,
        Scene::Hub
            | Scene::Desk
            | Scene::Toasts
            | Scene::Chat
            | Scene::Group
            | Scene::Links
            | Scene::Edits
            | Scene::Pictures
            | Scene::Rewind
            | Scene::Quiet
    ) {
        world.insert_resource(Showing::Thread);
    }
    if scene == Scene::Hub {
        world.resource_mut::<hub::Rail>().show_rows();
    }
    if scene == Scene::Scholar {
        let scholar = hamlet_client::realm::scholar(
            world,
            protocol::ScholarId(2),
        )
        .expect("the received scholar");
        world.insert_resource(Showing::Scholar(scholar));
        keep_notes(world);
    }
    if scene == Scene::Prompt {
        let agent = world.resource::<Convo>().agent;
        let text = "You are an agent of Hamlet.\n\nSandbox: \
                    seatbelt. Network for tools: off.\n\nKeep the \
                    orchard's notes in order."
            .into();
        world.insert_resource(hub::Briefing {
            agent: Some(agent),
            text,
        });
        world.insert_resource(Showing::Prompt);
    }
}

/// Sample papers received for the Memory page.
fn keep_notes(world: &mut World) {
    let character = String::from(
        "You are **Juniper**, the orchard's keeper of notes.\n\n- \
         Speak plainly, in short sentences.\n- Say what you do not \
         know.\n- Name the row and the tree, never \"that one\".\n",
    );
    let notes = String::from(
        "# Orchard\n\nWhat I know of the rows, **newest \
         first**.\n\n## Before the frost\n\n- Row 4 wants straw at \
         the roots\n- The stake by the gate is split\n- [x] The \
         last pears are off the high branches\n\n## The \
         press\n\nPip keeps the key, by the mill door. To press the \
         late fruit:\n\n```sh\npress --rows 4,7,9 --before \
         dusk\n```\n\n| Row | Grafted |\n|---|---|\n| Conference | \
         March |\n| Comice | April |\n\n> The quince stock came too \
         late for a fourth row.\n",
    );
    hub::keep_papers(world);
    let id = world
        .resource::<Outbox>()
        .iter()
        .filter_map(|message| match message {
            protocol::ClientMessage::Read(
                protocol::Read::Papers { id, .. },
            ) => Some(id.clone()),
            _ => None,
        })
        .last()
        .expect("the papers read");
    hamlet_client::receive(
        world,
        protocol::ServerMessage::Read(protocol::ReadResult::Papers {
            to: id,
            papers: protocol::Papers {
                character,
                learned: Vec::new(),
                memory: vec![protocol::MemoryFile {
                    description: String::new(),
                    path: "notes.md".into(),
                    text: notes,
                }],
                skills: Vec::new(),
            },
        }),
    );
}
