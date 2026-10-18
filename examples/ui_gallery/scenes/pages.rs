//! The Settings panel turned to a page a plugin added.

use bevy::ecs::system::{Res, ResMut};
use hamlet_sdk::settings::SettingsPages;
use plugin_settings::Pane;

use super::Scene;

/// Turns the Settings panel to the page of the scene, once it is
/// there.
pub(super) fn turn(
    scene: Res<Scene>,
    pages: Res<SettingsPages>,
    mut pane: ResMut<Pane>,
) {
    let title = match *scene {
        Scene::PluginSettingsPage => "NOTES",
        _ => "PLUGINS",
    };
    if let Some(line) =
        pages.0.iter().find(|line| line.title == title)
    {
        *pane = Pane::Contributed(line.item);
    }
}
