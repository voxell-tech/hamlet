use std::time::{Duration, Instant};

use bevy_app::App;
use bevy_ecs::prelude::*;
use futures::{SinkExt, StreamExt};
use hamlet_client::connection::{Connection, State};
use hamlet_client::outbox::Outbox;
use hamlet_client::realm::Settlement;
use hamlet_client::{ClientPlugin, Tokio};
use hamlet_protocol as protocol;
use hamlet_protocol::discovery::ServerAddress;
use hamlet_testing::realm;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding;

fn window(url: &str) -> App {
    let mut app = App::new();
    app.insert_resource(Tokio(tokio::runtime::Handle::current()));
    app.add_plugins(ClientPlugin {
        server: Some(ServerAddress {
            url: url.into(),
            token: "window-token".into(),
        }),
    });
    app
}

async fn until(
    app: &mut App,
    mut ready: impl FnMut(&mut World) -> bool,
) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if ready(app.world_mut()) {
            return;
        }
        assert!(
            Instant::now() < end,
            "The window did not reach the expected state: {:?}",
            app.world().resource::<Connection>()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn next(
    socket: &mut WebSocketStream<TcpStream>,
) -> protocol::ClientMessage {
    let frame =
        tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .expect("The window sent no frame.")
            .expect("The window closed its connection.")
            .unwrap();
    serde_json::from_str(frame.to_text().unwrap()).unwrap()
}

async fn send(
    socket: &mut WebSocketStream<TcpStream>,
    message: protocol::ServerMessage,
) {
    socket
        .send(Message::text(serde_json::to_string(&message).unwrap()))
        .await
        .unwrap();
}

async fn window_closed(socket: &mut WebSocketStream<TcpStream>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                // Heartbeats can already be in flight when the window
                // closes.
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {
                    break;
                }
                frame => panic!(
                    "Closing the window sent a request: {frame:?}"
                ),
            }
        }
    })
    .await
    .expect("Closing the window left its connection running.");
}

fn say(app: &mut App, text: &str) -> String {
    app.world_mut().resource_mut::<Outbox>().send(|id| {
        protocol::ClientMessage::Say(protocol::Say {
            id,
            to: protocol::Address {
                hamlet: "home".into(),
                agent: "mayor".into(),
            },
            text: text.into(),
        })
    })
}

#[tokio::test]
async fn hello_precedes_the_outbox_and_received_state_fills_the_world()
 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (sent, mut frames) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        sent.send(next(&mut socket).await).unwrap();
        socket.send(Message::text("not JSON")).await.unwrap();
        socket
            .send(Message::text(r#"{"type":"unknown"}"#))
            .await
            .unwrap();
        send(
            &mut socket,
            protocol::ServerMessage::State(realm::state()),
        )
        .await;
        sent.send(next(&mut socket).await).unwrap();
        sent.send(next(&mut socket).await).unwrap();
        window_closed(&mut socket).await;
    });
    let mut app = window(&url);
    let first = say(&mut app, "First");
    let second = say(&mut app, "Second");
    let mut received = Vec::new();
    until(&mut app, |world| {
        received
            .extend(std::iter::from_fn(|| frames.try_recv().ok()));
        received.len() == 3
            && world.query::<&Settlement>().iter(world).count() == 1
    })
    .await;
    assert_eq!(
        received[0],
        protocol::ClientMessage::Hello(protocol::Hello {
            v: protocol::V,
            token: "window-token".into(),
        })
    );
    for (message, id, text) in [
        (&received[1], first, "First"),
        (&received[2], second, "Second"),
    ] {
        let protocol::ClientMessage::Say(message) = message else {
            panic!("Expected a say after hello.");
        };
        assert_eq!(message.id, id);
        assert_eq!(message.text, text);
    }
    let connection = app.world().resource::<Connection>();
    assert_eq!(connection.state, State::Connected);
    assert_eq!(connection.words, format!("Connected to {url}."));
    assert_eq!(
        app.world_mut()
            .query::<&Settlement>()
            .single(app.world())
            .unwrap()
            .name,
        "home"
    );
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 0);
    drop(app);
    server.await.unwrap();
}

#[derive(Resource, Default)]
struct Reconnections(u32);

