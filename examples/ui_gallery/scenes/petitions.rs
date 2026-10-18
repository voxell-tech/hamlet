//! Plugins that wait for the chief, raised the way the app raises
//! them: one to install, one that asks for more, one that tried to
//! use what the chief is yet to decide and one to remove.

use bevy::ecs::world::World;
use hamlet_sdk::capability::{
    AskForMore, AskToInstall, AskToRemove, Asked, Asks, Capability,
    Choice, Choices, Uninstalled, Wanted,
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
    // An installed plugin that tried to use what the chief is yet to
    // decide.
    let tides = world
        .spawn((
            info("tides", "Tides", "1.2.0"),
            asks(&[(
                "net",
                "To fetch the tide table from the harbour.",
            )]),
            Choices::default().decided(
                Capability::Net,
                Choice::AskNextTime,
                false,
            ),
        ))
        .id();
    world.flush();
    world.trigger(Wanted {
        plugin: tides,
        capability: Capability::Net,
        target: None,
        reason: "The harbour's table is on another machine.".into(),
    });
    // An installed plugin that tried to use another plugin.
    let clipper = world
        .spawn((
            info("clipper", "Clipper", "0.1.0"),
            asks(&[(
                "plugin.use",
                "To send what it clips to Notes.",
            )]),
            Choices::allowing(&[Capability::PluginUse]),
        ))
        .id();
    world.flush();
    world.trigger(Wanted {
        plugin: clipper,
        capability: Capability::PluginUse,
        target: Some("notes".into()),
        reason: String::new(),
    });
    // An installed plugin the chief is removing.
    world.spawn((
        info("notes", "Notes", "0.1.0"),
        asks(&[(
            "read.papers",
            "To copy the selected agent's notes into a note.",
        )]),
        Choices::allowing(&[Capability::ReadPapers]),
    ));
    world.flush();
    world.trigger(AskToRemove {
        plugin: "notes".into(),
    });
    world.flush();
}
