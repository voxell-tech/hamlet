//! The Settings panel turned to a page a plugin added.

use bevy::ecs::system::{Res, ResMut};
use hamlet_sdk::settings::SettingsPages;
use hamlet_ui::settings::Pane;

/// Turns the Settings panel to the page titled `title`, once it is
/// there.
pub(super) fn turn_to(
    title: &str,
    pages: &SettingsPages,
    pane: &mut Pane,
) {
    if let Some(line) =
        pages.0.iter().find(|line| line.title == title)
    {
        *pane = Pane::Contributed(line.item);
    }
}

/// The Plugins page.
pub(super) fn plugins(
    pages: Res<SettingsPages>,
    mut pane: ResMut<Pane>,
) {
    turn_to("PLUGINS", &pages, &mut pane);
}
