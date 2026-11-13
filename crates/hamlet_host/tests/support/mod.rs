//! What the tests of `access.rs` and `create.rs` share.

use std::time::Duration;

use hamlet_actor::Action;
use hamlet_actor_error::ActorError;
use hamlet_host::{Address, Cause, Host};
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
            Cause::root("test"),
        )
        .await?;
    Ok(ciborium::from_reader(reply.as_slice())?)
}

/// The error's `group.code`, as `actor.duplicate_key`.
pub fn code(error: &anyhow::Error) -> String {
    let error = ActorError::extract(error);
    format!("{}.{}", error.group(), error.code())
}
