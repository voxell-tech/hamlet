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
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::prelude::ClearColor;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy_fynix::{Bevy, FynixPlugin, Theme};
pub use command::{
    Command, MenuEntry, RunCommand, ShellCommandsExt, Shortcut,
};
pub use dock::Panel;
use hamlet_engine_api::CommandId;
pub use settings::SettingsPage;
pub use style::{ButtonStyle, StyleExt};
pub use theme::EngineTheme;
pub use widgets::tree_view;
pub use window::window_plugin;
pub use workspace::{DockTreeExt, SuggestedWorkspace};

use crate::menu::{MenuBar, OpenMenu};
use crate::switcher::{Renaming, WorkspaceTab};
use crate::workspace::Workspaces;

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

/// The shell's methods on [`World`].
pub trait ShellWorldExt {
    /// Runs the command `id`, if there is one.
    fn run_command(&mut self, id: &CommandId);

    /// The rect of the UI node `node`, in the window's logical
    /// pixels.
    fn node_rect(&self, node: Entity) -> Option<Rect>;

    /// Opens the menu titled `title` under its title, `node`.
    fn open_menu(&mut self, title: &str, node: Entity);

    /// Opens the rename field over the tab of the workspace at `at`.
    fn start_rename(&mut self, at: usize);

    /// Puts `theme` in, for the views and the window behind them.
    fn apply_theme(&mut self, theme: EngineTheme);
}

impl ShellWorldExt for World {
    fn run_command(&mut self, id: &CommandId) {
        let found = self.try_query::<(Entity, &Command)>().and_then(
            |mut all| {
                all.iter(self)
                    .find(|(_, command)| command.id == *id)
                    .map(|(entity, _)| entity)
            },
        );
        if let Some(entity) = found {
            self.trigger(RunCommand { entity });
        }
    }

    fn node_rect(&self, node: Entity) -> Option<Rect> {
        let (computed, at) = self
            .get::<ComputedNode>(node)
            .zip(self.get::<UiGlobalTransform>(node))?;
        let scale = computed.inverse_scale_factor();
        let (_, _, centre) = at.to_scale_angle_translation();
        Some(Rect::from_center_size(
            centre * scale,
            computed.size() * scale,
        ))
    }

    fn open_menu(&mut self, title: &str, node: Entity) {
        let Some(rect) = self.node_rect(node) else {
            return;
        };
        let menu = self
            .resource::<MenuBar>()
            .0
            .iter()
            .find(|menu| menu.title == title)
            .cloned();
        if let Some(menu) = menu {
            self.resource_mut::<OpenMenu>().0 = Some((menu, rect));
        }
    }

    fn start_rename(&mut self, at: usize) {
        let Some(name) = self
            .resource::<Workspaces>()
            .list()
            .get(at)
            .map(|workspace| workspace.name.clone())
        else {
            return;
        };
        let anchor = self
            .try_query::<(&WorkspaceTab, &ComputedNode, &UiGlobalTransform)>()
            .and_then(|mut tabs| {
                tabs.iter(self).find(|(tab, ..)| tab.0 == at).map(
                    |(_, node, place)| {
                        let scale = node.inverse_scale_factor();
                        let (_, _, centre) =
                            place.to_scale_angle_translation();
                        Rect::from_center_size(
                            centre * scale,
                            node.size() * scale,
                        )
                    },
                )
            })
            .unwrap_or_default();
        self.resource_mut::<Renaming>().0 = Some((at, anchor, name));
    }

    fn apply_theme(&mut self, theme: EngineTheme) {
        self.insert_resource(ClearColor(*theme.colors.background));
        self.insert_resource(Theme(theme));
    }
}
