//! Plugins with the choices of a chief, for the Settings pages of
//! permissions.

use bevy::ecs::world::World;
use hamlet_sdk::capability::{
    Asked, Asks, Capability, Choice, Choices, Granted, Uninstalled,
};
use hamlet_sdk::command::{Command, kind};
use hamlet_sdk::context::ContextItem;
use hamlet_sdk::icons::Glyph;
use hamlet_sdk::inspector::InspectorSection;
use hamlet_sdk::keys::{Key, chord};
use hamlet_sdk::menu::{Menu, MenuEntry};
use hamlet_sdk::plugin::{
    ContributedBy, Imports, Manifest, PluginInfo, Status,
};
use hamlet_sdk::settings::SettingsPage;
use plugin_settings::{Open, Tab};

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
        essential: false,
    }
}

/// An installed plugin that the chief allowed `ui.menu`, with one
/// line in the VIEW menu.
pub(super) fn installed_line(world: &mut World) {
    let almanac = world
        .spawn((
            info("almanac", "Almanac", "0.1.0"),
            Granted::allowing(&[Capability::UiMenu]),
            Status::Running,
        ))
        .id();
    world.spawn((
        Command::new("look", "Look at the dates"),
        ContributedBy(almanac),
    ));
    world.spawn((
        MenuEntry {
            menu: Menu::View,
            section: 1,
            label: "Almanac".into(),
            command: "look".into(),
            icon: Some(Glyph::Clock),
        },
        ContributedBy(almanac),
    ));
}

/// A shortcut Notes and Almanac both want, so that neither runs.
fn chorded(command: &str) -> Key {
    Key {
        chord: chord::parse("Cmd+Shift+N").expect("a chord"),
        command: command.into(),
    }
}

/// Three plugins, one of them not installed, and the page of the
/// scene open: the permissions of Roster, the capabilities, or the
/// list with what each plugin adds.
pub(super) fn put(world: &mut World, scene: Scene) {
    use Capability::{
        ChatSend, Net, PluginUse, ReadAgents, ReadPapers, UiContext,
        UiInspector, UiKeys, UiMenu, UiSettings,
    };

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
    let almanac = world
        .spawn((
            info("almanac", "Almanac", "0.1.0"),
            asks(&[
                (
                    ReadPapers,
                    "To show an agent's notes beside the dates.",
                ),
                (Net, "To fetch the harvest calendar."),
                (UiKeys, "To open the dates with Cmd+Shift+N."),
            ]),
            Choices::default()
                .decided(ReadPapers, Choice::Allow, false)
                .decided(UiKeys, Choice::Allow, false),
            Status::Running,
        ))
        .id();
    world.spawn((
        Command::new("look", "Look at the dates"),
        ContributedBy(almanac),
    ));
    world.spawn((chorded("look"), ContributedBy(almanac)));
    world.spawn((
        info("tides", "Tides", "0.1.0"),
        asks(&[(Net, "To fetch the tide table from the harbour.")]),
        Choices::default(),
        Status::Disabled,
        Uninstalled,
    ));
    let clipper = Manifest {
        imports: Imports {
            commands: vec!["notes/add".into(), "almanac/look".into()],
            events: vec!["notes/note-saved".into()],
            ..Imports::default()
        },
        ..Manifest::built_in("clipper").needs("notes", ">=0.1, <1")
    };
    let notes = world
        .spawn((
            info("notes", "Notes", "0.1.0"),
            Manifest::built_in("notes"),
            asks(&[
                (ReadPapers, "To copy the selected agent's notes."),
                (UiMenu, "To put New note in the HAMLET menu."),
                (UiKeys, "To start a new note with Cmd+Shift+N."),
                (
                    UiSettings,
                    "To put its page, NOTES, in the Settings.",
                ),
                (
                    UiInspector,
                    "To put a NOTES tab in the Inspector's page of \
                     an agent.",
                ),
            ]),
            Choices::default()
                .decided(ReadPapers, Choice::Allow, false)
                .decided(UiMenu, Choice::Allow, false)
                .decided(UiKeys, Choice::Allow, false)
                .decided(UiSettings, Choice::Allow, false),
            Status::Running,
        ))
        .id();
    world.spawn((
        Command::new("new", "New note"),
        ContributedBy(notes),
    ));
    world.spawn((
        MenuEntry {
            menu: Menu::App,
            section: 1,
            label: "New note".into(),
            command: "new".into(),
            icon: None,
        },
        ContributedBy(notes),
    ));
    world.spawn((chorded("new"), ContributedBy(notes)));
    world.spawn((
        SettingsPage {
            title: "NOTES".into(),
            icon: Glyph::Notes,
            holds: String::new(),
            panel: "notes/settings".into(),
        },
        ContributedBy(notes),
    ));
    world.spawn((
        InspectorSection {
            target: kind::AGENT.into(),
            title: "NOTES".into(),
            panel: "notes/mentions".into(),
        },
        ContributedBy(notes),
    ));
    let clipper = world
        .spawn((
            info("clipper", "Clipper", "0.1.0"),
            clipper,
            asks(&[
                (ReadAgents, "To send the selected agent's name."),
                (
                    PluginUse,
                    "To send what it clips to Notes and Almanac.",
                ),
                (UiContext, "To put Clip name to Notes in a menu."),
            ]),
            Choices::default()
                .decided(ReadAgents, Choice::Allow, false)
                .decided(PluginUse, Choice::AskNextTime, false)
                .decided_for(PluginUse, "notes", Choice::Allow, false)
                .decided(UiContext, Choice::Deny, false),
            Status::Running,
        ))
        .id();
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
    match scene {
        Scene::PluginPermissions => {
            world.insert_resource(Open(Some("clipper".into())));
        }
        Scene::PluginCapabilities => {
            world.insert_resource(Tab::ByCapability);
        }
        _ => {}
    }
}