#[tokio::test]
async fn a_reconnection_keeps_the_realm_and_sends_offline_requests_after_hello()
 {
    use hamlet_client::messages::Reconnected;
    use hamlet_client::realm::Seen;
    use tokio::sync::oneshot;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (close, closed) = oneshot::channel();
    let (sent, mut frames) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        let mut state = realm::state();
        state.epoch = 19;
        state.revision = 5;
        send(&mut socket, protocol::ServerMessage::State(state))
            .await;
        send(
            &mut socket,
            protocol::ServerMessage::Change(protocol::Changes {
                epoch: 19,
                revision: 6,
                changes: Vec::new(),
            }),
        )
        .await;
        closed.await.unwrap();
        socket.close(None).await.unwrap();
        drop(socket);
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        sent.send(next(&mut socket).await).unwrap();
        sent.send(next(&mut socket).await).unwrap();
        window_closed(&mut socket).await;
    });
    let mut app = window(&url);
    app.init_resource::<Reconnections>();
    app.add_observer(
        |_: On<Reconnected>, mut count: ResMut<Reconnections>| {
            count.0 += 1
        },
    );
    until(&mut app, |world| {
        world
            .get_resource::<Seen>()
            .is_some_and(|seen| seen.revision == 6)
    })
    .await;
    assert_eq!(app.world().resource::<Reconnections>().0, 0);
    let home = app
        .world_mut()
        .query_filtered::<Entity, With<Settlement>>()
        .single(app.world())
        .unwrap();
    close.send(()).unwrap();
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Lost
    })
    .await;
    assert_eq!(
        app.world().resource::<Connection>().words,
        format!("Lost the server at {url}; trying again.")
    );
    let request = say(&mut app, "While away");
    app.update();
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
    let mut received = Vec::new();
    until(&mut app, |world| {
        received
            .extend(std::iter::from_fn(|| frames.try_recv().ok()));
        received.len() == 2
            && world.resource::<Reconnections>().0 == 1
    })
    .await;
    assert_eq!(
        received[0],
        protocol::ClientMessage::Hello(protocol::Hello {
            v: protocol::V,
            token: "window-token".into(),
        })
    );
    let protocol::ClientMessage::Say(message) = &received[1] else {
        panic!("Expected the offline say.");
    };
    assert_eq!(message.id, request);
    assert_eq!(message.text, "While away");
    assert!(app.world().get::<Settlement>(home).is_some());
    assert_eq!(
        app.world_mut()
            .query::<&Settlement>()
            .iter(app.world())
            .count(),
        1
    );
    assert_eq!(
        app.world().resource::<Connection>().state,
        State::Connected
    );
    drop(app);
    server.await.unwrap();
}

#[tokio::test]
async fn a_refused_token_keeps_the_faults_words_and_stops_retrying() {
    use hamlet_client::connection::Retry;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        send(
            &mut socket,
            protocol::ServerMessage::Fault(protocol::Fault {
                to: None,
                text: "That is not this server's token.".into(),
            }),
        )
        .await;
        socket
            .close(Some(CloseFrame {
                code: coding::CloseCode::Policy,
                reason: "".into(),
            }))
            .await
            .unwrap();
        drop(socket);
        assert!(
            tokio::time::timeout(
                Duration::from_millis(400),
                listener.accept()
            )
            .await
            .is_err(),
            "A refused token must not reconnect."
        );
    });
    let mut app = window(&url);
    app.insert_resource(Retry {
        first: Duration::from_millis(50),
        cap: Duration::from_millis(200),
    });
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Refused
    })
    .await;
    assert_eq!(
        app.world().resource::<Connection>().words,
        format!(
            "The server at {url} refused this window: That is not \
             this server's token."
        )
    );
    say(&mut app, "Do not send this");
    until(&mut app, |_| server.is_finished()).await;
    server.await.unwrap();
    assert_eq!(
        app.world().resource::<Connection>().state,
        State::Refused
    );
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
}

#[tokio::test]
async fn an_absent_server_explains_how_to_start_one_and_keeps_requests_in_the_outbox()
 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut app = window(&url);
    assert_eq!(
        app.world().resource::<Connection>().state,
        State::Connecting
    );
    assert_eq!(
        app.world().resource::<Connection>().words,
        format!("Connecting to {url}...")
    );
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Lost
    })
    .await;
    assert_eq!(
        app.world().resource::<Connection>().words,
        format!(
            "No server answered at {url}. Start one with hamlet \
             server."
        )
    );
    say(&mut app, "Keep this");
    app.update();
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
}

