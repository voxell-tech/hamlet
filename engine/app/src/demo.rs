//! A plugin built into the binary to show the path a plugin takes: it
//! adds a panel, a command in its own menu with a key, and a
//! workspace to take.

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy_fynix::views::{FrameProps, column};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{Layout, Manifest, PanelId, PluginId};
use hamlet_engine_core::LogLine;
use hamlet_engine_plugins::Entry;
use hamlet_engine_shell::style::{heading, note};
use hamlet_engine_shell::{AnyView, Cx, panel};

const ID: &str = "hamlet.demo";

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, add);
}

fn add(mut commands: Commands) {
    let manifest = Manifest::new(
        PluginId::try_from(ID).expect("a valid id"),
        "Demo",
        "0.1.0",
    );
    hamlet_engine_plugins::add_builtin(
        &mut commands,
        manifest,
        entry,
    );
}

fn entry(entry: &mut Entry) {
    let panels = [
        PanelId::new(ID, "hello").expect("a valid id"),
        PanelId::new(ENGINE, "console").expect("a valid id"),
    ];
    entry
        .panel("hello", "Hello", view)
        .command("greet", "Say hello", greet)
        .menu_entry("Demo", "greet")
        .shortcut("Primary+Shift+H", "greet")
        .workspace(
            "hello",
            "Hello",
            Layout::Tabs {
                panels: panels.to_vec(),
                shown: 0,
            },
        );
}

fn greet(world: &mut World, _: Entity) {
    world.write_message(LogLine::info(
        ID,
        "Hello from the demo plugin.",
    ));
}

fn view(_: Entity) -> AnyView {
    panel(AnyView::new(|cx: &mut Cx| {
        let gap = cx.theme().space.gap;
        cx.build(
            column((
                heading("Hello"),
                note(
                    "This panel is the demo plugin's. Switch the \
                     plugin off in Plugins and it goes, with its \
                     menu and its key.",
                ),
            ))
            .gap(gap),
        )
    }))
}
