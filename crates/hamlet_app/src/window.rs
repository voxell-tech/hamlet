//! The window's size and closing, independent of the server.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::app::{App, Startup, Update};
use bevy::ecs::message::MessageReader;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::resource_exists;
use bevy::ecs::system::{Commands, Local, Res};
use bevy::ecs::world::World;
use bevy::state::app::AppExtStates;
use bevy::state::state::States;
use bevy::time::Time;
use bevy::window::{
    ExitCondition, PresentMode, PrimaryWindow, Window,
    WindowCloseRequested, WindowPlugin, WindowResizeConstraints,
    WindowResized, WindowResolution,
};
use bevy::winit::{
    EventLoopProxyWrapper, UpdateMode, WinitSettings, WinitUserEvent,
};
use hamlet_client::ViewFolder;
use hamlet_client::connection::Wake;
use hamlet_realm::chief;

use crate::chrome;

/// The size the window opens at until the chief has resized it once.
const DEFAULT_SIZE: (u32, u32) = (1920, 1080);

/// The least size of the window.
const LEAST_SIZE: (u32, u32) = (1180, 680);

pub fn plugin(app: &mut App) {
    app.init_state::<WorldShown>()
        .insert_resource(winit_settings())
        .add_systems(
            Startup,
            (
                wake_on_frames
                    .run_if(resource_exists::<EventLoopProxyWrapper>),
                read_vsync,
            ),
        )
        .add_systems(Update, (intercept_close, remember_size));
}

/// The window draws the world.
#[derive(States, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub(crate) struct WorldShown;

/// Whether the window waits for the display before it shows each
/// frame, so that no frame shows torn. On unless the chief turned it
/// off, which the view folder keeps.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Vsync(pub(crate) bool);

fn size_file(view: &Path) -> PathBuf {
    view.join("window")
}

fn vsync_file(view: &Path) -> PathBuf {
    view.join("vsync")
}

/// Whether vsync is kept on in the view folder: on unless it says
/// off.
fn saved_vsync(view: &Path) -> bool {
    !std::fs::read_to_string(vsync_file(view))
        .is_ok_and(|text| text.trim() == "off")
}

fn present_mode(vsync: bool) -> PresentMode {
    if vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    }
}

fn read_vsync(mut commands: Commands, view: Res<ViewFolder>) {
    commands.insert_resource(Vsync(saved_vsync(&view.0)));
}

/// Turns vsync on or off, on the window and in the view folder.
pub(crate) fn set_vsync(world: &mut World, on: bool) {
    world.insert_resource(Vsync(on));
    let mut windows =
        world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    for mut window in windows.iter_mut(world) {
        window.present_mode = present_mode(on);
    }
    if let Some(view) = world.get_resource::<ViewFolder>()
        && std::fs::create_dir_all(&view.0).is_ok()
    {
        let _ = std::fs::write(
            vsync_file(&view.0),
            if on { "on" } else { "off" },
        );
    }
}

/// The window plugin at the size last kept in the view folder, with
/// vsync as kept there.
pub fn window_plugin(view: &Path) -> Result<WindowPlugin> {
    let size = saved_size(view)?;
    Ok(WindowPlugin {
        primary_window: Some(Window {
            title: "Hamlet".into(),
            // The app id on Wayland and the class on X11: what ties
            // the window to the menu's hamlet.desktop and its icon.
            name: Some("hamlet".into()),
            resolution: WindowResolution::new(size.0, size.1),
            present_mode: present_mode(saved_vsync(view)),
            resize_constraints: WindowResizeConstraints {
                min_width: LEAST_SIZE.0 as f32,
                min_height: LEAST_SIZE.1 as f32,
                ..WindowResizeConstraints::default()
            },
            ..chrome::bare()
        }),
        exit_condition: ExitCondition::DontExit,
        close_when_requested: false,
        ..WindowPlugin::default()
    })
}

/// The size kept in the view folder, else the first size.
fn saved_size(view: &Path) -> Result<(u32, u32)> {
    let file = size_file(view);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DEFAULT_SIZE);
        }
        Err(e) => {
            return Err(e).with_context(|| {
                format!("reading {}", file.display())
            });
        }
    };
    let parse = |text: &str| {
        let (w, h) = text.trim().split_once('x')?;
        Some((
            w.parse::<u32>().ok()?.max(LEAST_SIZE.0),
            h.parse::<u32>().ok()?.max(LEAST_SIZE.1),
        ))
    };
    parse(&text).ok_or_else(|| {
        chief::unparsed(
            &file,
            "it does not hold a size such as 1920x1080",
        )
    })
}

