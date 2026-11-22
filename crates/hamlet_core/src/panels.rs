//! The steps the panels take each frame before fynix mounts them.

use bevy::app::{App, PreUpdate};
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::input::InputSystems;
use bevy::picking::PickingSystems;
use hamlet_client::connection::ClientSystems;

/// The panels' steps before fynix mounts the views.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PanelSystems {
    /// The panels' state follows what was received or clicked.
    Follow,
    /// The values the views read, worked out from Follow's.
    Bind,
}

pub fn sets(app: &mut App) {
    app.configure_sets(
        // fynix mounts in `Update`, in sets it keeps private.
        PreUpdate,
        (PanelSystems::Follow, PanelSystems::Bind)
            .chain()
            .after(ClientSystems::Receive)
            .after(InputSystems)
            .after(PickingSystems::Last),
    );
}
