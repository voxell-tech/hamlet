//! The chiefs' clients: their connections, the state each was sent
//! and the changes after it, the conversations each watches, and the
//! replies owed to them.

pub(crate) mod check;
pub(crate) mod connection;
pub(crate) mod documents;
pub(crate) mod fold;
pub(crate) mod requests;
pub(crate) mod state;

use std::collections::BTreeMap;

use anyhow::Result;
use hamlet_protocol::client::{
    self as protocol, ChatRef, RequestId, ServerMessage, Update,
};
use hamlet_store::ids::ConversationId;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

use self::check::Protocol;
use self::connection::{Client, Event};
use crate::credentials::Credentials;
use crate::diagnostic::{Secrets, tell};

/// A conversation a client watches.
#[derive(Clone, Debug)]
pub(crate) struct Watch {
    pub client: u64,
    pub chat: ChatRef,
    pub conversation: ConversationId,
}

/// A reply owed to a client once an input of a conversation ends.
#[derive(Clone, Debug)]
pub(crate) struct Owed {
    pub client: u64,
    pub id: RequestId,
    pub conversation: ConversationId,
    pub request: String,
}

#[derive(bevy_ecs::resource::Resource)]
pub(crate) struct Clients {
    pub check: Protocol,
    pub list: BTreeMap<u64, Client>,
    next: u64,
    events: mpsc::Sender<Event>,
    closing: watch::Receiver<bool>,
    pub tasks: JoinSet<()>,
    pub watches: Vec<Watch>,
    pub owed: Vec<Owed>,
    /// The state the clients were sent last.
    pub sent: Option<protocol::State>,
    pub epoch: i64,
    pub revision: i64,
    /// What goes to the clients once the input that made it is done.
    outbox: Vec<Outgoing>,
}

/// Something for a client, queued.
enum Outgoing {
    Message(u64, Box<ServerMessage>),
    /// A refusal's words and its closing code.
    Refusal(u64, String, u16),
}

impl Clients {
    pub fn new(
        events: mpsc::Sender<Event>,
        closing: watch::Receiver<bool>,
        epoch: i64,
    ) -> Result<Self> {
        Ok(Self {
            check: Protocol::new()?,
            list: BTreeMap::new(),
            next: 0,
            events,
            closing,
            tasks: JoinSet::new(),
            watches: Vec::new(),
            owed: Vec::new(),
            sent: None,
            epoch,
            revision: 0,
            outbox: Vec::new(),
        })
    }

    pub fn accept(&mut self, stream: TcpStream) {
        self.next += 1;
        let client = connection::accept(
            stream,
            self.next,
            self.events.clone(),
            self.closing.clone(),
            &mut self.tasks,
        );
        self.list.insert(self.next, client);
    }

    /// Queues `message` for `client`, to go out once the input that
    /// made it has published the state it leaves.
    pub fn send(&mut self, client: u64, message: ServerMessage) {
        self.outbox
            .push(Outgoing::Message(client, Box::new(message)));
    }

    /// Queues a refusal of `client` in `text`, closing it with
    /// `code`.
    pub fn refuse(&mut self, client: u64, text: &str, code: u16) {
        if let Some(found) = self.list.get_mut(&client) {
            found.refused = true;
        }
        self.outbox.push(Outgoing::Refusal(
            client,
            text.into(),
            code,
        ));
    }

    /// Sends what is queued, in order, with every credential taken
    /// out. A message that cannot be sent is logged and dropped.
    pub fn deliver(&mut self, credentials: &Credentials) {
        for outgoing in std::mem::take(&mut self.outbox) {
            let sent = match outgoing {
                Outgoing::Message(client, message) => {
                    match self.list.get_mut(&client) {
                        Some(found) => connection::send(
                            found,
                            credentials,
                            *message,
                        ),
                        None => Ok(()),
                    }
                }
                Outgoing::Refusal(client, text, code) => {
                    match self.list.get_mut(&client) {
                        Some(found) => {
                            connection::refuse(found, &text, code)
                        }
                        None => Ok(()),
                    }
                }
            };
            if let Err(error) = sent {
                tell(
                    Secrets::Of(credentials),
                    format!(
                        "a message to a client failed: {error:#}"
                    ),
                );
            }
        }
    }

    /// Sends `message` to every welcomed client now.
    pub fn broadcast(
        &mut self,
        credentials: &Credentials,
        message: &ServerMessage,
    ) -> Result<()> {
        for client in self.list.values_mut().filter(|c| c.welcomed) {
            connection::send(client, credentials, message.clone())?;
        }
        Ok(())
    }

    /// `client` left: its watches and owed replies go.
    pub fn gone(&mut self, client: u64) {
        self.list.remove(&client);
        self.watches.retain(|w| w.client != client);
        self.owed.retain(|o| o.client != client);
    }

    /// The clients watching `conversation`, with what each watches.
    pub fn watching(
        &self,
        conversation: ConversationId,
    ) -> Vec<Watch> {
        self.watches
            .iter()
            .filter(|w| w.conversation == conversation)
            .cloned()
            .collect()
    }
}

/// The changes that take a client from `old` to `new`, or `None` when
/// only a whole state does: a part changed that has no change of its
/// own.
pub(crate) fn changes(
    old: &protocol::State,
    new: &protocol::State,
) -> Option<Vec<Update>> {
    let whole = old.closed != new.closed
        || old.library != new.library
        || old.deeds != new.deeds
        || old.hand != new.hand
        || old.limits != new.limits
        || old.confinement != new.confinement
        || old.realm != new.realm
        || old.chief != new.chief
        || old.hamlets.len() > new.hamlets.len()
        || new
            .hamlets
            .iter()
            .zip(&old.hamlets)
            .any(|(n, o)| n.id != o.id);
    if whole {
        return None;
    }
    let mut out = Vec::new();
    for hamlet in &new.hamlets {
        if old.hamlets.iter().find(|h| h.id == hamlet.id)
            != Some(hamlet)
        {
            out.push(Update::Hamlet {
                hamlet: hamlet.clone(),
            });
        }
    }
    for agent in &new.agents {
        if old.agents.iter().find(|a| a.id == agent.id) != Some(agent)
        {
            out.push(Update::Agent {
                agent: agent.clone(),
            });
        }
    }
    for gone in &old.agents {
        if !new.agents.iter().any(|a| a.id == gone.id) {
            out.push(Update::AgentGone { id: gone.id });
        }
    }
    for scholar in &new.scholars {
        if old.scholars.iter().find(|s| s.id == scholar.id)
            != Some(scholar)
        {
            out.push(Update::Scholar {
                scholar: scholar.clone(),
            });
        }
    }
    for gone in &old.scholars {
        if !new.scholars.iter().any(|s| s.id == gone.id) {
            out.push(Update::ScholarGone { id: gone.id });
        }
    }
    for wait in &new.waits {
        if old.waits.iter().find(|w| w.id == wait.id) != Some(wait) {
            out.push(Update::Wait { wait: wait.clone() });
        }
    }
    for gone in &old.waits {
        if !new.waits.iter().any(|w| w.id == gone.id) {
            out.push(Update::WaitClosed {
                how: "settled".into(),
                id: gone.id.clone(),
            });
        }
    }
    if old.grants != new.grants {
        out.push(Update::Grants {
            grants: new.grants.clone(),
        });
    }
    if old.settings != new.settings {
        out.push(Update::Settings {
            settings: new.settings.clone(),
        });
    }
    if old.providers != new.providers {
        out.push(Update::Providers {
            providers: new.providers.clone(),
        });
    }
    Some(out)
}
