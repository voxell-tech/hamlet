//! The Agent Bazaar's stalls, each at its tab of the bazaar's panel,
//! with buttons pressed by their words to open a page. OpenRouter's
//! and skills.sh's answers are given by hand, so nothing is asked of
//! the network, and the realm draws a hamlet template's town apart
//! from the world.

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
use hamlet_app::bazaar::{Offered, StallShown, offer};
use hamlet_client::realm::{Providers, Skills};
use hamlet_protocol::client as protocol;
use hamlet_realm::WorldCamera;
use hamlet_realm::generator::land::Land;
use hamlet_realm::generator::{DetailSettings, GeneratorSettings};
use hamlet_realm::palette::Palette;
use serde_json::json;

use super::Scene;
use super::dock::seed_dock;

/// The updates between one press and the next.
const BETWEEN: u32 = 40;

/// The seed of the land the template's towns are laid out on.
const SEED: u32 = 7;

pub(super) fn plugin(app: &mut App) {
    app.add_plugins((
        hamlet_realm::scene::plugin,
        hamlet_realm::diorama::plugin,
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
    let Scene::Stall(stall, _) = *world.resource::<Scene>() else {
        return;
    };
    world.insert_resource(StallShown(stall));
    let land = Land::new(
        SEED,
        GeneratorSettings::default(),
        world.resource::<Palette>().clone(),
    );
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
    let Scene::Stall(_, words) = *scene else {
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
