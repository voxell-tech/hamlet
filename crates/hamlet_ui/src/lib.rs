//! The frame of the window: the dock, the window's chrome and states,
//! the pointer, the hover card and the panels' sounds. What fills it
//! is plugins.

pub mod chrome;
pub mod commands;
pub mod cursor;
mod dock;
mod hover;
mod remember;
mod root;
mod sounds;
pub mod widgets;
mod window;

use bevy::app::{App, Plugin};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::state::state::OnEnter;
use bevy_fynix::FynixPlugin;
use bevy_fynix::dock::DockPlugin;
pub use hamlet_agents_api::roll::{self, Roll};
pub use hamlet_chat::me::{self, Me};
pub use hamlet_chat::{chat, links, markdown, pictures, select};
pub use hamlet_limits::{models, rules};
pub use hamlet_sdk::bar::Bar;
pub use hamlet_sdk::dock::{lit_panel, panel};
use hamlet_sdk::frame::{Mounting, PointerWait};
pub use hamlet_sdk::looks::{
    ICON, brass, grim, lamp, marked, retrace, well,
};
pub use hamlet_sdk::{
    AnyView, Cx, SdkPlugin, View, looks, theme, with_cx,
};

pub use self::root::toolbar;
use self::theme::{HamletTheme, Look};
pub use self::widgets::{feel, icons, platform, tabs, tuning};
pub use self::window::{ShowRequests, window_plugin};

/// The frame of the window: the dock and what the panels stand on.
/// Add it to the core's app after Bevy's default plugins, set with
/// [`window_plugin`], and a [`ShowRequests`] resource.
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            SdkPlugin,
            FynixPlugin::<HamletTheme>::default(),
            DockPlugin::<HamletTheme>::default(),
            (
                root::plugin,
                dock::plugin,
                window::plugin,
                chrome::plugin,
                commands::plugin,
                remember::plugin,
            ),
            hamlet_agents_api::asking::plugin,
            hover::plugin,
            (
                theme::plugin,
                widgets::plugin,
                cursor::plugin,
                sounds::plugin,
            ),
        ))
        .init_resource::<PointerWait>()
        .add_systems(
            OnEnter(Look::Ready),
            (dock::seed, root::mount).chain().in_set(Mounting),
        );
    }
}
