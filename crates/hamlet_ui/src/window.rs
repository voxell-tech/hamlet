//! Window states: full, minimal, and background (hidden, still
//! running).

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::{App, AppExit, Update};
use bevy::ecs::message::MessageReader;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::system::{Local, Res, ResMut, Single};
use bevy::state::app::AppExtStates;
use bevy::state::state::{
    ComputedStates, NextState, OnEnter, OnExit, OnTransition, State,
    States,
};
use bevy::time::Time;
use bevy::ui::{AlignItems, UiRect, px};
use bevy::window::{
    ExitCondition, PrimaryWindow, Window, WindowCloseRequested,
    WindowPlugin, WindowResizeConstraints, WindowResized,
    WindowResolution,
};
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_fynix::resource;
use bevy_fynix::tokens::Tone;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, label, row,
};
use hamlet_core::core::HomeDir;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::chrome::{self, Grip};
use crate::icons::Glyph;
use crate::looks::{action, bad_action, good_action};
use crate::platform::{DisplayText, Glow, Wraps};
use crate::status::{Bar, Lately};
use crate::{AnyView, Cx};

/// The size the window opens at until the user has resized it once.
const DEFAULT_SIZE: (u32, u32) = (1920, 1080);

const MINIMAL_SIZE: (u32, u32) = (440, 190);

/// The least size of the full window.
const LEAST_FULL: (u32, u32) = (1180, 680);

/// The window's states and what entering and leaving each does.
pub fn plugin(app: &mut App) {
    app.init_state::<Layout>()
        .init_state::<Showing>()
        .add_computed_state::<WorldShown>()
        .insert_resource(winit_settings(false))
        .add_systems(OnExit(Layout::Full), keep_size)
        .add_systems(OnEnter(Layout::Full), constrain)
        .add_systems(
            OnTransition {
                exited: Layout::Minimal,
                entered: Layout::Full,
            },
            resize,
        )
        .add_systems(
            OnEnter(Layout::Minimal),
            (constrain, resize).chain(),
        )
        .add_systems(OnEnter(Showing::Hidden), hide)
        .add_systems(OnEnter(Showing::Shown), show)
        .add_systems(
            Update,
            (intercept_close, poll_show_requests, remember_size),
        );
}

/// The least window size of a layout, below which its contents do not
/// fit.
fn least(layout: Layout) -> WindowResizeConstraints {
    let (min_width, min_height) = match layout {
        Layout::Full => (LEAST_FULL.0 as f32, LEAST_FULL.1 as f32),
        Layout::Minimal => (MINIMAL_SIZE.0 as f32, 170.0),
    };
    WindowResizeConstraints {
        min_width,
        min_height,
        ..WindowResizeConstraints::default()
    }
}

/// The window's layout: all of it, or the small status window.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub enum Layout {
    #[default]
    Full,
    Minimal,
}

/// The window on screen or in the background.
#[derive(
    States, Clone, Copy, PartialEq, Eq, Hash, Debug, Default,
)]
pub(crate) enum Showing {
    #[default]
    Shown,
    /// Background: the window is hidden and the agents keep running.
    Hidden,
}

/// The world is drawn: the window is full and on screen.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct WorldShown;

impl ComputedStates for WorldShown {
    type SourceStates = (Layout, Showing);

    fn compute((layout, showing): (Layout, Showing)) -> Option<Self> {
        (layout == Layout::Full && showing == Showing::Shown)
            .then_some(WorldShown)
    }
}

/// The size the full window goes back to.
#[derive(Resource)]
pub(crate) struct FullSize(pub (u32, u32));

fn size_file(home: &Path) -> PathBuf {
    home.join("window")
}

/// The plugin of the window, at the size it was last left at, for
/// Bevy's default plugins to be set with.
pub fn window_plugin(home: &Path) -> WindowPlugin {
    let size = saved_size(home);
    WindowPlugin {
        primary_window: Some(Window {
            title: "Hamlet".into(),
            resolution: WindowResolution::new(size.0, size.1),
            ..chrome::bare()
        }),
        exit_condition: ExitCondition::DontExit,
        close_when_requested: false,
        ..WindowPlugin::default()
    }
}

