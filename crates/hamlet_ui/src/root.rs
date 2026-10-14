//! The root of the window: the top bar, the dock with the first-run
//! screen over it, the foot, and what floats over them.

use bevy::app::{App, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::state::state::State;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy_fynix::dock::dock;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_core::core::{Settings, TestConnection};

use crate::looks::{Fills, filling, hairline};
use crate::platform::Glow;
use crate::status::status_bar;
use crate::theme::{BLACK, Lamplight, SEAM};
use crate::window::{self, Layout};
use crate::{Build, Ui, chrome, hover, menu, welcome, world_menu};

pub(crate) fn plugin(app: &mut App) {
    app.add_systems(Startup, open);
}

/// Mounts the views, as one tab group so Tab moves through the
/// controls.
pub(crate) fn mount(world: &mut World) {
    let node = bevy_fynix::mount::<Lamplight>(world, root());
    world.entity_mut(node).insert(TabGroup::default());
}

fn root() -> Ui {
    Ui::new(|cx: &mut Build| {
        cx.build(
            column((
                keyed(
                    resource::<State<Layout>, _>(|s| *s.get()),
                    |layout| match layout {
                        Layout::Full => full().boxed(),
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
            .fill(Lamplight::background())
            .gap(0.0),
        )
    })
}

fn full() -> Ui {
    column((
        toolbar(),
        column((dock::<Lamplight>(), welcome::view()))
            .fills_height()
            .gap(0.0),
        status_bar(),
    ))
    .fills_height()
    .gap(0.0)
    .boxed()
}

/// The bar across the top: the menus, on a surface that warms
/// toward its foot.
pub fn toolbar() -> Ui {
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
        .fill(BLACK)
        .tagged(Glow)
        .tagged(chrome::Grip),
        hairline(SEAM),
    ))
    .gap(0.0)
    .boxed()
}

/// The window's own camera, and a first test of the connection.
fn open(world: &mut World) {
    // The panels belong to the window's camera and the world to its
    // own.
    world.spawn((Camera2d, IsDefaultUiCamera));
    let config = world.resource::<Settings>().config.clone();
    world.trigger(TestConnection(config));
}
