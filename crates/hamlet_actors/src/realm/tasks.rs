//! The `task` table: what a chief's line or an agent's standing is
//! charged to (H12, C31).

use anyhow::{Context as _, Result};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_messages::realm::budget::Payer;
use hamlet_messages::workspace::{AgentNumber, RealmTask};
use serde::{Deserialize, Serialize};

use super::change::{self, Affected, Column, Rows, Writer, number};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub(super) struct TaskId(u64);

impl TaskId {
    pub fn on_the_wire(self) -> RealmTask {
        RealmTask(self.0.to_string())
    }

    /// The task a message names, when the text is one of the
    /// realm's.
    pub fn of(task: &RealmTask) -> Option<Self> {
        let n = task.0.parse::<u64>().ok()?;
        (n.to_string() == task.0).then_some(Self(n))
    }

    pub fn column(self) -> Result<BindParam> {
        Ok(BindParam::Integer(i64::try_from(self.0)?))
    }
}

impl Column for TaskId {
    fn of(value: &ColumnValue) -> Result<Self> {
        u64::of(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Origin {
    Chief,
    /// The task that pays for an agent's turns no line started.
    Standing,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Open {
        task: TaskId,
        origin: Origin,
        agent: AgentNumber,
        payer: Payer,
    },
    /// An agent's standing task, under an id of the next ones.
    Standing { agent: AgentNumber, payer: Payer },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Open {
            task,
            origin,
            agent,
            payer,
        } => {
            open(writer, Id::Given(task), origin, agent, payer).await
        }
        Change::Standing { agent, payer } => {
            open(writer, Id::Next, Origin::Standing, agent, payer)
                .await
        }
    }
}

enum Id {
    Given(TaskId),
    Next,
}

async fn open(
    writer: &Writer<'_>,
    id: Id,
    origin: Origin,
    agent: AgentNumber,
    payer: Payer,
) -> Result<Affected> {
    let id = match id {
        Id::Given(task) => task.column()?,
        Id::Next => BindParam::Null,
    };
    writer
        .execute(
            "INSERT INTO task (id, origin, agent, payer, at) VALUES \
             (?1, ?2, ?3, ?4, ?5)",
            vec![
                id,
                BindParam::Text(change::case_name(&origin)?),
                number(agent),
                BindParam::Text(serde_json::to_string(&payer)?),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(Affected::default())
}

pub(super) async fn fresh(rows: &Rows<'_>) -> Result<TaskId> {
    Ok(TaskId(rows.next_id("task").await?))
}

pub(super) async fn standing(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<TaskId> {
    rows.value(
        "SELECT id FROM task WHERE agent = ?1 AND origin = ?2",
        vec![
            number(agent),
            BindParam::Text(change::case_name(&Origin::Standing)?),
        ],
    )
    .await?
    .map(TaskId)
    .with_context(|| {
        format!("agent {} has no standing task", agent.0)
    })
}

/// `None` for a task the realm does not hold.
pub(super) async fn payer(
    rows: &Rows<'_>,
    task: TaskId,
) -> Result<Option<Payer>> {
    rows.value::<String>(
        "SELECT payer FROM task WHERE id = ?1",
        vec![task.column()?],
    )
    .await?
    .map(|payer| Ok(serde_json::from_str(&payer)?))
    .transpose()
}

#[cfg(test)]
mod tests {
    use hamlet_messages::realm::HamletId;
    use hamlet_messages::sender::Hand;
    use hamlet_messages::workspace::RealmTask;

    use super::super::testing::Started;
    use super::*;

    #[test]
    fn a_task_reads_back_only_from_its_own_wire_form() {
        assert_eq!(
            TaskId::of(&TaskId(7).on_the_wire()),
            Some(TaskId(7))
        );
        for other in ["07", "+7", "seven", ""] {
            assert_eq!(
                TaskId::of(&RealmTask(other.to_owned())),
                None
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_task_keeps_its_payer() {
        let started = Started::new().await;
        let hamlet = Payer::Hamlet {
            hamlet: HamletId("h".to_owned()),
        };
        let task = fresh(&started.rows()).await.unwrap();
        started
            .commit(Change::Open {
                task,
                origin: Origin::Chief,
                agent: AgentNumber(2),
                payer: hamlet.clone(),
            })
            .await
            .unwrap();

        let rows = started.rows();
        assert_eq!(payer(&rows, task).await.unwrap(), Some(hamlet));
        let hand = standing(&rows, Hand::NUMBER).await.unwrap();
        assert_ne!(hand, task);
        assert_eq!(
            payer(&rows, hand).await.unwrap(),
            Some(Payer::Realm)
        );
        assert_eq!(fresh(&rows).await.unwrap(), TaskId(task.0 + 1));
        assert_eq!(
            payer(&rows, TaskId(task.0 + 1)).await.unwrap(),
            None
        );
        started.stop().await;
    }
}