/// The size the user last left the full window at.
pub(crate) fn saved_size(home: &Path) -> (u32, u32) {
    let parse = |text: String| {
        let (w, h) = text.trim().split_once('x')?;
        Some((
            w.parse::<u32>().ok()?.max(LEAST_FULL.0),
            h.parse::<u32>().ok()?.max(LEAST_FULL.1),
        ))
    };
    std::fs::read_to_string(size_file(home))
        .ok()
        .and_then(parse)
        .unwrap_or(DEFAULT_SIZE)
}

/// Remembers the full window's size, written once the resizing has
/// settled.
pub(crate) fn remember_size(
    time: Res<Time>,
    home: Res<HomeDir>,
    shown: Option<Res<State<WorldShown>>>,
    mut resizes: MessageReader<WindowResized>,
    mut pending: Local<Option<(f32, (u32, u32))>>,
) {
    if shown.is_some() {
        if let Some(r) = resizes.read().last() {
            *pending = Some((
                time.elapsed_secs(),
                (r.width as u32, r.height as u32),
            ));
        }
    } else {
        resizes.clear();
    }
    if let Some((at, (w, h))) = *pending
        && time.elapsed_secs() - at > 0.8
    {
        let _ = std::fs::write(
            size_file(&home.0.root),
            format!("{w}x{h}"),
        );
        *pending = None;
    }
}

/// Asks from a second launch to show the window again.
#[derive(Resource)]
pub struct ShowRequests(pub UnboundedReceiver<()>);

pub(crate) fn winit_settings(hidden: bool) -> WinitSettings {
    if hidden {
        WinitSettings {
            focused_mode: UpdateMode::reactive_low_power(
                Duration::from_millis(500),
            ),
            unfocused_mode: UpdateMode::reactive_low_power(
                Duration::from_millis(500),
            ),
        }
    } else {
        // Frequent enough to stream replies; input wakes it sooner.
        WinitSettings {
            focused_mode: UpdateMode::reactive(
                Duration::from_millis(50),
            ),
            unfocused_mode: UpdateMode::reactive_low_power(
                Duration::from_millis(250),
            ),
        }
    }
}

/// The close button sends the app to the background.
fn intercept_close(
    mut closes: MessageReader<WindowCloseRequested>,
    mut showing: ResMut<NextState<Showing>>,
) {
    if closes.read().count() > 0 {
        showing.set(Showing::Hidden);
    }
}

fn poll_show_requests(
    mut requests: ResMut<ShowRequests>,
    mut showing: ResMut<NextState<Showing>>,
) {
    let mut asked = false;
    while requests.0.try_recv().is_ok() {
        asked = true;
    }
    if asked {
        // Set even when it is shown already, so the window takes
        // the focus again.
        showing.set(Showing::Shown);
    }
}

fn keep_size(
    window: Single<&Window, With<PrimaryWindow>>,
    mut full: ResMut<FullSize>,
) {
    full.0 = (
        window.resolution.width() as u32,
        window.resolution.height() as u32,
    );
}

fn constrain(
    layout: Res<State<Layout>>,
    showing: Res<State<Showing>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut winit: ResMut<WinitSettings>,
) {
    window.resize_constraints = least(*layout.get());
    if *showing.get() == Showing::Shown {
        *winit = winit_settings(false);
        window.focused = true;
    }
}

fn resize(
    layout: Res<State<Layout>>,
    full: Res<FullSize>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    let (w, h) = match layout.get() {
        Layout::Full => full.0,
        Layout::Minimal => MINIMAL_SIZE,
    };
    window.resolution.set(w as f32, h as f32);
}

fn hide(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut winit: ResMut<WinitSettings>,
) {
    *winit = winit_settings(true);
    window.visible = false;
}

fn show(
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut winit: ResMut<WinitSettings>,
) {
    *winit = winit_settings(false);
    if !window.visible {
        window.visible = true;
    }
    window.focused = true;
}

pub(crate) fn set_layout(
    world: &mut bevy::ecs::world::World,
    layout: Layout,
) {
    world.resource_mut::<NextState<Layout>>().set(layout);
}

pub(crate) fn background(world: &mut bevy::ecs::world::World) {
    world
        .resource_mut::<NextState<Showing>>()
        .set(Showing::Hidden);
}

pub(crate) fn quit(world: &mut bevy::ecs::world::World) {
    world.write_message(AppExit::Success);
}

