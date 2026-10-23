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

use bevy::app::{App, Plugin, PreUpdate};
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::schedule::{IntoScheduleConfigs, SystemSet};
use bevy::input::InputSystems;
use bevy::picking::PickingSystems;
use bevy::state::state::OnEnter;
use bevy_fynix::dock::DockPlugin;
use bevy_fynix::{Bevy, FynixPlugin};
use hamlet_client::connection::ClientSystems;
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
/// [`hamlet_client::ViewDir`] with the [`Remembered`] view and the
/// chiefs' [`hamlet_realm::chief::Kept`] files read from it, the
/// world's plugin and Bevy's default plugins set with
/// [`window_plugin`].
pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            panel_sets,
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
            PreUpdate,
            (
                me::reseat.run_if(me::roster_moved),
                inspector::follow_selection
                    .run_if(resource_changed::<Selected>),
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

    #[test]
    fn a_frame_from_the_server_turns_the_roll_and_shows_in_a_view_in_the_update_it_arrives()
     {
        use std::time::Duration;

        use bevy_fynix::views::label;
        use hamlet_assets::Tuning;
        use hamlet_client::Tokio;
        use hamlet_client::connection::Wake;
        use hamlet_client::realm::{
            HOME, mayor, name_of, settlement,
        };
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
        app.update();
        let before = *app.world().resource::<Roll>();
        // One wake for the connection, one for the state.
        let wait = Duration::from_secs(5);
        wakes.recv_timeout(wait).unwrap();
        wakes.recv_timeout(wait).unwrap();
        app.update();
        assert!(settlement(app.world(), HOME).is_some());
        assert_ne!(*app.world().resource::<Roll>(), before);
        let shown = testing::words(&mut app);
        assert!(
            shown.iter().any(|text| text == "founder"),
            "{shown:?}"
        );
    }
}
