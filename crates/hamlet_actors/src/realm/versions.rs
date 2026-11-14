//! The versions the realm keeps of what other actors hold a copy of:
//! each agent's reach, wanted and in force in the workspace; each
//! agent's place, as the agent was last sent it; and the revision of
//! the world a client reads. Every commit raises them here, from what
//! its changes reported, so no action raises one by hand.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use anyhow::Result;
use hamlet_messages::realm::world::Revision;
use hamlet_messages::workspace::{AgentNumber, ReachVersion};

use super::change::{Affected, Rows, Writer};

/// The changes to the versions that no other change implies.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The agent's reach is to be set: a hire's step 8. Writes
    /// nothing itself; it reports the reach as moved, so the commit
    /// raises its wanted version and pushes it.
    WantReach(AgentNumber),
    /// The workspace answered that `version` is in force: the second
    /// transaction of a reach's push.
    Applied {
        agent: AgentNumber,
        version: ReachVersion,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::WantReach(agent) => want_reach(agent),
        Change::Applied { agent, version } => {
            applied(writer, agent, version).await
        }
    }
}

fn want_reach(agent: AgentNumber) -> Result<Affected> {
    todo!()
}

async fn applied(
    writer: &Writer<'_>,
    agent: AgentNumber,
    version: ReachVersion,
) -> Result<Affected> {
    todo!()
}

/// Raises the wanted reach of each agent in `affected.reaches` and
/// the place version of each in `affected.places`, in the transaction
/// that moved them. Only `Transaction::commit` calls it.
pub(super) async fn raise(
    writer: &Writer<'_>,
    affected: &Affected,
) -> Result<()> {
    todo!()
}

/// Raises the world's revision and returns it (S26).
pub(super) async fn advance_revision(
    writer: &Writer<'_>,
) -> Result<Revision> {
    todo!()
}

/// The reach versions of `agent`: wanted, and in force when the
/// workspace has answered one. Pending while in force is below
/// wanted.
pub(super) async fn reach(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<(ReachVersion, Option<ReachVersion>)> {
    todo!()
}

pub(super) async fn place(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<u64> {
    todo!()
}

pub(super) async fn revision(rows: &Rows<'_>) -> Result<Revision> {
    todo!()
}
