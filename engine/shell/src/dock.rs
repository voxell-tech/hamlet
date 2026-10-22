//! The dock and its panels. A panel is an entity, and the dock can
//! show it while the entity is there.

use bevy::app::App;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy_fynix::dock::{
    DockPlugin, DockRegistry, DockTree, DockWindowKind,
};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{CommandId, PanelId};

use crate::command::{RunCommand, ShellCommandsExt};
use crate::icons::Glyph;
use crate::workspace::DockTreeExt;
use crate::{AnyView, EngineTheme};

/// The name of the engine's plugin list, which the plugins module
/// adds.
pub const PLUGIN_LIST: &str = "plugins";

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(DockPlugin::<EngineTheme>::default())
        .add_observer(offer)
        .add_observer(withdraw);
}

/// A panel the dock can show.
#[derive(Component, Clone, Debug)]
#[component(immutable)]
pub struct Panel {
    pub id: PanelId,
    pub title: String,
    /// Builds its content, given the panel's entity.
    pub content: fn(Entity) -> AnyView,
}

/// The icon on a panel's tab, beside its [`Panel`].
#[derive(Component, Clone, Copy, Debug)]
#[component(immutable)]
pub struct PanelIcon(pub Glyph);

/// Lets the dock show a new panel, and adds a command that opens it
/// to the View menu.
fn offer(
    added: On<Add, Panel>,
    panels: Query<(&Panel, Option<&PanelIcon>)>,
    theme: Res<EngineTheme>,
    mut registry: ResMut<DockRegistry<EngineTheme>>,
    mut commands: Commands,
) {
    let entity = added.entity;
    let Ok((panel, icon)) = panels.get(entity) else {
        return;
    };
    let content = panel.content;
    let mut kind =
        DockWindowKind::new(panel.title.to_uppercase(), move || {
            content(entity)
        });
    if let Some(PanelIcon(which)) = icon {
        kind = kind.icon(which.image(theme.space.icon));
    }
    registry.register(panel.id.as_str(), kind);
    let id = panel.id.clone();
    // Past the longest name, a panel is opened from a tab bar's "+".
    let Ok(command) = CommandId::new(
        ENGINE,
        &format!("show.{}", id.as_str().replace('/', ".")),
    ) else {
        return;
    };
    commands
        .spawn_command(command, &panel.title, Some("View"), &[])
        .insert(ChildOf(entity))
        .observe(
            move |_: On<RunCommand>, mut tree: ResMut<DockTree>| {
                tree.open_panel(id.as_str());
            },
        );
}

/// Takes a panel that is going out of the dock.
fn withdraw(
    removed: On<Remove, Panel>,
    panels: Query<&Panel>,
    mut registry: ResMut<DockRegistry<EngineTheme>>,
    mut tree: ResMut<DockTree>,
) {
    let Ok(panel) = panels.get(removed.entity) else {
        return;
    };
    registry.unregister(panel.id.as_str());
    tree.remove_window_kind(panel.id.as_str());
}
