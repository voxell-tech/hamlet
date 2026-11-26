//! `hamlet_server --actors` served to a client: the realm and the
//! agents run as actors, the agent in a Node child, behind the
//! client protocol. Every server here starts that Node host, so the
//! target needs Node and the `node-host` feature.

#![cfg(unix)]

mod chat;
mod commands;
mod ended;
mod host;
mod lines;
mod place;
mod reads;
mod resend;
mod secrets;
mod uploads;
mod view;

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

use hamlet_acceptance::Client;
use hamlet_protocol::client::{
    AgentId, ClientMessage, Login, LoginMethod, LoginReply,
    LoginStart, LoginStep, Reply, ReplyKind, RequestId,
    ServerMessage, State,
};
use hamlet_testing::{Models, Scratch, Server};
use serde_json::json;

/// The `hamlet_server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet_server");

/// Has the Node host list two loopback providers to sign in to.
const SIGN_IN: (&str, &str) = ("HAMLET_DEMO_SIGN_IN", "1");

/// A server started with `--actors` on a new realm, its faux model
/// answering with `responses` in order, and the folders it uses.
async fn serve(
    responses: serde_json::Value,
) -> (Server, Scratch, Scratch) {
    serve_with(responses, &[]).await
}

/// [`serve`], with the variables `env` set for the server.
async fn serve_with(
    responses: serde_json::Value,
    env: &[(&str, &str)],
) -> (Server, Scratch, Scratch) {
    let home = Scratch::new("front-home");
    let side = Scratch::new("front-side");
    std::fs::write(
        side.join("faux.json"),
        json!({ "responses": responses }).to_string(),
    )
    .unwrap();
    let server = serve_again(&home, &side, env).await;
    (server, home, side)
}

/// A server started with `--actors` on `home`, as [`serve_with`]
/// made it, its faux model answering from the script in `side` from
/// the first step.
async fn serve_again(
    home: &Path,
    side: &Path,
    env: &[(&str, &str)],
) -> Server {
    Server::start(
        &[SERVER.to_owned(), "--actors".to_owned()],
        home,
        Models::Faux(&side.join("faux.json")),
        env,
    )
    .await
    .unwrap_or_else(|failure| panic!("{failure}"))
}

/// How long a test waits for the realm to show a change; each
/// change takes well under a second here.
const PATIENCE: Duration = Duration::from_secs(20);

/// The realm as a client that says hello now is shown it.
async fn look(server: &Server) -> State {
    let client = Client::join(server).await;
    let realm = client.realm();
    client.close().await;
    realm
}

