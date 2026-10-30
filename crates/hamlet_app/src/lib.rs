//! The window (I4): the world, the dock of panels around it, the top
//! bar and its menus, the foot and the pointer, on the client's
//! state and built from the widgets of `hamlet_ui`.

pub mod asking;
mod chief;
pub mod chrome;
mod convo;
pub mod cursor;
mod desk;
mod dock;
mod folder;
pub mod history;
mod hover;
pub mod limits;
pub mod lists;
mod markdown;
pub mod menu;
pub mod models;
mod remember;
mod roll;
mod root;
mod saving;
pub mod secret;

pub mod screens;
pub mod select;
mod sounds;
mod status;
#[cfg(test)]
mod testing;
mod tuning;
pub mod welcome;
mod window;
mod world_menu;
mod world_view;

use bevy::app::{App, Plugin, PreUpdate};
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::input::InputSystems;
use bevy::picking::PickingSystems;
use bevy::state::state::OnEnter;
use bevy_fynix::FynixPlugin;
use bevy_fynix::dock::DockPlugin;
use hamlet_client::connection::ClientSystems;
use hamlet_realm::pick::Selected;
use hamlet_ui::theme::{HamletTheme, ThemeState};

pub use self::convo::Convo;
pub use self::desk::still as desk_still;
pub use self::dock::{highlight_panel, panel};
pub use self::remember::Remembered;
pub use self::roll::Roll;
pub use self::root::toolbar;
pub use self::screens::{
    chat, inspector, links, overview, petition, pictures, settings,
};
pub use self::status::{Bar, status_bar};
pub use self::window::window_plugin;

/// The panels' steps before fynix mounts the views, in order, after
/// this frame's server frames, input and clicks. They run in
/// `PreUpdate`: fynix mounts in `Update`, in sets the vendored crate
/// keeps private and that stay so, and only an earlier schedule can
/// come before them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum PanelSystems {
    /// The panels' state follows what was received, clicked or set:
    /// the chat's agent, the selection, the setup and the models.
    Follow,
    /// The values the views read, worked out from what was received
    /// and from what Follow set: the roll, the status bar's words and
    /// the question on show.
    Bind,
}

/// Orders [`PanelSystems`].
pub(crate) fn panel_sets(app: &mut App) {
    app.configure_sets(
        PreUpdate,
        (PanelSystems::Follow, PanelSystems::Bind)
            .chain()
            .after(ClientSystems::Receive)
            .after(InputSystems)
            .after(PickingSystems::Last),
    );
}

/// The app's panels, added after the client's plugin, a
/// [`hamlet_client::ViewFolder`] with the [`Remembered`] view and the
/// chiefs' [`hamlet_realm::chief::Kept`] files read from it, the
/// world's plugin and Bevy's default plugins set with
/// [`window_plugin`].
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            panel_sets,
            convo::plugin,
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
                desk::plugin,
            ),
            (
                screens::plugin,
                chief::plugin,
                folder::plugin,
                limits::plugin,
                models::plugin,
                asking::plugin,
                select::plugin,
                history::plugin,
                welcome::plugin,
            ),
            (
                world_view::plugin,
                world_menu::plugin,
                hover::plugin,
                hamlet_ui::plugin,
                tuning::plugin,
                cursor::plugin,
                sounds::plugin,
            ),
        ))
        // The world panel shows the world's camera, which the world
        // has spawned by now.
        .add_systems(
            OnEnter(ThemeState::Ready),
            (world_view::attach, dock::seed, root::mount).chain(),
        )
        .add_systems(
            PreUpdate,
            (
                convo::reseat.run_if(convo::roster_moved),
                inspector::follow_selection
                    .run_if(resource_changed::<Selected>),
                inspector::tell_page,
                chief::accompany,
            )
                .chain()
                .in_set(PanelSystems::Follow),
        );
    }
}

#[cfg(test)]
mod tests {
    use bevy::app::PluginGroup;
    use bevy::render::RenderPlugin;
    use bevy::render::settings::WgpuSettings;
    use bevy::winit::WinitPlugin;
    use hamlet_client::{ClientPlugin, ViewFolder};
    use hamlet_testing::Scratch;
    use hamlet_ui::theme;

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
        .insert_resource(ViewFolder(view.to_path_buf()))
        .insert_resource(Remembered::read(&view).unwrap())
        .insert_resource(
            hamlet_realm::chief::Kept::read(&view).unwrap(),
        );
        app.add_plugins(UiPlugin);
    }

    #[test]
    fn a_frame_from_the_server_turns_the_roll_and_shows_in_a_view_in_the_update_it_arrives()
     {
        use std::time::Duration;

        use bevy_fynix::views::label;
        use hamlet_assets::Tuning;
        use hamlet_client::Tokio;
        use hamlet_client::connection::Wake;
        use hamlet_client::realm::{HOME, hamlet, mayor, name_of};
        use hamlet_protocol::client as protocol;
        use hamlet_protocol::client::discovery::ServerAddress;

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let url = hamlet_testing::realm::serve(
            runtime.handle(),
            vec![protocol::ServerMessage::State(
                hamlet_testing::realm::state(),
            )],
        );
        let (woke, wakes) = std::sync::mpsc::channel();
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            ClientPlugin {
                server: Some(ServerAddress {
                    url,
                    token: "frame-token".into(),
                }),
            },
            FynixPlugin::<HamletTheme>::default(),
            panel_sets,
            roll::plugin,
        ))
        .insert_resource(Tokio(runtime.handle().clone()))
        .insert_resource(Wake::new(move || {
            let _ = woke.send(());
        }));
        theme::insert_theme(app.world_mut(), HamletTheme::designed());
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            label(roll::rolled(|world| {
                mayor(world)
                    .map(|agent| name_of(world, agent))
                    .unwrap_or_default()
            })),
        );
        // Read before the first update, in which the client connects
        // and the frame may already arrive.
        let before = *app.world().resource::<Roll>();
        app.update();
        // One wake for the connection, one for the state.
        let wait = Duration::from_secs(5);
        wakes.recv_timeout(wait).unwrap();
        wakes.recv_timeout(wait).unwrap();
        app.update();
        assert!(hamlet(app.world(), HOME).is_some());
        assert_ne!(*app.world().resource::<Roll>(), before);
        let shown = testing::words(&mut app);
        assert!(
            shown.iter().any(|text| text == "founder"),
            "{shown:?}"
        );
    }
}