/// Keeps the size once resizing has settled.
pub(crate) fn remember_size(
    time: Res<Time>,
    view: Res<ViewFolder>,
    mut resizes: MessageReader<WindowResized>,
    mut pending: Local<Option<(f32, (u32, u32))>>,
) {
    if let Some(r) = resizes.read().last() {
        *pending = Some((
            time.elapsed_secs(),
            (r.width as u32, r.height as u32),
        ));
    }
    if let Some((at, (w, h))) = *pending
        && time.elapsed_secs() - at > 0.8
    {
        if std::fs::create_dir_all(&view.0).is_ok() {
            let _ = std::fs::write(
                size_file(&view.0),
                format!("{w}x{h}"),
            );
        }
        *pending = None;
    }
}

pub(crate) fn winit_settings() -> WinitSettings {
    WinitSettings {
        focused_mode: UpdateMode::reactive(Duration::from_millis(50)),
        unfocused_mode: UpdateMode::reactive_low_power(
            Duration::from_millis(250),
        ),
    }
}

/// Wakes the reactive loop when the connection hands over a frame,
/// rather than at the loop's next timeout.
fn wake_on_frames(
    proxy: Res<EventLoopProxyWrapper>,
    mut commands: Commands,
) {
    let proxy = (**proxy).clone();
    commands.insert_resource(Wake::new(move || {
        // Fails only once the event loop has ended.
        let _ = proxy.send_event(WinitUserEvent::WakeUp);
    }));
}

fn intercept_close(
    mut closes: MessageReader<WindowCloseRequested>,
    mut commands: Commands,
) {
    if closes.read().count() > 0 {
        commands.queue(quit);
    }
}

pub(crate) fn quit(world: &mut World) {
    crate::saving::close(world);
}

#[cfg(test)]
mod tests {
    use bevy::app::AppExit;
    use bevy::ecs::query::With;
    use bevy::state::app::StatesPlugin;
    use bevy::window::PrimaryWindow;
    use hamlet_testing::Scratch;

    use super::*;

    fn app(tag: &str) -> (App, Scratch) {
        let dir = Scratch::new(tag);
        let mut app = App::new();
        app.add_plugins(StatesPlugin)
            .add_message::<WindowCloseRequested>()
            .add_message::<WindowResized>()
            .init_resource::<Time>()
            .insert_resource(ViewFolder(dir.join("view")));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.add_plugins(plugin);
        app.update();
        (app, dir)
    }

    fn window(app: &mut App) -> Window {
        let world = app.world_mut();
        world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .single(world)
            .unwrap()
            .clone()
    }

    #[test]
    fn closing_ends_only_the_window() {
        let (mut app, _dir) = app("window-close");
        let primary = app.world_mut()
            .query_filtered::<bevy::ecs::entity::Entity, With<PrimaryWindow>>()
            .single(app.world()).unwrap();
        app.world_mut()
            .write_message(WindowCloseRequested { window: primary });
        app.update();
        assert_eq!(app.should_exit(), Some(AppExit::Success));
    }

    #[test]
    fn resizing_is_kept_in_the_view_folder_and_read_at_launch() {
        let (mut app, dir) = app("window-view-size");
        let primary = app.world_mut()
            .query_filtered::<bevy::ecs::entity::Entity, With<PrimaryWindow>>()
            .single(app.world()).unwrap();
        app.world_mut().write_message(WindowResized {
            window: primary,
            width: 1420.0,
            height: 810.0,
        });
        app.update();
        assert!(!dir.join("view/window").exists());
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs(1));
        app.update();
        assert_eq!(
            std::fs::read_to_string(dir.join("view/window")).unwrap(),
            "1420x810"
        );
        assert!(!dir.join("window").exists());
        let mut relaunched = App::new();
        relaunched
            .add_plugins(window_plugin(&dir.join("view")).unwrap());
        let window = window(&mut relaunched);
        assert_eq!(window.resolution.width(), 1420.0);
        assert_eq!(window.resolution.height(), 810.0);
    }

    #[test]
    fn a_size_that_does_not_parse_is_refused_and_left() {
        let dir = Scratch::new("window-unparsed");
        assert_eq!(saved_size(&dir).unwrap(), DEFAULT_SIZE);
        std::fs::write(size_file(&dir), "wide").unwrap();
        let error = window_plugin(&dir).err().unwrap().to_string();
        assert!(
            error.starts_with(&format!(
                "{} does not parse",
                size_file(&dir).display()
            )),
            "{error}"
        );
        assert_eq!(
            std::fs::read_to_string(size_file(&dir)).unwrap(),
            "wide"
        );
    }
}
