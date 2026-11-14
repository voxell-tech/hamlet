//! Calls an actor at its address, for the demo and the host's tests.

use std::time::Duration;

use anyhow::Result;
use hamlet_host::{Address, Host};

/// A call's deadline where the caller does not test it: longer than
/// any kind's action timeout, which then decides.
pub const DEADLINE: Duration = Duration::from_secs(600);

/// Calls `action` on the actor at `at`.
pub async fn act(
    host: &Host,
    at: &Address,
    action: &str,
    args: Vec<u8>,
) -> Result<Vec<u8>> {
    host.call(at, action, args, DEADLINE).await
}
