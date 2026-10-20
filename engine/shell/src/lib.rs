//! The engine's window: its theme and looks, and what is built in
//! it.

pub mod command;
mod console;
pub mod dock;
pub mod fonts;
mod menu;
mod plugin_list;
mod status;
mod switcher;
pub mod theme;
pub mod window;
pub mod workspace;

use bevy::app::App;
use bevy_fynix::{Bevy, FynixPlugin};
pub use command::{Command, MenuEntry, RunCommand, Shortcut};
pub use dock::{Panel, panel};
pub use theme::EngineTheme;
pub use window::window_plugin;

/// A view built with the engine's theme.
pub type AnyView = bevy_fynix::AnyView<Bevy, EngineTheme>;
/// What a view is built in, with the engine's theme.
pub type Cx<'a> = bevy_fynix::Cx<'a, Bevy, EngineTheme>;

/// A view built with the engine's theme. A trait of its own until
/// Rust has trait aliases.
pub trait View:
    bevy_fynix::View<Bevy, EngineTheme> + 'static
{
}

impl<V: bevy_fynix::View<Bevy, EngineTheme> + 'static> View for V {}

pub fn plugin(app: &mut App) {
    app.add_plugins((
        FynixPlugin::<EngineTheme>::default(),
        theme::plugin,
        fonts::plugin,
        window::plugin,
        dock::plugin,
        console::plugin,
        plugin_list::plugin,
        workspace::plugin,
        switcher::plugin,
        command::plugin,
        menu::plugin,
        status::plugin,
    ));
}
