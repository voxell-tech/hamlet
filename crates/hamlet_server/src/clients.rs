//! The chiefs' clients: their connections, the state each was sent
//! and the changes after it, the conversations each watches, and the
//! replies owed to them.

pub(crate) mod check;
pub(crate) mod commands;
pub(crate) mod connection;
pub(crate) mod documents;
pub(crate) mod fold;
pub(crate) mod handlers;
pub(crate) mod requests;
pub(crate) mod state;

use std::net::SocketAddr;

use anyhow::Result;
use bevy_ecs::change_detection::Mut;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use bevy_ecs::message::{Message, Messages};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Commands, Query, ResMut, SystemParam};
use bevy_log::warn;
use hamlet_protocol::client::{
    self as protocol, ChatRef, RequestId, ServerMessage, Update,
};
use hamlet_store::ids::ConversationId;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinSet};

use self::check::Protocol;
use self::connection::Connection;
use self::handlers::ClientConnected;
use crate::app::input::InputSender;
use crate::realm::credentials::Credentials;

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_message::<OutgoingClientMessage>()
        .add_observer(requests::left)
        .add_plugins((handlers::plugin, commands::plugin));
}

/// A conversation a client watches, as one of its chats: an entity
/// of its own, which goes with the client's connection.
#[derive(Component)]
pub(crate) struct ConversationWatch {
    pub chat: ChatRef,
    pub conversation: ConversationId,
}

/// The connection whose client watches.
#[derive(Component)]
#[relationship(relationship_target = ConversationWatches)]
pub(crate) struct WatchedBy(pub Entity);

/// The conversations a connection's client watches.
#[derive(Component)]
#[relationship_target(relationship = WatchedBy, linked_spawn)]
pub(crate) struct ConversationWatches(Vec<Entity>);

/// A reply owed to a client once an input of a conversation ends: an
/// entity of its own, which goes with the client's connection.
#[derive(Component)]
pub(crate) struct OwedReply {
    pub id: RequestId,
    pub conversation: ConversationId,
    pub request: String,
}

/// The connection a reply is owed to.
#[derive(Component)]
#[relationship(relationship_target = OwedReplies)]
pub(crate) struct OwedReplyTo(pub Entity);

/// The replies owed to a connection's client.
#[derive(Component)]
#[relationship_target(relationship = OwedReplyTo, linked_spawn)]
pub(crate) struct OwedReplies(Vec<Entity>);

/// A client watching a conversation, and the chat it watches it as.
pub(crate) struct ConversationWatcher {
    pub watch: Entity,
    pub client: u64,
    pub chat: ChatRef,
}

/// Something for a client, sent once the input that made it has
/// published the state it leaves.
#[derive(Message)]
pub(crate) enum OutgoingClientMessage {
    Message(u64, Box<ServerMessage>),
    /// A refusal's words and its closing code.
    Refusal(u64, String, u16),
}

/// The connections as the handlers reach them, with what each client
/// watches, the replies owed to it and the messages for it.
#[derive(SystemParam)]
pub(crate) struct Connections<'w, 's> {
    connection_entities:
        Query<'w, 's, (Entity, &'static mut Connection)>,
    watches: Query<
        'w,
        's,
        (Entity, &'static WatchedBy, &'static mut ConversationWatch),
    >,
    owed_replies: Query<
        'w,
        's,
        (Entity, &'static OwedReplyTo, &'static OwedReply),
    >,
    outgoing_messages: ResMut<'w, Messages<OutgoingClientMessage>>,
    commands: Commands<'w, 's>,
}

impl Connections<'_, '_> {
    /// The entity of `client`'s connection, while it is there.
    pub fn entity(&self, client: u64) -> Option<Entity> {
        self.connection_entities
            .iter()
            .find(|(_, found)| found.id == client)
            .map(|(entity, _)| entity)
    }

    /// The connection of `client`, while it is there.
    pub fn get_mut(
        &mut self,
        client: u64,
    ) -> Option<Mut<'_, Connection>> {
        self.connection_entities
            .iter_mut()
            .find(|(_, found)| found.id == client)
            .map(|(_, found)| found)
    }

    /// The messages `client`'s queue has room for; none once it is
    /// gone.
    pub fn room(&self, client: u64) -> usize {
        self.connection_entities
            .iter()
            .find(|(_, found)| found.id == client)
            .map_or(0, |(_, found)| found.send.capacity())
    }

