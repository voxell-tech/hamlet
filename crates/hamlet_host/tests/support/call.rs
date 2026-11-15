//! A call from the program to an actor of the authoring layer.

use std::time::Duration;

use hamlet_actor::Action;
use hamlet_host::{Address, Host};
use serde::de::DeserializeOwned;

/// Calls `action` of the actor at `to` from the embedder, and decodes
/// its answer.
pub async fn call<M: Action>(
    host: &Host,
    to: &Address,
    action: &M,
) -> anyhow::Result<M::Output>
where
    M::Output: DeserializeOwned,
{
    let reply = host
        .call(
            to,
            M::NAME,
            hamlet_actor::action::encode_positional(action)?,
            Duration::from_secs(10),
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}
