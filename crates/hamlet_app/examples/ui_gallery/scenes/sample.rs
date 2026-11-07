//! The received realm shared by the gallery scenes.

mod chat;
mod history;
mod hub;
mod menu;
mod needs;
pub(super) mod overview;
mod settings;
mod welcome;

use std::path::PathBuf;

use bevy::ecs::world::World;
use hamlet_app::Convo;
use hamlet_client::chats::{Chat, Status, Usage};
use hamlet_client::{ViewFolder, realm};
use hamlet_protocol::client as protocol;
use hamlet_realm::pick::{Also, Selected, Thing};
pub(super) use welcome::first_run;

use super::Scene;

pub(super) fn setup(world: &mut World) {
    let scene = *world.resource::<Scene>();
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    state.agents[0].purpose =
        "Keeps the orchard's notes in order.".into();
    state.agents[0].spent.cost = "$0.10".into();
    let provider = |id: &str, name: &str| protocol::ProviderView {
        configured: true,
        id: id.into(),
        label: name.into(),
        methods: vec![protocol::LoginMethod::Key],
        name: name.into(),
    };
    state.providers = vec![
        provider("anthropic", "Anthropic"),
        // Named for its models; it takes no key.
        protocol::ProviderView {
            methods: Vec::new(),
            ..provider("faux", "Faux")
        },
        provider("google", "Google"),
        provider("groq", "Groq"),
        provider("openai", "OpenAI"),
        protocol::ProviderView {
            configured: false,
            id: "openrouter".into(),
            label: "Many makers' models, paid as you go".into(),
            methods: vec![
                protocol::LoginMethod::Key,
                protocol::LoginMethod::Subscription,
            ],
            name: "OpenRouter".into(),
        },
    ];
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
    // The key that was not accepted, still in its field.
    if scene == Scene::Welcome(hamlet_app::welcome::Step::Key) {
        hamlet_app::welcome::type_key(
            world,
            "openrouter",
            "sk-or-v1-5e0d2c41a7",
        );
    }
    let agent =
        realm::first_mayor(world).expect("the received mayor");
    let to = protocol::AgentId(2);
    let talk = chat::setup(world, scene);
    let usage = serde_json::json!({
        "calls": 3,
        "unreported": 0,
        "input": 12000,
        "cached": 0,
        "output": 400,
        "reasoning": 0,
        "ms": 500,
        "micros": 100000,
        "lastInput": 12000,
        "summary": "12.4k tokens",
    });
    let usage = serde_json::from_value::<protocol::UsageView>(usage)
        .expect("received usage");
    let session = world
        .spawn((
            Chat(protocol::ChatRef {
                kind: None,
                from: None,
                with: to,
                past: None,
            }),
            talk,
            Status::default(),
            Usage(usage),
        ))
        .id();
    if scene == Scene::Chat {
        // The first turn's steps open, with one call opened in them.
        world.insert_resource(hamlet_app::chat::Unfolded(vec![(
            session,
            "turn-0-tool-0".into(),
        )]));
        world.insert_resource(hamlet_app::chat::OpenTools(vec![(
            session,
            "turn-0-tool-1".into(),
        )]));
    }
    world.insert_resource(Convo {
        session,
        name: "chief".into(),
        agent,
        with: Some("home#0".into()),
    });
    world.init_resource::<Also>();
    world.insert_resource(Selected(Some(Thing::Agent(agent))));
    world.insert_resource(ViewFolder(scratch().join("window")));
    menu::setup(world, scene);
    settings::setup(world, scene);
    overview::setup(world, scene);
    hub::setup(world, scene);
    needs::setup(world, scene);
    if matches!(scene, Scene::History | Scene::Reading) {
        world.insert_resource(hamlet_app::hub::Showing::Sessions);
        history::setup(world, scene == Scene::Reading);
    }
    if scene == Scene::Chief {
        let chief = hamlet_realm::chief::arrive(
            world,
            "wren",
            hamlet_realm::chief::Look {
                hat: hamlet_realm::chief::Hat::Hood,
                ..Default::default()
            },
        );
        world.insert_resource(Selected(Some(Thing::ChiefPlace(
            chief,
        ))));
        world.insert_resource(hamlet_app::hub::Showing::Chief(chief));
    }
}

fn scratch() -> PathBuf {
    std::env::temp_dir()
        .join(format!("hamlet-gallery-{}", std::process::id()))
}
