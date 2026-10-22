//! The chat: the open conversation of the window's focus as one line
//! of time with every session its agent is a party to, the steps of
//! each turn folded, and the bar to write in. It follows the focus
//! the agents service keeps, so whoever another panel opens a
//! conversation with is who it shows.

mod bar;
mod bubble;
mod reply;
mod talk;
mod timeline;

use agents_ui::{
    AgentsWorldExt, CONFIRM, Focused, Heard, confirmation, start,
};
use hamlet_engine_bevy::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::{
    AnyView, FrameProps, Space, ViewExt, column,
};

use crate::bar::{Draft, Replying, message_bar};
use crate::talk::{Talk, follow, heard};
use crate::timeline::{Lines, Reach, Unfolded, refresh};

/// The name of the chat's panel.
const CHAT: &str = "chat";

#[derive(Default)]
struct Chat;

impl Plugin for Chat {
    fn build(&self, app: &mut App) {
        app.add_plugins(agents_ui::plugin)
            .init_resource::<Talk>()
            .init_saved_resource::<Draft>()
            .init_saved_resource::<Replying>()
            .init_resource::<Unfolded>()
            .init_resource::<Reach>()
            .init_resource::<Lines>()
            .add_panel(CHAT, "Chat", chat)
            .add_overlay(CONFIRM, confirmation)
            .add_systems(
                Declare,
                start(&[
                    AgentsRequest::Focus,
                    AgentsRequest::Roster,
                    AgentsRequest::Needs,
                ]),
            )
            .add_systems(Update, receive);
    }
}

hamlet_engine_bevy::export!(Chat);

fn chat(world: &mut World) -> AnyView {
    follow(world);
    refresh(world);
    column((bubble::header(), bubble::transcript(), message_bar()))
        .space(Space::Gap)
        .boxed()
}

fn receive(
    world: &mut World,
    mut cursor: Local<MessageCursor<Heard>>,
) {
    for told in world.heard(&mut cursor) {
        match &told {
            Heard::Reply(_, AgentsReply::Focus(_))
            | Heard::Event(AgentsEvent::Focus(_)) => follow(world),
            Heard::Failed(Some(request), why) => {
                hamlet_engine_bevy::warn(&format!(
                    "{request:?}: {why}"
                ));
            }
            _ => {}
        }
        heard(world, &told);
        if world.resource::<Focused>().0.is_some() {
            refresh(world);
        }
    }
}