    /// The client whose connection is `entity`.
    pub fn client(&self, entity: Entity) -> Option<u64> {
        self.connection_entities
            .get(entity)
            .ok()
            .map(|(_, found)| found.id)
    }

    /// The presence of a client that said hello.
    pub fn is_any_welcomed(&self) -> bool {
        self.connection_entities
            .iter()
            .any(|(_, found)| found.welcomed)
    }

    /// Queues `message` for `client`.
    pub fn send(&mut self, client: u64, message: ServerMessage) {
        self.outgoing_messages.write(OutgoingClientMessage::Message(
            client,
            Box::new(message),
        ));
    }

    /// Queues a refusal of `client` in `text`, closing it with
    /// `code`.
    pub fn refuse(&mut self, client: u64, text: &str, code: u16) {
        self.outgoing_messages.write(OutgoingClientMessage::Refusal(
            client,
            text.into(),
            code,
        ));
    }

    /// Drops the messages of an input that failed, whose changes they
    /// told of; its refusals of a client still go.
    pub fn discard(&mut self) {
        let refusals = self
            .outgoing_messages
            .drain()
            .filter(|message| {
                matches!(message, OutgoingClientMessage::Refusal(..))
            })
            .collect::<Vec<_>>();
        self.outgoing_messages.write_batch(refusals);
    }

    /// Sends `message` to every welcomed client now.
    pub fn broadcast(
        &mut self,
        credentials: &Credentials,
        message: &ServerMessage,
    ) -> Result<()> {
        for (_, mut found) in &mut self.connection_entities {
            if found.welcomed {
                connection::send(
                    &mut found,
                    credentials,
                    message.clone(),
                )?;
            }
        }
        Ok(())
    }

    /// Sends what is queued, in order, with every credential taken
    /// out. A message that cannot be sent is logged and dropped.
    pub fn deliver(&mut self, credentials: &Credentials) {
        let queued =
            self.outgoing_messages.drain().collect::<Vec<_>>();
        for outgoing in queued {
            let (client, sent) = match outgoing {
                OutgoingClientMessage::Message(client, message) => (
                    client,
                    self.get_mut(client).map_or(
                        Ok(()),
                        |mut found| {
                            connection::send(
                                &mut found,
                                credentials,
                                *message,
                            )
                        },
                    ),
                ),
                OutgoingClientMessage::Refusal(
                    client,
                    text,
                    code,
                ) => (
                    client,
                    self.get_mut(client).map_or(
                        Ok(()),
                        |mut found| {
                            connection::refuse(
                                &mut found, &text, code,
                            )
                        },
                    ),
                ),
            };
            if let Err(error) = sent {
                let words =
                    credentials.scrub_text(&format!("{error:#}"));
                warn!("a message to client {client} failed: {words}");
            }
        }
    }

    /// Watches `conversation` as `chat` for `client`, in place of
    /// what it watched as that chat.
    pub fn watch(
        &mut self,
        client: u64,
        chat: ChatRef,
        conversation: ConversationId,
    ) {
        let Some(connection) = self.entity(client) else {
            return;
        };
        if let Some((.., mut kept)) =
            self.watches.iter_mut().find(|(_, of, kept)| {
                of.0 == connection && kept.chat == chat
            })
        {
            kept.conversation = conversation;
            return;
        }
        self.commands.spawn((
            WatchedBy(connection),
            ConversationWatch { chat, conversation },
        ));
    }

    /// Stops `client` watching `chat`.
    pub fn unwatch(&mut self, client: u64, chat: &ChatRef) {
        let Some(connection) = self.entity(client) else {
            return;
        };
        for (watch, of, kept) in &self.watches {
            if of.0 == connection && kept.chat == *chat {
                self.commands.entity(watch).despawn();
            }
        }
    }

    /// The clients watching `conversation`.
    pub fn watching(
        &self,
        conversation: ConversationId,
    ) -> Vec<ConversationWatcher> {
        self.watchers(|watch| watch.conversation == conversation)
    }

    /// The clients whose watch `keep` keeps.
    pub fn watchers(
        &self,
        keep: impl Fn(&ConversationWatch) -> bool,
    ) -> Vec<ConversationWatcher> {
        self.watches
            .iter()
            .filter(|(.., kept)| keep(kept))
            .filter_map(|(watch, of, kept)| {
                Some(ConversationWatcher {
                    watch,
                    client: self.client(of.0)?,
                    chat: kept.chat.clone(),
                })
            })
            .collect()
    }

