//! The WebSocket connections: accepting them, carrying each one's
//! frames to the serve loop and back, and closing them with their
//! codes. A message is only text here: what it requests is for the
//! requests to read.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bevy_ecs::component::Component;
use bevy_ecs::entity::Entity;
use futures::{SinkExt, StreamExt};
use hamlet_protocol::client::ServerMessage;
use tokio::net::TcpStream;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio::task::JoinSet;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{
    CloseFrame, WebSocketConfig,
};
use tokio_tungstenite::tungstenite::{Error, Message};

use super::check::fault;
use super::handlers::ClientEvent;
use crate::credentials::Credentials;
use crate::server::InputSender;

const LARGEST: usize = 1_048_576;
pub(crate) const GRACE: Duration = Duration::from_secs(1);

/// The most messages of one connection the server holds before it
/// reads more from it, so a client that floods slows only itself.
const WAITING: usize = 32;

/// A connection's event, for the server.
pub(crate) enum Event {
    /// A message, holding one of its connection's places among those
    /// waiting until the server takes it in.
    Message(u64, String, OwnedSemaphorePermit),
    Late(u64),
    Gone(u64),
}

pub(crate) enum Out {
    Message(String),
    Close(u16),
}

/// The serve loop's end of one connection: an entity of its own,
/// despawned when the connection goes, which takes what the client
/// held with it.
#[derive(Component)]
pub(crate) struct Connection {
    /// The id the inputs of this connection carry.
    pub(crate) id: u64,
    pub(crate) send: mpsc::Sender<Out>,
    pub(crate) welcomed: bool,
    pub(crate) refused: bool,
}

/// Something a connection is sending in parts, such as an image or a
/// file: an entity of its own, which goes when the connection goes.
#[derive(Component)]
#[relationship(relationship_target = Uploading)]
pub(crate) struct UploadOf(pub Entity);

/// The uploads a connection is sending in parts.
#[derive(Component)]
#[relationship_target(relationship = UploadOf, linked_spawn)]
pub(crate) struct Uploading(Vec<Entity>);

/// Starts carrying `stream` as connection `id`, its task in `tasks`;
/// returns the loop's end of it.
pub(crate) fn accept(
    stream: TcpStream,
    id: u64,
    inputs: InputSender,
    closing: watch::Receiver<bool>,
    tasks: &mut JoinSet<()>,
) -> Connection {
    let (send, receive) = mpsc::channel(128);
    tasks.spawn(connection(stream, id, inputs, receive, closing));
    Connection {
        id,
        send,
        welcomed: false,
        refused: false,
    }
}

pub(crate) fn send(
    client: &mut Connection,
    credentials: &Credentials,
    message: ServerMessage,
) -> Result<()> {
    let mut value = serde_json::to_value(message)?;
    credentials.scrub(&mut value);
    if let Err(mpsc::error::TrySendError::Full(_)) =
        client.send.try_send(Out::Message(value.to_string()))
    {
        client.refused = true;
    }
    Ok(())
}

pub(crate) fn refuse(
    client: &mut Connection,
    text: &str,
    code: u16,
) -> Result<()> {
    client.refused = true;
    let _ = client.send.try_send(Out::Message(
        serde_json::to_string(&fault(None, text))?,
    ));
    let _ = client.send.try_send(Out::Close(code));
    Ok(())
}

async fn connection(
    stream: TcpStream,
    id: u64,
    inputs: InputSender,
    mut outgoing: mpsc::Receiver<Out>,
    mut closing: watch::Receiver<bool>,
) {
    let _ =
        talk(stream, id, &inputs, &mut outgoing, &mut closing).await;
    let _ = inputs.send(ClientEvent(Event::Gone(id)));
}

/// The cause that woke a connection's task.
enum Woke {
    Out(Out),
    Late,
    Frame(Option<Result<Message, Error>>),
}

/// Carries one connection: its frames to the loop as events, and what
/// the loop sends back, until either side closes.
async fn talk(
    stream: TcpStream,
    id: u64,
    inputs: &InputSender,
    outgoing: &mut mpsc::Receiver<Out>,
    closing: &mut watch::Receiver<bool>,
) -> Result<()> {
    let waiting = Arc::new(Semaphore::new(WAITING));
    // Holds a place among the waiting messages, then sends one.
    let message = async |text: String| {
        let Ok(place) = Arc::clone(&waiting).acquire_owned().await
        else {
            return false;
        };
        inputs
            .send(ClientEvent(Event::Message(id, text, place)))
            .is_ok()
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(LARGEST))
        .max_frame_size(Some(LARGEST));
    let handshake = tokio_tungstenite::accept_async_with_config(
        stream,
        Some(config),
    );
    let handshake =
        tokio::time::timeout(Duration::from_secs(10), handshake);
    let mut socket = tokio::select! {
        _ = closing.changed() => return Ok(()),
        accepted = handshake => accepted??,
    };
    let hello = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(hello);
    let mut timed = false;
    loop {
        let woke = tokio::select! {
            biased;
            _ = closing.changed() => Woke::Out(Out::Close(1001)),
            out = outgoing.recv() => {
                Woke::Out(out.unwrap_or(Out::Close(1011)))
            }
            _ = &mut hello, if !timed => Woke::Late,
            frame = socket.next() => Woke::Frame(frame),
        };
        let out = match woke {
            Woke::Out(out) => out,
            Woke::Late => {
                timed = true;
                let _ = inputs.send(ClientEvent(Event::Late(id)));
                continue;
            }
            Woke::Frame(Some(Ok(Message::Text(text)))) => {
                if !message(text.to_string()).await {
                    break;
                }
                continue;
            }
            Woke::Frame(Some(Ok(Message::Binary(bytes)))) => {
                let text =
                    String::from_utf8_lossy(&bytes).into_owned();
                if !message(text).await {
                    break;
                }
                continue;
            }
            Woke::Frame(Some(Ok(Message::Close(_))) | None) => {
                let _ = socket.flush().await;
                break;
            }
            Woke::Frame(Some(Ok(_))) => continue,
            Woke::Frame(Some(Err(Error::Capacity(_)))) => {
                let text = format!(
                    "A message is larger than the {LARGEST} bytes \
                     this connection takes."
                );
                let text = serde_json::to_string(&fault(None, text))?;
                let sent = socket.send(Message::Text(text.into()));
                let _ = tokio::time::timeout(GRACE, sent).await;
                Out::Close(1009)
            }
            Woke::Frame(Some(Err(_))) => break,
        };
        match out {
            Out::Message(text) => {
                let sent = socket.send(Message::Text(text.into()));
                tokio::time::timeout(GRACE, sent).await??;
            }
            Out::Close(code) => {
                let reason = if code == 1009 {
                    "Message too large."
                } else {
                    ""
                };
                let frame = CloseFrame {
                    code: CloseCode::from(code),
                    reason: reason.into(),
                };
                let _ = tokio::time::timeout(
                    GRACE,
                    socket.close(Some(frame)),
                )
                .await;
                // Drain bytes already in flight so closing a large
                // frame does not reset TCP before the
                // client reads the fault.
                if code == 1009 {
                    use tokio::io::AsyncReadExt;
                    let drain = async {
                        let mut bytes = [0; 8192];
                        while socket
                            .get_mut()
                            .read(&mut bytes)
                            .await
                            .is_ok_and(|n| n > 0)
                        {
                        }
                    };
                    let _ = tokio::time::timeout(GRACE, drain).await;
                } else {
                    let _ =
                        tokio::time::timeout(GRACE, socket.next())
                            .await;
                }
                break;
            }
        }
    }
    Ok(())
}
