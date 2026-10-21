//! The open conversation as the agents service tells it: the sessions
//! its chat shows and their messages, asked for when the focus moves
//! and kept in step as they come or change.

use std::collections::HashMap;

use agents_ui::{AgentsWorldExt, Focused, Heard};
use hamlet_engine_sdk::World;
use hamlet_engine_sdk::api::agents::{
    AgentsEvent, AgentsReply, AgentsRequest, Related, SessionMessage,
    SessionName,
};

use crate::bar::{Draft, Replying};
use crate::timeline::{Reach, Unfolded};

/// The most messages of a session asked for at once.
const MOST: u32 = 200;

/// The chat's sessions and what was said in them.
#[derive(Default)]
pub struct Talk {
    /// The open session the rest was asked for.
    pub open: Option<SessionName>,
    pub related: Vec<Related>,
    pub said: HashMap<SessionName, Vec<Kept>>,
    /// Notices of this chat's own, with their times: a line it would
    /// not send.
    pub notices: Vec<(u64, String)>,
}

/// A message, with a count of its changes.
#[derive(Clone)]
pub struct Kept {
    pub message: SessionMessage,
    pub rev: u64,
}

impl Talk {
    /// Puts in `message`, or the change of the one of its id, keeping
    /// the last [`MOST`] of its session.
    fn take(
        &mut self,
        session: &SessionName,
        message: SessionMessage,
    ) {
        let kept = self.said.entry(session.clone()).or_default();
        match kept
            .binary_search_by_key(&message.id, |kept| kept.message.id)
        {
            Ok(at) => {
                if kept[at].message != message {
                    kept[at].rev += 1;
                    kept[at].message = message;
                }
            }
            Err(at) => kept.insert(at, Kept { message, rev: 0 }),
        }
        let over = kept.len().saturating_sub(MOST as usize);
        kept.drain(..over);
    }
}

/// Asks for the sessions the chat shows when the focus moved to
/// another session.
pub fn follow(world: &mut World) {
    let open = world
        .resource::<Focused>()
        .0
        .as_ref()
        .map(|focus| focus.open.name.clone());
    let was = world.resource::<Talk>().open.clone();
    if was == open {
        return;
    }
    // Talking to someone else brings the chat to the front, and
    // what the next message replied to is let go.
    if was.is_some() {
        hamlet_engine_sdk::ui::open(crate::CHAT);
        world.resource_mut::<Replying>().0 = None;
    }
    world.insert(Talk {
        open: open.clone(),
        ..Talk::default()
    });
    world.insert(Reach::default());
    world.insert(Unfolded::default());
    if open.is_some() {
        world.ask(AgentsRequest::Related);
    }
}

/// Takes in what the agents service said of the chat's sessions.
pub fn heard(world: &mut World, heard: &Heard) {
    match heard {
        Heard::Reply(_, AgentsReply::Related(related)) => {
            let talk = world.resource_mut::<Talk>();
            talk.related.clone_from(related);
            talk.said.retain(|session, _| {
                related.iter().any(|one| one.session.name == *session)
            });
            let missing = related
                .iter()
                .map(|one| one.session.name.clone())
                .filter(|session| !talk.said.contains_key(session))
                .collect::<Vec<_>>();
            for session in missing {
                world.ask(messages(session));
            }
        }
        Heard::Reply(
            Some(AgentsRequest::Messages { session, .. }),
            AgentsReply::Messages(said),
        ) => {
            let talk = world.resource_mut::<Talk>();
            if talk
                .related
                .iter()
                .any(|one| one.session.name == *session)
            {
                talk.said.insert(
                    session.clone(),
                    said.iter()
                        .map(|message| Kept {
                            message: message.clone(),
                            rev: 0,
                        })
                        .collect(),
                );
            }
        }
        Heard::Event(AgentsEvent::Said {
            session,
            messages: said,
            again,
        }) => {
            let shown = world
                .resource::<Talk>()
                .related
                .iter()
                .any(|one| one.session.name == *session);
            if !shown {
                return;
            }
            if *again {
                // A session resumed is read where it is talked in.
                if world.resource::<Talk>().open.as_ref()
                    == Some(session)
                {
                    hamlet_engine_sdk::ui::open(crate::CHAT);
                }
                return world.ask(messages(session.clone()));
            }
            let talk = world.resource_mut::<Talk>();
            for message in said {
                talk.take(session, message.clone());
            }
        }
        Heard::Event(AgentsEvent::Sessions)
        | Heard::Event(AgentsEvent::Working { .. }) => {
            if world.resource::<Talk>().open.is_some() {
                world.ask(AgentsRequest::Related);
            }
        }
        Heard::Event(AgentsEvent::Rewound { session, text })
            if world.resource::<Talk>().open.as_ref()
                == Some(session) =>
        {
            world.resource_mut::<Draft>().0.clone_from(text);
        }
        _ => {}
    }
}

/// The request of the last messages of `session`.
fn messages(session: SessionName) -> AgentsRequest {
    AgentsRequest::Messages {
        session,
        after: None,
        most: MOST,
    }
}
