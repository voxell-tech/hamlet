//! A chief's connection that keeps every message the server sends.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use base64::prelude::*;
use futures::{SinkExt, StreamExt};
use hamlet_protocol::v0::*;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
};

use crate::server::Server;
use crate::{fold, realm};

/// The longest a scenario waits for one thing to happen.
pub const PATIENCE: Duration = Duration::from_secs(20);

/// The bytes of one `give-part`.
const PART: usize = 512 * 1024;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The mayor of the first hamlet.
pub fn mayor() -> Address {
    to("home", "mayor")
}

/// The agent `agent` of `hamlet`.
pub fn to(hamlet: &str, agent: &str) -> Address {
    Address {
        hamlet: hamlet.into(),
        agent: agent.into(),
    }
}

/// The chief's current conversation with `with`.
pub fn chat(with: Address) -> ChatRef {
    ChatRef {
        with,
        from: None,
        kind: None,
        past: None,
    }
}

/// A fresh request ID.
pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// An authenticated connection and everything it received.
pub struct Client {
    wire: Wire,
    log: Vec<ServerMessage>,
    pages: HashSet<String>,
}

impl Client {
    /// Connects, says hello with the server's token and reads the
    /// first state.
    pub async fn join(server: &Server) -> Self {
        let mut wire = Wire::open(&server.url).await;
        wire.send_json(&ClientMessage::Hello(Hello {
            token: server.token.clone(),
            v: 0.0,
            seen: None,
        }))
        .await;
        let first = wire.message().await;
        assert!(
            matches!(first, Some(ServerMessage::State(_))),
            "a hello gets a state first, not {first:?}"
        );
        Self {
            wire,
            log: first.into_iter().collect(),
            pages: HashSet::new(),
        }
    }

    /// Every message received, oldest first.
    pub fn messages(&self) -> &[ServerMessage] {
        &self.log
    }

    /// The realm as the received states and changes leave it.
    pub fn realm(&self) -> State {
        realm::fold(&self.log)
    }

    /// The agent named `name` in `hamlet`, living or departed, as the
    /// realm stands; the living one when both exist.
    pub fn agent(&self, hamlet: &str, name: &str) -> Option<Agent> {
        let realm = self.realm();
        let mut found = realm
            .agents
            .into_iter()
            .filter(|a| a.ref_.hamlet == hamlet && a.name == name)
            .collect::<Vec<_>>();
        found.sort_by_key(|a| a.left.is_none());
        found.pop()
    }

    /// The items of a watched chat, folded from its pages and turns.
    pub fn items(&self, chat: &ChatRef) -> Vec<Item> {
        fold::items(&self.log, chat, &self.pages)
    }

    /// The texts of a watched chat's items.
    pub fn texts(&self, chat: &ChatRef) -> Vec<String> {
        self.items(chat).into_iter().map(|i| i.text).collect()
    }

    /// Whether a watched chat is working, from its last status.
    pub fn working(&self, chat: &ChatRef) -> bool {
        fold::working(&self.log, chat)
    }

