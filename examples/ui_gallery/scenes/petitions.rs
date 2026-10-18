//! Plugins that wait for the chief, raised the way the app raises
//! them.

use bevy::ecs::world::World;
use hamlet_sdk::capability::{
    AskForMore, AskToInstall, Asked, Asks, Capability, Choices,
    Uninstalled,
};
use hamlet_sdk::plugin::PluginInfo;

fn info(id: &str, name: &str, version: &str) -> PluginInfo {
    PluginInfo {
        id: id.into(),
        name: name.into(),
        version: version.into(),
        builtin: false,
    }
}

fn asks(of: &[(&str, &str)]) -> Asks {
    Asks(
        of.iter()
            .map(|(capability, reason)| {
                Asked::parse(capability, reason, &[])
                    .expect("an ask that can be granted")
            })
            .collect(),
    )
}

/// A plugin to install, and an installed one that asks for more in
/// a new version.
pub(super) fn put(world: &mut World) {
    world.spawn((
        info("roster", "Roster", "0.1.0"),
        asks(&[
            (
                "read.agents",
                "To list the agents and their hamlets in its panel.",
            ),
            ("net", "To look up the weather over the orchard."),
        ]),
        Choices::default(),
        Uninstalled,
    ));
    world.trigger(AskToInstall {
        plugin: "roster".into(),
    });
    world.spawn((
        info("almanac", "Almanac", "0.3.0"),
        asks(&[
            (
                "read.papers",
                "To show an agent's notes beside the dates.",
            ),
            ("chat.send", "To post the day's frost warning."),
        ]),
        Choices::allowing(&[Capability::ReadPapers]),
    ));
    world.trigger(AskForMore {
        plugin: "almanac".into(),
        capabilities: vec![Capability::ChatSend],
    });
    world.flush();
}
