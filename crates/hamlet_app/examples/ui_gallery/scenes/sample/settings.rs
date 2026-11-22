//! The settings, charter, workspace, MCP servers, sight and
//! appearance gallery scenes.

use bevy::ecs::world::World;
use hamlet_app::pages;
use hamlet_assets::Tuning;
use hamlet_client::realm::Settings;
use hamlet_core::settings::{ENDPOINTS, MODELS, SettingsPane};
use hamlet_desktop::color_scheme::ColorScheme;
use hamlet_protocol::client as protocol;
use hamlet_realm::sky::{
    DistanceFogSettings, SkySettings, VolumetricFogSettings,
};
use hamlet_realm::{North, Sky};
use hamlet_ui::theme::{
    self, HamletTheme, Measures, ThemeId, Themes,
};
use hamlet_vfx::wind::WindChoice;

use crate::scenes::Scene;

pub(super) fn setup(world: &mut World, scene: Scene) {
    if !matches!(
        scene,
        Scene::Settings
            | Scene::SettingsModels
            | Scene::SettingsEndpoints
            | Scene::Charter
            | Scene::Workspace
            | Scene::Mcp
            | Scene::Sight
            | Scene::Appearance(_)
    ) {
        return;
    }
    world.resource_mut::<Settings>().0.endpoints.push(
        protocol::SettingsViewEndpoints {
            name: "far".into(),
            base_url: "https://models.example/api/v1".into(),
            key: true,
        },
    );
    world.init_resource::<hamlet_desktop::awake::KeepAwake>();
    world.init_resource::<hamlet_realm::sky::Quality>();
    if scene == Scene::Charter {
        world.insert_resource(hamlet_settings::Charter {
            text: include_str!(
                "../../../../../hamlet_server/assets/charter.md"
            )
            .into(),
            own: true,
        });
        world.insert_resource(SettingsPane(Some(
            hamlet_settings::CHARTER,
        )));
    }
    if scene == Scene::SettingsModels {
        world.resource_mut::<hamlet_core::models::Shelf>().models =
            super::hub::catalogue();
        world.insert_resource(SettingsPane(Some(MODELS)));
    }
    if scene == Scene::SettingsEndpoints {
        world.insert_resource(SettingsPane(Some(ENDPOINTS)));
    }
    if scene == Scene::Workspace {
        world.resource_mut::<Settings>().0.workspace =
            protocol::SettingsViewWorkspace {
                url: Some("https://workspace.example".into()),
                running: None,
                moving: None,
            };
        world.insert_resource(SettingsPane(Some(
            hamlet_settings::workspace::WORKSPACE,
        )));
    }
    if scene == Scene::Mcp {
        world.resource_mut::<Settings>().0.mcp = mcp();
        world.insert_resource(SettingsPane(Some(
            hamlet_settings::MCP,
        )));
    }
    if scene == Scene::Sight {
        world.init_resource::<SkySettings>();
        world.init_resource::<VolumetricFogSettings>();
        world.init_resource::<DistanceFogSettings>();
        world.init_resource::<WindChoice>();
        // Settled: the sky at the clock's hour and north where set.
        let now = jiff::Zoned::now();
        world.insert_resource(Sky {
            hour: f32::from(now.hour())
                + f32::from(now.minute()) / 60.0,
            ..Sky::default()
        });
        world.insert_resource(North::default());
        world
            .insert_resource(SettingsPane(Some(pages::sight::SIGHT)));
    }
    if let Scene::Appearance(shade) = scene {
        // The computer's, which the panels follow by default.
        world.insert_resource(ColorScheme::Says(shade));
        // The family `GALLERY_THEME` names, else Hamlet.
        let themes = world.resource::<Themes>();
        let palette = crate::scenes::chosen_family(themes)
            .unwrap_or_else(|| themes.get(&ThemeId::default()))
            .variants
            .get(shade)
            .palette
            .clone();
        theme::show(
            world,
            HamletTheme::new(palette, &Measures::designed()),
        );
        world.insert_resource(SettingsPane(Some(
            pages::appearance::APPEARANCE,
        )));
    }
}

/// A server up with its tools, one to sign in to, and the servers
/// other programs list.
fn mcp() -> protocol::McpView {
    let tool =
        |name: &str, about: &str, acts: bool| protocol::McpToolView {
            name: format!("mcp__github__{name}"),
            tool: name.into(),
            description: about.into(),
            acts,
        };
    protocol::McpView {
        servers: vec![
            protocol::McpServerView {
                name: "github".into(),
                entry: protocol::McpEntry {
                    command: vec![
                        "/usr/local/bin/npx".into(),
                        "-y".into(),
                        "@modelcontextprotocol/server-github".into(),
                    ],
                    env: [(
                        "GITHUB_PERSONAL_ACCESS_TOKEN".into(),
                        None,
                    )]
                    .into(),
                    net: true,
                    ..protocol::McpEntry::default()
                },
                state: protocol::McpState::Up,
                tools: vec![
                    tool("search_issues", "Search issues.", false),
                    tool(
                        "get_pull_request",
                        "Read a pull request.",
                        false,
                    ),
                    tool("create_issue", "Open an issue.", true),
                    tool(
                        "merge_pull_request",
                        "Merge a pull request.",
                        true,
                    ),
                ],
                sign_in: None,
            },
            protocol::McpServerView {
                name: "linear".into(),
                entry: protocol::McpEntry {
                    url: Some(
                        "https://mcp.linear.example/mcp".into(),
                    ),
                    net: true,
                    ..protocol::McpEntry::default()
                },
                state: protocol::McpState::Down {
                    reason: "it needs you to sign in".into(),
                    again: 0,
                },
                tools: Vec::new(),
                sign_in: None,
            },
        ],
        faults: Vec::new(),
        found: vec![
            protocol::McpFound {
                source: "Claude Desktop".into(),
                name: "filesystem".into(),
                entry: Some(protocol::McpEntry {
                    command: vec![
                        "/usr/local/bin/npx".into(),
                        "-y".into(),
                        "@modelcontextprotocol/server-filesystem"
                            .into(),
                    ],
                    net: true,
                    ..protocol::McpEntry::default()
                }),
                unrunnable: None,
            },
            protocol::McpFound {
                source: "Cursor".into(),
                name: "figma".into(),
                entry: None,
                unrunnable: Some(
                    "it speaks the older SSE transport, and Hamlet \
                     only streamable HTTP"
                        .into(),
                ),
            },
        ],
    }
}
