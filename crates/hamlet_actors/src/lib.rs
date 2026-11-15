//! Hamlet's actors, each run by `hamlet_host` in a SQLite file of its
//! own.

pub mod library;
pub mod profile;
pub mod realm;
mod repo;
pub mod workspace;

use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_host::HostRef;
use hamlet_messages::Message;
use hamlet_messages::address::Party;
use serde::Serialize;
use sha2::{Digest, Sha256};

fn cbor(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes)?;
    Ok(bytes)
}

fn request_hash(request: &impl Serialize) -> Result<[u8; 32]> {
    Ok(Sha256::digest(cbor(request)?).into())
}

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
