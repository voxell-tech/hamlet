//! The window and the root everything in it is built under.

use std::time::Duration;

use bevy::app::{App, AppExit, First, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::entity::Entity;
use bevy::ecs::event::Event;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy::window::{
    ExitCondition, PrimaryWindow, Window, WindowPlugin,
    WindowResizeConstraints, WindowResolution,
};
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_fynix::dock::dock;
use bevy_fynix::shortcut::{Chord, Mods};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use hamlet_engine_core::TuningPlugin;
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use serde::Deserialize;

use crate::command::{RunCommand, ShellCommandsExt, engine_id};
use crate::style::BottomGradient;
use crate::theme::{EngineTheme, ThemeChanged};
use crate::{
    AnyView, Cx, View, float, menu, overlay, status, switcher,
};

pub(crate) fn plugin(app: &mut App) {
    app.add_plugins(TuningPlugin::<WindowTuning>::default())
        .add_observer(show)
        .add_systems(Startup, (open, add_commands))
        // Before anything this frame queues work on the views it
        // would despawn.
        .add_systems(
            First,
            rebuild.run_if(on_message::<ThemeChanged>),
        );
}

/// The window's first size and its title.
#[derive(Resource, Clone, PartialEq, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WindowTuning {
    pub title: String,
    /// The size it opens at, in physical pixels.
    pub width: u32,
    pub height: u32,
    /// The least size it can be made, in logical pixels.
    pub least_width: u32,
    pub least_height: u32,
    /// Seconds between two frames while the window is hidden.
    pub hidden_wait: f32,
}

impl Tuning for WindowTuning {
    const FILE: TuningFile = ENGINE;
    const SECTION: Option<&'static str> = Some("window");
}

/// The window, for Bevy's default plugins to be set with.
pub fn window_plugin() -> WindowPlugin {
    let tuning = WindowTuning::designed();
    WindowPlugin {
        primary_window: Some(Window {
            title: tuning.title,
            resolution: WindowResolution::new(
                tuning.width,
                tuning.height,
            ),
            resize_constraints: WindowResizeConstraints {
                min_width: tuning.least_width as f32,
                min_height: tuning.least_height as f32,
                ..WindowResizeConstraints::default()
            },
            ..Window::default()
        }),
        // Closing the window quits; Run in Background hides it
        // instead.
        exit_condition: ExitCondition::OnPrimaryClosed,
        close_when_requested: true,
        ..WindowPlugin::default()
    }
}

/// Shows the window again, if it was hidden, and gives it the focus.
#[derive(Event, Clone, Copy, Debug)]
pub struct ShowWindow;

/// While the window is hidden: how the app was updated before.
#[derive(Resource)]
struct Hidden(WinitSettings);

/// The root the views are built under.
#[derive(Resource)]
pub(crate) struct Root(pub(crate) Entity);

fn open(world: &mut World) {
    world.spawn((Camera2d, IsDefaultUiCamera));
    let root = bevy_fynix::mount::<EngineTheme>(world, root());
    world.insert_resource(Root(root));
}

/// Builds the views again for a new theme, which each read as it was
/// built.
fn rebuild(world: &mut World) {
    let Some(Root(old)) = world.remove_resource::<Root>() else {
        return;
    };
    world.despawn(old);
    let root = bevy_fynix::mount::<EngineTheme>(world, root());
    world.insert_resource(Root(root));
}

/// Run in Background and Quit, in the Hamlet menu, Quit on the
/// primary key with Q.
fn add_commands(mut commands: Commands) {
    commands
        .spawn_command(
            engine_id("background"),
            "Run in Background",
            Some("Hamlet"),
            &[],
        )
        .observe(|_: On<RunCommand>, mut commands: Commands| {
            commands.run_system_cached(hide);
        });
    commands
        .spawn_command(
            engine_id("quit"),
            "Quit Hamlet",
            Some("Hamlet"),
            &[Chord {
                key: KeyCode::KeyQ,
                mods: Mods::PRIMARY,
            }],
        )
        .observe(
            |_: On<RunCommand>, mut exit: MessageWriter<AppExit>| {
                exit.write(AppExit::Success);
            },
        );
}

/// Hides the window and updates the engine seldom; it runs on in the
/// background.
fn hide(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    winit: Option<ResMut<WinitSettings>>,
    hidden: Option<Res<Hidden>>,
    tuning: Res<WindowTuning>,
    mut commands: Commands,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.visible = false;
    if let Some(mut winit) = winit
        && hidden.is_none()
    {
        commands.insert_resource(Hidden(winit.clone()));
        let wait = Duration::from_secs_f32(tuning.hidden_wait);
        *winit = WinitSettings {
            focused_mode: UpdateMode::reactive_low_power(wait),
            unfocused_mode: UpdateMode::reactive_low_power(wait),
        };
    }
}

fn show(
    _: On<ShowWindow>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    winit: Option<ResMut<WinitSettings>>,
    hidden: Option<Res<Hidden>>,
    mut commands: Commands,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    window.visible = true;
    window.focused = true;
    if let (Some(mut winit), Some(hidden)) = (winit, hidden) {
        *winit = hidden.0.clone();
        commands.remove_resource::<Hidden>();
    }
}

fn root() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let (background, separator, line) = (
            *theme.colors.background,
            *theme.colors.separator,
            theme.space.divider,
        );
        cx.build(
            column((
                bar(cx),
                column((dock::<EngineTheme>(),))
                    .grow(1.0)
                    .min_height(px(0.0))
                    .gap(0.0),
                frame().height(px(line)).fill(separator),
                status::view(),
                float::view(),
                overlay::view(),
                menu::open_view(),
                switcher::popup_view(),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .fill(background)
            .gap(0.0),
        )
    })
}

/// The bar across the top, the menus then the workspaces, on a
/// surface that shades toward its bottom, over a separator.
fn bar(cx: &Cx) -> impl View {
    let theme = cx.theme();
    let colors = &theme.colors;
    column((
        row((
            menu::view(),
            frame().width(px(theme.space.section)),
            switcher::view(),
        ))
        .height(px(theme.space.bar))
        .align(AlignItems::Stretch)
        .padding(UiRect::axes(px(theme.space.bar_padding), px(0.0)))
        .fill(*colors.background)
        .tagged(BottomGradient),
        frame()
            .height(px(theme.space.divider))
            .fill(*colors.separator),
    ))
    .gap(0.0)
}
