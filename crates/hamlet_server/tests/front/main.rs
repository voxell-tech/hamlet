//! `hamlet_server --actors` served to a client: the realm and the
//! agents run as actors, the agent in a Node child, behind the
//! client protocol. Every server here starts that Node host, so the
//! target needs Node and the `node-host` feature.

#![cfg(unix)]

mod chat;
mod commands;
mod lines;
mod view;

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use hamlet_acceptance::Client;
use hamlet_protocol::client::{AgentId, Reply, ReplyKind, State};
use hamlet_testing::{Models, Scratch, Server};
use serde_json::json;

/// The `hamlet_server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet_server");

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
    let script = side.join("faux.json");
    std::fs::write(
        &script,
        json!({ "responses": responses }).to_string(),
    )
    .unwrap();
    let server = Server::start(
        &[SERVER.to_owned(), "--actors".to_owned()],
        &home,
        Models::Faux(&script),
        env,
    )
    .await
    .unwrap_or_else(|failure| panic!("{failure}"));
    (server, home, side)
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

async fn said_once_hired(
    chief: &mut Client,
    agent: AgentId,
    text: &str,
) -> Reply {
    let began = Instant::now();
    loop {
        let reply = chief.say(agent, text).await;
        if reply.kind != ReplyKind::Error
            || !reply.text.ends_with(
                "is still being hired; send the line again",
            )
        {
            return reply;
        }
        assert!(
            began.elapsed() < PATIENCE,
            "{agent:?} was never hired"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
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
