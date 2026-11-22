//! The root of the window: the top bar, the dock and the foot.

use bevy::app::{App, First, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::system::Commands;
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy_fynix::dock::dock;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_ui::styles::{Fills, hairline};
use hamlet_ui::theme::{HamletTheme, ThemeChanged};
use hamlet_ui::widgets::platform::Glow;
use hamlet_ui::{Cx, View, built};

use crate::{chrome, desk, hover, menu, welcome, world_menu};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(
        Startup,
        (open, hamlet_core::models::test_connection),
    )
    // Before anything queues work on the panels it despawns.
    .add_systems(First, rebuild.run_if(on_message::<ThemeChanged>));
}

/// The root the panels are mounted on.
#[derive(Resource)]
struct Panels(Entity);

/// Mounts the views as one tab group.
pub(crate) fn mount(world: &mut World) {
    let root = hamlet_ui::rules::mount(world, root());
    world.entity_mut(root).insert(TabGroup::default());
    world.insert_resource(Panels(root));
}

/// Builds the panels again for a new theme.
fn rebuild(world: &mut World) {
    let Some(Panels(root)) = world.remove_resource::<Panels>() else {
        return;
    };
    world.despawn(root);
    mount(world);
}

fn root() -> impl View {
    built(|cx| {
        cx.build(
            column((
                layout(cx),
                crate::toasts::stack(),
                menu::open_menu(),
                crate::screens::terminal::open_menu(),
                crate::select::quote_button(),
                crate::screens::viewer::view(),
                desk::popups(),
                world_menu::view(),
                crate::saving::screen(),
                hover::view(),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .min_width(px(0.0))
            .min_height(px(0.0))
            .fill(cx.theme().background())
            .gap(0.0),
        )
    })
}

/// The window's bars and dock, with first-run setup over them.
pub fn layout(cx: &Cx) -> impl View {
    column((
        toolbar(cx),
        // First run covers the panels but not the window's buttons.
        column((
            dock::<HamletTheme>(),
            welcome::view(),
            welcome::projects::view(),
        ))
        .fills_height()
        .gap(0.0),
    ))
    .fills_height()
    .gap(0.0)
}

/// The bar across the top: the menus and the desks.
fn toolbar(cx: &Cx) -> impl View {
    let palette = &cx.theme().palette;
    column((
        row((
            chrome::system_room(),
            menu::titles(),
            // No desk under first-run setup; adding models keeps it.
            keyed(
                resource::<welcome::Welcome, _>(|welcome| {
                    welcome.is_shown() && !welcome.adding
                }),
                |shown| {
                    if *shown {
                        frame().boxed()
                    } else {
                        desk::switcher().boxed()
                    }
                },
            )
            .within(
                row(())
                    .gap(0.0)
                    .min_width(px(0.0))
                    .shrink(1.0)
                    .height(percent(100.0)),
            ),
            // The rest of the bar is where the window is held.
            frame()
                .grow(1.0)
                .height(percent(100.0))
                .tagged(chrome::Grip),
            chrome::buttons(),
        ))
        .gap(0.0)
        .align(AlignItems::Center)
        .padding(UiRect::axes(
            px(cx.theme().space.bar_padding),
            px(0.0),
        ))
        .fill(palette.surface)
        .tagged(Glow)
        .tagged(chrome::Grip),
        hairline(palette.border),
    ))
    .gap(0.0)
}

/// The window's own camera.
fn open(mut commands: Commands) {
    commands.spawn((Camera2d, IsDefaultUiCamera));
}
