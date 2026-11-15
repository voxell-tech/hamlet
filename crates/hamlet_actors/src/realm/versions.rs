//! The versions the realm keeps of what other actors hold a copy of:
//! each agent's reach, wanted and in force in the workspace; each
//! agent's place, as the agent was last sent it; and the revision of
//! the world a client reads.

#![expect(
    unused_variables,
    dead_code,
    reason = "the scaffold: U2 fills this file"
)]

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::realm::world::Revision;
use hamlet_messages::workspace::{AgentNumber, ReachVersion};

use super::change::{Affected, Rows, Writer};

/// The changes to the versions that no other change implies.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The agent's reach is to be set.
    WantReach(AgentNumber),
    /// The workspace answered that `version` is in force.
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
/// the place version of each in `affected.places`.
pub(super) async fn raise(
    writer: &Writer<'_>,
    affected: &Affected,
) -> Result<()> {
    for &agent in &affected.reaches {
        let (wanted, _) = reach(&writer.rows(), agent).await?;
        writer
            .execute(
                "INSERT INTO reach (agent, wanted) VALUES (?1, ?2) \
                 ON CONFLICT (agent) DO UPDATE SET wanted = \
                 excluded.wanted",
                vec![
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Integer(i64::try_from(wanted.0 + 1)?),
                ],
            )
            .await?;
    }
    for &agent in &affected.places {
        let version = place(&writer.rows(), agent).await?;
        writer
            .execute(
                "INSERT INTO place (agent, version) VALUES (?1, ?2) \
                 ON CONFLICT (agent) DO UPDATE SET version = \
                 excluded.version",
                vec![
                    BindParam::Integer(i64::from(agent.0)),
                    BindParam::Integer(i64::try_from(version + 1)?),
                ],
            )
            .await?;
    }
    Ok(())
}

pub(super) async fn advance_revision(
    writer: &Writer<'_>,
) -> Result<Revision> {
    let next = Revision(revision(&writer.rows()).await?.0 + 1);
    writer
        .execute(
            "INSERT INTO revision (id, n) VALUES (1, ?1) ON \
             CONFLICT (id) DO UPDATE SET n = excluded.n",
            vec![BindParam::Integer(i64::try_from(next.0)?)],
        )
        .await?;
    Ok(next)
}

/// The reach versions of `agent`: wanted, and in force when the
/// workspace has answered one.
pub(super) async fn reach(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<(ReachVersion, Option<ReachVersion>)> {
    todo!()
}

/// 1 until the place first moves: the version an agent is created
/// with.
pub(super) async fn place(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<u64> {
    Ok(rows
        .value(
            "SELECT version FROM place WHERE agent = ?1",
            vec![BindParam::Integer(i64::from(agent.0))],
        )
        .await?
        .unwrap_or(1))
}

pub(super) async fn revision(rows: &Rows<'_>) -> Result<Revision> {
    Ok(Revision(
        rows.value("SELECT n FROM revision WHERE id = 1", Vec::new())
            .await?
            .unwrap_or(0),
    ))
}
