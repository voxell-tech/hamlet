//! Spending: the `charge` and `budget_reset` tables, and the check
//! before a model call (H8).

use anyhow::{Context as _, Result};
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::budget::{
    Budget, Charge, MaySpend, Payer, ResetBudget, SpendRefusal,
};
use hamlet_messages::realm::world::Spend;
use hamlet_messages::realm::{
    Done, HamletId, HamletRefusal, Left, Refusable, Standing,
};
use hamlet_messages::sender::Client;
use hamlet_messages::workspace::{AgentNumber, CallId};

use super::act::{Act, Decided, Decision};
use super::calls::Made;
use super::change::{self, Affected, Rows, Writer, number};
use super::chiefs::{self, limits};
use super::oversight::asking;
use super::tasks::{self, TaskId};
use super::workers::errands::{self, Spending};
use super::world::Part;
use super::{allowance, laid, tree, waits};

const ROAD: AgentNumber = AgentNumber(0);

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// One model call's cost and time, once per (agent, call).
    Charge {
        agent: AgentNumber,
        call: CallId,
        payer: Payer,
        micros: u32,
        took_ms: u32,
        tokens: Option<u32>,
    },
    Road {
        a: AgentNumber,
        b: AgentNumber,
        hamlet: HamletId,
    },
    Reset {
        payer: Payer,
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Charge {
            agent,
            call,
            payer,
            micros,
            took_ms,
            tokens,
        } => {
            let cost = Cost {
                micros: u64::from(micros),
                took_ms,
                tokens,
            };
            charge(writer, agent, call.0, &payer, cost).await
        }
        Change::Road { a, b, hamlet } => {
            road(writer, a, b, hamlet).await
        }
        Change::Reset { payer } => reset(writer, &payer).await,
    }
}

/// What one charge costs.
struct Cost {
    micros: u64,
    took_ms: u32,
    tokens: Option<u32>,
}

