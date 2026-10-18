//! The greeter data plugin's panel, drawn from its program with the
//! agents of the scene.

use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use hamlet_openui::Source;
use hamlet_sdk::agents::{Agent, AgentList};
use hamlet_sdk::capability::{Capability, Granted};
use hamlet_sdk::dock::Panel;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::plugin::{ContributedBy, PluginInfo};
use hamlet_sdk::tree::id::dock_id;
use hamlet_sdk::tree::view;

const PLUGIN: &str = "greeter_ui";

/// The id of the panel in the dock.
pub(super) fn id() -> String {
    dock_id(PLUGIN, hamlet_openui::data::PANEL)
}

/// Has the agents be the ones made up for the scene, whatever the
/// core's are.
pub(super) fn made_up(mut list: ResMut<AgentList>) {
    let agent = |hamlet: &str, name: &str| Agent {
        name: name.into(),
        hamlet: hamlet.into(),
    };
    let made = vec![
        agent("Pear Tree Hollow", "Juniper"),
        agent("Pear Tree Hollow", "press-hand"),
        agent("The Quay", "harbourmaster"),
        agent("The Quay", "tally-clerk"),
    ];
    if list.0 != made {
        list.0 = made;
    }
}

/// Adds the plugin that reads the agents and its panel.
pub(super) fn put(world: &mut World) {
    let plugin = world
        .spawn((
            PluginInfo {
                id: PLUGIN.into(),
                name: "Greeter".into(),
                version: "0.1.0".into(),
                builtin: false,
                essential: false,
            },
            Granted::allowing(&[Capability::ReadAgents]),
        ))
        .id();
    world.spawn((
        Panel::by_id(id(), "GREETER", Glyph::Talk, view::panel),
        ContributedBy(plugin),
        Source(
            include_str!("../../../guests/greeter_ui/panel.ui")
                .into(),
        ),
    ));
}