#[tokio::test]
async fn retries_double_to_the_cap_and_a_successful_hello_resets_the_wait()
 {
    use hamlet_client::connection::Retry;

    let defaults = Retry::default();
    assert_eq!(defaults.first, Duration::from_secs(1));
    assert_eq!(defaults.cap, Duration::from_secs(10));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (sent, mut intervals) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let mut attempts = Vec::new();
        for _ in 0..5 {
            let (stream, _) = listener.accept().await.unwrap();
            attempts.push(Instant::now());
            drop(stream);
        }
        sent.send(
            attempts
                .windows(2)
                .map(|pair| pair[1] - pair[0])
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        send(
            &mut socket,
            protocol::ServerMessage::State(realm::state()),
        )
        .await;
        let closed = Instant::now();
        socket.close(None).await.unwrap();
        drop(socket);
        let (stream, _) = listener.accept().await.unwrap();
        let elapsed = closed.elapsed();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        sent.send(vec![elapsed]).unwrap();
        window_closed(&mut socket).await;
    });
    let mut app = window(&url);
    app.insert_resource(Retry {
        first: Duration::from_millis(50),
        cap: Duration::from_millis(200),
    });
    let mut received = Vec::new();
    until(&mut app, |_| {
        received
            .extend(std::iter::from_fn(|| intervals.try_recv().ok()));
        received.len() == 2
    })
    .await;
    for (elapsed, expected) in
        received[0].iter().zip([50, 100, 200, 200])
    {
        assert!(
            *elapsed >= Duration::from_millis(expected - 5),
            "Retried too soon: {elapsed:?}"
        );
        assert!(
            *elapsed < Duration::from_millis(expected + 300),
            "Retried too late: {elapsed:?}"
        );
    }
    assert!(received[1][0] >= Duration::from_millis(45));
    assert!(
        received[1][0] < Duration::from_millis(160),
        "Hello did not reset the retry wait: {:?}",
        received[1][0]
    );
    drop(app);
    server.await.unwrap();
}

#[tokio::test]
async fn no_selected_server_leaves_the_outbox_alone() {
    let mut app = App::new();
    app.insert_resource(Tokio(tokio::runtime::Handle::current()));
    app.add_plugins(ClientPlugin { server: None });
    say(&mut app, "Wait for a server");
    for _ in 0..3 {
        app.update();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
    assert_eq!(
        app.world().resource::<Connection>().state,
        State::Lost
    );
    assert_eq!(
        app.world().resource::<Connection>().words,
        "No server selected. Start one with hamlet server."
    );
}

#[tokio::test]
async fn the_runtime_may_arrive_later() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let mut app = window(&url);
    app.world_mut().remove_resource::<Tokio>();
    say(&mut app, "Once the runtime arrives");
    app.update();
    assert!(
        tokio::time::timeout(
            Duration::from_millis(40),
            listener.accept()
        )
        .await
        .is_err()
    );
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
    let (sent, mut frames) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        sent.send(next(&mut socket).await).unwrap();
        sent.send(next(&mut socket).await).unwrap();
        window_closed(&mut socket).await;
    });
    app.insert_resource(Tokio(tokio::runtime::Handle::current()));
    let mut received = Vec::new();
    until(&mut app, |_| {
        received
            .extend(std::iter::from_fn(|| frames.try_recv().ok()));
        received.len() == 2
    })
    .await;
    assert!(matches!(received[0], protocol::ClientMessage::Hello(_)));
    assert!(matches!(received[1], protocol::ClientMessage::Say(_)));
    drop(app);
    server.await.unwrap();
}

#[tokio::test]
async fn a_socket_error_after_state_and_fault_reconnects_instead_of_refusing()
 {
    use hamlet_client::connection::Retry;
    use hamlet_client::messages::Reconnected;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        send(
            &mut socket,
            protocol::ServerMessage::State(realm::state()),
        )
        .await;
        send(
            &mut socket,
            protocol::ServerMessage::Fault(protocol::Fault {
                text: "The server's work failed.".into(),
                to: None,
            }),
        )
        .await;
        drop(socket);
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        window_closed(&mut socket).await;
    });
    let mut app = window(&url);
    app.insert_resource(Retry {
        first: Duration::from_millis(50),
        cap: Duration::from_millis(50),
    });
    app.init_resource::<Reconnections>();
    app.add_observer(
        |_: On<Reconnected>, mut count: ResMut<Reconnections>| {
            count.0 += 1
        },
    );
    until(&mut app, |world| world.resource::<Reconnections>().0 == 1)
        .await;
    assert_eq!(
        app.world().resource::<Connection>().state,
        State::Connected
    );
    assert_eq!(
        app.world_mut()
            .query::<&Settlement>()
            .iter(app.world())
            .count(),
        1
    );
    drop(app);
    server.await.unwrap();
}

