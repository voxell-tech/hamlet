//! The root of the window: the top bar, the dock with the first-run
//! screen over it, the foot, and what floats over them.

use bevy::app::{App, First, Last, Startup};
use bevy::camera::Camera2d;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::ecs::schedule::common_conditions::on_message;
use bevy::ecs::system::{Commands, Query, Res, ResMut};
use bevy::ecs::world::World;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::ui::{
    AlignItems, IsDefaultUiCamera, ScrollPosition, UiRect, percent,
    px,
};
use bevy_fynix::dock::dock;
use bevy_fynix::resource;
use bevy_fynix::views::{
    BehaviorExt, FrameProps, column, frame, row,
};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{Fills, action, hairline, note};
use hamlet_ui::theme::{HamletTheme, ThemeChanged};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::widgets::platform::Glow;
use hamlet_ui::{Cx, View, built};

use crate::status::status_bar;
use crate::{chrome, desk, hover, menu, welcome, world_menu};

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<ConnectionTest>()
        .init_resource::<Lasted>()
        .add_observer(checked)
        .add_observer(outlast)
        .add_observer(last_on)
        .add_systems(Startup, open)
        // Before anything this frame queues work on the panels it
        // would despawn.
        .add_systems(
            First,
            rebuild.run_if(on_message::<ThemeChanged>),
        )
        .add_systems(Last, forget);
}

/// A node whose scroll and keyboard focus pass to the node of the
/// same key built in its place in the same frame.
#[derive(Component, Clone, PartialEq, Eq, Debug)]
pub(crate) struct Lasting(pub String);

/// The scrolls and the focus of [`Lasting`] nodes despawned this
/// frame, by key.
#[derive(Resource, Default)]
struct Lasted {
    scrolls: Vec<(String, ScrollPosition)>,
    focus: Option<String>,
}

fn outlast(
    gone: On<Remove<Lasting>>,
    nodes: Query<(&Lasting, Option<&ScrollPosition>)>,
    focus: Res<InputFocus>,
    mut lasted: ResMut<Lasted>,
) {
    let Ok((lasting, scroll)) = nodes.get(gone.entity) else {
        return;
    };
    if let Some(scroll) = scroll {
        lasted.scrolls.push((lasting.0.clone(), scroll.clone()));
    }
    if focus.get() == Some(gone.entity) {
        lasted.focus = Some(lasting.0.clone());
    }
}

fn last_on(
    built: On<Add<Lasting>>,
    nodes: Query<&Lasting>,
    mut lasted: ResMut<Lasted>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    let Ok(Lasting(key)) = nodes.get(built.entity) else {
        return;
    };
    if let Some(at) =
        lasted.scrolls.iter().position(|(kept, _)| kept == key)
    {
        let (_, scroll) = lasted.scrolls.swap_remove(at);
        commands.entity(built.entity).insert(scroll);
    }
    if lasted.focus.as_ref() == Some(key) {
        lasted.focus = None;
        focus.set(built.entity, FocusCause::Auto);
    }
}

/// A node not built again by the end of its frame is gone for good.
fn forget(mut lasted: ResMut<Lasted>) {
    if !lasted.scrolls.is_empty() || lasted.focus.is_some() {
        *lasted = Lasted::default();
    }
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

fn root() -> impl View {
    built(|cx| {
        cx.build(
            column((
                layout(cx),
                crate::toasts::stack(),
                menu::open_menu(),
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
        // First run covers the status bar too, and leaves the
        // toolbar lit for the window's own buttons.
        column((
            dock::<HamletTheme>(),
            status_bar(),
            welcome::view(),
        ))
        .fills_height()
        .gap(0.0),
    ))
    .fills_height()
    .gap(0.0)
}

/// The bar across the top: the menus and the desks, on a surface
/// that warms toward its foot.
fn toolbar(cx: &Cx) -> impl View {
    let palette = &cx.theme().palette;
    column((
        row((
            menu::titles(),
            desk::switcher(),
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
    // The panels belong to the window's camera and the world to its
    // own.
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
                    "Test connection",
                    test_connection,
                ),
                note(resource::<ConnectionTest, _>(|test| {
                    test.words.clone()
                })),
            ))
            .gap(12.0)
            .align(AlignItems::Center)
            .padding(UiRect::axes(px(16.0), px(5.0))),
        )
    })
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;
    use hamlet_client::outbox::Outbox;
    use hamlet_protocol::client as protocol;

    use super::*;

    #[test]
    fn a_lasting_node_built_again_keeps_its_scroll_and_focus() {
        let mut app = crate::testing::panel_app();
        app.add_plugins(plugin);
        let lasting = || Lasting("page".into());
        let world = app.world_mut();
        let old = world
            .spawn((lasting(), ScrollPosition(Vec2::new(0.0, 240.0))))
            .id();
        world
            .resource_mut::<InputFocus>()
            .set(old, FocusCause::Navigated);
        world.despawn(old);
        let new =
            world.spawn((lasting(), ScrollPosition::default())).id();
        world.flush();

        assert_eq!(
            world.get::<ScrollPosition>(new).unwrap().y,
            240.0
        );
        assert_eq!(world.resource::<InputFocus>().get(), Some(new));
    }

    #[test]
    fn opening_the_window_checks_the_servers_connection() {
        let mut app = crate::testing::panel_app();
        app.add_plugins(plugin);
        app.update();
        assert!(app.world().resource::<Outbox>().iter().any(
            |message| matches!(
                message,
                protocol::ClientMessage::Check(
                    protocol::Check::Connection { .. }
                )
            )
        ));
    }
    #[test]
    fn the_connection_test_button_sends_a_check_and_shows_only_its_reply()
     {
        let mut app = crate::testing::panel_app();
        app.add_plugins(plugin);
        app.update();
        app.world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .for_each(drop);
        bevy_fynix::mount::<HamletTheme>(
            app.world_mut(),
            crate::models::endpoints(),
        );
        app.update();
        crate::testing::press(&mut app, "Test connection");
        let sent = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .collect::<Vec<_>>();
        let [
            protocol::ClientMessage::Check(
                protocol::Check::Connection { id },
            ),
        ] = &sent[..]
        else {
            panic!("{sent:?}");
        };
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: protocol::RequestId::new(),
                kind: protocol::ReplyKind::Notice,
                text: "Not this test".into(),
                upload: None,
            }),
        );
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Reply(protocol::Reply {
                to: id.clone(),
                kind: protocol::ReplyKind::Notice,
                text: "Reached the default model.".into(),
                upload: None,
            }),
        );
        app.update();
        let shown = crate::testing::words(&mut app);
        assert!(
            shown
                .iter()
                .any(|text| text == "Reached the default model."),
            "{shown:?}"
        );
        assert!(!shown.iter().any(|text| text == "Not this test"));
        crate::testing::press(&mut app, "Test connection");
        let id = app
            .world_mut()
            .resource_mut::<Outbox>()
            .drain()
            .find_map(|message| match message {
                protocol::ClientMessage::Check(
                    protocol::Check::Connection { id },
                ) => Some(id),
                _ => None,
            })
            .unwrap();
        hamlet_client::receive(
            app.world_mut(),
            protocol::ServerMessage::Fault(protocol::Fault {
                to: Some(id),
                text: "No model answered.".into(),
            }),
        );
        app.update();
        assert!(
            crate::testing::words(&mut app)
                .iter()
                .any(|text| text == "No model answered.")
        );
    }
}
