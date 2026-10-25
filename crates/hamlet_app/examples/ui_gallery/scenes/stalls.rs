//! The Agent Bazaar's stalls, each at its tab of the bazaar's panel,
//! with buttons pressed by their words to open a page. OpenRouter's
//! and skills.sh's answers are given by hand, so nothing is asked of
//! the network, and the realm draws a hamlet template's town and an
//! official look apart from the world.

use bevy::app::{App, PostStartup, Update};
use bevy::camera::Camera;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::With;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Commands, Local, Query, Res};
use bevy::ecs::world::World;
use bevy::ui::widget::Text;
use bevy::ui_widgets::{Activate, Button};
use hamlet_app::bazaar::{
    Dealt, Offered, Staged, StallShown, offer, stage,
};
use hamlet_client::realm::{Outfit, Providers, Skills};
use hamlet_protocol::client as protocol;
use hamlet_realm::WorldCamera;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::{
    DetailSettings, GeneratorSettings, house,
};
use hamlet_realm::palette::Palette;
use serde_json::json;

use super::dock::seed_dock;
use super::{Scene, Trade};

/// The updates between one press and the next.
const BETWEEN: u32 = 40;

/// The seed of the land the template's towns are laid out on.
const SEED: u32 = 7;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        hamlet_realm::scene::plugin,
        hamlet_realm::generator::plugin,
        hamlet_realm::diorama::plugin,
        hamlet_realm::viewer::plugin,
    ))
    // The robots of the dioramas are drawn as near ones are.
    .init_resource::<DetailSettings>()
    .add_systems(PostStartup, (at_the_stall.after(seed_dock), unseen))
    .add_systems(Update, press);
}

/// The world's own camera draws nothing here: the world is a still.
fn unseen(mut cameras: Query<&mut Camera, With<WorldCamera>>) {
    for mut camera in &mut cameras {
        camera.is_active = false;
    }
}

/// The bazaar at the stall of the scene, its stalls stocked as if
/// OpenRouter and skills.sh had just answered.
fn at_the_stall(world: &mut World) {
    let (stall, trade) = match *world.resource::<Scene>() {
        Scene::Stall(stall, _) => (stall, None),
        Scene::Market(stall, trade, _) => (stall, Some(trade)),
        _ => return,
    };
    world.insert_resource(StallShown(stall));
    let land = Land::new(
        SEED,
        GeneratorSettings::default(),
        world.resource::<Palette>().clone(),
    );
    let views = views(&land);
    world.insert_resource(land);
    world.resource_mut::<Providers>().0.push(
        protocol::ProviderView {
            configured: true,
            id: "openrouter".into(),
            label: "Sign in with OpenRouter".into(),
            methods: Vec::new(),
            name: "OpenRouter".into(),
        },
    );
    world.resource_mut::<Skills>().0.push(protocol::Skill {
        name: "frontend-design".into(),
        description: "Distinctive interfaces.".into(),
    });
    offer(world, offered());
    if let Some(trade) = trade {
        stage(
            world,
            Staged {
                views,
                ..market(trade)
            },
        );
        if trade == Trade::SignedIn
            && let Some(mayor) = hamlet_client::realm::mayor(world)
        {
            world.entity_mut(mayor).insert(Outfit(protocol::Looks {
                skin: Some(format!("{SAKURA}@1.0.0")),
                ..protocol::Looks::default()
            }));
        }
    }
}

/// What OpenRouter and skills.sh answer in the scenes.
fn offered() -> Offered {
    let model = |id: &str, name: &str, input: &str, context: u64| {
        json!({"id": id, "name": name, "created": 1_790_000_000u64,
            "description": "Its maker's most capable model, for agents \
                that plan, call tools and read pictures.\n\nMore of it.",
            "context_length": context,
            "architecture": {"input_modalities": ["text", "image"]},
            "pricing": {"prompt": input, "completion": "0.000015"},
            "supported_parameters": ["tools", "reasoning"],
            "benchmarks": {"artificial_analysis":
                {"intelligence_index": 62.0}}})
    };
    let skill =
        |source: &str, id: &str, installs: u64, official: bool| {
            json!({"source": source, "skillId": id, "name": id,
            "installs": installs, "isOfficial": official})
        };
    Offered {
        models: json!({"data": [
            model("anthropic/claude-sonnet-4.5",
                "Anthropic: Claude Sonnet 4.5", "0.000003", 1_000_000),
            model("google/gemini-2.5-flash",
                "Google: Gemini 2.5 Flash", "0.0000003", 1_048_576),
            model("deepseek/deepseek-v3.2",
                "DeepSeek: DeepSeek V3.2", "0.00000027", 163_840),
            model("openai/gpt-5-mini",
                "OpenAI: GPT-5 Mini", "0.00000025", 400_000),
        ]}),
        skills: json!({"skills": [
            skill("vercel-labs/agent-skills",
                "vercel-react-best-practices", 57_200, true),
            skill("anthropics/skills", "frontend-design", 41_800, true),
            skill("obra/superpowers", "brainstorming", 12_400, false),
            skill("obra/superpowers", "test-driven-development", 9_800,
                false),
        ], "hasMore": false}),
        files: vec![(
            "vercel-labs/agent-skills/vercel-react-best-practices"
                .into(),
            json!({"files": [
                {"path": "SKILL.md", "contents": "---\nname: \
                    vercel-react-best-practices\ndescription: React and \
                    Next.js performance guidelines from Vercel \
                    Engineering. Use when writing, reviewing or \
                    refactoring React code.\n---\n\n# React best \
                    practices\n\nA guide to writing fast React and \
                    Next.js apps, kept by Vercel's engineers.\n\n## When \
                    to use it\n\n- Writing new components or pages\n- \
                    Reviewing a pull request for speed\n\n## Rules\n\n1. \
                    Fetch on the server where you can.\n2. Keep client \
                    components small.\n"},
                {"path": "rules/server.md", "contents": "Fetch early.\n"}
            ]}),
        )],
    }
}

