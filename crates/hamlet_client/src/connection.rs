//! The server's address and the window's WebSocket connection.

use std::collections::VecDeque;
use std::future::poll_fn;
use std::path::PathBuf;
use std::task::Poll;
use std::time::{Duration, Instant};

use bevy_app::{App, Update};
use bevy_ecs::prelude::*;
use futures::{SinkExt, StreamExt};
use hamlet_protocol as protocol;
use hamlet_protocol::discovery::ServerAddress;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding;
use tokio_tungstenite::tungstenite::{Error, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use crate::Tokio;
use crate::messages::Reconnected;
use crate::outbox::Outbox;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// The window's connection and incoming-frame system.
pub fn plugin(app: &mut App) {
    app.init_resource::<Connection>()
        .init_resource::<Retry>()
        .init_resource::<Heartbeat>()
        .init_resource::<Discovery>()
        .add_systems(Update, update);
    if let Some(Server(server)) = app.world().get_resource::<Server>()
    {
        let words = format!("Connecting to {}...", server.url);
        app.insert_resource(Connection {
            state: State::Connecting,
            words,
        });
    }
}

/// The chosen server.
#[derive(Resource, Clone, Debug)]
pub struct Server(pub ServerAddress);

/// The first reconnect wait and its ceiling.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Retry {
    pub first: Duration,
    pub cap: Duration,
}

impl Default for Retry {
    fn default() -> Self {
        Self {
            first: Duration::from_secs(1),
            cap: Duration::from_secs(10),
        }
    }
}

/// The ping interval and maximum silence from the server.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Heartbeat {
    pub every: Duration,
    pub timeout: Duration,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            every: Duration::from_secs(15),
            timeout: Duration::from_secs(45),
        }
    }
}

