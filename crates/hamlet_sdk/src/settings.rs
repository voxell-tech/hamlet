//! The pages plugins add to the Settings panel.

mod model;
mod page;

use bevy::app::{App, Update};
use bevy::ecs::event::Event;
use bevy::ecs::schedule::IntoScheduleConfigs;

pub use self::model::{Line, SettingsPages};
pub use self::page::SettingsPage;

/// Asks the Settings panel to turn to its page of the models.
#[derive(Event, Clone, Copy)]
pub struct ShowModels;

/// Asks the window for its first-run screen again, to choose where
/// the models come from.
#[derive(Event, Clone, Copy)]
pub struct SetUpAgain;

/// Asks the window for its first-run screen to add another place
/// for models.
#[derive(Event, Clone, Copy)]
pub struct AddEndpoint;

/// Keeps [`SettingsPages`] in step with the [`SettingsPage`] entities
/// there are.
pub fn plugin(app: &mut App) {
    app.init_resource::<SettingsPages>()
        .add_systems(Update, model::keep.run_if(model::moved));
}