/// Presses, one after another, the buttons whose words the scene
/// names.
fn press(
    mut commands: Commands,
    scene: Res<Scene>,
    texts: Query<(Entity, &Text)>,
    parents: Query<&ChildOf>,
    buttons: Query<(), With<Button>>,
    mut update: Local<u32>,
) {
    let (Scene::Stall(_, words) | Scene::Market(_, _, words)) =
        *scene
    else {
        return;
    };
    *update += 1;
    if !update.is_multiple_of(BETWEEN) {
        return;
    }
    let Some(word) = words.get((*update / BETWEEN) as usize - 1)
    else {
        return;
    };
    let pressed = texts
        .iter()
        .filter(|(_, text)| text.0 == *word)
        .find_map(|(text, _)| {
            parents
                .iter_ancestors(text)
                .find(|node| buttons.contains(*node))
        });
    if let Some(button) = pressed {
        commands.trigger(Activate { entity: button });
    }
}

/// The bazaar's server as `trade` has it: three agents, two tools
/// and a hamlet on its shelves, one of each the chief's in some way.
fn market(trade: Trade) -> Staged {
    let seller = |login: &str, name: &str| json!({"id": "u_1", "login": login, "name": name, "verified": true});
    let version = |version: &str,
                   changes: &str,
                   meta: serde_json::Value| {
        json!({"version": version, "changes": changes, "size": 18_234,
            "sha256": "0", "createdAt": "2026-10-20T00:00:00Z", "meta": meta})
    };
    let agent = |name: &str, character: &str, skills: &[&str]| {
        json!({"name": name, "purpose": "", "model": null,
            "character": character, "skills": skills, "notes": true})
    };
    let listing = |id: &str,
                   kind: &str,
                   title: &str,
                   summary: &str,
                   cents: u32,
                   owners: u32,
                   latest: serde_json::Value| {
        json!({"id": id, "kind": kind, "title": title, "summary": summary,
            "description": "Wick reads what you hand it and keeps short \
                notes of each.\n\nIt files them by topic, and answers \
                from them when asked.",
            "category": "Research", "tags": ["reading", "notes"],
            "priceCents": cents, "currency": "usd", "status": "listed",
            "seller": seller("ada", "Ada Okafor"),
            "coverUrl": format!("/v1/listings/{id}/cover?v=1"),
            "latest": latest, "owners": owners, "stars": 4.7,
            "reviews": 18, "createdAt": "2026-10-01T00:00:00Z",
            "updatedAt": "2026-10-20T00:00:00Z"})
    };
    let wick = listing(
        "l_wick",
        "agent",
        "Wick Lantern",
        "Keeps a light on for late readers: finds, summarises and \
         files what you read.",
        499,
        120,
        version(
            "1.2.0",
            "Files notes by topic.",
            agent(
                "wick",
                "Patient and exact. It reads twice before it writes.",
                &["summarise", "cite"],
            ),
        ),
    );
    let owl = listing(
        "l_owl",
        "agent",
        "Tutor Owl",
        "Quizzes you on your notes with spaced repetition.",
        300,
        44,
        version(
            "2.0.0",
            "Asks harder questions.",
            agent(
                "owl",
                "Kind, and never gives the answer away.",
                &["quiz"],
            ),
        ),
    );
    let scribe = listing(
        "l_scribe",
        "agent",
        "Letter Scribe",
        "Drafts letters in your voice from a few lines.",
        0,
        310,
        version(
            "1.1.0",
            "Learns your sign-off.",
            agent(
                "scribe",
                "Warm, brief, and careful with names.",
                &["letters"],
            ),
        ),
    );
    let tools = |name: &str, about: &str| {
        json!({"tools": [{"name": name,
        "description": about, "permissions": ["run_program", "network"]}]})
    };
    let cal = listing(
        "l_cal",
        "plugin",
        "Cal Clock",
        "Gives agents the date, the time and your calendar's next \
         days.",
        0,
        870,
        version(
            "1.0.3",
            "Reads time zones.",
            tools("cal", "Tells the date and the time anywhere."),
        ),
    );
    let ink = listing(
        "l_ink",
        "plugin",
        "Ink Press",
        "Turns an agent's Markdown into a printable PDF.",
        199,
        92,
        version(
            "0.4.0",
            "Page numbers.",
            tools("ink", "Prints Markdown to PDF."),
        ),
    );
    let settler = |name: &str, purpose: &str| {
        json!({"name": name,
        "purpose": purpose, "model": null, "character": "",
        "skills": [], "notes": false})
    };
    let press = listing(
        "l_press",
        "hamlet",
        "Small Press",
        "A publishing house: an editor, a copywriter and a designer.",
        1_200,
        15,
        version(
            "1.0.0",
            "First edition.",
            json!({"name": "press",
            "title": "Small Press", "kind": "Publishing house",
            "purpose": "Publish short books.", "seed": "42",
            "mayor": settler("editor", "Runs the list."),
            "agents": [settler("copy", "Writes the blurbs."),
                settler("designer", "Sets the pages.")]}),
        ),
    );
    let signed_in = trade != Trade::SignedOut && trade != Trade::Code;
    let cover =
        std::fs::read(super::super::WORLD).unwrap_or_default();
    Staged {
        listings: json!({"items": [wick, owl, scribe, cal, ink, press],
            "total": 6}),
        user: signed_in.then(|| json!({"id": "u_2", "login": "mira",
            "name": "Mira", "payouts": {"connected": true, "enabled": true}})),
        mine: if signed_in { vec!["l_owl".into()] } else { Vec::new() },
        owned: if signed_in {
            vec!["l_cal".into(), "l_scribe".into()]
        } else {
            Vec::new()
        },
        bought: if signed_in {
            vec![protocol::Bought {
                hamlet: Some("home".into()),
                kind: protocol::PackageKind::Agent,
                listing: "l_scribe".into(),
                names: vec!["scribe".into()],
                title: "Letter Scribe".into(),
                version: "1.0.0".into(),
            }]
        } else {
            Vec::new()
        },
        covers: ["l_wick", "l_owl", "l_scribe", "l_cal", "l_ink"]
            .into_iter()
            .map(|id| (format!("/v1/listings/{id}/cover?v=1"), cover.clone()))
            .collect(),
        code: (trade == Trade::Code).then(|| {
            ("WDJB-MJHT".into(), "https://github.com/login/device".into())
        }),
        deal: match trade {
            Trade::Paying => Some(("l_wick".into(), Dealt::Paying)),
            Trade::Installing => Some(("l_wick".into(), Dealt::Installing)),
            _ => None,
        },
        assets: assets(),
        owned_assets: if signed_in {
            vec![SAKURA.into(), "agent-emote-wave".into()]
        } else {
            Vec::new()
        },
        views: Vec::new(),
    }
}