    /// Points `watch` at `conversation`.
    pub fn rewatch(
        &mut self,
        watch: Entity,
        conversation: ConversationId,
    ) {
        if let Ok((.., mut kept)) = self.watches.get_mut(watch) {
            kept.conversation = conversation;
        }
    }

    /// Owes `client` the reply `owed`.
    pub fn owe(&mut self, client: u64, owed: OwedReply) {
        if let Some(connection) = self.entity(client) {
            self.commands.spawn((OwedReplyTo(connection), owed));
        }
    }

    /// Takes the replies owed once `request` of `conversation` ended,
    /// with the clients they are owed to.
    pub fn settle(
        &mut self,
        conversation: ConversationId,
        request: &str,
    ) -> Vec<(u64, RequestId)> {
        let mut settled = Vec::new();
        for (entity, to, owed) in &self.owed_replies {
            if owed.conversation == conversation
                && owed.request == request
            {
                self.commands.entity(entity).despawn();
                if let Some(client) = self.client(to.0) {
                    settled.push((client, owed.id.clone()));
                }
            }
        }
        settled
    }
}

/// What the clients share: the protocol's check, the tasks carrying
/// their connections and the state they were sent last.
#[derive(Resource)]
pub(crate) struct Clients {
    pub protocol_schema: Protocol,
    next_client_id: u64,
    /// Where each connection's events go, as inputs.
    inputs: InputSender,
    /// Tells every connection to close, once the server closes.
    closing_signal: watch::Sender<bool>,
    pub connection_tasks: JoinSet<()>,
    /// The state the clients were sent last.
    pub last_sent_state: Option<protocol::State>,
    pub state_epoch: i64,
    pub state_revision: i64,
}

impl Clients {
    pub fn new(inputs: InputSender, epoch: i64) -> Result<Self> {
        Ok(Self {
            protocol_schema: Protocol::new()?,
            next_client_id: 0,
            inputs,
            closing_signal: watch::Sender::new(false),
            connection_tasks: JoinSet::new(),
            last_sent_state: None,
            state_epoch: epoch,
            state_revision: 0,
        })
    }

    /// Starts carrying `stream`; gives the connection to spawn.
    pub fn accept(&mut self, stream: TcpStream) -> Connection {
        self.next_client_id += 1;
        connection::accept(
            stream,
            self.next_client_id,
            self.inputs.clone(),
            self.closing_signal.subscribe(),
            &mut self.connection_tasks,
        )
    }

    /// Tells every connection to close.
    pub fn close_connections(&self) {
        self.closing_signal.send_replace(true);
    }
}

/// The task taking clients' connections on `address`, each an input;
/// it stops taking them when this goes.
#[derive(Resource)]
pub(crate) struct Listening {
    pub address: SocketAddr,
    accepting: AbortHandle,
}

impl Listening {
    /// Takes clients' connections from `listener`, sending each to
    /// `inputs`.
    pub fn start(
        listener: TcpListener,
        inputs: InputSender,
    ) -> Result<Self> {
        let address = listener.local_addr()?;
        let accepting = tokio::spawn(async move {
            loop {
                if let Ok((stream, _)) = listener.accept().await
                    && inputs.send(ClientConnected(stream)).is_err()
                {
                    return;
                }
            }
        });
        Ok(Self {
            address,
            accepting: accepting.abort_handle(),
        })
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.accepting.abort();
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
    for profile in &new.profiles {
        if old.profiles.iter().find(|s| s.id == profile.id)
            != Some(profile)
        {
            out.push(Update::Profile {
                profile: profile.clone(),
            });
        }
    }
    for gone in &old.profiles {
        if !new.profiles.iter().any(|s| s.id == gone.id) {
            out.push(Update::ProfileGone { id: gone.id });
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
    for terminal in &new.terminals {
        if old.terminals.iter().find(|t| t.id == terminal.id)
            != Some(terminal)
        {
            out.push(Update::Terminal {
                terminal: terminal.clone(),
            });
        }
    }
    for gone in &old.terminals {
        if !new.terminals.iter().any(|t| t.id == gone.id) {
            out.push(Update::TerminalGone {
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