#[tokio::test]
async fn a_refusal_is_read_even_while_the_outbox_is_backpressured() {
    use tokio::sync::oneshot;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (refuse, refused) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        refused.await.unwrap();
        send(
            &mut socket,
            protocol::ServerMessage::Fault(protocol::Fault {
                to: None,
                text: "Use this home's token.".into(),
            }),
        )
        .await;
        socket
            .close(Some(CloseFrame {
                code: coding::CloseCode::Policy,
                reason: "".into(),
            }))
            .await
            .unwrap();
        while let Some(Ok(message)) = socket.next().await {
            if matches!(message, Message::Close(_)) {
                break;
            }
        }
    });
    let mut app = window(&url);
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Connected
    })
    .await;
    for _ in 0..32 {
        say(&mut app, &"x".repeat(512 * 1024));
    }
    app.update();
    tokio::time::sleep(Duration::from_millis(30)).await;
    refuse.send(()).unwrap();
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Refused
    })
    .await;
    assert_eq!(
        app.world().resource::<Connection>().words,
        format!(
            "The server at {url} refused this window: Use this \
             home's token."
        )
    );
    server.await.unwrap();
}

#[tokio::test]
async fn only_policy_after_an_unaddressed_fault_refuses() {
    use hamlet_client::connection::Retry;
    use hamlet_client::messages::Reconnected;

    for (code, addressed) in [
        (Some(coding::CloseCode::Error), Some(false)),
        (Some(coding::CloseCode::Size), Some(false)),
        (Some(coding::CloseCode::Away), Some(false)),
        (None, Some(false)),
        (Some(coding::CloseCode::Policy), Some(true)),
        (Some(coding::CloseCode::Away), Some(true)),
        (Some(coding::CloseCode::Policy), None),
    ] {
        let listener =
            TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .unwrap();
            next(&mut socket).await;
            if let Some(addressed) = addressed {
                send(
                    &mut socket,
                    protocol::ServerMessage::Fault(protocol::Fault {
                        to: addressed.then(|| "a-request".into()),
                        text: "The work failed, not the token".into(),
                    }),
                )
                .await;
            }
            socket
                .close(code.map(|code| CloseFrame {
                    code,
                    reason: "".into(),
                }))
                .await
                .unwrap();
            drop(socket);
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .unwrap();
            assert!(matches!(
                next(&mut socket).await,
                protocol::ClientMessage::Hello(_)
            ));
            window_closed(&mut socket).await;
        });
        let mut app = window(&url);
        app.insert_resource(Retry {
            first: Duration::from_millis(25),
            cap: Duration::from_millis(25),
        });
        app.init_resource::<Reconnections>().add_observer(
            |_: On<Reconnected>, mut count: ResMut<Reconnections>| {
                count.0 += 1
            },
        );
        until(&mut app, |world| {
            assert_ne!(
                world.resource::<Connection>().state,
                State::Refused,
                "close {code:?}, addressed {addressed:?}"
            );
            world.resource::<Reconnections>().0 == 1
        })
        .await;
        drop(app);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn a_request_fault_after_skipped_state_and_close_retries() {
    use hamlet_client::connection::Retry;
    use hamlet_client::messages::Reconnected;
    use hamlet_client::realm::Seen;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        next(&mut socket).await;
        send(
            &mut socket,
            protocol::ServerMessage::State(realm::state()),
        )
        .await;
        next(&mut socket).await;
        socket.close(None).await.unwrap();
        drop(socket);
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        assert!(matches!(
            next(&mut socket).await,
            protocol::ClientMessage::Hello(_)
        ));
        let protocol::ClientMessage::Say(say) =
            next(&mut socket).await
        else {
            panic!("offline say");
        };
        send(
            &mut socket,
            protocol::ServerMessage::Fault(protocol::Fault {
                to: Some(say.id),
                text: "No agent named ghost".into(),
            }),
        )
        .await;
        socket
            .close(Some(CloseFrame {
                code: coding::CloseCode::Away,
                reason: "".into(),
            }))
            .await
            .unwrap();
        drop(socket);
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        next(&mut socket).await;
        window_closed(&mut socket).await;
    });
    let mut app = window(&url);
    app.insert_resource(Retry {
        first: Duration::from_millis(100),
        cap: Duration::from_millis(100),
    });
    app.init_resource::<Reconnections>().add_observer(
        |_: On<Reconnected>, mut count: ResMut<Reconnections>| {
            count.0 += 1
        },
    );
    until(&mut app, |world| world.contains_resource::<Seen>()).await;
    say(&mut app, "Close the first connection");
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Lost
    })
    .await;
    say(&mut app, "Queued while away");
    until(&mut app, |world| {
        assert_ne!(
            world.resource::<Connection>().state,
            State::Refused
        );
        world.resource::<Reconnections>().0 == 2
    })
    .await;
    drop(app);
    server.await.unwrap();
}