#[derive(Resource)]
struct Bridge {
    incoming: mpsc::Receiver<Incoming>,
    outgoing: mpsc::Sender<Vec<protocol::ClientMessage>>,
    task: JoinHandle<()>,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[expect(
    clippy::large_enum_variant,
    reason = "Protocol frames cross the channel by value."
)]
enum Incoming {
    Connected { again: bool },
    Lost { answered: bool },
    Refused(String),
    Invalid(String),
    Frame(protocol::ServerMessage),
}

#[derive(Resource, Default)]
struct Discovery {
    next: Option<Instant>,
    delay: Duration,
}

fn discover(world: &mut World) {
    if world.contains_resource::<Server>() {
        return;
    }
    let Some(local) = world.get_resource::<LocalServer>() else {
        return;
    };
    let now = Instant::now();
    if world
        .resource::<Discovery>()
        .next
        .is_some_and(|next| now < next)
    {
        return;
    }
    let found = ServerAddress::in_home(&local.0);
    match found {
        Ok(server) => {
            world.insert_resource(Connection {
                state: State::Connecting,
                words: format!("Connecting to {}...", server.url),
            });
            world.insert_resource(Server(server));
        }
        Err(words) => {
            world.insert_resource(Connection {
                state: State::Lost,
                words,
            });
            let retry = *world.resource::<Retry>();
            let mut discovery = world.resource_mut::<Discovery>();
            discovery.delay = if discovery.next.is_none() {
                retry.first
            } else {
                discovery.delay.saturating_mul(2)
            }
            .min(retry.cap);
            discovery.next = Some(now + discovery.delay);
        }
    }
}

fn update(world: &mut World) {
    discover(world);
    if !world.contains_resource::<Bridge>() {
        let (Some(Server(server)), Some(runtime)) = (
            world.get_resource::<Server>(),
            world.get_resource::<Tokio>(),
        ) else {
            return;
        };
        let (frames, incoming) = mpsc::channel(256);
        let (outgoing, requests) = mpsc::channel(1);
        let retry = *world.resource::<Retry>();
        let heartbeat = *world.resource::<Heartbeat>();
        let task = runtime.0.spawn(connect(
            server.clone(),
            frames,
            requests,
            retry,
            heartbeat,
        ));
        world.insert_resource(Bridge {
            incoming,
            outgoing,
            task,
        });
    }
    world.resource_scope(|world, mut bridge: Mut<Bridge>| {
        while let Ok(incoming) = bridge.incoming.try_recv() {
            match incoming {
                Incoming::Connected { again } => {
                    let url = &world.resource::<Server>().0.url;
                    let words = format!("Connected to {url}.");
                    world.insert_resource(Connection {
                        state: State::Connected,
                        words,
                    });
                    if again {
                        world.trigger(Reconnected);
                    }
                }
                Incoming::Lost { answered } => {
                    let url = &world.resource::<Server>().0.url;
                    let words = if answered {
                        format!(
                            "Lost the server at {url}; trying again."
                        )
                    } else {
                        format!(
                            "No server answered at {url}. Start one \
                             with hamlet server."
                        )
                    };
                    world.insert_resource(Connection {
                        state: State::Lost,
                        words,
                    });
                }
                Incoming::Refused(text) => {
                    let url = &world.resource::<Server>().0.url;
                    let words = format!(
                        "The server at {url} refused this window: \
                         {text}"
                    );
                    world.insert_resource(Connection {
                        state: State::Refused,
                        words,
                    });
                }
                Incoming::Invalid(words) => {
                    world.insert_resource(Connection {
                        state: State::Lost,
                        words,
                    });
                }
                Incoming::Frame(message) => {
                    crate::receive(world, message)
                }
            }
        }
        if world.resource::<Connection>().state == State::Connected
            && let Ok(permit) = bridge.outgoing.try_reserve()
        {
            let messages = world
                .resource_mut::<Outbox>()
                .drain()
                .collect::<Vec<_>>();
            if !messages.is_empty() {
                permit.send(messages);
            }
        }
    });
}

async fn connect(
    server: ServerAddress,
    frames: mpsc::Sender<Incoming>,
    mut requests: mpsc::Receiver<Vec<protocol::ClientMessage>>,
    retry: Retry,
    heartbeat: Heartbeat,
) {
    if let Err(words) = validate(&server.url) {
        let _ = frames.send(Incoming::Invalid(words)).await;
        return;
    }
    let mut delay = retry.first.min(retry.cap);
    let mut answered = false;
    let mut pending = VecDeque::new();
    let mut reported = false;
    loop {
        if let Ok((mut socket, _)) =
            tokio_tungstenite::connect_async(&server.url).await
        {
            let hello =
                protocol::ClientMessage::Hello(protocol::Hello {
                    v: protocol::V,
                    token: server.token.clone(),
                });
            if socket.send(wire(&hello)).await.is_ok() {
                if frames
                    .send(Incoming::Connected { again: answered })
                    .await
                    .is_err()
                {
                    return;
                }
                answered = true;
                delay = retry.first.min(retry.cap);
                if let Some(text) = exchange(
                    &mut socket,
                    &frames,
                    &mut requests,
                    &mut pending,
                    &mut reported,
                    heartbeat,
                )
                .await
                {
                    let _ =
                        frames.send(Incoming::Refused(text)).await;
                    return;
                }
            }
        }
        if frames.send(Incoming::Lost { answered }).await.is_err() {
            return;
        }
        tokio::time::sleep(delay).await;
        delay = delay.saturating_mul(2).min(retry.cap);
    }
}

fn validate(url: &str) -> Result<(), String> {
    let invalid = || {
        format!("Not a WebSocket address: {url}. Use ws://host:port.")
    };
    let request = url.into_client_request().map_err(|_| invalid())?;
    match request.uri().scheme_str() {
        Some("ws") => Ok(()),
        Some("wss") => Err(format!(
            "Cannot connect to {url}: TLS is not available in this \
             window. Use ws:// through a secure tunnel."
        )),
        _ => Err(invalid()),
    }
}

fn wire(message: &protocol::ClientMessage) -> Message {
    Message::text(serde_json::to_string(message).unwrap())
}

async fn exchange(
    socket: &mut Socket,
    frames: &mpsc::Sender<Incoming>,
    requests: &mut mpsc::Receiver<Vec<protocol::ClientMessage>>,
    pending: &mut VecDeque<protocol::ClientMessage>,
    reported: &mut bool,
    heartbeat: Heartbeat,
) -> Option<String> {
    let mut fault = None;
    let mut writing = None;
    let mut ping = false;
    let now = tokio::time::Instant::now();
    let mut ticks = tokio::time::interval_at(
        now + heartbeat.every,
        heartbeat.every,
    );
    ticks.set_missed_tick_behavior(
        tokio::time::MissedTickBehavior::Delay,
    );
    let deadline = tokio::time::sleep(heartbeat.timeout);
    tokio::pin!(deadline);
    loop {
        let event = tokio::select! {
            _ = &mut deadline => return None,
            _ = ticks.tick() => {
                ping = true;
                continue;
            }
            event = advance(
                socket, pending, &mut writing, &mut ping,
            ) => event,
            batch = requests.recv(), if pending.is_empty() => {
                match batch {
                    Some(batch) => pending.extend(batch),
                    None => return None,
                }
                continue;
            }
        };
        if matches!(event, SocketEvent::Frame(Some(Ok(_)))) {
            deadline.as_mut().reset(
                tokio::time::Instant::now() + heartbeat.timeout,
            );
        }
        match event {
            SocketEvent::Frame(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<protocol::ServerMessage>(
                    &text,
                ) {
                    Ok(message) => {
                        if let protocol::ServerMessage::Fault(message) =
                            &message
                            && message.to.is_none()
                        {
                            fault = Some(message.text.clone());
                        }
                        if frames
                            .send(Incoming::Frame(message))
                            .await
                            .is_err()
                        {
                            return None;
                        }
                    }
                    Err(_) if !*reported => {
                        eprintln!(
                            "The server sent a message that does \
                             not fit the protocol."
                        );
                        *reported = true;
                    }
                    Err(_) => {}
                }
            }
            SocketEvent::Frame(Some(Ok(Message::Close(close)))) => {
                return close
                    .filter(|close| {
                        close.code == coding::CloseCode::Policy
                    })
                    .and(fault);
            }
            SocketEvent::Frame(Some(Err(_)) | None)
            | SocketEvent::Sent(Err(_)) => return None,
            _ => {}
        }
    }
}

enum SocketEvent {
    Frame(Option<Result<Message, Error>>),
    Sent(Result<(), Error>),
}

enum Writing {
    Request,
    Ping,
}

async fn advance(
    socket: &mut Socket,
    pending: &mut VecDeque<protocol::ClientMessage>,
    writing: &mut Option<Writing>,
    ping: &mut bool,
) -> SocketEvent {
    // A blocked write must not hide a refusal.
    poll_fn(|cx| {
        if let Poll::Ready(frame) = socket.poll_next_unpin(cx) {
            return Poll::Ready(SocketEvent::Frame(frame));
        }
        if writing.is_none() {
            let message = if *ping {
                Message::Ping(Vec::new().into())
            } else if let Some(message) = pending.front() {
                wire(message)
            } else {
                return Poll::Pending;
            };
            if let Err(error) =
                futures::ready!(socket.poll_ready_unpin(cx))
            {
                return Poll::Ready(SocketEvent::Sent(Err(error)));
            }
            if let Err(error) = socket.start_send_unpin(message) {
                return Poll::Ready(SocketEvent::Sent(Err(error)));
            }
            *writing = Some(if *ping {
                Writing::Ping
            } else {
                Writing::Request
            });
            *ping = false;
        }
        socket.poll_flush_unpin(cx).map(|result| {
            if result.is_ok()
                && matches!(writing, Some(Writing::Request))
            {
                pending.pop_front();
            }
            *writing = None;
            SocketEvent::Sent(result)
        })
    })
    .await
}

/// The state of the window's connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Connecting,
    Connected,
    Lost,
    Refused,
}

