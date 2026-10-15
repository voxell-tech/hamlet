//! The full window, the small status window and the size kept between
//! launches.

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::{App, AppExit, Update};
use bevy::ecs::message::{MessageReader, MessageWriter};
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
use hamlet_client::ViewDir;

use crate::chrome::{self, Grip};
use crate::icons::Glyph;
use crate::looks::{bad_action, good_action};
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
        .add_computed_state::<WorldShown>()
        .insert_resource(winit_settings())
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
        .add_systems(Update, (intercept_close, remember_size));
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

/// The world is drawn while the window is full.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct WorldShown;

impl ComputedStates for WorldShown {
    type SourceStates = Layout;

    fn compute(layout: Layout) -> Option<Self> {
        (layout == Layout::Full).then_some(WorldShown)
    }
}

/// The size the full window goes back to.
#[derive(Resource)]
pub(crate) struct FullSize(pub (u32, u32));

fn size_file(view: &Path) -> PathBuf {
    view.join("window")
}

/// The plugin of the window, at the size it was last left at, for
/// Bevy's default plugins to be set with.
pub fn window_plugin(view: &Path) -> WindowPlugin {
    let size = saved_size(view);
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
pub(crate) fn saved_size(view: &Path) -> (u32, u32) {
    let parse = |text: String| {
        let (w, h) = text.trim().split_once('x')?;
        Some((
            w.parse::<u32>().ok()?.max(LEAST_FULL.0),
            h.parse::<u32>().ok()?.max(LEAST_FULL.1),
        ))
    };
    std::fs::read_to_string(size_file(view))
        .ok()
        .and_then(parse)
        .unwrap_or(DEFAULT_SIZE)
}

/// Remembers the full window's size, written once the resizing has
/// settled.
pub(crate) fn remember_size(
    time: Res<Time>,
    view: Res<ViewDir>,
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
    // Frequent enough to stream replies; input wakes it sooner.
    WinitSettings {
        focused_mode: UpdateMode::reactive(Duration::from_millis(50)),
        unfocused_mode: UpdateMode::reactive_low_power(
            Duration::from_millis(250),
        ),
    }
}

fn intercept_close(
    mut closes: MessageReader<WindowCloseRequested>,
    mut exit: MessageWriter<AppExit>,
) {
    if closes.read().count() > 0 {
        exit.write(AppExit::Success);
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
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut winit: ResMut<WinitSettings>,
) {
    window.resize_constraints = least(*layout.get());
    *winit = winit_settings();
    window.focused = true;
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

pub(crate) fn set_layout(
    world: &mut bevy::ecs::world::World,
    layout: Layout,
) {
    world.resource_mut::<NextState<Layout>>().set(layout);
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
                    frame().grow(1.0),
                    bad_action(Glyph::Power, "CLOSE", quit),
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
    use hamlet_client::ViewDir;
    use hamlet_testing::Scratch;

    use super::*;

    fn app(tag: &str) -> (App, Scratch) {
        let dir = Scratch::new(tag);
        let mut app = App::new();
        app.add_plugins(StatesPlugin)
            .add_message::<WindowCloseRequested>()
            .add_message::<WindowResized>()
            .init_resource::<Time>()
            .insert_resource(ViewDir(dir.join("view")));
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
    fn the_small_window_offers_expand_and_close_only() {
        use bevy_fynix::FynixPlugin;
        use hamlet_assets::Tuning;

        use crate::me::tests::{press, words};
        use crate::theme::HamletTheme;

        let (mut app, _dir) = app("window-minimal-actions");
        app.add_plugins(FynixPlugin::<HamletTheme>::default())
            .init_resource::<Bar>()
            .init_resource::<Lately>();
        crate::theme::insert_theme(
            app.world_mut(),
            HamletTheme::designed(),
        );
        set_layout(app.world_mut(), Layout::Minimal);
        app.update();
        bevy_fynix::mount::<HamletTheme>(app.world_mut(), minimal());
        app.update();
        let shown = words(&mut app);
        assert!(
            shown.iter().any(|word| word == "EXPAND"),
            "{shown:?}"
        );
        assert!(
            shown.iter().any(|word| word == "CLOSE"),
            "{shown:?}"
        );
        assert!(
            !shown.iter().any(|word| word == "RUN IN BACKGROUND"
                || word == "QUIT HAMLET")
        );
        press(&mut app, "EXPAND");
        app.update();
        assert_eq!(
            *app.world().resource::<State<Layout>>().get(),
            Layout::Full
        );
        set_layout(app.world_mut(), Layout::Minimal);
        app.update();
        press(&mut app, "CLOSE");
        assert_eq!(app.should_exit(), Some(AppExit::Success));
    }

    #[test]
    fn closing_either_layout_ends_only_the_window() {
        for layout in [Layout::Full, Layout::Minimal] {
            let (mut app, _dir) = app("window-close");
            set_layout(app.world_mut(), layout);
            app.update();
            let primary = app.world_mut()
                .query_filtered::<bevy::ecs::entity::Entity, With<PrimaryWindow>>()
                .single(app.world()).unwrap();
            app.world_mut().write_message(WindowCloseRequested {
                window: primary,
            });
            app.update();
            assert_eq!(app.should_exit(), Some(AppExit::Success));
        }
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
        relaunched.add_plugins(window_plugin(&dir.join("view")));
        let window = window(&mut relaunched);
        assert_eq!(window.resolution.width(), 1420.0);
        assert_eq!(window.resolution.height(), 810.0);
    }
}
