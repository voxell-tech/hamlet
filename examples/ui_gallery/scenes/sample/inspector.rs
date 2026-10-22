//! The notes, character and prompt gallery scenes.

use bevy::ecs::world::World;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;
use hamlet_ui::{Me, inspector};

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if matches!(
        scene,
        Scene::Notes | Scene::Character | Scene::Prompt
    ) {
        keep_notes(world);
    }
    if scene == Scene::Character {
        world.insert_resource(inspector::Section::Character);
    }
    if scene == Scene::Prompt {
        let agent = world.resource::<Me>().agent;
        let text = "You are an agent of Hamlet.\n\nSandbox: \
                    seatbelt. Network for tools: off.\n\nKeep the \
                    orchard's notes in order."
            .into();
        world.insert_resource(inspector::Briefing {
            agent: Some(agent),
            text,
        });
        world.insert_resource(inspector::Section::Prompt);
    }
}

/// Sample papers received for the Notes page.
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
    inspector::keep_papers(world);
    let id = world
        .resource::<Outbox>()
        .iter()
        .find_map(|message| match message {
            protocol::ClientMessage::Read(
                protocol::Read::Papers { id, .. },
            ) => Some(id.clone()),
            _ => None,
        })
        .expect("the papers read");
    hamlet_client::receive(
        world,
        protocol::ServerMessage::Read(protocol::ReadResult::Papers {
            to: id,
            papers: protocol::Papers {
                notes,
                character,
                skills: Vec::new(),
            },
        }),
    );
    world.insert_resource(inspector::Section::Notes);
}
