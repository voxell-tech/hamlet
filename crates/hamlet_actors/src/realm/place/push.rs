//! The push of a place: after a commit that moved an agent's lead,
//! its hires, its roads or its tools, the agent is sent its place as
//! the rows give it then. The agent keeps only a newer version, so a
//! push sent twice, or late, does no harm.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U9 fills this file"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::workspace::AgentNumber;

use super::super::Realm;

/// Sends `agent` its place. A failed call is the caller's to log: the
/// agent's copy stays old until the next change to its place.
pub(in crate::realm) async fn send(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
) -> Result<()> {
    Ok(())
}
