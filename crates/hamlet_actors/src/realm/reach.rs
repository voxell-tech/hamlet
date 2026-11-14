//! Each agent's reach in the workspace, as a function of the realm's
//! rows, and the one push that carries it there.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U3 fills this file"
)]

use std::sync::Arc;

use anyhow::Result;
use hamlet_actor::Ctx;
use hamlet_messages::workspace::{AgentNumber, SetReach};

use super::change::Rows;
use super::{Realm, not_built};

/// `agent`'s reach as the rows give it now, under its wanted version:
/// the hamlet's folder and mode, the agent's own filesystem, the
/// network setting; `Reach::Ended` for an agent that ended. The Hand
/// has no project, so its reach is its own filesystem.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<SetReach> {
    todo!()
}

/// Carries `agent`'s wanted reach to the workspace and commits its
/// answer: the call, then the second transaction. Every reach goes
/// this way: a hire's step 8, an end, and each later change.
///
/// The wanted version was committed before this runs. A call that
/// fails leaves the reach pending, and nothing calls again.
pub(super) async fn push(
    realm: &Arc<Realm>,
    ctx: &Ctx<Realm>,
    agent: AgentNumber,
) -> Result<()> {
    Err(not_built("U3"))
}
