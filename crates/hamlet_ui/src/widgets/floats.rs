//! Every popup, menu and tooltip casts the theme's shadow of what
//! floats, so it reads as lifted off the panel under it.

use bevy::app::{App, PostUpdate};
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::system::{Commands, Query};
use bevy::ui::BoxShadow;
use bevy::ui_widgets::popover::Popover;

use crate::theme::Themed;

pub fn plugin(app: &mut App) {
    app.add_systems(PostUpdate, cast);
}

/// Gives each surface placed as a popover the theme's shadow, anew
/// when the theme changes.
fn cast(
    theme: Themed,
    floats: Query<(Entity, Option<&BoxShadow>), With<Popover>>,
    mut commands: Commands,
) {
    let shadow = theme.shadow();
    for (float, cast) in &floats {
        if cast != Some(&shadow) {
            commands.entity(float).try_insert(shadow.clone());
        }
    }
}
