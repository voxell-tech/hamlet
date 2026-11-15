//! The versions the realm keeps of what other actors hold a copy of:
//! each agent's reach, wanted and in force in the workspace; each
//! agent's place, as the agent was last sent it; and the revision of
//! the world a client reads.

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::realm::world::Revision;
use hamlet_messages::workspace::{AgentNumber, ReachVersion};

use super::change::{Affected, Rows, Writer, number};
use super::world::Part;

/// The changes to the versions that no other change implies.
#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The agent's reach is to be set.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "U3 wants a reach with it")
    )]
    WantReach(AgentNumber),
    /// The workspace answered that `version` is in force.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "U3 records a reach in force with it"
        )
    )]
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
    Ok(Affected {
        reaches: BTreeSet::from([agent]),
        ..Affected::seen([Part::Reaches])
    })
}

async fn applied(
    writer: &Writer<'_>,
    agent: AgentNumber,
    version: ReachVersion,
) -> Result<Affected> {
    writer
        .execute(
            "UPDATE reach SET applied = ?2 WHERE agent = ?1 AND \
             (applied IS NULL OR applied < ?2)",
            vec![
                number(agent),
                BindParam::Integer(i64::try_from(version.0)?),
            ],
        )
        .await?;
    Ok(Affected::seen([Part::Reaches]))
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
                    number(agent),
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
                    number(agent),
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
    let found = rows
        .row::<(u64, Option<u64>)>(
            "SELECT wanted, applied FROM reach WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?;
    Ok(found.map_or((ReachVersion(0), None), |(wanted, applied)| {
        (ReachVersion(wanted), applied.map(ReachVersion))
    }))
}

pub(super) async fn place(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<u64> {
    Ok(rows
        .value(
            "SELECT version FROM place WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?
        .unwrap_or(FIRST_PLACE))
}

/// The version of the place an agent is created with.
const FIRST_PLACE: u64 = 1;

pub(super) async fn revision(rows: &Rows<'_>) -> Result<Revision> {
    Ok(Revision(
        rows.value("SELECT n FROM revision WHERE id = 1", Vec::new())
            .await?
            .unwrap_or(0),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_reach_in_force_only_moves_forward() {
        let started = Started::new().await;
        let agent = AgentNumber(5);
        let commit =
            async |made: Change| started.commit(made).await.unwrap();
        let reach =
            async || reach(&started.rows(), agent).await.unwrap();
        assert_eq!(reach().await, (ReachVersion(0), None));
        commit(Change::WantReach(agent)).await;
        commit(Change::WantReach(agent)).await;
        assert_eq!(reach().await, (ReachVersion(2), None));
        commit(Change::Applied {
            agent,
            version: ReachVersion(2),
        })
        .await;
        commit(Change::Applied {
            agent,
            version: ReachVersion(1),
        })
        .await;
        assert_eq!(
            reach().await,
            (ReachVersion(2), Some(ReachVersion(2)))
        );
        started.stop().await;
    }
}