#[test]
fn the_server_address_debug_view_redacts_its_token() {
    let address = ServerAddress {
        url: "ws://127.0.0.1:7313".into(),
        token: "a-secret-window-token".into(),
    };
    for shown in [format!("{address:?}"), format!("{address:#?}")] {
        assert!(
            !shown.contains(&address.token),
            "the token escaped Debug"
        );
        assert!(shown.contains("[REDACTED]"));
        assert!(shown.contains(&address.url));
    }
}

#[tokio::test]
async fn invalid_addresses_stop_in_words_without_attempting_a_socket()
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    for (url, words) in [
        (
            format!("http://{}", listener.local_addr().unwrap()),
            "ws://",
        ),
        (format!("wss://{}", listener.local_addr().unwrap()), "TLS"),
        ("not a URL".into(), "WebSocket address"),
    ] {
        let mut app = window(&url);
        say(&mut app, "Keep this request");
        until(&mut app, |world| {
            world.resource::<Connection>().state == State::Lost
        })
        .await;
        let connection = app.world().resource::<Connection>();
        assert!(
            connection.words.contains(words),
            "{}",
            connection.words
        );
        assert!(!connection.words.contains("No server answered"));
        assert!(
            tokio::time::timeout(
                Duration::from_millis(150),
                listener.accept()
            )
            .await
            .is_err()
        );
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Outbox>().iter().count(),
            1
        );
    }
}

#[tokio::test]
async fn pongs_keep_a_socket_alive_and_silence_loses_it() {
    use hamlet_client::connection::{Heartbeat, Retry};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let (sent, mut pings) = mpsc::unbounded_channel();
    let (arrived, mut requests) = mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        next(&mut socket).await;
        for _ in 0..4 {
            let frame = socket.next().await.unwrap().unwrap();
            let Message::Ping(data) = frame else {
                panic!("expected a ping: {frame:?}");
            };
            socket.send(Message::Pong(data)).await.unwrap();
            sent.send(()).unwrap();
        }
        // Keeping the socket open without polling simulates a
        // half-open link.
        let (stream, _) = listener.accept().await.unwrap();
        let mut recovered =
            tokio_tungstenite::accept_async(stream).await.unwrap();
        arrived.send(next(&mut recovered).await).unwrap();
        arrived.send(next(&mut recovered).await).unwrap();
        window_closed(&mut recovered).await;
        drop(socket);
    });
    let mut app = window(&url);
    app.insert_resource(Heartbeat {
        every: Duration::from_millis(40),
        timeout: Duration::from_millis(120),
    });
    app.insert_resource(Retry {
        first: Duration::from_millis(100),
        cap: Duration::from_millis(100),
    });
    let mut count = 0;
    until(&mut app, |_| {
        count += std::iter::from_fn(|| pings.try_recv().ok()).count();
        count == 4
    })
    .await;
    assert_eq!(
        app.world().resource::<Connection>().state,
        State::Connected
    );
    until(&mut app, |world| {
        world.resource::<Connection>().state == State::Lost
    })
    .await;
    assert_eq!(
        app.world().resource::<Connection>().words,
        format!("Lost the server at {url}; trying again.")
    );
    let id = say(&mut app, "After the half-open socket");
    app.update();
    assert_eq!(app.world().resource::<Outbox>().iter().count(), 1);
    let mut received = Vec::new();
    until(&mut app, |_| {
        received
            .extend(std::iter::from_fn(|| requests.try_recv().ok()));
        received.len() == 2
    })
    .await;
    assert!(matches!(received[0], protocol::ClientMessage::Hello(_)));
    let protocol::ClientMessage::Say(say) = &received[1] else {
        panic!("queued say");
    };
    assert_eq!(say.id, id);
    assert_eq!(say.text, "After the half-open socket");
    drop(app);
    server.await.unwrap();
}
