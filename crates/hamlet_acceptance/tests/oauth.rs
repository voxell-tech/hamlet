//! Public sign-in and refresh through the real server and Pi, with
//! offline providers.

use std::path::Path;
use std::time::Duration;

use hamlet_testing::{Home, Scratch, Server, Socket, send};
use serde_json::{Value, json};

const SECRETS: &[&str] = &[
    "test-oauth-access-before",
    "test-oauth-refresh-before",
    "test-oauth-access-after",
    "test-oauth-refresh-after",
    "test-other-provider-secret",
    "test-openai-separate-key",
    "reject-transient-code",
];

async fn start(home: &mut Home, fixture: &Scratch) -> Server {
    std::fs::write(home.file("credentials.json"), json!({
        "installationId":"stable-offline-installation", "connection":{"token":"offline-connection"},
        "models": {"openai":{"type":"api_key","key":"test-openai-separate-key"},
            "other":{"type":"api_key","key":"test-other-provider-secret"}}
    }).to_string()).unwrap();
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../harnesses/pi/test/fixtures/oauth.ts");
    home.serve_providers(&[
        ("HAMLET_PI_HARNESS", entry.to_str().unwrap()),
        ("HAMLET_OAUTH_FIXTURE", fixture.to_str().unwrap()),
        ("HAMLET_REFLECT_EVERY", "0"),
        ("HAMLET_MODEL", "chatgpt::offline"),
        ("OPENAI_API_KEY", "ambient-must-not-be-used"),
    ])
    .await
}

/// [`hamlet_testing::until`], checking that no message on the way
/// carries a secret.
async fn until(
    socket: &mut Socket,
    check: impl Fn(&Value) -> bool,
) -> Value {
    hamlet_testing::until(socket, |value| {
        let text = value.to_string();
        for secret in SECRETS {
            assert!(
                !text.contains(secret),
                "secret in public protocol"
            );
        }
        check(value)
    })
    .await
}

async fn login(
    socket: &mut Socket,
    key: &str,
    provider: &str,
) -> String {
    let key = send(socket, json!({"type":"login", "id":key,"start":{"provider":provider,"method":"subscription"}})).await;
    let mut steps = Vec::new();
    let prompt =
        until(socket, |m| m["type"] == "login" && m["to"] == key)
            .await;
    steps.push(prompt);
    while steps.last().unwrap()["step"]["prompt"].is_null() {
        assert!(
            steps.last().unwrap()["step"]["failed"].is_null(),
            "{steps:?}"
        );
        steps.push(
            until(socket, |m| m["type"] == "login" && m["to"] == key)
                .await,
        );
    }
    assert!(
        steps
            .iter()
            .any(|m| m["step"]["url"]
                == "https://example.invalid/oauth")
    );
    assert!(steps.iter().any(|m| m["step"]["code"] == "SAFE-CODE"));
    assert!(
        steps.iter().any(|m| m["step"]["progress"]
            == "Waiting for the offline code")
    );
    let prompt = steps.last().unwrap();
    assert_eq!(prompt["step"]["secret"], true);
    prompt["login"].as_str().unwrap().into()
}

async fn reply(
    socket: &mut Socket,
    login: &str,
    value: &str,
) -> Value {
    send(socket, json!({"type":"login","id":"answer","reply":{"login":login,"value":value}})).await;
    until(socket, |m| {
        m["login"] == login
            && (!m["step"]["done"].is_null()
                || !m["step"]["failed"].is_null())
    })
    .await
}

async fn file(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("fixture barrier");
}