/// The small status window.
pub(crate) fn minimal() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        cx.build(
            column((
                row((
                    label("HAMLET")
                        .size(16.0)
                        .bold(true)
                        .tagged(DisplayText),
                    frame().grow(1.0),
                    label(resource::<Bar, _>(|s| s.running.clone()))
                        .tone(resource::<Bar, _>(|s| s.running_tone)),
                ))
                .align(AlignItems::Center),
                label(resource::<Bar, _>(|s| {
                    format!("Model: {}", s.model)
                }))
                .tone(Tone::Dim),
                label(resource::<Lately, _>(|lately| {
                    format!("Last activity: {}", lately.0)
                }))
                .tone(Tone::Dim),
                frame().grow(1.0),
                row((
                    good_action(Glyph::Expand, "EXPAND", |w| {
                        set_layout(w, Layout::Full)
                    }),
                    action(
                        Glyph::Lower,
                        "RUN IN BACKGROUND",
                        background,
                    ),
                    frame().grow(1.0),
                    bad_action(Glyph::Power, "QUIT HAMLET", quit),
                ))
                .gap(8.0)
                .tagged(Wraps),
            ))
            .grow(1.0)
            .gap(6.0)
            .padding(UiRect::all(px(16.0)))
            .fill(cx.theme().palette.black)
            .tagged(Glow)
            // The small window is held anywhere but on its buttons.
            .tagged(Grip),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::state::app::StatesPlugin;
    use hamlet_testing::Scratch;

    use super::*;

    fn app(tag: &str) -> (App, Scratch) {
        let dir = Scratch::new(tag);
        let mut app = App::new();
        app.add_plugins(StatesPlugin)
            .add_message::<WindowCloseRequested>()
            .add_message::<WindowResized>()
            .init_resource::<Time>()
            .insert_resource(HomeDir(
                hamlet_home::Home::open(dir.to_path_buf(), None)
                    .unwrap(),
            ));
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        app.insert_resource(ShowRequests(rx));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.insert_resource(FullSize((1000, 700)))
            .add_plugins(plugin);
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

    fn shown(app: &App) -> bool {
        app.world().contains_resource::<State<WorldShown>>()
    }

    #[test]
    fn the_window_goes_small_and_comes_back_at_its_size() {
        let (mut app, _dir) = app("window-small");
        assert!(shown(&app));
        let full = window(&mut app).resolution.size();
        assert_eq!(
            window(&mut app).resize_constraints.min_width,
            least(Layout::Full).min_width
        );

        set_layout(app.world_mut(), Layout::Minimal);
        app.update();
        assert!(!shown(&app));
        let small = window(&mut app);
        assert_eq!(small.resolution.width(), MINIMAL_SIZE.0 as f32);
        assert_eq!(
            small.resize_constraints.min_height,
            least(Layout::Minimal).min_height
        );

        set_layout(app.world_mut(), Layout::Full);
        app.update();
        assert!(shown(&app));
        assert_eq!(window(&mut app).resolution.size(), full);
    }

    #[test]
    fn a_hidden_window_keeps_its_layout_and_is_shown_on_request() {
        let (mut app, _dir) = app("window-hidden");
        set_layout(app.world_mut(), Layout::Minimal);
        app.update();
        background(app.world_mut());
        app.update();
        assert!(!window(&mut app).visible);
        assert_eq!(
            app.world().resource::<WinitSettings>().focused_mode,
            winit_settings(true).focused_mode
        );

        let ask = |app: &mut App| {
            app.world_mut()
                .resource_mut::<NextState<Showing>>()
                .set(Showing::Shown);
            app.update();
        };
        ask(&mut app);
        let back = window(&mut app);
        assert!(back.visible && back.focused);
        assert_eq!(
            *app.world().resource::<State<Layout>>().get(),
            Layout::Minimal
        );
        assert_eq!(
            app.world().resource::<WinitSettings>().focused_mode,
            winit_settings(false).focused_mode
        );

        // A second launch asks for a window that is already shown:
        // it takes the focus again.
        let world = app.world_mut();
        world
            .query_filtered::<&mut Window, With<PrimaryWindow>>()
            .single_mut(world)
            .unwrap()
            .focused = false;
        ask(&mut app);
        assert!(window(&mut app).focused);
    }
}
