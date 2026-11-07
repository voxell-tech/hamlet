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
        let agent = world.resource::<Convo>().agent;
        world.entity_mut(agent).insert((
            hamlet_client::realm::Allowance(5.0),
            hamlet_client::realm::ToolsOff(vec!["bash".into()]),
        ));
    }
    if matches!(
        scene,
        Scene::Hub
            | Scene::Models
            | Scene::ModelsSearch
            | Scene::Desk
            | Scene::Toasts
            | Scene::Chat
            | Scene::Group
            | Scene::Links
            | Scene::Edits
            | Scene::Pictures
            | Scene::Rewind
            | Scene::Quiet
            | Scene::Field
    ) {
        world.insert_resource(Showing::Thread);
    }
    if scene == Scene::Hub {
        world.resource_mut::<hub::Rail>().show_rows();
    }
    if matches!(scene, Scene::Models | Scene::ModelsSearch) {
        offer_models(world);
    }
    if scene == Scene::ModelsSearch {
        hamlet_app::world_menu::search_models(world, "wren");
    }
    if scene == Scene::Scholar {
        let scholar = hamlet_client::realm::scholar(
            world,
            protocol::ScholarId(2),
        )
        .expect("the received scholar");
        world.insert_resource(Showing::Scholar(scholar));
        // As the hub turns its rail to a scholar's page.
        world.insert_resource(hub::RailTab::City);
        keep_notes(world);
    }
    if scene == Scene::Prompt {
        let agent = world.resource::<Convo>().agent;
        let section =
            |name: &str, text: &str| protocol::PromptSection {
                name: name.into(),
                text: text.into(),
            };
        world.insert_resource(hub::Briefing {
            agent: Some(agent),
            prompt: vec![
                section(
                    "charter",
                    "You are an agent of Hamlet, a personal \
                     assistant shared by a few people.\nBe concise. \
                     Act rather than describe what you would do.",
                ),
                section(
                    "machine",
                    "System: Linux.\nSandbox: seatbelt. Network for \
                     tools: off.",
                ),
                section(
                    "character",
                    "You are Juniper, the orchard's keeper of \
                     notes.\n- Speak plainly, in short \
                     sentences.\n- Say what you do not know.",
                ),
                section(
                    "memory",
                    "notes.md: Row 4 wants straw at the roots. Pip \
                     keeps the key to the press.",
                ),
                section("skills", "No skills yet."),
                section("budget", "Spent today: $1.20 of $20.00."),
            ],
            ..Default::default()
        });
        world.insert_resource(Showing::Prompt);
    }
}

/// A catalogue of a few models over several providers.
pub(super) fn catalogue() -> Vec<protocol::Listed> {
    use hamlet_protocol::Effort;
    let card = |name: &str, efforts: &[Effort], fast: bool| {
        protocol::Listed {
            context: Some(200_000),
            default_effort: efforts
                .contains(&Effort::Medium)
                .then_some(Effort::Medium),
            efforts: efforts.to_vec(),
            fast,
            name: name.into(),
            note: String::new(),
            price: Some(protocol::Price {
                input: 3.0,
                output: 15.0,
            }),
            priced: protocol::ListedPriced::Listed,
        }
    };
    let thinks = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];
    vec![
        card("faux::faux-1", &[], false),
        card("openai::wren-large", &thinks, true),
        card("openai::wren-small", &thinks[..3], true),
        card("openai::wren-mini", &thinks[..2], false),
        card("openai::sparrow-4", &thinks, false),
        card("openai::sparrow-4-turbo", &thinks[..3], true),
        card("google::wren-flash", &[Effort::Off], false),
        card("anthropic::finch-pro", &thinks[..4], false),
        card("groq::finch-lite", &thinks[..3], false),
    ]
}

/// That catalogue, and the popup of the seated agent's model open
/// over its composer while it runs `openai::wren-large`.
fn offer_models(world: &mut World) {
    world.resource_mut::<hamlet_app::models::Shelf>().models =
        catalogue();
    let agent = world.resource::<Convo>().agent;
    if let Some(mut account) =
        world.get_mut::<hamlet_client::realm::Account>(agent)
    {
        account.model = "openai::wren-large".into();
    }
    hamlet_app::world_menu::open_model(world, agent);
    // Starred after it opens, so it opens on the agent's provider.
    world.resource_mut::<hamlet_app::world_menu::Favorites>().0 =
        vec!["openai::wren-large".into()];
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
