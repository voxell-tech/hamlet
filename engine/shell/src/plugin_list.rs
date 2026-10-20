//! The list of plugins, empty while the engine runs none.

use bevy::app::{App, Startup};
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::label;
use hamlet_engine_api::PanelId;
use hamlet_engine_api::id::ENGINE;

use crate::AnyView;
use crate::dock::{Panel, panel};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, open);
}

pub(crate) fn id() -> PanelId {
    PanelId::new(ENGINE, "plugins").expect("a valid id")
}

fn open(mut commands: Commands) {
    commands.spawn(Panel {
        id: id(),
        title: "Plugins".into(),
        content: view,
    });
}

fn view(_: Entity) -> AnyView {
    panel(label("No plugins yet.").tone(Tone::Dim))
}