async fn charge(
    writer: &Writer<'_>,
    agent: AgentNumber,
    call: String,
    payer: &Payer,
    cost: Cost,
) -> Result<Affected> {
    let charged = writer
        .execute(
            "INSERT INTO charge (agent, call, payer, micros, \
             took_ms, tokens, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, \
             ?7) ON CONFLICT (agent, call) DO NOTHING",
            vec![
                number(agent),
                BindParam::Text(call),
                BindParam::Text(serde_json::to_string(payer)?),
                change::integer(cost.micros)?,
                BindParam::Integer(i64::from(cost.took_ms)),
                cost.tokens.map_or(BindParam::Null, |tokens| {
                    BindParam::Integer(i64::from(tokens))
                }),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(if charged == 0 {
        Affected::default()
    } else {
        Affected::seen([Part::Spend])
    })
}

async fn road(
    writer: &Writer<'_>,
    a: AgentNumber,
    b: AgentNumber,
    hamlet: HamletId,
) -> Result<Affected> {
    let price = chiefs::hamlet(&writer.rows(), &hamlet)
        .await?
        .with_context(|| {
            format!("the hamlet `{}` of a laid road", hamlet.0)
        })?
        .limits
        .road
        .price;
    let laid = laid::laid_at(&writer.rows(), a, b)
        .await?
        .context("a charged road is laid first")?;
    let laying = format!("{}-{}@{}", a.min(b).0, a.max(b).0, laid.0);
    let cost = Cost {
        micros: price,
        took_ms: 0,
        tokens: None,
    };
    charge(writer, ROAD, laying, &Payer::Hamlet { hamlet }, cost)
        .await
}

async fn reset(
    writer: &Writer<'_>,
    payer: &Payer,
) -> Result<Affected> {
    writer
        .execute(
            "INSERT INTO budget_reset (payer, at) VALUES (?1, ?2)",
            vec![
                BindParam::Text(serde_json::to_string(payer)?),
                BindParam::Integer(writer.at().0),
            ],
        )
        .await?;
    Ok(Affected::seen([Part::Spend]))
}

/// A payer's spending since its period began and since its last
/// reset, as of `rows.at()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Spent {
    pub payer: Payer,
    pub micros: u64,
    pub took_ms: u64,
    pub budget: limits::Budget,
}

impl Spent {
    fn over(self) -> Option<Budget> {
        if let Some(budget_micros) = self.budget.micros
            && self.micros >= budget_micros
        {
            return Some(Budget::OverCost {
                payer: self.payer,
                spent_micros: self.micros,
                budget_micros,
            });
        }
        if let Some(budget_ms) = self.budget.ms
            && self.took_ms >= budget_ms
        {
            return Some(Budget::OverTime {
                payer: self.payer,
                spent_ms: self.took_ms,
                budget_ms,
            });
        }
        None
    }
}

impl From<Spent> for Spend {
    fn from(spent: Spent) -> Self {
        Self {
            payer: spent.payer,
            spent_micros: spent.micros,
            budget_micros: spent.budget.micros,
            spent_ms: spent.took_ms,
            budget_ms: spent.budget.ms,
        }
    }
}

pub(super) async fn spent(
    rows: &Rows<'_>,
    payer: &Payer,
) -> Result<Spent> {
    let budget = limits::budget(rows, payer).await?;
    let (micros, took_ms) = rows
        .row::<(u64, u64)>(
            "SELECT COALESCE(SUM(micros), 0), \
             COALESCE(SUM(took_ms), 0) FROM charge WHERE payer = ?1 \
             AND at >= ?2 AND at > COALESCE((SELECT MAX(at) FROM \
             budget_reset WHERE payer = ?1), -1)",
            vec![
                BindParam::Text(serde_json::to_string(payer)?),
                BindParam::Integer(began(rows, budget.period).await?),
            ],
        )
        .await?
        .context("the sums of a payer's charges")?;
    Ok(Spent {
        payer: payer.clone(),
        micros,
        took_ms,
        budget,
    })
}

/// The start of the period holding `rows.at()`, in ms.
pub(super) async fn began(
    rows: &Rows<'_>,
    period: limits::Period,
) -> Result<i64> {
    let start = match period {
        limits::Period::Day => "start of day",
        limits::Period::Month => "start of month",
    };
    rows.value(
        "SELECT CAST(strftime('%s', ?1 / 1000, 'unixepoch', \
         'localtime', ?2, 'utc') AS INTEGER) * 1000",
        vec![
            BindParam::Integer(rows.at().0),
            BindParam::Text(start.to_owned()),
        ],
    )
    .await?
    .context("the start of a budget's period")
}

/// What an agent has spent, by its charges.
async fn spent_by(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<u64> {
    rows.value(
        "SELECT COALESCE(SUM(micros), 0) FROM charge WHERE agent = \
         ?1",
        vec![number(agent)],
    )
    .await?
    .context("the sum of an agent's charges")
}

impl Act<Agent> for MaySpend {
    type Output = Done<SpendRefusal>;
    /// The refusal of the call, if any.
    type Loaded = Option<SpendRefusal>;
    type Done = ();

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<SpendRefusal>> {
        if let Some(Left { how, why }) =
            tree::left(rows, by.0).await?
        {
            return Ok(Some(SpendRefusal::Standing(
                Standing::YouLeft { how, why },
            )));
        }
        let errand = match errands::spending(rows, by.0).await? {
            Spending::NotAnErrand => None,
            Spending::Open { task, cap } => Some((task, cap)),
            Spending::Refused(refusal) => return Ok(Some(refusal)),
        };
        let Some(payer) = tasks::payer_of(rows, &self.task).await?
        else {
            return Ok(Some(SpendRefusal::Budget(
                Budget::NoSuchTask,
            )));
        };
        if let Some((task, cap)) = errand {
            if TaskId::of(&self.task) != Some(task) {
                return Ok(Some(SpendRefusal::Budget(
                    Budget::NoSuchTask,
                )));
            }
            let spent_micros = spent_by(rows, by.0).await?;
            if spent_micros >= cap {
                return Ok(Some(SpendRefusal::Budget(
                    Budget::OverCost {
                        payer,
                        spent_micros,
                        budget_micros: cap,
                    },
                )));
            }
        }
        if let Some(hold) =
            asking::held(rows, by.0, &self.task).await?
        {
            return Ok(Some(SpendRefusal::Held(hold)));
        }
        if let Some(over) = spent(rows, &payer).await?.over() {
            return Ok(Some(SpendRefusal::Budget(over)));
        }
        let over = allowance::over(rows, by.0).await?;
        Ok(over.map(SpendRefusal::Budget))
    }

    fn decide(
        &self,
        _by: &Agent,
        refusal: Option<SpendRefusal>,
    ) -> Decision<(), Self::Output> {
        Decision::Answer(refusal.map_or(Done::Done, Done::refused))
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl Act<Agent> for Charge {
    type Output = ();
    type Loaded = Option<Payer>;
    type Done = ();

    async fn load(
        &self,
        _by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Option<Payer>> {
        tasks::payer_of(rows, &self.task).await
    }

    fn decide(
        &self,
        by: &Agent,
        payer: Option<Payer>,
    ) -> Decision<(), ()> {
        Decision::Change(Decided::new(()).change(Change::Charge {
            agent: by.0,
            call: self.call.clone(),
            payer: payer.unwrap_or(Payer::Realm),
            micros: self.micros,
            took_ms: self.took_ms,
            tokens: self.tokens,
        }))
    }

    fn answer(&self, (): (), _made: &Made) {}
}

pub(super) enum PayerRow {
    /// The payer's row, with the changes that close its budget
    /// faults.
    Held {
        faults: Vec<change::Change>,
    },
    Missing(HamletId),
}

impl Act<Client> for ResetBudget {
    type Output = Done<HamletRefusal>;
    type Loaded = PayerRow;
    type Done = ();

    async fn load(
        &self,
        _by: &Client,
        rows: &Rows<'_>,
    ) -> Result<PayerRow> {
        if let Payer::Hamlet { hamlet } = &self.payer
            && chiefs::hamlet(rows, hamlet).await?.is_none()
        {
            return Ok(PayerRow::Missing(hamlet.clone()));
        }
        Ok(PayerRow::Held {
            faults: waits::budget_faults(rows, &self.payer).await?,
        })
    }

    fn decide(
        &self,
        _by: &Client,
        row: PayerRow,
    ) -> Decision<(), Self::Output> {
        match row {
            PayerRow::Held { faults } => {
                let mut decided =
                    Decided::new(()).change(Change::Reset {
                        payer: self.payer.clone(),
                    });
                decided.changes.extend(faults);
                Decision::Change(decided)
            }
            PayerRow::Missing(id) => Decision::Answer(Done::refused(
                HamletRefusal::NoSuchHamlet { id },
            )),
        }
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

#[cfg(test)]
mod tests {
    use super::super::change;
    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn a_road_is_charged_once_per_laying_to_its_hamlet() {
        // laid and charged, then charged again: one price. Taken up
        // and laid again in the same millisecond: a second.
        let started = Started::new().await;
        let hamlet = HamletId("h".to_owned());
        started.found(&hamlet, AgentNumber(2)).await;
        let (three, four) = (AgentNumber(3), AgentNumber(4));
        let lay = || laid::Change::Lay {
            a: three,
            b: four,
            hamlet: hamlet.clone(),
        };
        let road = |a, b| Change::Road {
            a,
            b,
            hamlet: hamlet.clone(),
        };
        let take_up = laid::Change::TakeUp { a: four, b: three };
        let price = limits::RoadRules::default().price;
        let commit = async |changes: Vec<change::Change>| {
            change::commit(&started.realm, &started.ctx, changes)
                .await
                .unwrap();
            let rows = started.rows();
            let payer = Payer::Hamlet {
                hamlet: hamlet.clone(),
            };
            spent(&rows, &payer).await.unwrap().micros
        };

        let once =
            commit(vec![lay().into(), road(three, four).into()])
                .await;
        let repeated = commit(vec![road(four, three).into()]).await;
        let relaid = commit(vec![
            take_up.into(),
            lay().into(),
            road(three, four).into(),
        ])
        .await;
        let realm =
            spent(&started.rows(), &Payer::Realm).await.unwrap();

        assert_eq!(
            (once, repeated, relaid, realm.micros),
            (price, price, 2 * price, 0)
        );
        started.stop().await;
    }
}
