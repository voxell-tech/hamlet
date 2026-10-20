//! The engine's window: its theme and styles, and what is built in
//! it.

pub mod command;
mod console;
pub mod dock;
pub mod fonts;
pub mod icons;
mod menu;
pub mod settings;
mod status;
pub mod style;
mod switcher;
pub mod theme;
pub mod widgets;
pub mod window;
pub mod workspace;

use bevy::app::App;
use bevy_fynix::{Bevy, FynixPlugin};
pub use command::{Command, MenuEntry, RunCommand, Shortcut};
pub use dock::{Panel, panel};
pub use settings::SettingsPage;
pub use style::ButtonStyle;
pub use theme::EngineTheme;
pub use widgets::tree_view;
pub use window::window_plugin;
pub use workspace::SuggestedWorkspace;

/// A view built with the engine's theme.
pub type AnyView = bevy_fynix::AnyView<Bevy, EngineTheme>;
/// What a view is built in, with the engine's theme.
pub type Cx<'a> = bevy_fynix::Cx<'a, Bevy, EngineTheme>;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        views_plugin,
        style::plugin,
        icons::plugin,
        fonts::plugin,
        window::plugin,
        dock::plugin,
        console::plugin,
        settings::plugin,
        workspace::plugin,
        switcher::plugin,
        command::plugin,
        menu::plugin,
        status::plugin,
    ));
}

/// What views are built with, without the window: fynix and the
/// theme.
pub fn views_plugin(app: &mut App) {
    app.add_plugins((
        FynixPlugin::<EngineTheme>::default(),
        theme::plugin,
    ));
}

/// A view built with the engine's theme. A trait of its own until
/// Rust has trait aliases.
pub trait View:
    bevy_fynix::View<Bevy, EngineTheme> + 'static
{
}

impl<V: bevy_fynix::View<Bevy, EngineTheme> + 'static> View for V {}
