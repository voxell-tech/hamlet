//! The `hamlet-server` executable.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use hamlet_server::{Held, Home};
use hamlet_testing::Scratch;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

#[cfg(feature = "harness")]
#[path = "server/society.rs"]
mod society;

#[cfg(feature = "harness")]
#[path = "server/oauth.rs"]
mod oauth;

#[cfg(feature = "harness")]
#[path = "server/remote.rs"]
mod remote;

#[test]
fn the_server_holds_its_home_until_terminated() {
    let dir = Scratch::new("server-binary");
    let mut server =
        Command::new(env!("CARGO_BIN_EXE_hamlet-server"))
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

struct Server {
    child: std::process::Child,
    url: String,
    token: String,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(home: &Scratch, env: &[(&str, &str)]) -> Self {
        Self::start_with_script(home, env, None)
    }

    fn start_with_script(
        home: &Scratch,
        env: &[(&str, &str)],
        script: Option<&std::path::Path>,
    ) -> Self {
        let mut command =
            Command::new(env!("CARGO_BIN_EXE_hamlet-server"));
        if let Some(script) = script {
            command.arg("--faux").arg(script);
        }
        for key in [
            "HAMLET_MODEL",
            "HAMLET_BASE_URL",
            "HAMLET_API_KEY",
            "HAMLET_REFLECT_EVERY",
            "HAMLET_NET",
        ] {
            command.env_remove(key);
        }
        let mut child = command
            .args(["--port", "0"])
            .env("HAMLET_HOME", home.as_os_str())
            .env("HAMLET_USER", "chief")
            .envs(env.iter().copied())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        let published = loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "server exited during startup"
            );
            if let Ok(bytes) = std::fs::read(home.join("server.json"))
                && let Ok(value) =
                    serde_json::from_slice::<Value>(&bytes)
                && value["pid"] == child.id()
            {
                break value;
            }
            assert!(Instant::now() < until, "server did not publish");
            std::thread::sleep(Duration::from_millis(10));
        };
        let credentials = serde_json::from_slice::<Value>(
            &std::fs::read(home.join("credentials.json")).unwrap(),
        )
        .unwrap();
        Self {
            child,
            url: published["url"].as_str().unwrap().into(),
            token: credentials["connection"]["token"]
                .as_str()
                .unwrap()
                .into(),
        }
    }

    fn signal(&self, signal: i32) {
        assert_eq!(
            // SAFETY: kill sends a signal to this child's PID.
            unsafe { libc::kill(self.child.id() as i32, signal) },
            0
        );
    }

    async fn exit(&mut self) -> i32 {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    break status.code().unwrap();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("server failed to stop")
    }

    async fn connect(
        &self,
    ) -> (
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        Value,
    ) {
        let (mut socket, _) =
            tokio_tungstenite::connect_async(&self.url)
                .await
                .unwrap();
        socket
            .send(Message::Text(
                json!({"type":"hello","v":0,"token":self.token})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let state = tokio::time::timeout(
            Duration::from_secs(3),
            socket.next(),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap();
        let state =
            serde_json::from_str::<Value>(state.to_text().unwrap())
                .unwrap();
        assert_eq!(state["type"], "state");
        (socket, state)
    }
}

#[tokio::test]
async fn requests_sent_before_close_are_kept_in_order() {
    let home = Scratch::new("protocol-disconnected-edit");
    let mut server = Server::start(&home, &[]);
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
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
}

#[tokio::test]
async fn overrides_and_secrets_stay_in_memory_but_the_chief_is_retained()
 {
    let home = Scratch::new("protocol-overrides");
    let key = "process-only-key-123456789";
    let mut server = Server::start(
        &home,
        &[
            ("HAMLET_MODEL", "local::other"),
            ("HAMLET_BASE_URL", "http://localhost:9999/v1"),
            ("HAMLET_REFLECT_EVERY", "+2"),
            ("HAMLET_NET", "1"),
            ("HAMLET_API_KEY", key),
        ],
    );
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
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    let token = server.token.clone();
    drop(server);
    let mut server =
        Server::start(&home, &[("HAMLET_USER", "someone-else")]);
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
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
    for file in ["realm.sqlite", "credentials.json"] {
        let bytes = std::fs::read(home.join(file)).unwrap();
        assert!(
            !bytes.windows(key.len()).any(|w| w == key.as_bytes())
        );
    }
}

#[test]
fn a_missing_pi_harness_is_refused() {
    let home = Scratch::new("server-no-pi");
    let entry = home.join("missing/main.ts");
    let words = refusal(
        &home,
        &[("HAMLET_PI_HARNESS", entry.to_str().unwrap())],
    );
    assert!(
        words.contains(&format!(
            "HAMLET_PI_HARNESS names {}, which is not a file.",
            entry.display()
        )),
        "{words}"
    );
}

/// The words `hamlet-server` gives on standard error as it refuses to
/// start on `home` with `env`.
fn refusal(home: &Scratch, env: &[(&str, &str)]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_hamlet-server"))
        .args(["--home".as_ref(), home.as_os_str()])
        .args(["--port", "0"])
        .env_remove("HAMLET_HOME")
        .env_remove("HAMLET_MODEL")
        .env_remove("HAMLET_BASE_URL")
        .env("HAMLET_USER", "chief")
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(!output.status.success());
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn a_model_not_named_endpoint_and_id_is_refused() {
    let home = Scratch::new("server-bad-model");
    let words = refusal(&home, &[("HAMLET_MODEL", "garbage")]);
    assert!(
        words.contains(
            "HAMLET_MODEL is `endpoint::id`, not `garbage`"
        ),
        "{words}"
    );
}

#[test]
fn a_home_record_without_a_chief_is_refused() {
    use hamlet_server::record::FoundRecord;
    use hamlet_server::{OperationId, RuntimeInstance};

    let dir = Scratch::new("server-chiefless");
    let mut home = Home::open(&dir).unwrap();
    let id = OperationId {
        runtime: RuntimeInstance::new("seed").unwrap(),
        key: "found".into(),
    };
    let found = FoundRecord {
        hamlet: "home".into(),
        name: "founder".into(),
        purpose: String::new(),
        chief: "chief".into(),
        at: 0,
    };
    home.store().run(&id, &found).unwrap();
    home.store()
        .connection()
        .execute(
            "UPDATE record_hamlet SET document = json_set(document, \
             '$.chiefs', json('[]')) WHERE id = 'home'",
            [],
        )
        .unwrap();
    home.close().unwrap();
    let words = refusal(&dir, &[]);
    assert!(
        words.contains("The home hamlet's record names no chief."),
        "{words}"
    );
}

#[test]
fn a_new_home_without_a_chief_name_is_refused() {
    let home = Scratch::new("server-no-chief");
    let output = Command::new(env!("CARGO_BIN_EXE_hamlet-server"))
        .args(["--home".as_ref(), home.as_os_str()])
        .args(["--port", "0"])
        .env_remove("HAMLET_HOME")
        .env_remove("HAMLET_USER")
        .env_remove("USER")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let words = String::from_utf8(output.stderr).unwrap();
    assert!(
        words.contains(
            "A new home needs its chief's name: set HAMLET_USER or \
             USER."
        ),
        "{words}"
    );
}

#[test]
fn a_base_url_for_an_endpoint_the_settings_do_not_list_is_refused() {
    let home = Scratch::new("server-unlisted-base-url");
    let words = refusal(
        &home,
        &[
            ("HAMLET_MODEL", "elsewhere::large"),
            ("HAMLET_BASE_URL", "http://localhost:9999/v1"),
        ],
    );
    assert!(
        words.contains(
            "HAMLET_BASE_URL is for the endpoint of the model \
             `elsewhere::large`, which the settings do not list."
        ),
        "{words}"
    );
    let words = refusal(
        &home,
        &[
            ("HAMLET_MODEL", "anthropic::large"),
            ("HAMLET_BASE_URL", "http://localhost:9999/v1"),
        ],
    );
    assert!(
        words.contains(
            "HAMLET_BASE_URL is for the endpoint of the model \
             `anthropic::large`, a provider pi knows the address \
             of. Unset HAMLET_BASE_URL."
        ),
        "{words}"
    );
    // With no model chosen in Settings and none in HAMLET_MODEL.
    let none = Scratch::new("server-base-url-no-model");
    let mut store = Home::open(&none).unwrap();
    let settings = json!({
        "model": "", "reflectEvery": 10, "allowNet": false,
        "endpoints": [], "saved": true,
        "link": {"state": "untested", "models": null, "words": null},
    });
    store
        .store()
        .connection()
        .execute(
            "INSERT INTO chief_document(kind, key, value) VALUES \
             ('settings', 'home', ?1)",
            [settings.to_string()],
        )
        .unwrap();
    store.close().unwrap();
    let words = refusal(
        &none,
        &[("HAMLET_BASE_URL", "http://localhost:9999/v1")],
    );
    assert!(
        words.contains(
            "HAMLET_BASE_URL needs a model: set HAMLET_MODEL or \
             choose one in Settings."
        ),
        "{words}"
    );
}

#[cfg(feature = "harness")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_known_credentials_are_absent_from_both_stores() {
    use hamlet_acceptance::openai::{Endpoint, Response};

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
    let mut server = Server::start(
        &home,
        &[
            ("HAMLET_MODEL", "local::org/large"),
            ("HAMLET_BASE_URL", &endpoint.url),
            ("HAMLET_REFLECT_EVERY", "0"),
        ],
    );
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
    server.signal(libc::SIGTERM);
    assert_eq!(server.exit().await, 0);
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

#[tokio::test]
async fn ctrl_c_stops_even_with_unresponsive_clients_and_a_second_interrupt_forces_exit()
 {
    for twice in [false, true] {
        let home = Scratch::new("protocol-interrupt");
        let mut server = Server::start(&home, &[]);
        let (mut reader, _) = server.connect().await;
        let (_unresponsive, _) = server.connect().await;
        server.signal(libc::SIGINT);
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
            server.signal(libc::SIGINT);
        }
        assert_eq!(server.exit().await, if twice { 130 } else { 0 });
        if !twice {
            assert!(!home.join("server.json").exists());
        }
    }
}
