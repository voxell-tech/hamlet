//! Hamlet's actors, each run by `hamlet_host` in a SQLite file of its
//! own.

pub mod realm;
pub mod workspace;

use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_host::HostRef;
use hamlet_messages::Message;
use hamlet_messages::address::Party;

async fn call<M: Message>(
    host: &HostRef,
    to: &M::To,
    message: &M,
    deadline: Duration,
) -> Result<M::Output> {
    let args = hamlet_actor::action::encode_positional(message)?;
    let answer =
        host.call(&to.address()?, M::NAME, args, deadline).await?;
    ciborium::from_reader(answer.as_slice())
        .with_context(|| format!("decode the answer to {}", M::NAME))
}
