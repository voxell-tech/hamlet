//! What a hire and everyone under it may spend: the `allowance`
//! table, `SetAllowance` and `ReadBudget`.

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::budget::{
    self, Budget, BudgetRead, Payer, ReadBudget, ReadBudgetRefusal,
};
use hamlet_messages::realm::workers::SetAllowance;
use hamlet_messages::realm::{
    AgentRefusal, Done, OnlyAHire, Refusable,
};
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decided, Decision, Unchanged};
use super::callers::Lead;
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer, number};
use super::chiefs::limits;
use super::tree::{self, Lineage, Status};
use super::world::Part;
use super::{record, tasks, workers};

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// Gives `hire` an allowance, or takes it away when `micros` is
    /// `None`.
    Set {
        hire: AgentNumber,
        micros: Option<u64>,
        why: Option<String>,
        by: record::By,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    let Change::Set {
        hire,
        micros,
        why,
        by,
    } = change;
    match micros {
        Some(micros) => {
            writer
                .execute(
                    "INSERT INTO allowance (hire, micros, why, by, \
                     at) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT \
                     (hire) DO UPDATE SET micros = excluded.micros, \
                     why = excluded.why, by = excluded.by, at = \
                     excluded.at",
                    vec![
                        number(hire),
                        change::integer(micros)?,
                        why.map_or(BindParam::Null, BindParam::Text),
                        BindParam::Text(by.name()),
                        BindParam::Integer(writer.at().0),
                    ],
                )
                .await?
        }
        None => {
            writer
                .execute(
                    "DELETE FROM allowance WHERE hire = ?1",
                    vec![number(hire)],
                )
                .await?
        }
    };
    Ok(Affected::seen([Part::Agents]))
}

/// The first allowance on the way from `agent` up its leads that is
/// spent in the period of `payer`, the task's, as of `rows.at()`. A
/// worker spends as its starter.
pub(super) async fn over(
    rows: &Rows<'_>,
    agent: AgentNumber,
    payer: &Payer,
) -> Result<Option<Budget>> {
    let Some(first) = bounded(rows, agent).await? else {
        return Ok(None);
    };
    let Some(lineage) = tree::lineage(rows, first).await? else {
        return Ok(None);
    };
    if lineage.hamlet.is_none() {
        return Ok(None);
    }
    let period = limits::budget(rows, payer).await?.period;
    let since = super::budget::began(rows, period).await?;
    for hire in std::iter::once(&lineage.agent).chain(&lineage.leads)
    {
        let Some(allowance_micros) = of(rows, hire.agent).await?
        else {
            continue;
        };
        let spent_micros =
            charged(rows, hire.agent, Counted::WithAllUnder, since)
                .await?;
        if spent_micros >= allowance_micros {
            return Ok(Some(Budget::OverAllowance {
                hire: hire.agent,
                name: record::name_of(rows, hire.agent).await?,
                spent_micros,
                allowance_micros,
            }));
        }
    }
    Ok(None)
}

