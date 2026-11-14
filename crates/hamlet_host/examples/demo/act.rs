//! Calls an actor by its id, for the demo and the host's tests.

use std::time::Duration;

use anyhow::{Context, Result};
use hamlet_host::{Address, Host};

/// A call's deadline where the caller does not test it: longer than
/// any kind's action timeout, which then decides.
pub const DEADLINE: Duration = Duration::from_secs(600);

/// Calls `action` on the actor `actor_id` names, `<kind>/<key>`.
pub async fn act(
    host: &Host,
    actor_id: &str,
    action: &str,
    args: Vec<u8>,
) -> Result<Vec<u8>> {
    let (kind, key) =
        actor_id.split_once('/').with_context(|| {
            format!("`{actor_id}` is not <kind>/<key>")
        })?;
    host.call(&Address::new(kind, key)?, action, args, DEADLINE)
        .await
}
