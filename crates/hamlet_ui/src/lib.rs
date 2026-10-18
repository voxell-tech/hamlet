//! The command center (I4): a Bevy and fynix window added to the
//! core's app.

mod chief;
pub mod chrome;
pub mod cursor;
mod dock;
mod hover;
pub mod menu;
mod remember;
mod root;
mod sounds;
mod status;
pub mod welcome;
pub mod widgets;
mod window;
pub mod world_menu;
mod world_view;

use bevy::app::{App, Plugin, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::state::state::OnEnter;
use bevy_fynix::FynixPlugin;
use bevy_fynix::dock::DockPlugin;
pub use hamlet_chat::me::{self, Me};
pub use hamlet_chat::{chat, links, markdown, pictures, select};
pub use hamlet_limits::{models, rules};
pub use hamlet_sdk::dock::{lit_panel, panel};
pub use hamlet_sdk::looks::{
    ICON, brass, grim, lamp, marked, retrace, well,
};
pub use hamlet_sdk::roll::{self, Roll};
pub use hamlet_sdk::{
    AnyView, Cx, SdkPlugin, View, looks, theme, with_cx,
};

pub use self::root::toolbar;
pub use self::status::{Bar, status_bar};
use self::theme::{HamletTheme, Look};
pub use self::widgets::{feel, icons, platform, tabs, tuning};
pub use self::window::{ShowRequests, window_plugin};

/// The panels: the dock, its panels and the world's place in them.
/// Add it to the core's app after Bevy's default plugins, set with
/// [`window_plugin`], the world's plugin and a [`ShowRequests`]
/// resource.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            SdkPlugin,
            hamlet_chat::plugin,
            FynixPlugin::<HamletTheme>::default(),
            DockPlugin::<HamletTheme>::default(),
            (
                root::plugin,
                dock::plugin,
                window::plugin,
                chrome::plugin,
                menu::plugin,
                status::plugin,
                remember::plugin,
            ),
            (
                chief::plugin,
                hamlet_limits::plugin,
                hamlet_sdk::asking::plugin,
                welcome::plugin,
            ),
            (world_view::plugin, world_menu::plugin, hover::plugin),
            (
                theme::plugin,
                widgets::plugin,
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
        .add_systems(Update, chief::accompany.after(me::Followed));
    }
}
