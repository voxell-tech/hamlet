//! The command center (I4): a Bevy and fynix window on the client's
//! state.

pub mod asking;
mod chief;
pub mod chrome;
pub mod cursor;
mod dock;
mod folder;
pub mod history;
mod hover;
pub mod lists;
mod looks;
mod markdown;
mod me;
pub mod menu;
pub mod models;
mod remember;
mod roll;
mod root;
mod saving;
pub mod secret;

pub mod rules;
pub mod screens;
pub mod select;
mod sounds;
mod status;
#[cfg(test)]
mod testing;
pub mod theme;
pub mod welcome;
pub mod widgets;
mod window;
mod world_menu;
mod world_view;

use bevy::app::{App, Plugin, Update};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::state::state::OnEnter;
use bevy_fynix::dock::DockPlugin;
use bevy_fynix::{Bevy, FynixPlugin};
use hamlet_realm::pick::Selected;

pub use self::dock::{lit_panel, panel};
pub use self::looks::{
    ICON, brass, grim, lamp, marked, retrace, well,
};
pub use self::me::Me;
pub use self::remember::Remembered;
pub use self::roll::Roll;
pub use self::root::toolbar;
pub use self::screens::{
    chat, inspector, links, overview, petition, pictures, settings,
};
pub use self::status::{Bar, status_bar};
use self::theme::{HamletTheme, Look};
pub use self::widgets::{feel, icons, platform, tabs, tuning};
pub use self::window::window_plugin;

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

/// The app's panels, added after the client's plugin, a
/// [`hamlet_client::ViewDir`] with the [`Remembered`] view and the
/// chiefs' [`hamlet_realm::chief::Kept`] files read from it, the
/// world's plugin and Bevy's default plugins set with
/// [`window_plugin`].
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            me::plugin,
            FynixPlugin::<HamletTheme>::default(),
            DockPlugin::<HamletTheme>::default(),
            (
                roll::plugin,
                root::plugin,
                dock::plugin,
                window::plugin,
                saving::plugin,
                secret::plugin,
                chrome::plugin,
                menu::plugin,
                status::plugin,
                remember::plugin,
            ),
            (
                screens::plugin,
                chief::plugin,
                folder::plugin,
                rules::plugin,
                models::plugin,
                asking::plugin,
                select::plugin,
                history::plugin,
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

#[cfg(test)]
mod tests {
    use bevy::app::PluginGroup;
    use bevy::render::RenderPlugin;
    use bevy::render::settings::WgpuSettings;
    use bevy::winit::WinitPlugin;
    use hamlet_client::{ClientPlugin, ViewDir};
    use hamlet_testing::Scratch;

    use super::*;

    #[test]
    fn the_windows_files_leave_the_servers_home_empty() {
        let home = Scratch::new("view-untouched-home");
        for test in [
            "window::tests::resizing_is_kept_in_the_view_folder_and_read_at_launch",
            "remember::tests::a_changed_dock_and_view_choices_return_from_the_view_folder",
            "chief::tests::the_chiefs_look_returns_from_the_view_folder_at_launch",
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test])
                .env("HAMLET_HOME", home.as_os_str())
                .output().unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(output.status.success() && stdout.contains("1 passed"), "{test}: {stdout}{}", String::from_utf8_lossy(&output.stderr));
        }
        assert_eq!(std::fs::read_dir(&*home).unwrap().count(), 0);
    }

    #[test]
    fn the_panels_build_with_only_a_view_folder_and_the_client() {
        let view = Scratch::new("ui-view-only");
        let mut app = App::new();
        app.add_plugins((
            bevy::DefaultPlugins
                .build()
                .disable::<WinitPlugin>()
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..Default::default()
                    }
                    .into(),
                    ..Default::default()
                }),
            ClientPlugin { server: None },
        ))
        .insert_resource(ViewDir(view.to_path_buf()))
        .insert_resource(Remembered::read(&view).unwrap())
        .insert_resource(
            hamlet_realm::chief::Kept::read(&view).unwrap(),
        );
        app.add_plugins(UiPlugin);
    }
}