/// The connection state and words shown to the chief.
#[derive(Resource, Clone, Debug)]
pub struct Connection {
    pub state: State,
    pub words: String,
}

impl Default for Connection {
    fn default() -> Self {
        Self {
            state: State::Lost,
            words: "No server selected. Start one with hamlet \
                    server."
                .into(),
        }
    }
}

/// The local home searched while no server address is available.
#[derive(Resource, Clone, Debug)]
pub struct LocalServer(pub PathBuf);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ClientPlugin;

    #[test]
    fn a_window_started_first_discovers_its_local_server_later() {
        let home = hamlet_testing::Scratch::new("late-server");
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = runtime
            .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))
            .unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let task = runtime.spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .unwrap();
            let hello = socket.next().await.unwrap().unwrap();
            assert!(hello.to_text().unwrap().contains("late-token"));
            tokio::time::sleep(Duration::from_secs(1)).await;
        });
        let mut app = App::new();
        app.add_plugins(ClientPlugin { server: None })
            .insert_resource(Tokio(runtime.handle().clone()))
            .insert_resource(LocalServer(home.to_path_buf()))
            .insert_resource(Retry {
                first: Duration::ZERO,
                cap: Duration::ZERO,
            });
        app.update();
        assert!(!app.world().contains_resource::<Server>());
        std::fs::write(
            home.join("server.json"),
            format!(r#"{{"url":"{url}","pid":7,"started":"2026-01-01T00:00:00.000Z"}}"#),
        )
        .unwrap();
        std::fs::write(
            home.join("credentials.json"),
            r#"{"connection":{"token":"late-token"}}"#,
        )
        .unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until
            && app.world().resource::<Connection>().state
                != State::Connected
        {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            app.world().resource::<Connection>().state,
            State::Connected
        );
        assert_eq!(app.world().resource::<Server>().0.url, url);
        task.abort();
    }
}
