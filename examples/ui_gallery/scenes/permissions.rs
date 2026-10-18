//! Plugins with the choices of a chief, for the Settings pages of
//! permissions.

use bevy::ecs::world::World;
use hamlet_sdk::capability::{
    Asked, Asks, Capability, Choice, Choices, Uninstalled,
};
use hamlet_sdk::plugin::{PluginInfo, Status};
use hamlet_ui::settings::{Open, Tab};

use super::Scene;

fn asks(of: &[(Capability, &str)]) -> Asks {
    Asks(
        of.iter()
            .map(|(capability, reason)| {
                Asked::parse(capability.name(), reason, &[])
                    .expect("an ask that can be granted")
            })
            .collect(),
    )
}

fn info(id: &str, name: &str, version: &str) -> PluginInfo {
    PluginInfo {
        id: id.into(),
        name: name.into(),
        version: version.into(),
        builtin: false,
    }
}

/// Three plugins, one of them not installed, and the page of the
/// scene open: the permissions of Roster, or the capabilities.
pub(super) fn put(world: &mut World, scene: Scene) {
    use Capability::{ChatSend, Net, ReadAgents, ReadPapers};

    let roster = Choices::default()
        .decided(ReadAgents, Choice::Allow, false)
        .decided(Net, Choice::AskNextTime, false)
        .decided(ChatSend, Choice::Deny, false);
    world.spawn((
        info("roster", "Roster", "0.1.0"),
        asks(&[
            (
                ReadAgents,
                "To list the agents and their hamlets in its panel.",
            ),
            (Net, "To look up the weather over the orchard."),
            (ChatSend, "To post the day's frost warning."),
        ]),
        roster,
        Status::Running,
    ));
    world.spawn((
        info("almanac", "Almanac", "0.3.0"),
        asks(&[
            (
                ReadPapers,
                "To show an agent's notes beside the dates.",
            ),
            (Net, "To fetch the harvest calendar."),
        ]),
        Choices::default().decided(ReadPapers, Choice::Allow, false),
        Status::Running,
    ));
    world.spawn((
        info("tides", "Tides", "1.2.0"),
        asks(&[(Net, "To fetch the tide table from the harbour.")]),
        Choices::default(),
        Status::Disabled,
        Uninstalled,
    ));
    if scene == Scene::PluginPermissions {
        world.insert_resource(Open(Some("roster".into())));
    } else {
        world.insert_resource(Tab::ByCapability);
    }
}
