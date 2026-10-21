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
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest,
};
use hamlet_engine_sdk::ui::{
    AnyView, FrameProps, Space, ViewExt, column,
};
use hamlet_engine_sdk::{
    Entry, Message, Plugin, World, decode, encode,
};

use self::bar::{Draft, Replying, message_bar};
use self::reply::Answers;
use self::talk::{Talk, follow, heard};
use self::timeline::{Lines, Reach, Unfolded, refresh};

/// The name of the chat's panel.
const CHAT: &str = "chat";

/// What is typed and not yet sent, kept across a reload: the draft,
/// and the speaker and words it replies to.
type Kept = (Option<String>, Option<(String, String)>);

#[derive(Default)]
struct Chat;

/// Puts in what the chat's views read.
fn prepare(world: &mut World) {
    world.init_agents();
    world.init_resource::<Talk>();
    world.init_resource::<Draft>();
    world.init_resource::<Replying>();
    world.init_resource::<Unfolded>();
    world.init_resource::<Reach>();
    world.init_resource::<Lines>();
}

impl Plugin for Chat {
    fn declare(&self, entry: &mut Entry) {
        entry.panel(CHAT, "Chat").overlay(CONFIRM);
        start(&[
            AgentsRequest::Focus,
            AgentsRequest::Roster,
            AgentsRequest::Needs,
        ]);
    }

    fn mount(&mut self, name: &str, world: &mut World) -> AnyView {
        prepare(world);
        if name == CONFIRM {
            return confirmation();
        }
        follow(world);
        refresh(world);
        column((
            bubble::header(),
            bubble::transcript(),
            message_bar(),
        ))
        .space(Space::Gap)
        .boxed()
    }

    fn receive(&mut self, message: Message, world: &mut World) {
        prepare(world);
        let Some(told) = world.hear(message) else {
            return;
        };
        match &told {
            Heard::Reply(_, AgentsReply::Focus(_))
            | Heard::Event(AgentsEvent::Focus(_)) => follow(world),
            Heard::Failed(Some(request), why) => {
                hamlet_engine_sdk::warn(&format!(
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

    fn save(&self, world: &World) -> Vec<u8> {
        let draft = world.get::<Draft>().map(|draft| draft.0.clone());
        let replying = world
            .get::<Replying>()
            .and_then(|replying| replying.0.clone())
            .map(|answers| (answers.by, answers.text));
        encode(&(draft, replying)).unwrap_or_default()
    }

    fn restore(&mut self, state: &[u8], world: &mut World) {
        let Ok((draft, replying)) = decode::<Kept>(state) else {
            return;
        };
        world.insert(Draft(draft.unwrap_or_default()));
        world.insert(Replying(
            replying.map(|(by, text)| Answers { by, text }),
        ));
    }
}

hamlet_engine_sdk::export!(Chat);
