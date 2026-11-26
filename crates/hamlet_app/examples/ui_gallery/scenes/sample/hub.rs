//! The memory, character and prompt gallery scenes.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use hamlet_client::outbox::Outbox;
use hamlet_hub::convo::Convo;
use hamlet_hub::hub::{self, Showing};
use hamlet_protocol::client as protocol;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if matches!(
        scene,
        Scene::Memory | Scene::Character | Scene::Prompt
    ) {
        keep_notes(world);
        // The thread with its details open beside it.
        let first = hub::first(world);
        world
            .entity_mut(first)
            .insert((Showing::Thread, hub::Sheet::Details));
        let agent = agent(world);
        world.entity_mut(agent).insert((
            hamlet_client::realm::Allowance(5.0),
            hamlet_client::realm::ToolsOff(vec!["bash".into()]),
        ));
    }
    if matches!(
        scene,
        Scene::Hub
            | Scene::HubMenu
            | Scene::Models
            | Scene::ModelsSearch
            | Scene::Desk
            | Scene::Toasts
            | Scene::Chat
            | Scene::Group
            | Scene::Links
            | Scene::Edits
            | Scene::Pictures
            | Scene::Images
            | Scene::NoImages
            | Scene::Rewind
            | Scene::Quiet
            | Scene::Field
            | Scene::Code
            | Scene::Context
            | Scene::Compacting
    ) {
        show(world, Showing::Thread);
    }
    if matches!(scene, Scene::Context | Scene::Compacting) {
        measure(world, scene == Scene::Compacting);
    }
    if scene == Scene::HubMenu {
        let agent = agent(world);
        hamlet_hub::world_menu::open_agent(
            world,
            agent,
            bevy::math::Vec2::new(1580.0, 150.0),
        );
    }
    if matches!(scene, Scene::Models | Scene::ModelsSearch) {
        offer_models(world);
    }
    if scene == Scene::ModelsSearch {
        hamlet_hub::world_menu::search_models(world, "wren");
    }
    if scene == Scene::Profile {
        let profile = hamlet_client::realm::profile(
            world,
            protocol::ProfileId(2),
        )
        .expect("the received profile");
        // As the hub turns its rail to a profile's page.
        let first = hub::first(world);
        world
            .entity_mut(first)
            .insert((Showing::Profile(profile), hub::RailTab::City));
        keep_notes(world);
    }
    if scene == Scene::Prompt {
        let agent = agent(world);
        let section =
            |name: &str, text: &str| protocol::PromptSection {
                name: name.into(),
                text: text.into(),
            };
        let first = hub::first(world);
        world.entity_mut(first).insert(hub::Briefing {
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
        show(world, Showing::Prompt);
    }
}

/// The agent the first Hub's chat is with.
pub(super) fn agent(world: &mut World) -> Entity {
    let first = hub::first(world);
    world
        .get::<Convo>(first)
        .map_or(Entity::PLACEHOLDER, |convo| convo.agent)
}

/// Turns the first Hub to `showing`.
pub(super) fn show(world: &mut World, showing: Showing) {
    let first = hub::first(world);
    world.entity_mut(first).insert(showing);
}

/// A catalogue of a few models over several providers.
pub(super) fn catalogue() -> Vec<protocol::Listed> {
    use hamlet_protocol::Effort;
    let card = |name: &str,
                efforts: &[Effort],
                fast: bool,
                (input, output): (f64, f64),
                context: i64| protocol::Listed {
        context: Some(context),
        default_effort: efforts
            .contains(&Effort::Medium)
            .then_some(Effort::Medium),
        efforts: efforts.to_vec(),
        fast,
        images: None,
        name: name.into(),
        note: String::new(),
        price: Some(protocol::Price { input, output }),
        priced: protocol::ListedPriced::Listed,
        title: String::new(),
    };
    let thinks = [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::XHigh,
        Effort::Max,
    ];
    vec![
        card("faux::faux-1", &[], false, (0.0, 0.0), 32_000),
        card(
            "openai::wren-large",
            &thinks,
            true,
            (3.0, 15.0),
            200_000,
        ),
        card(
            "openai::wren-small",
            &thinks[..3],
            true,
            (0.4, 1.6),
            128_000,
        ),
        card(
            "openai::wren-mini",
            &thinks[..2],
            false,
            (0.15, 0.6),
            128_000,
        ),
        card(
            "openai::sparrow-4",
            &thinks,
            false,
            (2.5, 10.0),
            128_000,
        ),
        card(
            "openai::sparrow-4-turbo",
            &thinks[..3],
            true,
            (10.0, 30.0),
            128_000,
        ),
        card(
            "google::wren-flash",
            &[Effort::Off],
            false,
            (0.3, 2.5),
            1_000_000,
        ),
        card(
            "anthropic::finch-pro",
            &thinks[..4],
            false,
            (15.0, 75.0),
            200_000,
        ),
        card(
            "groq::finch-lite",
            &thinks[..3],
            false,
            (0.8, 4.0),
            200_000,
        ),
    ]
}

/// That catalogue, and the popup of the seated agent's model open
/// over its composer while it runs `openai::wren-large`.
/// The first Hub's chat with most of a 200k context held, and
/// `compacting` it.
fn measure(world: &mut World, compacting: bool) {
    world.resource_mut::<hamlet_core::models::Shelf>().models =
        catalogue();
    let agent = agent(world);
    if let Some(mut account) =
        world.get_mut::<hamlet_client::realm::Account>(agent)
    {
        account.model = "openai::wren-large".into();
    }
    let first = hub::first(world);
    let Some(chat) =
        world.get::<Convo>(first).map(|convo| convo.chat)
    else {
        return;
    };
    if let Some(mut status) =
        world.get_mut::<hamlet_client::chats::Status>(chat)
    {
        status.context = Some(148_600);
        status.compacting = compacting;
    }
}

/// Opens the first Hub's context popup once its ring is laid out.
pub(in crate::scenes) fn open_context(
    world: &mut World,
    mut update: bevy::ecs::system::Local<u32>,
) {
    *update += 1;
    if *update == 30 {
        let first = hub::first(world);
        hub::open_context(world, first);
    }
}

fn offer_models(world: &mut World) {
    world.resource_mut::<hamlet_core::models::Shelf>().models =
        catalogue();
    let agent = agent(world);
    if let Some(mut account) =
        world.get_mut::<hamlet_client::realm::Account>(agent)
    {
        account.model = "openai::wren-large".into();
    }
    hamlet_hub::world_menu::open_model(world, agent);
    // Starred after it opens, so it opens on the agent's provider.
    world.resource_mut::<hamlet_hub::world_menu::Favorites>().0 =
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
