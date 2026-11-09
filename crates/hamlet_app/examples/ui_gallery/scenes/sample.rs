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

/// The providers the server offers that take only a key, by id and
/// name.
const PROVIDERS: [(&str, &str); 38] = [
    ("amazon-bedrock", "Amazon Bedrock"),
    ("ant-ling", "Ant Ling"),
    ("anthropic", "Anthropic"),
    ("azure", "Azure"),
    ("baseten", "Baseten"),
    ("cerebras", "Cerebras"),
    ("cloudflare-ai-gateway", "Cloudflare AI Gateway"),
    ("cloudflare-workers-ai", "Cloudflare Workers AI"),
    ("deepseek", "DeepSeek"),
    ("fireworks", "Fireworks"),
    ("github-copilot", "GitHub Copilot"),
    ("google", "Google"),
    ("google-vertex", "Google Vertex AI"),
    ("groq", "Groq"),
    ("huggingface", "Hugging Face"),
    ("kimi-coding", "Kimi For Coding"),
    ("meta", "Meta"),
    ("minimax", "MiniMax"),
    ("minimax-cn", "MiniMax CN"),
    ("mistral", "Mistral"),
    ("moonshotai", "Moonshot AI"),
    ("moonshotai-cn", "Moonshot AI CN"),
    ("nvidia", "NVIDIA"),
    ("openai", "OpenAI"),
    ("opencode", "OpenCode Zen"),
    ("opencode-go", "OpenCode Go"),
    ("qwen-token-plan", "Qwen Token Plan"),
    ("qwen-token-plan-cn", "Qwen Token Plan CN"),
    ("qwen-token-plan-individual", "Qwen Token Plan Individual"),
    ("radius", "Radius"),
    ("together", "Together"),
    ("typesafe", "TypeSafe"),
    ("vercel-ai-gateway", "Vercel AI Gateway"),
    ("xai", "xAI"),
    ("xiaomi", "Xiaomi"),
    ("xiaomi-token-plan-ams", "Xiaomi Token Plan AMS"),
    ("xiaomi-token-plan-cn", "Xiaomi Token Plan CN"),
    ("xiaomi-token-plan-sgp", "Xiaomi Token Plan SGP"),
];

pub(super) fn setup(world: &mut World) {
    let scene = *world.resource::<Scene>();
    let mut state = hamlet_testing::realm::state();
    state.chief = "chief".into();
    state.agents[0].purpose =
        "Keeps the orchard's notes in order.".into();
    state.agents[0].spent.cost = "$0.10".into();
    // Every provider the server offers, sorted as it sends them.
    let configured =
        ["anthropic", "faux", "google", "groq", "openai"];
    let provider = |id: &str, name: &str| protocol::ProviderView {
        configured: configured.contains(&id),
        id: id.into(),
        label: name.into(),
        methods: vec![protocol::LoginMethod::Key],
        name: name.into(),
    };
    state.providers = PROVIDERS
        .iter()
        .map(|(id, name)| provider(id, name))
        .collect();
    state.providers.extend([
        protocol::ProviderView {
            label: "Continue with ChatGPT".into(),
            methods: vec![protocol::LoginMethod::Subscription],
            ..provider("chatgpt", "ChatGPT")
        },
        // Named for its models; it takes no key.
        protocol::ProviderView {
            methods: Vec::new(),
            ..provider("faux", "Faux")
        },
        protocol::ProviderView {
            methods: Vec::new(),
            ..provider("openai-codex", "OpenAI Codex (legacy)")
        },
        protocol::ProviderView {
            label: "Many makers' models, paid as you go".into(),
            methods: vec![
                protocol::LoginMethod::Key,
                protocol::LoginMethod::Subscription,
            ],
            ..provider("openrouter", "OpenRouter")
        },
    ]);
    state.providers.sort_by(|a, b| a.id.cmp(&b.id));
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
    if scene.first_run().is_some() {
        world.resource_mut::<hamlet_app::models::Shelf>().models =
            welcome::catalogue();
    }
    // The key that was not accepted, still in its field.
    if scene.first_run() == Some(hamlet_app::welcome::Step::Key) {
        hamlet_app::welcome::type_key(
            world,
            "openrouter",
            "sk-or-v1-5e0d2c41a7",
        );
    }
    if let Scene::KeySearch(words) = scene {
        hamlet_app::welcome::search(world, words);
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