/// The agent whose allowances bound `agent`: a worker's starter, and
/// none for a chief's worker.
async fn bounded(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Option<AgentNumber>> {
    let starter = rows
        .value::<Option<u32>>(
            "SELECT starter FROM worker WHERE agent = ?1",
            vec![number(agent)],
        )
        .await?;
    Ok(match starter {
        Some(starter) => starter.map(AgentNumber),
        None => Some(agent),
    })
}

async fn of(
    rows: &Rows<'_>,
    hire: AgentNumber,
) -> Result<Option<u64>> {
    rows.value(
        "SELECT micros FROM allowance WHERE hire = ?1",
        vec![number(hire)],
    )
    .await
}

enum Counted {
    Own,
    /// The agent's, its living hires' down the tree, and the living
    /// workers' of each.
    WithAllUnder,
}

/// What `agent` and those `counted` with it were charged for model
/// calls from `since`, a time in ms.
async fn charged(
    rows: &Rows<'_>,
    agent: AgentNumber,
    counted: Counted,
    since: i64,
) -> Result<u64> {
    let agents = match counted {
        Counted::Own => "SELECT ?1".to_owned(),
        Counted::WithAllUnder => format!(
            "WITH RECURSIVE under (agent) AS (SELECT ?1 UNION \
             SELECT hire.agent FROM hire JOIN under ON hire.lead = \
             under.agent JOIN agent ON agent.id = hire.agent WHERE \
             agent.status != 'ended') SELECT agent FROM under UNION \
             {}",
            workers::living("SELECT agent FROM under")
        ),
    };
    rows.value(
        &format!(
            "SELECT COALESCE(SUM(micros), 0) FROM charge WHERE at \
             >= ?2 AND agent IN ({agents})"
        ),
        vec![number(agent), BindParam::Integer(since)],
    )
    .await?
    .context("the sum of the charges under an allowance")
}

impl<By: Lead> Act<By> for SetAllowance {
    type Output = Done<By::Why<OnlyAHire>>;
    type Loaded = Option<(Lineage, String)>;
    type Done = ();

    async fn load(
        &self,
        by: &By,
        rows: &Rows<'_>,
    ) -> Result<Self::Loaded> {
        tree::shown(rows, tree::named(rows, by, &self.hire).await?)
            .await
    }

    fn decide(
        &self,
        by: &By,
        named: Self::Loaded,
    ) -> Decision<(), Self::Output> {
        let refuse =
            |why| Decision::Answer(Done::refused(By::refused(why)));
        let Some((lineage, name)) = named else {
            return refuse(OnlyAHire::NoSuchAgent {
                address: self.hire.to_string(),
            });
        };
        if lineage.agent.kind != AgentKind::Hire {
            return refuse(OnlyAHire::NotAHire { name });
        }
        let hire = lineage.agent.agent;
        if let Err(not_a_lead) = by.leads(&lineage, hire) {
            return Decision::Answer(Done::refused(not_a_lead));
        }
        if let Status::Ended(left) = &lineage.agent.status {
            return refuse(OnlyAHire::Left {
                hire,
                left: left.clone(),
            });
        }
        Decision::Change(
            Decided::new(())
                .change(Change::Set {
                    hire,
                    micros: self.micros.map(|micros| {
                        micros.0.min(i64::MAX.unsigned_abs())
                    }),
                    why: self.why.clone(),
                    by: by.by(),
                })
                .record(record::Entry::Allowance {
                    hire,
                    by: by.by(),
                    why: self.why.clone(),
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Agent> for ReadBudget {
    type Output = BudgetRead<AgentRefusal<ReadBudgetRefusal>>;
    type Loaded = Self::Output;
    type Done = Unchanged;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Self::Output> {
        let payer = match &self.task {
            Some(task) => tasks::payer_of(rows, task).await?,
            None => tasks::own_payer(rows, by.0).await?,
        };
        let Some(payer) = payer else {
            return Ok(BudgetRead::refused(AgentRefusal::Act(
                ReadBudgetRefusal::NoSuchTask,
            )));
        };
        let spent = super::budget::spent(rows, &payer).await?;
        let period = spent.budget.period;
        let since = super::budget::began(rows, period).await?;
        Ok(BudgetRead::Budget {
            spend: spent.into(),
            period: match period {
                limits::Period::Day => budget::Period::Day,
                limits::Period::Month => budget::Period::Month,
            },
            own_micros: charged(rows, by.0, Counted::Own, since)
                .await?,
            allowance_micros: of(rows, by.0).await?,
        })
    }

    fn decide(
        &self,
        _by: &Agent,
        read: Self::Output,
    ) -> Decision<Unchanged, Self::Output> {
        Decision::Answer(read)
    }

    fn answer(&self, done: Unchanged, _made: &Made) -> Self::Output {
        match done {}
    }
}
