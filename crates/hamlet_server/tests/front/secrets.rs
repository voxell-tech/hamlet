//! The realm's secrets across restarts of the door: once a credential
//! changed, a command and a read the gate weighs run after a restart
//! as they ran before it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use diesel::sql_types::BigInt;
use diesel::{
    Connection, QueryableByName, RunQueryDsl, SqliteConnection,
};
use hamlet_acceptance::{Client, chat};
use hamlet_protocol::client::{
    Check, ClientMessage, ItemKind, Login, LoginMethod, LoginReply,
    LoginStart, LoginStep, ReplyKind, RequestId,
};
use hamlet_testing::Server;
use serde_json::{Value, json};

use super::{
    KEY, PATIENCE, SIGN_IN, finish_sign_in, frames, keeps_to_itself,
    login_step, serve_again, serve_with, start_sign_in,
};

/// The demo's OAuth login up to its quote, which a frame escapes.
const OAUTH: [&str; 2] =
    ["made-up-oauth-access-", "made-up-oauth-refresh-"];

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_holds_across_a_restart() {
    let (server, home, side) =
        serve_with(json!([]), &[SIGN_IN]).await;
    let mut life = Life::begin(server).await;
    life.noted().await;
    life.signed_in().await;
    life.gated("before").await;
    let mut seen = life.stop().await;
    settled(&home);
    let mut life = Life::again(&home, &side).await;
    life.gated("after").await;
    seen.push(life.stop().await);
    seen.hold_none(&home, &[KEY]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_holds_across_two_restarts() {
    let (server, home, side) =
        serve_with(json!([]), &[SIGN_IN]).await;
    let mut life = Life::begin(server).await;
    life.noted().await;
    life.signed_in().await;
    life.gated("before").await;
    let mut seen = life.stop().await;
    settled(&home);
    for word in ["once", "twice"] {
        let mut life = Life::again(&home, &side).await;
        life.gated(word).await;
        seen.push(life.stop().await);
    }
    seen.hold_none(&home, &[KEY]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_holds_across_a_crash() {
    let (server, home, side) =
        serve_with(json!([]), &[SIGN_IN]).await;
    let mut life = Life::begin(server).await;
    life.noted().await;
    life.signed_in().await;
    life.gated("before the crash").await;
    let mut seen = life.kill();
    settled(&home);
    let mut life = Life::again(&home, &side).await;
    life.gated("after the crash").await;
    seen.push(life.stop().await);
    seen.hold_none(&home, &[KEY]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refreshed_login_holds_across_a_restart() {
    let (server, home, side) =
        serve_with(json!([]), &[SIGN_IN]).await;
    let mut life = Life::begin(server).await;
    life.noted().await;
    life.subscribed().await;
    life.gated("before").await;
    let mut seen = life.stop().await;
    settled(&home);
    expire(&home);
    let mut life = Life::again(&home, &side).await;
    life.refreshed(&home).await;
    life.gated("after the refresh").await;
    seen.push(life.stop().await);
    settled(&home);
    let mut life = Life::again(&home, &side).await;
    life.gated("after a restart").await;
    seen.push(life.stop().await);
    seen.hold_none(&home, &OAUTH);
}

/// One start of the server, and a chief following the Hand's chat.
struct Life {
    server: Server,
    chief: Client,
}

/// What every life sent and printed.
struct Seen {
    frames: Vec<String>,
    logs: Vec<String>,
}

impl Life {
    async fn begin(server: Server) -> Self {
        let mut chief = Client::join(&server).await;
        let hand = chief.hand();
        chief.open(&chat(hand)).await;
        Self { server, chief }
    }

    async fn again(home: &Path, side: &Path) -> Self {
        Self::begin(serve_again(home, side, &[SIGN_IN]).await).await
    }

    /// Writes the file [`Life::gated`] reads.
    async fn noted(&mut self) {
        let wrote =
            self.answer("write notes.txt: kept for later").await;
        assert!(
            wrote.starts_with("Done: Successfully wrote"),
            "{wrote}"
        );
    }

    async fn signed_in(&mut self) {
        let start = start_sign_in(&mut self.chief).await;
        finish_sign_in(&mut self.chief, &start).await;
    }

    /// Signs in to the loopback subscription, an OAuth login.
    async fn subscribed(&mut self) {
        let start = RequestId::new();
        self.chief
            .send(ClientMessage::Login(Login::Start {
                id: start.clone(),
                start: LoginStart {
                    method: LoginMethod::Subscription,
                    provider: "chatgpt".into(),
                },
            }))
            .await;
        login_step(&mut self.chief, &start, "the prompt", |step| {
            matches!(step, LoginStep::Prompt { .. })
        })
        .await;
        let answered = self
            .chief
            .request(|id| {
                ClientMessage::Login(Login::Reply {
                    id,
                    reply: LoginReply {
                        login: start.to_string(),
                        value: "SAFE-CODE".into(),
                    },
                })
            })
            .await;
        assert_eq!(
            answered.kind,
            ReplyKind::Notice,
            "{}",
            answered.text
        );
        let ended =
            login_step(&mut self.chief, &start, "the end", |step| {
                matches!(
                    step,
                    LoginStep::Done { .. } | LoginStep::Failed { .. }
                )
            })
            .await;
        assert_eq!(
            ended,
            LoginStep::Done {
                done: "chatgpt".into()
            }
        );
    }

    /// Asks the Hand to try the subscription's model, which refreshes
    /// an expired login, and waits until the realm kept the new one.
    async fn refreshed(&mut self, home: &Path) {
        let tried = self
            .chief
            .request(|id| {
                ClientMessage::Check(Check::Model {
                    id,
                    model: Some("chatgpt::large".into()),
                })
            })
            .await;
        assert_eq!(tried.kind, ReplyKind::Notice, "{}", tried.text);
        let began = Instant::now();
        while login(home)["expires"].as_u64() == Some(EXPIRED) {
            assert!(
                began.elapsed() < PATIENCE,
                "the refreshed login was never kept"
            );
            tokio::time::sleep(std::time::Duration::from_millis(50))
                .await;
        }
    }

    /// Runs a command and reads a file through the Hand's gate, and
    /// asserts that both ran.
    async fn gated(&mut self, word: &str) {
        let ran = self.answer(&format!("run: echo ran {word}")).await;
        assert_eq!(ran, format!("Done: ran {word}\n"));
        let read = self.answer("read notes.txt").await;
        assert!(read.contains("kept for later"), "{read}");
    }

    /// The Hand's words that end the turn `line` began.
    async fn answer(&mut self, line: &str) -> String {
        let hand = self.chief.hand();
        let chat = chat(hand);
        let said = self.chief.say(hand, line).await;
        assert_eq!(said.kind, ReplyKind::Notice, "{}", said.text);
        let after = |chief: &Client| {
            let items = chief.items(&chat);
            let at = items.iter().rposition(|item| {
                item.kind == ItemKind::User && item.text == line
            })?;
            items[at + 1..]
                .iter()
                .find(|item| item.kind == ItemKind::Assistant)
                .map(|item| item.text.clone())
        };
        self.chief
            .until(&format!("the answer to {line:?}"), |chief| {
                after(chief).is_some()
            })
            .await;
        after(&self.chief).unwrap()
    }

    async fn stop(mut self) -> Seen {
        let frames = frames(&self.chief);
        self.chief.close().await;
        assert_eq!(
            self.server.stop().await,
            Some(0),
            "{}",
            self.server.log()
        );
        Seen {
            frames,
            logs: vec![self.server.log()],
        }
    }

    fn kill(mut self) -> Seen {
        let frames = frames(&self.chief);
        self.server.kill();
        Seen {
            frames,
            logs: vec![self.server.log()],
        }
    }
}

impl Seen {
    fn push(&mut self, life: Seen) {
        self.frames.extend(life.frames);
        self.logs.extend(life.logs);
    }

    /// Asserts that no frame, no log and no file in `home` but
    /// `credentials.json` holds one of `secrets`.
    fn hold_none(&self, home: &Path, secrets: &[&str]) {
        keeps_to_itself(&self.logs, &self.frames, secrets);
        for secret in secrets {
            for file in files(home) {
                if file.ends_with("credentials.json") {
                    continue;
                }
                let bytes = std::fs::read(&file).unwrap();
                assert!(
                    !bytes
                        .windows(secret.len())
                        .any(|window| window == secret.as_bytes()),
                    "{} holds {secret}",
                    file.display()
                );
            }
        }
    }
}

/// Asserts that the workspace held every reach the realm wanted, each
/// naming the secrets of the last credential kept, when the server
/// on `home` ended.
fn settled(home: &Path) {
    let mut realm = SqliteConnection::establish(&format!(
        "file:{}?mode=ro",
        realm_database(home).display()
    ))
    .unwrap();
    let Behind { behind } = diesel::sql_query(
        "SELECT count(*) AS behind FROM reach WHERE applied IS NULL \
         OR applied < wanted",
    )
    .get_result(&mut realm)
    .unwrap();
    assert_eq!(behind, 0, "reaches the workspace never took");
}

#[derive(QueryableByName)]
struct Behind {
    #[diesel(sql_type = BigInt)]
    behind: i64,
}

/// The expiry, in milliseconds, of a login that needs a refresh.
const EXPIRED: u64 = 1_000;

/// Marks the kept subscription login as expired, keeping every other
/// field of the file.
fn expire(home: &Path) {
    let file = home.join("credentials.json");
    let mut stored = serde_json::from_slice::<Value>(
        &std::fs::read(&file).unwrap(),
    )
    .unwrap();
    stored["models"]["chatgpt"]["expires"] = EXPIRED.into();
    std::fs::write(
        &file,
        serde_json::to_vec_pretty(&stored).unwrap(),
    )
    .unwrap();
}

fn login(home: &Path) -> Value {
    let stored = serde_json::from_slice::<Value>(
        &std::fs::read(home.join("credentials.json")).unwrap(),
    )
    .unwrap();
    stored["models"]["chatgpt"].clone()
}

fn realm_database(home: &Path) -> PathBuf {
    let found = std::fs::read_dir(home.join("actors").join("realm"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "sqlite")
        })
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 1, "one realm database: {found:?}");
    found.into_iter().next().unwrap()
}

fn files(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            files.extend(self::files(&entry.path()));
        } else if kind.is_file() {
            files.push(entry.path());
        }
    }
    files
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_cannot_answer_another_clients_sign_in() {
    let (mut server, _home, _side) =
        serve_with(json!([]), &[SIGN_IN]).await;
    let mut owner = Client::join(&server).await;
    let mut other = Client::join(&server).await;
    let start = start_sign_in(&mut owner).await;

    let answered = other
        .request(|id| {
            ClientMessage::Login(Login::Reply {
                id,
                reply: LoginReply {
                    login: start.to_string(),
                    value: "made-up-other-key".into(),
                },
            })
        })
        .await;
    let cancelled = other
        .request(|id| {
            ClientMessage::Login(Login::Cancel {
                id,
                cancel: start.to_string(),
            })
        })
        .await;
    finish_sign_in(&mut owner, &start).await;

    assert_eq!(answered.kind, ReplyKind::Error, "{}", answered.text);
    assert_eq!(
        cancelled.kind,
        ReplyKind::Error,
        "{}",
        cancelled.text
    );
    owner.close().await;
    other.close().await;
    assert_eq!(server.stop().await, Some(0), "{}", server.log());
}