    /// Every realm update received.
    pub fn changes(&self) -> Vec<Update> {
        self.log
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Change(c) => Some(c.changes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Every transcript op received for `chat`.
    pub fn ops(&self, chat: &ChatRef) -> Vec<Op> {
        self.log
            .iter()
            .filter_map(|m| match m {
                ServerMessage::Turn(t) if &t.chat == chat => {
                    Some(t.ops.clone())
                }
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Sends a message without waiting for anything.
    pub async fn send(&mut self, message: ClientMessage) {
        self.wire.send_json(&message).await;
    }

    /// Sends any text frame, as a broken client might.
    pub async fn send_raw(&mut self, text: &str) {
        self.wire.send_text(text).await;
    }

    /// Sends the request `make` builds from a fresh ID, and waits for
    /// the message that answers it.
    pub async fn ask(
        &mut self,
        make: impl FnOnce(String) -> ClientMessage,
    ) -> ServerMessage {
        let id = id();
        self.send(make(id.clone())).await;
        self.answer(&id).await
    }

    /// Waits for the message answering the request `id`.
    pub async fn answer(&mut self, id: &str) -> ServerMessage {
        let found = |log: &[ServerMessage]| {
            log.iter().find(|m| answers(m) == Some(id)).cloned()
        };
        self.until(&format!("an answer to {id}"), |c| {
            found(&c.log).is_some()
        })
        .await;
        found(&self.log).unwrap()
    }

    /// Sends a chief's line or typed command and waits for its reply.
    pub async fn say(&mut self, to: Address, text: &str) -> Reply {
        let id = self.tell(to, text).await;
        self.reply(&id).await
    }

    /// Sends a chief's line without waiting, giving its request ID.
    pub async fn tell(&mut self, to: Address, text: &str) -> String {
        let id = id();
        self.send(ClientMessage::Say(Say {
            id: id.clone(),
            to,
            text: text.into(),
        }))
        .await;
        id
    }

    /// Waits for the reply to the request `id`.
    pub async fn reply(&mut self, id: &str) -> Reply {
        match self.answer(id).await {
            ServerMessage::Reply(reply) => reply,
            other => panic!("{id} got {other:?}, not a reply"),
        }
    }

    /// Sends a request that answers with a reply.
    pub async fn request(
        &mut self,
        make: impl FnOnce(String) -> ClientMessage,
    ) -> Reply {
        match self.ask(make).await {
            ServerMessage::Reply(reply) => reply,
            other => panic!("a request got {other:?}, not a reply"),
        }
    }

    /// Gives `files` to `to`: the manifest, then each file's bytes in
    /// parts of 512 KiB. Gives the first refusal, else the last
    /// reply.
    pub async fn give(
        &mut self,
        to: Address,
        files: &[(&str, &[u8])],
    ) -> Reply {
        let manifest = files
            .iter()
            .map(|(path, bytes)| GiveFilesFiles {
                path: path.to_string(),
                size: bytes.len() as i64,
            })
            .collect();
        let mut reply = self
            .request(|id| {
                ClientMessage::Give(Give::Files {
                    files: manifest,
                    id,
                    to,
                })
            })
            .await;
        let Some(upload) = reply.upload.clone() else {
            return reply;
        };
        for (path, bytes) in files {
            for (n, part) in bytes.chunks(PART).enumerate() {
                reply =
                    self.part(&upload, path, n * PART, part).await;
                if reply.kind == ReplyKind::Error {
                    return reply;
                }
            }
        }
        reply
    }

    /// Sends the bytes of `path` at `offset` for the gift `upload`.
    pub async fn part(
        &mut self,
        upload: &str,
        path: &str,
        offset: usize,
        bytes: &[u8],
    ) -> Reply {
        let data = BASE64_STANDARD.encode(bytes);
        self.request(|id| {
            ClientMessage::GivePart(GivePart {
                data,
                id,
                offset: offset as i64,
                path: path.into(),
                upload: upload.into(),
            })
        })
        .await
    }

    /// Watches `chat` and gives its newest page.
    pub async fn open(&mut self, chat: &ChatRef) -> Transcript {
        let chat = chat.clone();
        match self
            .ask(|id| {
                ClientMessage::Open(Open {
                    id,
                    chat,
                    before: None,
                })
            })
            .await
        {
            ServerMessage::Transcript(t) => t,
            other => panic!("open got {other:?}, not a transcript"),
        }
    }

    /// The page of `chat` before the item `before`, or the fault.
    pub async fn page(
        &mut self,
        chat: &ChatRef,
        before: &str,
    ) -> ServerMessage {
        let id = id();
        self.pages.insert(id.clone());
        self.send(ClientMessage::Open(Open {
            id: id.clone(),
            chat: chat.clone(),
            before: Some(before.into()),
        }))
        .await;
        self.answer(&id).await
    }

    /// Reads `what`, built from a fresh ID.
    pub async fn read(
        &mut self,
        make: impl FnOnce(String) -> Read,
    ) -> ServerMessage {
        self.ask(|id| ClientMessage::Read(make(id))).await
    }

    /// Reads an agent's composed prompt.
    pub async fn prompt(&mut self, agent: Address) -> String {
        match self.read(|id| Read::Prompt { agent, id }).await {
            ServerMessage::Read(ReadResult::Prompt {
                prompt,
                ..
            }) => prompt,
            other => panic!("a prompt read got {other:?}"),
        }
    }

    /// Reads an agent's notes, character and skills.
    pub async fn papers(&mut self, agent: Address) -> Papers {
        match self
            .read(|id| Read::Papers {
                agent,
                full: None,
                id,
            })
            .await
        {
            ServerMessage::Read(ReadResult::Papers {
                papers,
                ..
            }) => papers,
            other => panic!("a papers read got {other:?}"),
        }
    }

    /// Reads an agent's numbered chief conversations.
    pub async fn history(&mut self, agent: Address) -> Vec<Past> {
        match self.read(|id| Read::History { agent, id }).await {
            ServerMessage::Read(ReadResult::History {
                history,
                ..
            }) => history,
            other => panic!("a history read got {other:?}"),
        }
    }

    /// Answers the wait `wait` with `choice`.
    pub async fn choose(
        &mut self,
        wait: &str,
        choice: Choice,
        text: Option<&str>,
    ) -> Reply {
        let wait = wait.to_string();
        let text = text.map(String::from);
        self.request(|id| {
            ClientMessage::Answer(Answer {
                choice,
                id,
                text,
                wait,
            })
        })
        .await
    }

    /// Changes one limit of `hamlet`.
    pub async fn limit(
        &mut self,
        hamlet: &str,
        section: &str,
        key: &str,
        value: Value,
    ) -> Reply {
        let set = EditLimitsSetSet {
            section: section.into(),
            key: key.into(),
            value,
        };
        let hamlet = hamlet.to_string();
        self.request(|id| {
            ClientMessage::Edit(Edit::Limits(EditLimits::Set {
                hamlet,
                id,
                reason: None,
                set,
            }))
        })
        .await
    }

    /// The open waits as the realm stands.
    pub fn waits(&self) -> Vec<Wait> {
        self.realm().waits
    }

    /// Waits until a wait is open for which `matches` holds, and
    /// gives it.
    pub async fn wait_for(
        &mut self,
        what: &str,
        matches: impl Fn(&Wait) -> bool,
    ) -> Wait {
        self.until(what, |c| c.waits().iter().any(&matches)).await;
        self.waits().into_iter().find(|w| matches(w)).unwrap()
    }

    /// Reads messages until `done` holds, failing after
    /// [`PATIENCE`].
    pub async fn until(
        &mut self,
        what: &str,
        mut done: impl FnMut(&Self) -> bool,
    ) {
        let began = Instant::now();
        while !done(self) {
            assert!(
                began.elapsed() < PATIENCE,
                "waited {PATIENCE:?} for {what}; last messages:\n{}",
                self.recent()
            );
            // Wakes now and then: `done` may look at files as well.
            let tick = Duration::from_millis(100);
            match tokio::time::timeout(tick, self.wire.message())
                .await
            {
                Ok(Some(message)) => self.log.push(message),
                Ok(None) => panic!(
                    "the server closed the connection while waiting \
                     for {what}; last messages:\n{}",
                    self.recent()
                ),
                Err(_) => {}
            }
        }
    }

    /// Reads whatever arrives within `time`, for checking that
    /// nothing more happens.
    pub async fn settle(&mut self, time: Duration) {
        while let Ok(Some(message)) =
            tokio::time::timeout(time, self.wire.message()).await
        {
            self.log.push(message);
        }
    }

    /// Waits until the agent `name` of `hamlet` is not busy.
    pub async fn idle(&mut self, hamlet: &str, name: &str) {
        self.until(&format!("{name}@{hamlet} to rest"), |c| {
            c.agent(hamlet, name).is_some_and(|a| !a.busy)
        })
        .await;
    }

    /// Waits until a watched chat holds an item with `text`.
    pub async fn shows(&mut self, chat: &ChatRef, text: &str) {
        self.until(&format!("{text:?} in the chat"), |c| {
            c.texts(chat).iter().any(|t| t == text)
        })
        .await;
    }

    /// Closes the connection.
    pub async fn close(mut self) {
        self.wire.close().await;
    }

    /// The connection itself, for reading how it ends.
    pub fn into_wire(self) -> Wire {
        self.wire
    }

    fn recent(&self) -> String {
        self.log[self.log.len().saturating_sub(8)..]
            .iter()
            .map(|m| {
                let text = serde_json::to_string(m).unwrap();
                text.chars().take(600).collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// The request a server message answers.
pub fn answers(message: &ServerMessage) -> Option<&str> {
    match message {
        ServerMessage::Reply(reply) => Some(&reply.to),
        ServerMessage::Fault(fault) => fault.to.as_deref(),
        ServerMessage::Transcript(t) => t.to.as_deref(),
        ServerMessage::Login(login) => login.to.as_deref(),
        ServerMessage::Read(
            ReadResult::Prompt { to, .. }
            | ReadResult::History { to, .. }
            | ReadResult::Models { to, .. }
            | ReadResult::Papers { to, .. }
            | ReadResult::Charter { to, .. },
        ) => Some(to),
        ServerMessage::State(_)
        | ServerMessage::Change(_)
        | ServerMessage::Turn(_) => None,
    }
}

/// How a raw connection ended.
#[derive(Debug, PartialEq)]
pub enum Ending {
    /// Closed with this WebSocket close code.
    Closed(u16),
    /// Ended without a close frame.
    Dropped,
}

/// A WebSocket to a server, before or without a hello.
pub struct Wire {
    socket: Socket,
    ended: Option<Ending>,
}

impl Wire {
    /// Opens a WebSocket to `url`.
    pub async fn open(url: &str) -> Self {
        let (socket, _) =
            connect_async(url).await.unwrap_or_else(|e| {
                panic!("cannot connect to {url}: {e}")
            });
        Self {
            socket,
            ended: None,
        }
    }

    /// Sends a protocol message.
    pub async fn send_json(&mut self, message: &ClientMessage) {
        self.send_text(&serde_json::to_string(message).unwrap())
            .await;
    }

    /// Sends any text frame.
    pub async fn send_text(&mut self, text: &str) {
        self.socket
            .send(Message::Text(text.into()))
            .await
            .expect("the connection takes a message");
    }

    /// The next server message, checked against protocol v0, or
    /// `None` once the connection ends.
    pub async fn message(&mut self) -> Option<ServerMessage> {
        while self.ended.is_none() {
            match self.socket.next().await {
                Some(Ok(Message::Text(text))) => {
                    return Some(parse(&text));
                }
                Some(Ok(Message::Close(frame))) => {
                    self.ended = Some(Ending::Closed(
                        frame.map_or(1005, |f: CloseFrame| {
                            f.code.into()
                        }),
                    ));
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => {
                    self.ended = Some(Ending::Dropped)
                }
            }
        }
        None
    }

    /// Reads to the end of the connection, giving what arrived and
    /// how it ended.
    pub async fn rest(&mut self) -> (Vec<ServerMessage>, Ending) {
        let mut got = Vec::new();
        loop {
            match tokio::time::timeout(PATIENCE, self.message()).await
            {
                Ok(Some(message)) => got.push(message),
                Ok(None) => break,
                Err(_) => {
                    panic!("the connection stayed open: {got:?}")
                }
            }
        }
        (got, self.ended.take().unwrap())
    }

    /// Closes the connection.
    pub async fn close(&mut self) {
        let _ = self.socket.close(None).await;
    }
}

/// Parses a server message, failing on any field protocol v0 does not
/// define.
fn parse(text: &str) -> ServerMessage {
    let raw =
        serde_json::from_str::<Value>(text).unwrap_or_else(|e| {
            panic!("the server sent bad JSON {text}: {e}")
        });
    let message =
        serde_json::from_value::<ServerMessage>(raw.clone())
            .unwrap_or_else(|e| {
                panic!(
                    "the server sent a message outside protocol v0: \
                     {e}\n{text}"
                )
            });
    let again = serde_json::to_value(&message).unwrap();
    assert!(
        same(&raw, &again),
        "the server sent fields outside protocol v0:\nsent: \
         {raw}\nread: {again}"
    );
    message
}

/// Whether two JSON values are equal, numbers compared by value.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x.as_f64() == y.as_f64()
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|(x, y)| same(x, y))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter().all(|(k, v)| {
                    y.get(k).is_some_and(|w| same(v, w))
                })
        }
        _ => a == b,
    }
}
