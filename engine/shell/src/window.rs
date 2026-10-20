//! The window and the root everything in it is built under.

use bevy::app::{App, AppExit, First, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::entity::Entity;
use bevy::ecs::message::MessageWriter;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::input::keyboard::KeyCode;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy::window::{
    Window, WindowPlugin, WindowResizeConstraints, WindowResolution,
};
use bevy_fynix::dock::dock;
use bevy_fynix::shortcut::{Chord, Mods};
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use hamlet_engine_core::tuning::{ENGINE, Tuning, TuningFile};
use serde::Deserialize;

use crate::command::{RunCommand, engine_id, spawn_command};
use crate::style::Shade;
use crate::theme::{EngineTheme, ThemeChanged};
use crate::{AnyView, Cx, View, menu, status, switcher};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, (open, add_quit))
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
        ..WindowPlugin::default()
    }
}

/// The root the views are built under.
#[derive(Resource)]
struct Root(Entity);

fn open(world: &mut World) {
    world.spawn((Camera2d, IsDefaultUiCamera));
    mount(world);
}

fn mount(world: &mut World) {
    let root = bevy_fynix::mount::<EngineTheme>(world, root());
    world.insert_resource(Root(root));
}

/// Builds the views again for a new theme, which each read as it was
/// built.
fn rebuild(world: &mut World) {
    let Some(Root(root)) = world.remove_resource::<Root>() else {
        return;
    };
    world.despawn(root);
    mount(world);
}

/// Quit, in the Hamlet menu and on the primary key with Q.
fn add_quit(mut commands: Commands) {
    spawn_command(
        &mut commands,
        engine_id("quit"),
        "Quit",
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

fn root() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        let theme = cx.theme();
        let (background, seam, line) = (
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
                frame().height(px(line)).fill(seam),
                status::view(),
                menu::open_view(),
                switcher::rename_view(),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .fill(background)
            .gap(0.0),
        )
    })
}

/// The bar across the top, the menus then the workspaces, on a
/// surface that shades toward its bottom, over a seam.
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
        .tagged(Shade),
        frame()
            .height(px(theme.space.divider))
            .fill(*colors.separator),
    ))
    .gap(0.0)
}
