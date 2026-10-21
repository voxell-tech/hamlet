//! The agents service as the panels read it: each request remembered
//! by its ticket until its reply comes, and what every panel follows
//! kept as resources of the plugin's world.

use std::any::Any;
use std::collections::BTreeMap;

use hamlet_engine_sdk::api::Capability;
use hamlet_engine_sdk::api::agents::{
    Focus, Need, Recipient, Roster, Skill,
};
use hamlet_engine_sdk::ui::show_overlay;
use hamlet_engine_sdk::{
    AgentsEvent, AgentsReply, AgentsRequest, Failure, Message, Reply,
    Ticket, World, agents,
};

use crate::change::Changes;
use crate::confirm::{CONFIRM, Confirm, Confirming};

/// The chat's panel, of the plugin `hamlet.chat`.
const CHAT: &str = "hamlet.chat/chat";

/// The most requests remembered at once. A reply the inbox left out
/// never comes, so the oldest are forgotten past it.
const REMEMBERED: usize = 256;

/// Brings the chat to the front.
pub fn show_chat() {
    hamlet_engine_sdk::ui::open(CHAT);
}

/// Watches the agents service and asks for `first`, for a plugin
/// that has no world at hand, as it declares what it adds. Their
/// replies are known by what they hold.
pub fn start(first: &[AgentsRequest]) {
    agents(&AgentsRequest::Watch);
    for request in first {
        agents(request);
    }
}

/// The requests asked and not yet answered, by their tickets.
#[derive(Default)]
pub struct Asked(BTreeMap<Ticket, AgentsRequest>);

impl Asked {
    /// Whether `request` was asked again and waits for its reply.
    fn again(&self, request: &AgentsRequest) -> bool {
        self.0.values().any(|asked| asked == request)
    }
}

/// The window's focus, as last told.
#[derive(Default, PartialEq)]
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

/// What waits for the chief, oldest first.
#[derive(Default, PartialEq)]
pub struct Waits(pub Vec<Need>);

/// The library's skills.
#[derive(Default, PartialEq)]
pub struct Shelves(pub Vec<Skill>);

/// What a message from the agents service came to.
pub enum Heard {
    /// The reply to a request, with the request when it was asked
    /// through [`AgentsWorldExt::ask`].
    Reply(Option<AgentsRequest>, AgentsReply),
    Failed(Option<AgentsRequest>, Failure),
    Event(AgentsEvent),
}

/// The agents' methods on [`World`].
pub trait AgentsWorldExt {
    /// Puts in each resource this crate keeps, so views can read
    /// them.
    fn init_agents(&mut self);

    /// Puts in `value` when it differs from the one there, so the
    /// views that read it are not looked at again for nothing.
    fn keep<R: PartialEq + Any + Send + Sync>(&mut self, value: R);

    /// Asks the agents service `request`, remembered for its reply.
    fn ask(&mut self, request: AgentsRequest);

    /// Takes in `message`: what it tells of the focus, the roster,
    /// the needs and the library is kept, and what it came to is
    /// given back for the plugin's own. The reply to a reading asked
    /// again since is passed over, as the later one tells what is so
    /// now.
    fn hear(&mut self, message: Message) -> Option<Heard>;

    /// Sends `text` to the open conversation.
    fn tell(&mut self, text: impl Into<String>);

    /// Gives the chief's command `text` to the hamlet `hamlet`.
    fn decree(&mut self, hamlet: &str, text: impl Into<String>);

    /// Asks `question` before it is done.
    fn confirm(&mut self, question: Confirm);
}

impl AgentsWorldExt for World {
    fn init_agents(&mut self) {
        self.init_resource::<Asked>();
        self.init_resource::<Focused>();
        self.init_resource::<Roster>();
        self.init_resource::<Waits>();
        self.init_resource::<Shelves>();
        self.init_resource::<Changes>();
        self.init_resource::<Confirming>();
    }

    fn keep<R: PartialEq + Any + Send + Sync>(&mut self, value: R) {
        if self.get::<R>() != Some(&value) {
            self.insert(value);
        }
    }

    fn ask(&mut self, request: AgentsRequest) {
        let ticket = agents(&request);
        if ticket == 0 {
            return;
        }
        let asked = &mut self.resource_or_init::<Asked>().0;
        asked.insert(ticket, request);
        if asked.len() > REMEMBERED {
            asked.pop_first();
        }
    }

    fn hear(&mut self, message: Message) -> Option<Heard> {
        let Message::Reply { ticket, reply } = message else {
            return None;
        };
        let asked =
            self.resource_or_init::<Asked>().0.remove(&ticket);
        if let (Some(request), Ok(_)) = (&asked, &reply)
            && request.capability() == Capability::AgentsRead
            && self.resource::<Asked>().again(request)
        {
            return None;
        }
        let heard = match reply {
            Ok(Reply::Agents(AgentsReply::Event(event))) => {
                Heard::Event(event)
            }
            Ok(Reply::Agents(reply)) => Heard::Reply(asked, reply),
            Ok(_) => return None,
            Err(why) => return Some(Heard::Failed(asked, why)),
        };
        match &heard {
            Heard::Reply(_, AgentsReply::Focus(focus))
            | Heard::Event(AgentsEvent::Focus(focus)) => {
                self.keep(Focused(Some(focus.clone())));
            }
            Heard::Reply(_, AgentsReply::Roster(roster))
            | Heard::Event(AgentsEvent::Roster(roster)) => {
                self.keep(roster.clone());
            }
            Heard::Reply(_, AgentsReply::Needs(needs))
            | Heard::Event(AgentsEvent::Needs(needs)) => {
                self.keep(Waits(needs.clone()));
            }
            Heard::Reply(_, AgentsReply::Skills(skills))
            | Heard::Event(AgentsEvent::Skills(skills)) => {
                self.keep(Shelves(skills.clone()));
            }
            _ => {}
        }
        Some(heard)
    }

    fn tell(&mut self, text: impl Into<String>) {
        if let Some(to) = self.resource_or_init::<Focused>().with() {
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
        self.resource_or_init::<Confirming>().0 = Some(question);
        show_overlay(CONFIRM, true);
    }
}
