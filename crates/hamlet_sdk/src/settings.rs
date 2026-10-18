//! The pages plugins add to the Settings panel.

mod model;
mod page;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use self::model::{Line, SettingsPages};
pub use self::page::SettingsPage;

/// Keeps [`SettingsPages`] in step with the [`SettingsPage`] entities
/// there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<SettingsPages>()
        .add_systems(Update, model::keep.run_if(model::moved));
}
