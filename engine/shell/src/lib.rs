//! The engine's window: its theme and styles, and what is built in
//! it.

mod about;
mod backdrop;
pub mod command;
mod console;
pub mod dock;
pub mod fade;
pub mod fonts;
pub mod icons;
mod menu;
pub mod message;
pub mod overlay;
pub mod pointer;
mod rank;
mod section;
pub mod settings;
pub mod sounds;
mod status;
pub mod stick;
pub mod style;
mod switcher;
pub mod theme;
mod volumes;
pub mod window;
pub mod workspace;

use bevy::app::App;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;
use bevy::math::Rect;
use bevy::prelude::ClearColor;
use bevy::ui::{ComputedNode, UiGlobalTransform};
use bevy_fynix::dock::DockTree;
use bevy_fynix::{Bevy, FynixPlugin, Theme};
use hamlet_engine_api::id::ENGINE;
use hamlet_engine_api::{CommandId, PanelId};

pub use crate::backdrop::Backdrop;
pub use crate::command::{
    Command, MenuEntry, RunCommand, ShellCommandsExt, Shortcut,
};
pub use crate::dock::Panel;
use crate::menu::{MenuBar, OpenMenu};
pub use crate::overlay::{Overlay, OverlayShown};
pub use crate::pointer::{NodePointer, PointerMode, ResizeAxis};
pub use crate::rank::StartRank;
pub use crate::section::{PanelSection, section_content};
use crate::settings::{Pages, SETTINGS};
pub use crate::settings::{SettingsFooter, SettingsPage};
pub use crate::status::StatusItem;
pub use crate::style::{ButtonStyle, StyleExt};
use crate::switcher::Renaming;
pub use crate::theme::EngineTheme;
pub use crate::window::{ShowWindow, window_plugin};
use crate::workspace::Workspaces;
pub use crate::workspace::{DockTreeExt, SuggestedWorkspace};

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
        (message::plugin, stick::plugin, fade::plugin),
        status::plugin,
        (
            overlay::plugin,
            backdrop::plugin,
            pointer::plugin,
            sounds::plugin,
            volumes::plugin,
            about::plugin,
        ),
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

    /// Opens the rename field in the tab of the workspace at `at`.
    fn start_rename(&mut self, at: usize);

    /// Puts `theme` in, for the views and the window behind them.
    fn apply_theme(&mut self, theme: EngineTheme);

    /// Brings the panel `id` to the front, or opens it in the first
    /// area.
    fn open_panel(&mut self, id: &PanelId);

    /// Shows the settings page `page` in Settings, and brings
    /// Settings to the front.
    fn open_settings_page(&mut self, page: Entity);
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
        self.resource_mut::<Renaming>().0 = Some((at, name));
    }

    fn apply_theme(&mut self, theme: EngineTheme) {
        self.insert_resource(ClearColor(*theme.colors.background));
        self.insert_resource(Theme(theme));
    }

    fn open_panel(&mut self, id: &PanelId) {
        if let Some(mut tree) = self.get_resource_mut::<DockTree>() {
            tree.open_panel(id.as_str());
        }
    }

    fn open_settings_page(&mut self, page: Entity) {
        if let Some(mut pages) = self.get_resource_mut::<Pages>() {
            pages.chosen = Some(page);
        }
        if let Ok(settings) = PanelId::new(ENGINE, SETTINGS) {
            self.open_panel(&settings);
        }
    }
}
