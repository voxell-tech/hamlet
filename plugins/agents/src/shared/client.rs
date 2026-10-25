//! The agents service as the panels read it: each request remembered
//! by its ticket until its reply comes, and what every panel follows
//! kept as resources of the plugin's world.

use std::collections::BTreeMap;

use hamlet_engine_bevy::api::Capability;
use hamlet_engine_bevy::api::agents::{
    ChiefRecord, Focus, Need, Recipient, Roster, Skill,
};
use hamlet_engine_bevy::bevy_ecs::message::MessageCursor;
use hamlet_engine_bevy::prelude::*;
use hamlet_engine_bevy::ui::show_overlay;
use hamlet_engine_bevy::{
    AgentsEvent, AgentsReply, AgentsRequest, Failure, Reply, Ticket,
};

use crate::chat::CHAT;
use crate::shared::change::Changes;
use crate::shared::confirm::{CONFIRM, Confirm, Confirming};

/// The most requests remembered at once. A reply the inbox left out
/// never comes, so the oldest are forgotten past it.
const REMEMBERED: usize = 256;

/// What the agents' panels share: the resources views read, and what
/// the agents service says, heard in [`PreUpdate`] as [`Heard`].
pub fn plugin(app: &mut App) {
    app.init_resource::<Asked>()
        .init_resource::<Focused>()
        .init_resource::<Rostered>()
        .init_resource::<Chiefed>()
        .init_resource::<Waits>()
        .init_resource::<Shelves>()
        .init_resource::<Changes>()
        .init_resource::<Confirming>()
        .add_message::<Heard>()
        .add_systems(PreUpdate, hear);
}

/// Brings the chat to the front.
pub fn show_chat() {
    hamlet_engine_bevy::ui::open(CHAT);
}

/// A system for [`Declare`] that watches the agents service and asks
/// for `first` as the plugin starts. Their replies are known by what
/// they hold.
pub fn start(
    first: &[AgentsRequest],
) -> impl FnMut(&mut World) + Send + Sync + 'static {
    let first = first.to_vec();
    move |world: &mut World| {
        world.agents(&AgentsRequest::Watch);
        for request in &first {
            world.agents(request);
        }
    }
}

/// The requests asked and not yet answered, by their tickets.
#[derive(Resource, Default)]
pub struct Asked(BTreeMap<Ticket, AgentsRequest>);

impl Asked {
    /// Whether `request` was asked again and waits for its reply.
    fn again(&self, request: &AgentsRequest) -> bool {
        self.0.values().any(|asked| asked == request)
    }
}

/// The window's focus, as last told.
#[derive(Resource, Default, PartialEq)]
pub struct Focused(pub Option<Focus>);

impl Focused {
    /// Who the open conversation is with.
    pub fn with(&self) -> Option<Recipient> {
        self.0.as_ref().map(|focus| focus.with.clone())
    }

    /// The agent of the open conversation, by its name.
    pub fn agent(&self) -> Option<String> {
        self.0.as_ref().and_then(|focus| focus.open.agent.clone())
    }

    /// Who the open conversation is with, in capitals for a head:
    /// the agent's name, or THE MAYOR.
    pub fn whose(&self) -> String {
        match self.with() {
            Some(Recipient::Agent(name)) => name.to_uppercase(),
            _ => "THE MAYOR".to_string(),
        }
    }
}

/// The roster, as last told.
#[derive(Resource, Default, PartialEq)]
pub struct Rostered(pub Roster);

/// The chief at this window, as the home keeps it.
#[derive(Resource, Default, PartialEq)]
pub struct Chiefed(pub ChiefRecord);

/// What waits for the chief, oldest first.
#[derive(Resource, Default, PartialEq)]
pub struct Waits(pub Vec<Need>);

/// The library's skills.
#[derive(Resource, Default, PartialEq)]
pub struct Shelves(pub Vec<Skill>);

/// What a message from the agents service came to.
#[derive(Message, Clone, Debug)]
pub enum Heard {
    /// The reply to a request, with the request when it was asked
    /// through [`AgentsWorldExt::ask`].
    Reply(Option<AgentsRequest>, AgentsReply),
    Failed(Option<AgentsRequest>, Failure),
    Event(AgentsEvent),
}

/// The agents' methods on [`World`].
pub trait AgentsWorldExt {
    /// What the agents service said since `cursor` last read, for a
    /// system that takes the whole world.
    fn heard(&self, cursor: &mut MessageCursor<Heard>) -> Vec<Heard>;

    /// Puts in `value` when it differs from the one there, so the
    /// views that read it are not looked at again for nothing.
    fn keep<R: Resource + PartialEq>(&mut self, value: R);

    /// Asks the agents service `request`, remembered for its reply.
    fn ask(&mut self, request: AgentsRequest);

    /// Sends `text` to the open conversation.
    fn tell(&mut self, text: impl Into<String>);

