//! The command center (I4): a Bevy and fynix window added to the
//! core's app.

mod app_icon;
pub mod awake;
pub mod chat;
mod chief;
pub mod chrome;
pub mod cursor;
mod dock;
pub mod feel;
mod folder;
mod hand;
pub mod history;
mod hover;
pub mod icons;
pub mod inspector;
pub mod links;
pub mod lists;
mod looks;
mod markdown;
mod me;
pub mod menu;
pub mod models;
pub mod needs;
pub mod overview;
pub mod pictures;
pub mod platform;
mod remember;
mod roll;
mod root;
pub mod rules;
pub mod settings;
mod sounds;
mod status;
pub mod tabs;
pub mod theme;
pub mod tuning;
pub mod welcome;
mod window;
mod world_menu;
mod world_view;

use std::time::Duration;

use bevy::DefaultPlugins;
use bevy::app::{App, Plugin, PluginGroup, PostStartup, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::prelude::ClearColor;
use bevy::window::{
    ExitCondition, Window, WindowPlugin, WindowResolution,
};
use bevy_fynix::dock::DockPlugin;
use bevy_fynix::{AnyView, Bevy, Cx, FynixPlugin, Theme};
use hamlet_core::agents::WatchEvery;
use hamlet_core::core::HomeDir;
use hamlet_world::hamlet::HamletPlugin;
use hamlet_world::pick::Selected;

pub use self::dock::{lit_panel, panel};
pub use self::looks::{
    ICON, brass, grim, lamp, marked, retrace, well,
};
pub use self::me::Me;
pub use self::roll::Roll;
pub use self::root::toolbar;
pub use self::status::{Bar, status_bar};
use self::theme::Lamplight;
pub use self::window::ShowRequests;

/// The context a view of the window is built in.
pub type Build<'a> = Cx<'a, Bevy, Lamplight>;

/// A view of the window, its type erased.
pub(crate) type Ui = AnyView<Bevy, Lamplight>;

/// The window: the dock, its panels and the world. Add it to the
/// core's app, after a [`ShowRequests`] resource is in place.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        // A window draws far more often than the records change.
        app.insert_resource(WatchEvery(Duration::from_millis(200)))
            .add_plugins(me::plugin);
        let home = &app.world().resource::<HomeDir>().0.root;
        let size = window::saved_size(home);
        app.add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Hamlet".into(),
                    resolution: WindowResolution::new(size.0, size.1),
                    ..chrome::bare()
                }),
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            }),
            FynixPlugin::<Lamplight>::default(),
            DockPlugin::<Lamplight>::default(),
            HamletPlugin,
        ))
        .insert_resource(Theme(Lamplight))
        .insert_resource(ClearColor(Lamplight::background()))
        .insert_resource(window::FullSize(size))
        .add_plugins((
            (
                roll::plugin,
                root::plugin,
                dock::plugin,
                window::plugin,
                chrome::plugin,
                menu::plugin,
                status::plugin,
                remember::plugin,
                app_icon::plugin,
                awake::plugin,
            ),
            (
                inspector::plugin,
                overview::plugin,
                chief::plugin,
                folder::plugin,
                rules::plugin,
                models::plugin,
                chat::plugin,
                links::plugin,
                pictures::plugin,
                needs::plugin,
                history::plugin,
                settings::plugin,
                welcome::plugin,
            ),
            (
                world_view::plugin,
                world_menu::plugin,
                hover::plugin,
                hand::plugin,
            ),
            (
                tuning::plugin,
                tabs::plugin,
                platform::plugin,
                icons::plugin,
                feel::plugin,
                cursor::plugin,
                sounds::plugin,
            ),
        ))
        // The world panel shows the world's camera, which the world
        // has spawned by now.
        .add_systems(
            PostStartup,
            (world_view::attach, dock::seed, root::mount).chain(),
        )
        .add_systems(
            Update,
            (
                me::reseat.run_if(me::roster_moved),
                inspector::follow_selection
                    .run_if(resource_changed::<Selected>),
                chief::accompany,
            )
                .chain(),
        );
    }
}
