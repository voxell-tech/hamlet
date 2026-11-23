//! The received realm shared by the gallery scenes.

mod chat;
pub(super) mod hub;
mod images;
mod menu;
mod needs;
pub(super) mod overview;
mod settings;
mod terminal;
pub(super) use terminal::print;
mod welcome;

use std::path::PathBuf;

use bevy::ecs::world::World;
use hamlet_client::chats::{Chat, Status, Usage};
use hamlet_client::{ViewFolder, realm};
use hamlet_core::pick::{Also, Selected, Thing};
use hamlet_hub::convo::Convo;
use hamlet_protocol::client as protocol;
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
        limits: None,
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
    if matches!(scene, Scene::Overview | Scene::SettingsEndpoints) {
        plans(&mut state.providers);
    }
    state.providers.sort_by(|a, b| a.id.cmp(&b.id));
    terminal::state(&mut state, scene);
    hamlet_client::receive(
        world,
        protocol::ServerMessage::State(state),
    );
    if scene.first_run().is_some() {
        world.resource_mut::<hamlet_core::models::Shelf>().models =
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
    if let Scene::ProviderSearch(words) = scene {
        hamlet_app::welcome::search(world, words);
    }
    let agent = realm::first_lead(world).expect("the received lead");
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
    let chat = world
        .spawn((
            Chat(protocol::ChatRef { with: to }),
            talk,
            Status::default(),
            Usage(usage),
        ))
        .id();
    let first = hamlet_hub::hub::first(world);
    if scene == Scene::Chat {
        // The last turn's steps open, with its thinking and its
        // failed call opened in them.
        world.insert_resource(hamlet_hub::chat::Unfolded(vec![(
            first,
            chat,
            "last-ask".into(),
        )]));
        world.insert_resource(hamlet_hub::chat::OpenTools(vec![
            (first, chat, "last-think".into()),
            (first, chat, "last-test".into()),
        ]));
    }
    world.entity_mut(first).insert(Convo {
        chat,
        agent,
        with: Some("agent#2".into()),
    });
    world.init_resource::<Also>();
    world.insert_resource(Selected(Some(Thing::Agent(agent))));
    world.insert_resource(ViewFolder(scratch().join("window")));
    menu::setup(world, scene);
    settings::setup(world, scene);
    overview::setup(world, scene);
    hub::setup(world, scene);
    needs::setup(world, scene);
    if matches!(scene, Scene::Images | Scene::NoImages) {
        images::setup(world, scene, chat);
    }
    terminal::setup(world, scene);
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
        hub::show(world, hamlet_hub::hub::Showing::Chief(chief));
    }
}

fn scratch() -> PathBuf {
    std::env::temp_dir()
        .join(format!("hamlet-gallery-{}", std::process::id()))
}

/// Claude, ChatGPT and OpenRouter signed in, each with what its
/// provider told of its limits: a week used faster than it lasts, a
/// plan used slowly, and a key with no cap of its own.
fn plans(providers: &mut Vec<protocol::ProviderView>) {
    let now = jiff::Timestamp::now().as_millisecond();
    let (hour, minute) = (3_600_000, 60_000);
    let window =
        |id: &str, name: &str, length: i64, used: f64, left: i64| {
            protocol::LimitWindow {
                id: id.into(),
                length: Some(length),
                money: None,
                name: name.into(),
                resets: Some(now + left),
                used: Some(used),
            }
        };
    providers.push(protocol::ProviderView {
        configured: true,
        id: "claude".into(),
        label: "Sign in to Claude".into(),
        limits: Some(protocol::ProviderLimits {
            checked: now - 2 * minute,
            plan: Some("Max".into()),
            state: protocol::LimitsState::Ok,
            windows: vec![
                window(
                    "five_hour",
                    "Session",
                    300,
                    46.0,
                    2 * hour + 26 * minute,
                ),
                window(
                    "seven_day",
                    "Weekly",
                    10_080,
                    61.0,
                    104 * hour,
                ),
                window(
                    "model:Fable",
                    "Weekly · Fable",
                    10_080,
                    12.0,
                    104 * hour,
                ),
            ],
        }),
        methods: vec![protocol::LoginMethod::Subscription],
        name: "Claude".into(),
    });
    for (id, name) in
        [("chatgpt", "ChatGPT"), ("openrouter", "OpenRouter")]
    {
        if !providers.iter().any(|p| p.id == id) {
            providers.push(protocol::ProviderView {
                configured: true,
                id: id.into(),
                label: name.into(),
                limits: None,
                methods: vec![protocol::LoginMethod::Subscription],
                name: name.into(),
            });
        }
    }
    for provider in providers.iter_mut() {
        match provider.id.as_str() {
            "chatgpt" => {
                provider.configured = true;
                provider.limits = Some(protocol::ProviderLimits {
                    checked: now - minute,
                    plan: Some("Plus".into()),
                    state: protocol::LimitsState::Ok,
                    windows: vec![
                        window(
                            "primary",
                            "Session",
                            300,
                            18.0,
                            3 * hour,
                        ),
                        window(
                            "secondary",
                            "Weekly",
                            10_080,
                            34.0,
                            61 * hour,
                        ),
                    ],
                });
            }
            "openrouter" => {
                provider.configured = true;
                // Most keys carry no cap of their own: OpenRouter
                // bills the account's credits.
                provider.limits = Some(protocol::ProviderLimits {
                    checked: now - minute,
                    plan: None,
                    state: protocol::LimitsState::Ok,
                    windows: Vec::new(),
                });
            }
            _ => {}
        }
    }
}
