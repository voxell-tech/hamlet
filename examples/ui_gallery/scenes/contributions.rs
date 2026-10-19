//! What installed plugins add to the menu of the world, the
//! Inspector and the Settings, drawn in the panels' own places.

use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Vec2;
use hamlet_realm::pick::Thing;
use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::command::{Command, kind};
use hamlet_sdk::context::ContextItem;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::inspector::InspectorSection;
use hamlet_sdk::plugin::{ContributedBy, PluginInfo, Status};
use hamlet_sdk::settings::SettingsPage;
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::node::{Kind, Node};
use hamlet_sdk::tree::store::{Drawn, PluginTrees};
use hamlet_sdk::tree::view;
use plugin_menus::world_menu;

/// An installed plugin that the chief allowed `capability`.
fn installed(
    world: &mut World,
    id: &str,
    name: &str,
    capability: Capability,
) -> Entity {
    world
        .spawn((
            PluginInfo {
                id: id.into(),
                name: name.into(),
                version: "0.1.0".into(),
                builtin: false,
                essential: false,
            },
            Granted::allowing(&[capability]),
            Status::Running,
        ))
        .id()
}

/// The panel `local` of the plugin `plugin`, showing `tree`.
fn drawn(
    world: &mut World,
    plugin: Entity,
    id: &str,
    local: &str,
    title: &str,
    icon: Glyph,
    tree: Node,
) -> String {
    let dock = dock_id(id, local);
    world.spawn((
        Panel::by_id(dock.clone(), title, icon, view::panel),
        ContributedBy(plugin),
    ));
    let mut trees = PluginTrees::default();
    trees.set(&dock, Drawn::Tree(tree));
    world.insert_resource(trees);
    dock
}

/// Clipper's item in the menu of an agent, with the menu open on the
/// agent of the scene.
pub(super) fn context(world: &mut World, agent: Entity) {
    let clipper =
        installed(world, "clipper", "Clipper", Capability::UiContext);
    world.spawn((
        Command::new("clip", "Clip name to Notes"),
        ContributedBy(clipper),
    ));
    world.spawn((
        ContextItem {
            target: kind::AGENT.into(),
            section: 200,
            label: "Clip name to Notes".into(),
            command: "clip".into(),
            icon: None,
        },
        ContributedBy(clipper),
    ));
    let at = Vec2::new(760.0, 180.0);
    world_menu::show(world, Thing::Resident(agent), at);
}

/// Notes' tab in the Inspector's page of an agent: the notes that
/// mention the agent.
pub(super) fn tab(world: &mut World) {
    let notes =
        installed(world, "notes", "Notes", Capability::UiInspector);
    let tree = Node::new(Kind::Column).of(vec![
        Node::new(Kind::Heading("NOTES THAT MENTION JUNIPER".into())),
        Node::new(Kind::Label("Juniper grafts the pears".into())),
        Node::new(Kind::Label("Orchard rows, by the press".into())),
    ]);
    let panel = drawn(
        world,
        notes,
        "notes",
        "mentions",
        "NOTES OF AN AGENT",
        Glyph::Book,
        tree,
    );
    world.spawn((
        InspectorSection {
            target: kind::AGENT.into(),
            title: "NOTES".into(),
            panel,
        },
        ContributedBy(notes),
    ));
}

/// Notes' page in the Settings: how a note opens.
pub(super) fn page(world: &mut World) {
    let notes =
        installed(world, "notes", "Notes", Capability::UiSettings);
    let tree = Node::new(Kind::Column).of(vec![
        Node::new(Kind::Note("How a note opens.".into())),
        Node::new(Kind::Checkbox {
            label: "Open notes in preview".into(),
            checked: true,
        })
        .id("open-in-preview"),
    ]);
    let panel = drawn(
        world,
        notes,
        "notes",
        "settings",
        "NOTES SETTINGS",
        Glyph::Sliders,
        tree,
    );
    world.spawn((
        SettingsPage {
            title: "NOTES".into(),
            icon: Glyph::Notes,
            holds: String::new(),
            panel,
        },
        ContributedBy(notes),
    ));
}
