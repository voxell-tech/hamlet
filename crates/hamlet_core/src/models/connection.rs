//! The check that the default model answers, made as the window opens
//! and again when the chief asks.

use bevy::app::App;
use bevy::ecs::observer::On;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::ResMut;
use bevy::ecs::world::World;
use bevy::ui::{AlignItems, UiRect, px};
use bevy_fynix::resource;
use bevy_fynix::views::{FrameProps, row};
use hamlet_client::messages::Response;
use hamlet_client::outbox::Outbox;
use hamlet_protocol::client as protocol;
use hamlet_ui::styles::{action, note};
use hamlet_ui::widgets::icons::Glyph;
use hamlet_ui::{View, built};

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<ConnectionTest>().add_observer(checked);
}

#[derive(Resource, Default)]
struct ConnectionTest {
    request: Option<protocol::RequestId>,
    words: String,
}

/// Asks the server whether the default model answers.
pub fn test_connection(world: &mut World) {
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
