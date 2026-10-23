//! The `hamlet-server` executable serving a home: crashed, killed
//! and driven through its fault points and the pi harness. Every
//! server here starts pi for its model providers, so the target needs
//! Node and the `harness` feature.

#![cfg(unix)]

mod remote;
mod society;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use hamlet_server::{Held, Home};
use hamlet_testing::openai::{Endpoint, Response};
use hamlet_testing::{
    Models, Scratch, Server, Socket, answers, strip_overrides,
};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// The `hamlet-server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet-server");

/// Starts [`SERVER`] on `home`.
async fn start(
    home: &Path,
    env: &[(&str, &str)],
    models: Models<'_>,
) -> Server {
    Server::start(&[SERVER.into()], home, models, env)
        .await
        .unwrap_or_else(|failure| panic!("{failure}"))
}

#[test]
fn the_server_holds_its_home_until_terminated() {
    let dir = Scratch::new("server-binary");
    let mut server = strip_overrides(&mut Command::new(SERVER))
        .args(["--home".as_ref(), dir.as_os_str()])
        .args(["--port", "0"])
        .env_remove("HAMLET_HOME")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(server.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.contains("holding"), "{line}");
    let refused = Home::open(&dir).err().unwrap();
    assert!(refused.downcast_ref::<Held>().is_some(), "{refused}");
    // SAFETY: kill sends a signal to the child's PID and has no
    // preconditions.
    let sent =
        unsafe { libc::kill(server.id() as i32, libc::SIGTERM) };
    assert_eq!(sent, 0);
    assert!(server.wait().unwrap().success());
    let mut home = Home::open(&dir).unwrap();
    assert!(!dir.join("server.json").exists());
    assert_eq!(
        home.store().record("home").unwrap().unwrap().agents[0].name,
        "founder"
    );
}