    /// Gives the chief's command `text` to the hamlet `hamlet`.
    fn decree(&mut self, hamlet: &str, text: impl Into<String>);

    /// Asks `question` before it is done.
    fn confirm(&mut self, question: Confirm);
}

impl AgentsWorldExt for World {
    fn heard(&self, cursor: &mut MessageCursor<Heard>) -> Vec<Heard> {
        cursor
            .read(self.resource::<Messages<Heard>>())
            .cloned()
            .collect()
    }

    fn keep<R: Resource + PartialEq>(&mut self, value: R) {
        if self.get_resource::<R>() != Some(&value) {
            self.insert_resource(value);
        }
    }

    fn ask(&mut self, request: AgentsRequest) {
        let ticket = self.agents(&request);
        if ticket == 0 {
            return;
        }
        let mut asked = self.get_resource_or_init::<Asked>();
        asked.0.insert(ticket, request);
        if asked.0.len() > REMEMBERED {
            asked.0.pop_first();
        }
    }

    fn tell(&mut self, text: impl Into<String>) {
        if let Some(to) = self.resource::<Focused>().with() {
            self.ask(AgentsRequest::Send {
                to,
                text: text.into(),
            });
        }
    }

    fn decree(&mut self, hamlet: &str, text: impl Into<String>) {
        self.ask(AgentsRequest::Decree {
            hamlet: hamlet.to_string(),
            text: text.into(),
        });
    }

    fn confirm(&mut self, question: Confirm) {
        self.resource_mut::<Confirming>().0 = Some(question);
        show_overlay(CONFIRM, true);
    }
}

/// Takes in each reply and event of the agents service: what it tells
/// of the focus, the roster, the needs and the library is kept, and
/// what it came to is written as [`Heard`], a request that failed
/// told in the log. The reply to a reading
/// asked again since is passed over, as the later one tells what is
/// so now.
fn hear(
    world: &mut World,
    mut replies: Local<MessageCursor<Replied>>,
    mut events: Local<MessageCursor<AgentsEventReceived>>,
) {
    let replied = replies
        .read(world.resource::<Messages<Replied>>())
        .cloned()
        .collect::<Vec<_>>();
    let told = events
        .read(world.resource::<Messages<AgentsEventReceived>>())
        .map(|told| told.event.clone())
        .collect::<Vec<_>>();
    for Replied { ticket, reply } in replied {
        if let Some(heard) = Heard::reply(world, ticket, reply) {
            match &heard {
                Heard::Failed(Some(request), why) => {
                    hamlet_engine_bevy::warn(&format!(
                        "{request:?}: {why}"
                    ));
                }
                Heard::Failed(None, why) => {
                    hamlet_engine_bevy::warn(&why.to_string());
                }
                _ => {}
            }
            heard.keep(world);
            world.write_message(heard);
        }
    }
    for event in told {
        let heard = Heard::Event(event);
        heard.keep(world);
        world.write_message(heard);
    }
}

impl Heard {
    /// What the reply of `ticket` came to, if it is the agents'.
    fn reply(
        world: &mut World,
        ticket: Ticket,
        reply: Result<Reply, Failure>,
    ) -> Option<Self> {
        let asked = world.resource_mut::<Asked>().0.remove(&ticket);
        if let (Some(request), Ok(_)) = (&asked, &reply)
            && request.capability() == Capability::AgentsRead
            && world.resource::<Asked>().again(request)
        {
            return None;
        }
        match reply {
            Ok(Reply::Agents(reply)) => {
                Some(Heard::Reply(asked, reply))
            }
            Ok(_) => None,
            Err(why) => Some(Heard::Failed(asked, why)),
        }
    }

    /// Keeps what this tells of the focus, the roster, the needs and
    /// the library in `world`.
    fn keep(&self, world: &mut World) {
        match self {
            Heard::Reply(_, AgentsReply::Focus(focus))
            | Heard::Event(AgentsEvent::Focus(focus)) => {
                world.keep(Focused(Some(focus.clone())));
            }
            Heard::Reply(_, AgentsReply::Roster(roster))
            | Heard::Event(AgentsEvent::Roster(roster)) => {
                world.keep(Rostered(roster.clone()));
            }
            Heard::Reply(_, AgentsReply::Chief(record))
            | Heard::Event(AgentsEvent::Chief(record)) => {
                world.keep(Chiefed(record.clone()));
            }
            Heard::Reply(_, AgentsReply::Needs(needs))
            | Heard::Event(AgentsEvent::Needs(needs)) => {
                world.keep(Waits(needs.clone()));
            }
            Heard::Reply(_, AgentsReply::Skills(skills))
            | Heard::Event(AgentsEvent::Skills(skills)) => {
                world.keep(Shelves(skills.clone()));
            }
            _ => {}
        }
    }
}
