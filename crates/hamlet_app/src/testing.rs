//! Helpers the tests of several modules share.

use std::io::Cursor;
use std::path::PathBuf;

use bevy::app::{App, Update};
use bevy::ecs::entity::Entity;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::math::UVec2;
use bevy_fynix::dock::{DockRegistry, DockTree};
use hamlet_client::ViewFolder;
use hamlet_core::panels::DeskCapture;
use hamlet_core::pick::Selected;
use hamlet_core::roll::Roll;
use hamlet_core::settings::SettingsPane;
use hamlet_core::testing::settle;
use hamlet_desktop::awake::KeepAwake;
use hamlet_realm::North;
use hamlet_realm::sky::{
    DistanceFogSettings, Quality, VolumetricFogSettings,
};
use hamlet_realm::sound::SoundSettings;
use hamlet_ui::theme::HamletTheme;
use hamlet_vfx::wind::WindChoice;
use image::{ImageFormat, RgbaImage};

use crate::appearance::Appearance;
use crate::desk::{self, Desks};
use crate::remember::Remembered;
use crate::world_menu::Favorites;
use crate::world_view::{NorthTarget, TimeOfDay};

/// A window's panels on the fixture state, with no first-run setup.
pub(crate) fn panel_app() -> App {
    let mut app = hamlet_core::testing::panel_app();
    app.add_plugins(crate::asking::plugin);
    app.init_resource::<DockTree>()
        .init_resource::<crate::select::Selecting>();
    hub(&mut app);
    app
}

/// The first Hub in a dock of its own, following the world's
/// selection.
pub(crate) fn hub(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.get_resource_or_init::<DockTree>().set_root_leaf(
        bevy_fynix::dock::DockLeaf::new(
            "side",
            bevy_fynix::dock::DockAreaStyle::TabBar,
        )
        .with_windows(vec![crate::dock::HUB.into()]),
    );
    crate::hub::first(world)
}

/// The chat of the first Hub.
pub(crate) fn convo(app: &App) -> &crate::convo::Convo {
    let hub = crate::hub::following(app.world()).unwrap();
    app.world().get::<crate::convo::Convo>(hub).unwrap()
}

pub(crate) fn seat_hand(app: &mut App) -> Entity {
    let hand =
        hamlet_client::realm::named(app.world_mut(), "hand").unwrap();
    let hub = hub(app);
    crate::convo::seat(app.world_mut(), hub, hand);
    hand
}

pub(crate) fn seat_ada(app: &mut App) -> Entity {
    let ada = hamlet_client::realm::named(app.world_mut(), "agent#2")
        .unwrap();
    let hub = hub(app);
    crate::convo::seat(app.world_mut(), hub, ada);
    ada
}

/// First-run setup with a browser that fails the test if opened.
pub(crate) fn welcome_app() -> App {
    let mut app = hamlet_core::testing::app();
    app.add_plugins((
        crate::welcome::plugin,
        hamlet_ui::widgets::secret::plugin,
    ));
    app
}

/// The settings screen open on `pane`, over [`panel_app`].
pub(crate) fn settings_page(pane: Option<&'static str>) -> App {
    let mut app = panel_app();
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
        .init_resource::<crate::screens::terminal::Shown>()
        .init_resource::<crate::screens::terminal::Folds>()
        .init_resource::<crate::screens::terminal::Pins>()
        .init_resource::<crate::hub::CollapsedRails>()
        .init_resource::<crate::updates::UpdateChannel>()
        .add_systems(Update, desk::capture.in_set(DeskCapture))
        .add_plugins(crate::remember::plugin);
}

/// A PNG of `size`, one colour all over.
pub(crate) fn png(size: UVec2) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    RgbaImage::from_pixel(
        size.x,
        size.y,
        image::Rgba([200, 150, 60, 255]),
    )
    .write_to(&mut bytes, ImageFormat::Png)
    .unwrap();
    bytes.into_inner()
}
