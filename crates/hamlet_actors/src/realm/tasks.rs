//! The `task` table: what a chief's line or an agent's standing is
//! charged to (H12, C31).

use anyhow::{Context as _, Result};
use hamlet_actor::{BindParam, ColumnValue};
use hamlet_messages::address::Agent;
use hamlet_messages::realm::budget::{
    OpenTask, Payer, TaskOpened, TaskTurn,
};
use hamlet_messages::realm::{HamletId, Left, Standing};
use hamlet_messages::workspace::{AgentNumber, Numbered, RealmTask};
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::calls::Made;
use super::change::{
    self, Affected, Column, Rows, Writer, integer, number,
};
use super::tree;

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
        integer(self.0)
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
    Retry,
    Reflection,
}

impl From<TaskTurn> for Origin {
    fn from(turn: TaskTurn) -> Self {
        match turn {
            TaskTurn::Retry => Self::Retry,
            TaskTurn::Reflection => Self::Reflection,
        }
    }
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
    standing_of(rows, agent).await?.with_context(|| {
        format!("agent {} has no standing task", agent.0)
    })
}

async fn standing_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<TaskId>> {
    Ok(rows
        .value(
            "SELECT id FROM task WHERE agent = ?1 AND origin = ?2",
            vec![
                number(agent),
                BindParam::Text(change::case_name(
                    &Origin::Standing,
                )?),
            ],
        )
        .await?
        .map(TaskId))
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

/// The task `task` names when `agent` may name it: a task the realm
/// holds that is the agent's own, that the agent works or runs an
/// errand on, or that a letter given to the agent carries.
pub(super) async fn named(
    rows: &Rows<'_>,
    agent: AgentNumber,
    task: &RealmTask,
) -> Result<Option<TaskId>> {
    let Some(task) = TaskId::of(task) else {
        return Ok(None);
    };
    let errand = match agent.numbered() {
        Numbered::Errand(n) => integer(n.get())?,
        Numbered::Tree(_) => BindParam::Null,
    };
    let named = rows
        .value::<i64>(
            "SELECT 1 FROM task WHERE id = ?1 AND (agent = ?2 OR id \
             IN (SELECT task FROM worker WHERE agent = ?2) OR id IN \
             (SELECT task FROM letter WHERE receiver = ?2 AND \
             given_at IS NOT NULL) OR id IN (SELECT task FROM \
             errand WHERE n = ?3))",
            vec![task.column()?, number(agent), errand],
        )
        .await?;
    Ok(named.map(|_| task))
}

/// The payer of [`named`]'s task; `None` for a task `agent` may not
/// name.
pub(super) async fn payer_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
    task: &RealmTask,
) -> Result<Option<Payer>> {
    match named(rows, agent, task).await? {
        Some(task) => payer(rows, task).await,
        None => Ok(None),
    }
}

/// Who pays for what `agent` does on its own account: its standing
/// task's payer, its starter's for a worker, and its hamlet for a
/// chief's worker. `None` for an agent with none of these.
pub(super) async fn own_payer(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<Payer>> {
    let worker = rows
        .row::<(Option<u32>, Option<String>)>(
            "SELECT starter, hamlet FROM worker WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?;
    let paying = match worker {
        Some((Some(starter), _)) => AgentNumber(starter),
        Some((None, hamlet)) => {
            return Ok(Some(hamlet.map_or(Payer::Realm, |hamlet| {
                Payer::Hamlet {
                    hamlet: HamletId(hamlet),
                }
            })));
        }
        None => agent,
    };
    match standing_of(rows, paying).await? {
        Some(task) => payer(rows, task).await,
        None => Ok(None),
    }
}

impl Act<Agent> for OpenTask {
    type Output = TaskOpened;
    type Loaded = Result<(TaskId, Payer), Standing>;
    type Done = TaskId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Result<(TaskId, Payer), Standing>> {
        if let Some(Left { how, why }) =
            tree::left(rows, by.0).await?
        {
            return Ok(Err(Standing::YouLeft { how, why }));
        }
        let Some(payer) = own_payer(rows, by.0).await? else {
            return Ok(Err(Standing::NotAMember));
        };
        Ok(Ok((fresh(rows).await?, payer)))
    }

    fn decide(
        &self,
        by: &Agent,
        opening: Result<(TaskId, Payer), Standing>,
    ) -> Decision<TaskId, Self::Output> {
        match opening {
            Ok((task, payer)) => Decision::Change(
                Decided::new(task).change(Change::Open {
                    task,
                    origin: self.turn.into(),
                    agent: by.0,
                    payer,
                }),
            ),
            Err(why) => Decision::Answer(TaskOpened::Refused { why }),
        }
    }

    fn answer(&self, task: TaskId, _made: &Made) -> Self::Output {
        TaskOpened::Opened {
            task: task.on_the_wire(),
        }
    }
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