/// The realm once a hello shows it as `holds` says, read again until
/// then.
async fn until(
    server: &Server,
    what: &str,
    holds: impl Fn(&State) -> bool,
) -> State {
    let began = Instant::now();
    loop {
        let realm = look(server).await;
        if holds(&realm) {
            return realm;
        }
        assert!(began.elapsed() < PATIENCE, "waited for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The reply to `text` said to `agent`, which the realm gives it once
/// it is hired.
async fn said_once_hired(
    chief: &mut Client,
    agent: AgentId,
    text: &str,
) -> Reply {
    chief.say(agent, text).await
}

/// The address of the hire a "`hire#3 (ada)` is hired." reply names.
fn hired_address(hired: &str) -> String {
    let named = hired
        .strip_suffix(" is hired.")
        .unwrap_or_else(|| panic!("{hired}"))
        .trim_matches('`');
    named.split(" (").next().unwrap_or(named).to_owned()
}

/// A `node` for the server that, once its first start ended, starts
/// again only when released.
struct HeldNode {
    side: Scratch,
}

impl HeldNode {
    fn new() -> Self {
        let side = Scratch::new("front-held-node");
        let script = format!(
            "#!/bin/sh\nif [ -e {started} ]; then\nwhile [ ! -e \
             {go} ]; do sleep 0.05; done\nfi\ntouch {started}\nexec \
             node \"$@\"\n",
            started = side.join("started").display(),
            go = side.join("go").display(),
        );
        let node = side.join("node");
        std::fs::write(&node, script).unwrap();
        std::fs::set_permissions(
            &node,
            Permissions::from_mode(0o755),
        )
        .unwrap();
        Self { side }
    }

    async fn serve(
        &self,
        responses: serde_json::Value,
    ) -> (Server, Scratch, Scratch) {
        let node = self.side.join("node");
        serve_with(
            responses,
            &[("HAMLET_NODE", node.to_str().unwrap())],
        )
        .await
    }

    /// Lets the Node host start again.
    fn release(&self) {
        std::fs::write(self.side.join("go"), "").unwrap();
    }
}

async fn kill_node(home: &Scratch) {
    let pid = std::fs::read_to_string(home.join("demo-node.pid"))
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    hamlet_testing::signals::kill(pid).unwrap();
    let began = Instant::now();
    while !hamlet_os::process::has_ended(pid).unwrap() {
        assert!(
            began.elapsed() < PATIENCE,
            "the Node host never ended"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// A key typed in a sign-in, which nothing the server sends or
/// prints may hold.
const KEY: &str = "made-up-door-key-6b1e0c2f";

/// Starts a sign-in with a key on the loopback provider, and waits
/// for its prompt for the key.
async fn start_sign_in(chief: &mut Client) -> RequestId {
    let start = RequestId::new();
    chief
        .send(ClientMessage::Login(Login::Start {
            id: start.clone(),
            start: LoginStart {
                method: LoginMethod::Key,
                provider: "loopback".into(),
            },
        }))
        .await;
    let step =
        login_step(chief, &start, "the prompt for the key", |step| {
            matches!(step, LoginStep::Prompt { .. })
        })
        .await;
    assert_eq!(
        step,
        LoginStep::Prompt {
            prompt: "Paste your key.".into(),
            secret: true,
        }
    );
    start
}

/// Answers the sign-in `start` began with the key, and waits for it
/// to end signed in.
async fn finish_sign_in(chief: &mut Client, start: &RequestId) {
    let answered = chief
        .request(|id| {
            ClientMessage::Login(Login::Reply {
                id,
                reply: LoginReply {
                    login: start.to_string(),
                    value: KEY.into(),
                },
            })
        })
        .await;
    assert_eq!(
        (answered.kind, answered.text.as_str()),
        (ReplyKind::Notice, "Sign-in answer received.")
    );
    let ended =
        login_step(chief, start, "the sign-in's end", |step| {
            matches!(
                step,
                LoginStep::Done { .. } | LoginStep::Failed { .. }
            )
        })
        .await;
    assert_eq!(
        ended,
        LoginStep::Done {
            done: "loopback".into()
        }
    );
}

/// The first step of the sign-in `start` began that `matches`.
async fn login_step(
    chief: &mut Client,
    start: &RequestId,
    what: &str,
    matches: impl Fn(&LoginStep) -> bool,
) -> LoginStep {
    let found = |chief: &Client| {
        chief.messages().iter().find_map(|message| match message {
            ServerMessage::Login(progress)
                if progress.to.as_ref() == Some(start)
                    && matches(&progress.step) =>
            {
                Some(progress.step.clone())
            }
            _ => None,
        })
    };
    chief.until(what, |chief| found(chief).is_some()).await;
    found(chief).unwrap()
}

fn frames(chief: &Client) -> Vec<String> {
    chief
        .messages()
        .iter()
        .map(|message| serde_json::to_string(message).unwrap())
        .collect()
}

/// Asserts that no log of `logs` and no frame of `frames` holds one
/// of `secrets`.
fn keeps_to_itself(
    logs: &[String],
    frames: &[String],
    secrets: &[&str],
) {
    for secret in secrets {
        for frame in frames {
            assert!(
                !frame.contains(secret),
                "a frame carried {secret}: {frame}"
            );
        }
        for log in logs {
            assert!(
                !log.contains(secret),
                "the server printed {secret}"
            );
        }
    }
}