/// Waits until `server` refuses connections: it drops its listener
/// as it takes a signal, before it closes the rest.
async fn stopped_listening(server: &Server) {
    let address = server.url.trim_start_matches("ws://");
    tokio::time::timeout(Duration::from_secs(10), async {
        while std::net::TcpStream::connect(address).is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the server stops listening");
}

fn stored(home: &Path) -> Value {
    serde_json::from_slice(
        &std::fs::read(home.join("credentials.json")).unwrap(),
    )
    .unwrap()
}

fn no_store_secrets(home: &Path) {
    let mut files = vec![home.join("realm.sqlite")];
    files.extend(
        ["realm.sqlite-wal", "realm.sqlite-shm"]
            .map(|name| home.join(name))
            .into_iter()
            .filter(|path| path.exists()),
    );
    for runtime in std::fs::read_dir(home.join("harnesses")).unwrap()
    {
        for entry in
            std::fs::read_dir(runtime.unwrap().path()).unwrap()
        {
            files.push(entry.unwrap().path());
        }
    }
    assert!(files.len() > 1);
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        for secret in SECRETS {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|part| part == secret.as_bytes()),
                "credential in {}",
                path.display()
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn oauth_is_client_owned_and_prompts_cancel_on_disconnect_and_shutdown()
 {
    let mut home = Home::new();
    let fixture = Scratch::new("oauth-login-provider");
    let mut server = start(&mut home, &fixture).await;
    let (mut owner, state) = server.connect().await;
    let (mut foreign, _) = server.connect().await;
    let providers = state["providers"].as_array().unwrap();
    assert_eq!(
        providers.iter().find(|p| p["id"] == "chatgpt").unwrap()
            ["methods"],
        json!(["subscription"])
    );
    assert_eq!(
        providers.iter().find(|p| p["id"] == "openai").unwrap()
            ["methods"],
        json!(["key"])
    );
    let pending = login(&mut owner, "owned", "chatgpt").await;
    let foreign_id = send(&mut foreign, json!({"type":"login","reply":{"login":pending,"value":"not-owner"}})).await;
    assert_eq!(
        until(&mut foreign, |m| {
            assert_ne!(
                m["type"], "login",
                "another client received private sign-in progress"
            );
            m["to"] == foreign_id
        })
        .await["kind"],
        "error"
    );
    let failed =
        reply(&mut owner, &pending, "reject-transient-code").await;
    assert!(
        failed["step"]["failed"]
            .as_str()
            .unwrap()
            .contains("[REDACTED]")
    );
    assert!(stored(home.path())["models"]["chatgpt"].is_null());

    let pending = login(&mut owner, "cancel", "chatgpt").await;
    send(&mut owner, json!({"type":"login","id":"cancel-request","cancel":pending})).await;
    assert_eq!(
        until(&mut owner, |m| m["login"] == pending).await["step"]
            ["failed"],
        "Sign-in cancelled."
    );
    let _pending = login(&mut owner, "disconnect", "chatgpt").await;
    owner.close(None).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if std::fs::read_to_string(fixture.join("login-ended"))
                .unwrap_or_default()
                .lines()
                .count()
                == 3
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let _pending =
        login(&mut foreign, "shutdown", "openrouter").await;
    assert_eq!(server.stop().await, Some(0));
    assert_eq!(
        std::fs::read_to_string(fixture.join("login-ended"))
            .unwrap()
            .lines()
            .count(),
        4
    );
    assert!(stored(home.path())["models"]["chatgpt"].is_null());
    assert!(stored(home.path())["models"]["openrouter"].is_null());
    no_store_secrets(home.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_waits_for_a_model_calls_started_rotation_to_persist()
 {
    let mut home = Home::new();
    let fixture = Scratch::new("oauth-shutdown-provider");
    let mut server = start(&mut home, &fixture).await;
    let (mut owner, _) = server.connect().await;
    let pending = login(&mut owner, "sign-in", "chatgpt").await;
    assert_eq!(
        reply(&mut owner, &pending, "expire").await["step"]["done"],
        "chatgpt"
    );
    std::fs::write(fixture.join("hold-refresh"), "hold").unwrap();
    send(&mut owner, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Hello"})).await;
    file(&fixture.join("refresh-started")).await;
    server.terminate();
    stopped_listening(&server).await;
    assert!(
        server.is_running(),
        "shutdown abandoned a started rotation"
    );
    std::fs::write(fixture.join("release-refresh"), "release")
        .unwrap();
    assert_eq!(server.ended().await, Some(0));
    assert_eq!(
        stored(home.path())["models"]["chatgpt"]["refresh"],
        "test-oauth-refresh-after"
    );
    no_store_secrets(home.path());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_refresh_persists_after_disconnect_and_retired_tokens_never_reach_either_store()
 {
    let mut home = Home::new();
    let fixture = Scratch::new("oauth-provider");
    let mut server = start(&mut home, &fixture).await;
    let (mut first, _) = server.connect().await;
    let (mut second, _) = server.connect().await;
    let pending = login(&mut first, "sign-in", "chatgpt").await;
    assert_eq!(
        reply(&mut first, &pending, "expire").await["step"]["done"],
        "chatgpt"
    );
    let before = stored(home.path());
    assert_eq!(
        before["models"]["chatgpt"]["clientId"],
        "stable-offline-installation"
    );
    assert_eq!(before["models"]["chatgpt"]["agent"], "Hamlet");
    std::fs::write(fixture.join("hold-refresh"), "hold").unwrap();
    send(&mut first, json!({"type":"check","what":"connection"}))
        .await;
    file(&fixture.join("refresh-started")).await;
    let check = send(&mut second, json!({"type":"check","what":"model","model":"chatgpt::offline"})).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        while std::fs::read_to_string(fixture.join("acquires"))
            .unwrap()
            .lines()
            .count()
            < 3
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    first.close(None).await.unwrap();
    std::fs::write(fixture.join("release-refresh"), "release")
        .unwrap();
    let checked = until(&mut second, |m| m["to"] == check).await;
    assert_eq!(checked["kind"], "notice", "{checked}");
    assert!(
        checked["text"]
            .as_str()
            .unwrap()
            .contains("OAuth echoed [REDACTED]")
    );
    let after = stored(home.path());
    assert_eq!(
        after["models"]["chatgpt"]["access"],
        "test-oauth-access-after"
    );
    assert_eq!(
        after["models"]["chatgpt"]["refresh"],
        "test-oauth-refresh-after"
    );
    assert_eq!(
        after["models"]["chatgpt"]["scopes"],
        json!(["offline", "profile"])
    );
    assert_eq!(after["models"]["openai"], before["models"]["openai"]);
    assert_eq!(
        std::fs::read_to_string(fixture.join("refresh.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    send(&mut second, json!({"type":"open","id":"open-chat","chat":{"with":{"hamlet":"home","agent":"mayor"}}})).await;
    let chat = send(&mut second, json!({"type":"say","to":{"hamlet":"home","agent":"mayor"},"text":"Hello"})).await;
    until(&mut second, |m| {
        m.to_string().contains(
            "OAuth echoed [REDACTED] [REDACTED] [REDACTED] \
             [REDACTED] [REDACTED].",
        )
    })
    .await;
    until(&mut second, |m| m["to"] == chat).await;
    let check = send(&mut second, json!({"type":"check","what":"model","model":"openai::offline"})).await;
    assert!(
        until(&mut second, |m| m["to"] == check).await["text"]
            .as_str()
            .unwrap()
            .contains("OAuth echoed [REDACTED]")
    );
    let requests =
        std::fs::read_to_string(fixture.join("requests.jsonl"))
            .unwrap();
    let requests = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 3, "{requests:?}");
    assert_eq!(requests[2]["provider"], "openai");
    assert_eq!(requests[2]["key"], "test-openai-separate-key");
    for request in &requests[..2] {
        assert_eq!(request["key"], "test-oauth-access-after");
        assert_eq!(
            request["headers"]["x-refresh-control"],
            "test-oauth-refresh-after"
        );
    }
    no_store_secrets(home.path());
    drop(second);
    assert_eq!(server.stop().await, Some(0));
    no_store_secrets(home.path());
}
