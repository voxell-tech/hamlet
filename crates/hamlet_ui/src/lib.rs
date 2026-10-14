//! The command center (I4): a Bevy and fynix window added to the
//! core's app.

pub mod chat;
mod chief;
pub mod chrome;
pub mod cursor;
mod dock;
pub mod feel;
mod folder;
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

use bevy::app::{App, Plugin, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::state::state::OnEnter;
use bevy_fynix::dock::DockPlugin;
use bevy_fynix::{Bevy, FynixPlugin};
use hamlet_core::core::HomeDir;
use hamlet_world::pick::Selected;

pub use self::dock::{lit_panel, panel};
pub use self::looks::{
    ICON, brass, grim, lamp, marked, retrace, well,
};
pub use self::me::Me;
pub use self::roll::Roll;
pub use self::root::toolbar;
pub use self::status::{Bar, status_bar};
use self::theme::{HamletTheme, Look};
pub use self::window::{ShowRequests, window_plugin};

/// The context a view of the window is built in.
pub type Cx<'a> = bevy_fynix::Cx<'a, Bevy, HamletTheme>;

/// A view of the window, its type erased.
pub(crate) type AnyView = bevy_fynix::AnyView<Bevy, HamletTheme>;

/// A view of the window. A trait of its own until Rust has trait
/// aliases.
pub trait View:
    bevy_fynix::View<Bevy, HamletTheme> + 'static
{
}

impl<V: bevy_fynix::View<Bevy, HamletTheme> + 'static> View for V {}

/// The view `make` gives with the context in hand, for one made
/// where there is none, as in a `keyed` view.
pub(crate) fn with_cx<V: View>(
    make: impl FnOnce(&Cx) -> V + 'static,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let view = make(cx);
        cx.build(view)
    })
}

/// The panels: the dock, its panels and the world's place in them.
/// Add it to the core's app after Bevy's default plugins, set with
/// [`window_plugin`], the world's plugin and a [`ShowRequests`]
/// resource.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(me::plugin);
        let home = &app.world().resource::<HomeDir>().0.root;
        let size = window::saved_size(home);
        app.add_plugins((
            FynixPlugin::<HamletTheme>::default(),
            DockPlugin::<HamletTheme>::default(),
        ))
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
            (world_view::plugin, world_menu::plugin, hover::plugin),
            (
                theme::plugin,
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
            OnEnter(Look::Ready),
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
