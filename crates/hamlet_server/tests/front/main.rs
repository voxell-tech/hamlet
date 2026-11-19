//! `hamlet_server --actors` served to a client: the realm and the
//! agents run as actors, the agent in a Node child, behind today's
//! client protocol. Every server here starts that Node host, so the
//! target needs Node and the `node-host` feature.
//!
//! Each wait waits for the thing itself. Their backstops are the
//! fixtures': 30 s for a server to publish its address, 20 s for an
//! answer or a chat's item, and 10 s for a server to stop. Each step
//! takes well under a second here.

#![cfg(unix)]

mod chat;
mod commands;
mod view;

use hamlet_testing::{Models, Scratch, Server};
use serde_json::json;

/// The `hamlet_server` these tests are built with.
const SERVER: &str = env!("CARGO_BIN_EXE_hamlet_server");

/// A server started with `--actors` on a new realm, its faux model
/// answering with `responses` in order, and the folders it uses.
async fn serve(
    responses: serde_json::Value,
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
        &[],
    )
    .await
    .unwrap_or_else(|failure| panic!("{failure}"));
    (server, home, side)
}
