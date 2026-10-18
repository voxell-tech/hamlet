//! The root of the window: the top bar, the dock with the first-run
//! screen over it, the foot, and what floats over them.

use bevy::app::{App, First, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::state::state::State;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy_fynix::dock::dock;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{keyed, resource};
use hamlet_core::core::{Settings, TestConnection};

use crate::looks::{Fills, filling, hairline};
use crate::platform::Glow;
use crate::status::status_bar;
use crate::theme::{HamletTheme, ThemeChanged};
use crate::window::{self, Layout};
use crate::{
    AnyView, Cx, View, chrome, hover, menu, welcome, world_menu,
};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, open)
        // Before anything this frame queues work on the panels it
        // would despawn.
        .add_systems(
            First,
            rebuild.run_if(on_message::<ThemeChanged>),
        );
}

/// The root the panels are mounted on.
#[derive(Resource)]
struct Panels(Entity);

/// Mounts the views, as one tab group so Tab moves through the
/// controls.
pub(crate) fn mount(world: &mut World) {
    let root = bevy_fynix::mount::<HamletTheme>(world, root());
    world.entity_mut(root).insert(TabGroup::default());
    world.insert_resource(Panels(root));
}

/// Builds the panels again for a new theme, which each read as it
/// was built.
fn rebuild(world: &mut World) {
    let Some(Panels(root)) = world.remove_resource::<Panels>() else {
        return;
    };
    world.despawn(root);
    mount(world);
}

fn root() -> AnyView {
    AnyView::new(|cx: &mut Cx| {
        cx.build(
            column((
                keyed(
                    resource::<State<Layout>, _>(|s| *s.get()),
                    |layout| match layout {
                        Layout::Full => {
                            AnyView::new(|cx: &mut Cx| {
                                let full = full(cx);
                                cx.build(full)
                            })
                        }
                        Layout::Minimal => window::minimal(),
                    },
                )
                .within(filling()),
                menu::open_menu(),
                world_menu::view(),
                hover::view(),
            ))
            .width(percent(100.0))
            .height(percent(100.0))
            .fill(cx.theme().background())
            .gap(0.0),
        )
    })
}

fn full(cx: &Cx) -> impl View {
    column((
        toolbar(cx),
        column((dock::<HamletTheme>(), welcome::view()))
            .fills_height()
            .gap(0.0),
        status_bar(),
    ))
    .fills_height()
    .gap(0.0)
}

/// The bar across the top: the menus, on a surface that warms
/// toward its foot.
pub fn toolbar(cx: &Cx) -> impl View {
    let palette = &cx.theme().palette;
    column((
        row((
            menu::titles(),
            // The rest of the bar is where the window is held.
            frame()
                .grow(1.0)
                .height(percent(100.0))
                .tagged(chrome::Grip),
            chrome::buttons(),
        ))
        .gap(0.0)
        .align(AlignItems::Center)
        .padding(UiRect::axes(px(8.0), px(0.0)))
        .fill(palette.black)
        .tagged(Glow)
        .tagged(chrome::Grip),
        hairline(palette.seam),
    ))
    .gap(0.0)
}

/// The window's own camera, and a first test of the connection.
fn open(world: &mut World) {
    // The panels belong to the window's camera and the world to its
    // own.
    world.spawn((Camera2d, IsDefaultUiCamera));
    let config = world.resource::<Settings>().config.clone();
    world.trigger(TestConnection(config));
}
