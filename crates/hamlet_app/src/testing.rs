//! Helpers the tests of several modules share.

use std::path::PathBuf;

use bevy::app::{App, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use hamlet_client::ViewFolder;
use hamlet_core::fynix::dock::{DockRegistry, DockTree};
use hamlet_core::panels::DeskCapture;
use hamlet_core::pick::Selected;
use hamlet_core::roll::Roll;
use hamlet_core::settings::SettingsPane;
use hamlet_core::testing::settle;
use hamlet_desktop::awake::KeepAwake;
use hamlet_hub::world_menu::Favorites;
use hamlet_realm::North;
use hamlet_realm::sky::{
    DistanceFogSettings, Quality, VolumetricFogSettings,
};
use hamlet_realm::sound::SoundSettings;
use hamlet_ui::theme::HamletTheme;
use hamlet_vfx::wind::WindChoice;

use crate::appearance::Appearance;
use crate::desk::{self, Desks};
use crate::remember::Remembered;
use crate::world_view::{NorthTarget, TimeOfDay};

/// First-run setup with a browser that fails the test if opened.
pub(crate) fn welcome_app() -> App {
    let mut app = hamlet_core::testing::app();
    app.add_plugins((
        crate::welcome::plugin,
        hamlet_ui::widgets::secret::plugin,
    ));
    app
}

/// The settings screen open on `pane`, over the Hub's panels.
pub(crate) fn settings_page(pane: Option<&'static str>) -> App {
    let mut app = hamlet_hub::testing::panel_app();
    app.add_plugins((
        hamlet_settings::plugin,
        crate::pages::plugin,
        hamlet_ui::widgets::secret::plugin,
    ))
    .init_resource::<KeepAwake>()
    .insert_resource(SettingsPane(pane));
    bevy_fynix::mount::<HamletTheme>(
        app.world_mut(),
        hamlet_settings::screen(),
    );
    settle(&mut app);
    app
}

/// Writes the view into `folder` as the window does.
pub(crate) fn keeping(app: &mut App, folder: PathBuf) {
    app.insert_resource(ViewFolder(folder))
        .init_resource::<Selected>()
        .init_resource::<Roll>()
        .init_resource::<DockTree>()
        .init_resource::<TimeOfDay>()
        .init_resource::<SoundSettings>()
        .init_resource::<Quality>()
        .init_resource::<VolumetricFogSettings>()
        .init_resource::<DistanceFogSettings>()
        .init_resource::<WindChoice>()
        .init_resource::<North>()
        .init_resource::<NorthTarget>()
        .init_resource::<KeepAwake>()
        .init_resource::<DockRegistry<HamletTheme>>()
        .init_resource::<Desks>()
        .init_resource::<Favorites>()
        .init_resource::<Appearance>()
        .init_resource::<Remembered>()
        .init_resource::<desk::Shown>()
        .init_resource::<hamlet_terminal::Shown>()
        .init_resource::<hamlet_terminal::Folds>()
        .init_resource::<hamlet_terminal::Pins>()
        .init_resource::<hamlet_hub::hub::CollapsedRails>()
        .init_resource::<crate::updates::UpdateChannel>()
        .add_systems(Update, desk::capture.in_set(DeskCapture))
        .add_plugins(crate::remember::plugin);
}
