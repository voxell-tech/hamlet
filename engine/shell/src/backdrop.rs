//! The window's background, which one plugin at a time may draw a
//! scene into behind the dock: while one does, the root has no fill,
//! lets the pointer through, and the interface is drawn over the
//! scene without clearing it.

use bevy::app::{App, Update};
use bevy::camera::{Camera, ClearColorConfig};
use bevy::color::Color;
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::common_conditions::{
    resource_changed, resource_exists_and_changed,
};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemCondition};
use bevy::ecs::system::{Commands, Query, Res};
use bevy::picking::Pickable;
use bevy::ui::{BackgroundColor, IsDefaultUiCamera};

use crate::theme::EngineTheme;
use crate::window::Root;

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<Backdrop>().add_systems(
        Update,
        show.run_if(
            resource_changed::<Backdrop>
                .or_else(resource_exists_and_changed::<Root>),
        ),
    );
}

/// The plugin that draws behind the dock, if one does.
#[derive(Resource, Default, Clone, Copy, PartialEq, Debug)]
pub struct Backdrop(pub Option<Entity>);

fn show(
    backdrop: Res<Backdrop>,
    root: Option<Res<Root>>,
    theme: Option<Res<EngineTheme>>,
    mut fills: Query<&mut BackgroundColor>,
    mut cameras: Query<&mut Camera, With<IsDefaultUiCamera>>,
    mut commands: Commands,
) {
    let behind = backdrop.0.is_some();
    if let Some(root) = root {
        if let Ok(mut fill) = fills.get_mut(root.0) {
            fill.0 = match (&theme, behind) {
                (Some(theme), false) => *theme.colors.background,
                _ => Color::NONE,
            };
        }
        if behind {
            commands.entity(root.0).insert(Pickable::IGNORE);
        } else {
            commands.entity(root.0).remove::<Pickable>();
        }
    }
    for mut camera in &mut cameras {
        camera.clear_color = if behind {
            ClearColorConfig::None
        } else {
            ClearColorConfig::Default
        };
    }
}
