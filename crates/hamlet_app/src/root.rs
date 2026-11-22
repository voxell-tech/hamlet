//! The root of the window: the top bar, the dock and the foot.

use bevy::app::{App, First, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::entity::Entity;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::ui::{AlignItems, IsDefaultUiCamera, UiRect, percent, px};
use bevy_fynix::dock::dock;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use bevy_fynix::{ViewExt, keyed, resource};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{Fills, action, hairline, note};
use hamlet_ui::theme::{HamletTheme, ThemeChanged};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Glow;
use hamlet_ui::{Cx, View, built};

use crate::{chrome, desk, hover, menu, welcome, world_menu};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<ConnectionTest>()
        .add_observer(checked)
        .add_systems(Startup, open)
        // Before anything queues work on the panels it despawns.
        .add_systems(
            First,
            rebuild.run_if(on_message::<ThemeChanged>),
        );
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

/// The window's own camera, and a first test of the connection.
fn open(world: &mut World) {
    world.spawn((Camera2d, IsDefaultUiCamera));
    test_connection(world);
}

#[derive(Resource, Default)]
struct ConnectionTest {
    request: Option<protocol::RequestId>,
    words: String,
}

fn test_connection(world: &mut World) {
    let id = world.resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Check(protocol::Check::Connection {
            id,
        })
    });
    let mut test = world.resource_mut::<ConnectionTest>();
    test.request = Some(id);
    test.words.clear();
}

fn checked(response: On<Response>, mut test: ResMut<ConnectionTest>) {
    if test.request.as_ref() != Some(&response.to) {
        return;
    }
    match &response.message {
        protocol::ServerMessage::Reply(reply) => {
            test.words.clone_from(&reply.text);
        }
        protocol::ServerMessage::Fault(fault) => {
            test.words.clone_from(&fault.text);
        }
        _ => return,
    }
    test.request = None;
}

/// The default model's connection check and its received answer.
pub fn connection_test() -> impl View {
    built(|cx| {
        cx.world.init_resource::<ConnectionTest>();
        cx.build(
            row((
                action(
                    Glyph::Signal,
                    "Test Connection",
                    test_connection,
                ),
                note(resource::<ConnectionTest, _>(|test| {
                    test.words.clone()
                })),
            ))
            .gap(12.0)
            .align(AlignItems::Center)
            .padding(UiRect::axes(px(16.0), px(4.0))),
        )
    })
}