#[tokio::test]
async fn requests_sent_before_close_are_kept_in_order() {
    let home = Scratch::new("protocol-disconnected-edit");
    let mut server = start(&home, &[], Models::Providers).await;
    let (mut socket, _) = server.connect().await;
    for text in ["first", "second", "last"] {
        socket.send(Message::Text(json!({"type":"edit","doc":"charter","id":uuid::Uuid::new_v4().to_string(),"text":text}).to_string().into())).await.unwrap();
    }
    socket.close(None).await.unwrap();
    drop(socket);
    let (mut reader, _) = server.connect().await;
    tokio::time::timeout(Duration::from_secs(3),async {
        loop {
            reader.send(Message::Text(json!({"type":"read","what":"charter","id":uuid::Uuid::new_v4().to_string()}).to_string().into())).await.unwrap();
            let result = reader.next().await.unwrap().unwrap();
            let result = serde_json::from_str::<Value>(result.to_text().unwrap()).unwrap();
            if result["charter"]["text"] == "last" { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.unwrap();
    drop(reader);
    assert_eq!(server.stop().await, Some(0));
}

#[tokio::test]
async fn a_request_whose_handler_fails_is_faulted_and_the_next_answered()
 {
    let home = Scratch::new("server-handler-fails");
    let mut server = start(&home, &[], Models::Providers).await;
    let (mut socket, _) = server.connect().await;
    // The founder's house becomes a file, so opening it fails.
    let house = home.join("houses/home/0");
    std::fs::rename(&house, home.join("house-moved-away")).unwrap();
    std::fs::write(&house, "not a folder").unwrap();
    let edit = json!({"type": "edit", "doc": "notes",
        "id": uuid::Uuid::new_v4().to_string(),
        "agent": {"hamlet": "home", "agent": "founder"},
        "text": "Kept?"});
    let read = json!({"type": "read", "what": "charter",
        "id": uuid::Uuid::new_v4().to_string()});
    let answers = answers(&mut socket, &[&edit, &read]).await;
    assert_eq!(answers[0]["type"], "fault", "{answers:?}");
    assert_eq!(answers[0]["to"], edit["id"]);
    assert_eq!(
        answers[0]["text"],
        "The server could not complete this request."
    );
    assert_eq!(answers[1]["type"], "read", "{answers:?}");
    drop(socket);
    assert_eq!(server.stop().await, Some(0));
}

#[tokio::test]
async fn overrides_and_secrets_stay_in_memory_but_the_chief_is_retained()
 {
    let home = Scratch::new("protocol-overrides");
    let key = "process-only-key-123456789";
    let mut server = start(
        &home,
        &[
            ("HAMLET_MODEL", "local::other"),
            ("HAMLET_BASE_URL", "http://localhost:9999/v1"),
            ("HAMLET_REFLECT_EVERY", "+2"),
            ("HAMLET_NET", "1"),
            ("HAMLET_API_KEY", key),
        ],
        Models::Providers,
    )
    .await;
    let (mut socket, state) = server.connect().await;
    assert_eq!(state["settings"]["model"], "local::other");
    assert_eq!(
        state["settings"]["endpoints"][0]["baseUrl"],
        "http://localhost:9999/v1"
    );
    assert_eq!(state["settings"]["reflectEvery"], 2);
    assert_eq!(state["settings"]["allowNet"], true);
    socket
        .send(Message::Text(json!({"type":key}).to_string().into()))
        .await
        .unwrap();
    let answer = socket.next().await.unwrap().unwrap();
    assert!(answer.to_text().unwrap().contains("[REDACTED]"));
    assert!(!answer.to_text().unwrap().contains(key));
    drop(socket);
    assert_eq!(server.stop().await, Some(0));
    let token = server.token.clone();
    drop(server);
    let mut server = start(
        &home,
        &[("HAMLET_USER", "someone-else")],
        Models::Providers,
    )
    .await;
    assert_eq!(server.token, token);
    let (socket, state) = server.connect().await;
    assert_eq!(state["chief"], "chief");
    assert_eq!(state["settings"]["model"], "local::qwen3.6:35b-a3b");
    assert_eq!(
        state["settings"]["endpoints"][0]["baseUrl"],
        "http://localhost:11434/v1"
    );
    assert_eq!(state["settings"]["reflectEvery"], 10);
    assert_eq!(state["settings"]["allowNet"], false);
    drop(socket);
    assert_eq!(server.stop().await, Some(0));
    for file in ["realm.sqlite", "credentials.json"] {
        let bytes = std::fs::read(home.join(file)).unwrap();
        assert!(
            !bytes.windows(key.len()).any(|w| w == key.as_bytes())
        );
    }
}

#[tokio::test]
async fn ctrl_c_stops_even_with_unresponsive_clients_and_a_second_interrupt_forces_exit()
 {
    for twice in [false, true] {
        let home = Scratch::new("protocol-interrupt");
        let mut server = start(&home, &[], Models::Providers).await;
        let (mut reader, _) = server.connect().await;
        let (_unresponsive, _) = server.connect().await;
        server.interrupt();
        let closed = tokio::time::timeout(
            Duration::from_secs(2),
            reader.next(),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        assert!(
            matches!(closed,Message::Close(Some(frame)) if u16::from(frame.code)==1001)
        );
        if twice {
            server.interrupt();
        }
        assert_eq!(
            server.ended().await,
            Some(if twice { 130 } else { 0 })
        );
        if !twice {
            assert!(!home.join("server.json").exists());
        }
    }
}

#[tokio::test]
async fn a_request_whose_handler_panics_is_faulted_and_the_server_goes_on()
 {
    let home = Scratch::new("server-handler-panics");
    let faults = home.join("faults");
    std::fs::create_dir_all(&faults).unwrap();
    let mut server = start(
        &home,
        &[
            ("HAMLET_FAULT", "edit_document"),
            ("HAMLET_FAULT_DIR", faults.to_str().unwrap()),
        ],
        Models::Providers,
    )
    .await;
    let published = std::fs::read(home.join("server.json")).unwrap();
    let (mut socket, _) = server.connect().await;
    let edit = json!({"type": "edit", "doc": "charter",
        "id": uuid::Uuid::new_v4().to_string(), "text": "Kept?"});
    let read = json!({"type": "read", "what": "charter",
        "id": uuid::Uuid::new_v4().to_string()});
    let answers = answers(&mut socket, &[&edit, &read]).await;
    assert!(faults.join("server-panicked").exists());
    assert_eq!(answers[0]["type"], "fault", "{answers:?}");
    assert_eq!(answers[0]["to"], edit["id"]);
    assert_eq!(
        answers[0]["text"],
        "The server could not complete this request."
    );
    assert_eq!(answers[1]["type"], "read", "{answers:?}");
    assert_ne!(answers[1]["charter"]["text"], "Kept?");
    assert!(server.running());
    assert_eq!(
        std::fs::read(home.join("server.json")).unwrap(),
        published
    );
    drop(socket);
    assert_eq!(server.stop().await, Some(0));
    let log = server.log();
    // The fault is the caught panic's, not some other failure's.
    assert!(
        log.contains(
            "the handler panicked: the fault point edit_document \
             panics"
        ),
        "{log}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_known_credentials_are_absent_from_both_stores() {
    let endpoint = Endpoint::start().await;
    endpoint.enqueue([Response {
        text: "Hello sk-other-secret.".into(),
        ..Default::default()
    }]);
    let home = Scratch::new("server-other-credential");
    std::fs::write(
        home.join("credentials.json"),
        json!({
            "installationId": "scratch",
            "connection": {"token": "scratch-connection-token"},
            "models": {
                "local": {"type": "api_key", "key": "sk-held"},
                "other": {"type": "api_key", "key": "sk-other-secret"}
            }
        })
        .to_string(),
    )
    .unwrap();
    let mut server = start(
        &home,
        &[
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", &endpoint.url),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
        Models::Providers,
    )
    .await;
    let (mut socket, _) = server.connect().await;
    let request = uuid::Uuid::new_v4().to_string();
    for message in [
        json!({"type": "open", "id": uuid::Uuid::new_v4().to_string(),
            "chat": {"with": {"hamlet": "home", "agent": "mayor"}}}),
        json!({"type": "say", "id": request,
            "to": {"hamlet": "home", "agent": "mayor"}, "text": "Hello?"}),
    ] {
        socket
            .send(Message::Text(message.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        let mut answered = false;
        let mut shown = false;
        while !answered || !shown {
            let frame = socket.next().await.unwrap().unwrap();
            let text = frame.to_text().unwrap();
            assert!(!text.contains("sk-other-secret"), "{text}");
            assert!(!text.contains("sk-held"), "{text}");
            shown |= text.contains("Hello [REDACTED].");
            let message =
                serde_json::from_str::<Value>(text).unwrap();
            if message["type"] == "reply" && message["to"] == request
            {
                assert_eq!(message["kind"], "notice", "{message}");
                answered = true;
            }
        }
    })
    .await
    .unwrap();
    drop(socket);
    assert_eq!(server.stop().await, Some(0));
    let mut files = vec![home.join("realm.sqlite")];
    for runtime in std::fs::read_dir(home.join("harnesses")).unwrap()
    {
        for file in
            std::fs::read_dir(runtime.unwrap().path()).unwrap()
        {
            files.push(file.unwrap().path());
        }
    }
    assert!(files.len() > 1, "the Pi store must exist");
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        for secret in [b"sk-held".as_slice(), b"sk-other-secret"] {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|part| part == secret),
                "credential persisted in {}",
                file.display()
            );
        }
    }
}
