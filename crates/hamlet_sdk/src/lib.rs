//! What a plugin of the command center is built with. It does not
//! know the agents' runtime: its read models of the agents are plain
//! data that `hamlet_agents_api` keeps. A plugin crate never depends
//! on `hamlet_ui`.

pub mod agents;
pub mod bar;
pub mod capability;
pub mod cards;
pub mod checks;
pub mod command;
pub mod confirm;
pub mod context;
pub mod dock;
pub mod events;
pub mod feel;
pub mod icons;
pub mod inspector;
pub mod keys;
pub mod looks;
pub mod menu;
pub mod page;
pub mod papers;
pub mod platform;
pub mod plugin;
pub mod reload;
pub mod settings;
pub mod tabs;
pub mod theme;
pub mod tree;

use bevy::app::{App, Plugin};
use bevy_fynix::Bevy;

use self::theme::HamletTheme;

/// What the plugins stand on. Add it to the app before any plugin.
pub struct SdkPlugin;

impl Plugin for SdkPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            capability::plugin,
            command::plugin,
            context::plugin,
            dock::plugin,
            inspector::plugin,
            keys::plugin,
            menu::plugin,
            plugin::plugin,
            settings::plugin,
            tree::plugin,
            bar::plugin,
            tabs::plugin,
        ));
    }
}

/// The context a view of the window is built in.
pub type Cx<'a> = bevy_fynix::Cx<'a, Bevy, HamletTheme>;

/// A view of the window, its type erased.
pub type AnyView = bevy_fynix::AnyView<Bevy, HamletTheme>;

/// A view of the window. A trait of its own until Rust has trait
/// aliases.
pub trait View:
    bevy_fynix::View<Bevy, HamletTheme> + 'static
{
}

impl<V: bevy_fynix::View<Bevy, HamletTheme> + 'static> View for V {}

/// The view `make` gives with the context in hand, for one made
/// where there is none, as in a `keyed` view.
pub fn with_cx<V: View>(
    make: impl FnOnce(&Cx) -> V + 'static,
) -> AnyView {
    AnyView::new(move |cx: &mut Cx| {
        let view = make(cx);
        cx.build(view)
    })
}
