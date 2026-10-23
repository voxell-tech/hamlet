//! The pointer's place over the world.

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

use super::WorldSurface;
use crate::WorldCamera;

/// The pointer's place over the world.
#[derive(Resource, Default, Clone, Copy)]
pub struct Reach {
    /// In the window, in pixels.
    pub at: Option<Vec2>,
    /// From the eye through the pointer.
    pub ray: Option<Ray3d>,
}

/// Works out where the pointer is over the world. A world with no
/// window, or one shown in no panel, keeps what it was told.
pub(super) fn aim(
    window: Query<&Window, With<PrimaryWindow>>,
    surfaces: Query<
        (&ComputedNode, &UiGlobalTransform),
        With<WorldSurface>,
    >,
    camera: Query<(&Camera, &GlobalTransform), With<WorldCamera>>,
    mut reach: ResMut<Reach>,
) {
    let Some(window) = window.iter().next() else {
        return;
    };
    if surfaces.is_empty() {
        return;
    }
    let at = window.cursor_position();
    let ray = at.and_then(|at| {
        let (node, place) = surfaces.iter().next()?;
        let (camera, view) = camera.iter().next()?;
        // The camera's picture fills the node, whatever its scale.
        let scale = node.inverse_scale_factor();
        let size = node.size() * scale;
        let corner = place.translation * scale - size * 0.5;
        let seen = camera.logical_viewport_size()?;
        let local = (at - corner) / size.max(Vec2::ONE) * seen;
        camera.viewport_to_world(view, local).ok()
    });
    *reach = Reach { at, ray };
}