/// The official look the LOOKS scenes open, which the mayor wears
/// once the chief is signed in.
const SAKURA: &str = "agent-skin-sakura";

/// The LOOKS stall's catalogue, as `GET /v1/assets` answers.
fn assets() -> serde_json::Value {
    let asset = |id: &str,
                 category: &str,
                 title: &str,
                 summary: &str,
                 cents: u32| {
        json!({"id": id, "target": id.split('-').next(),
            "category": category, "title": title, "summary": summary,
            "priceCents": cents, "currency": "usd", "version": "1.0.0",
            "owned": false})
    };
    json!({"items": [
        asset("agent-emote-wave", "emote", "Wave",
            "A friendly wave hello.", 0),
        asset(SAKURA, "skin", "Sakura",
            "Soft pinks and white trim for your agent.", 99),
        asset("hamlet-mesh-minka", "mesh", "Minka",
            "A thatched farmhouse for every house of the hamlet.", 499),
    ]})
}

/// The files of the looks, as the viewer is given them: written
/// here, the hamlet's mesh the world's own house.
fn views(land: &Land) -> Vec<(String, Vec<u8>)> {
    let skin = json!({"format": 1,
        "colours": {"paint": "#f4b6c2", "brass": "#e9d8a6"},
        "finish": {"metallic": 0.0, "roughness": 0.5, "glow": 0.0}});
    let wave = json!({"format": 1, "kind": "clip", "seconds": 2.4,
        "every": 45, "keys": [
            {"at": 0.0, "parts": {}},
            {"at": 0.6, "parts": {"arm_r": {"turn": [0, 0, 150]}}},
            {"at": 1.8, "parts": {"arm_r": {"turn": [0, 0, 150]}}},
            {"at": 2.4, "parts": {}}]});
    vec![
        (SAKURA.into(), skin.to_string().into_bytes()),
        ("agent-emote-wave".into(), wave.to_string().into_bytes()),
        ("hamlet-mesh-minka".into(), house::plain(land).glb()),
    ]
}
