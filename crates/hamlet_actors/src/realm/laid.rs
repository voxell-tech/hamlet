//! Roads a lead lays between two hires: the `road` table, `LayRoad`,
//! `TakeUpRoad` and `RequestRoad` (H3).

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    AgentRefusal, Done, HamletId, LayRefusal, LayRoad, LetterId,
    Named, Refusable, RequestRoad, RequestRoadRefusal, TakeUpRefusal,
    TakeUpRoad,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::act::{Act, Decided, Decision};
use super::callers::Lead;
use super::calls::{Calls, Delivery, Made};
use super::change::{Affected, At, Rows, Writer, number};
use super::roads::{self, Way};
use super::tasks::TaskId;
use super::tree::{self, Lineage, Status};
use super::world::Part;
use super::{budget, letters, record};

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Lay {
        a: AgentNumber,
        b: AgentNumber,
        hamlet: HamletId,
    },
    TakeUp {
        a: AgentNumber,
        b: AgentNumber,
    },
    Used {
        between: (AgentNumber, AgentNumber),
    },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Lay { a, b, hamlet } => {
            lay(writer, a, b, hamlet).await
        }
        Change::TakeUp { a, b } => take_up(writer, a, b).await,
        Change::Used { between } => used(writer, between).await,
    }
}

/// The road's key, its lower end first.
fn key(a: AgentNumber, b: AgentNumber) -> Vec<BindParam> {
    vec![number(a.min(b)), number(a.max(b))]
}

fn moved(a: AgentNumber, b: AgentNumber) -> Affected {
    Affected {
        places: BTreeSet::from([a, b]),
        ..Affected::seen([Part::Roads])
    }
}

async fn lay(
    writer: &Writer<'_>,
    a: AgentNumber,
    b: AgentNumber,
    hamlet: HamletId,
) -> Result<Affected> {
    let mut params = key(a, b);
    params.extend([
        BindParam::Text(hamlet.0),
        BindParam::Integer(writer.at().0),
    ]);
    // A laying's charge is keyed by `at`, so a road laid again gets a
    // later one even within the same millisecond.
    writer
        .execute(
            "INSERT INTO road (a, b, hamlet, at) VALUES (?1, ?2, \
             ?3, ?4) ON CONFLICT (a, b) DO UPDATE SET hamlet = ?3, \
             used_at = NULL, up_at = NULL, at = MAX(?4, at + 1)",
            params,
        )
        .await?;
    Ok(moved(a, b))
}

async fn take_up(
    writer: &Writer<'_>,
    a: AgentNumber,
    b: AgentNumber,
) -> Result<Affected> {
    let mut params = key(a, b);
    params.push(BindParam::Integer(writer.at().0));
    writer
        .execute(
            "UPDATE road SET up_at = ?3 WHERE a = ?1 AND b = ?2 AND \
             up_at IS NULL",
            params,
        )
        .await?;
    Ok(moved(a, b))
}

/// A road unused past its days is only shown so: it still carries
/// letters (H3).
async fn used(
    writer: &Writer<'_>,
    (a, b): (AgentNumber, AgentNumber),
) -> Result<Affected> {
    let mut params = key(a, b);
    params.push(BindParam::Integer(writer.at().0));
    writer
        .execute(
            "UPDATE road SET used_at = ?3 WHERE a = ?1 AND b = ?2 \
             AND up_at IS NULL",
            params,
        )
        .await?;
    Ok(Affected::default())
}

/// Whether an open laid road joins `a` and `b`.
pub(super) async fn between(
    rows: &Rows<'_>,
    a: AgentNumber,
    b: AgentNumber,
) -> Result<bool> {
    Ok(rows
        .value::<i64>(
            "SELECT 1 FROM road WHERE a = ?1 AND b = ?2 AND up_at \
             IS NULL",
            key(a, b),
        )
        .await?
        .is_some())
}

/// The other ends of `agent`'s open laid roads.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<AgentNumber>> {
    Ok(rows
        .values::<u32>(
            "SELECT CASE WHEN a = ?1 THEN b ELSE a END FROM road \
             WHERE (a = ?1 OR b = ?1) AND up_at IS NULL ORDER BY 1",
            vec![number(agent)],
        )
        .await?
        .into_iter()
        .map(AgentNumber)
        .collect())
}

/// When the road between `a` and `b` was last laid.
pub(super) async fn laid_at(
    rows: &Rows<'_>,
    a: AgentNumber,
    b: AgentNumber,
) -> Result<Option<At>> {
    Ok(rows
        .value::<i64>(
            "SELECT at FROM road WHERE a = ?1 AND b = ?2",
            key(a, b),
        )
        .await?
        .map(At))
}

/// The open laid roads of `hamlet` whose ends have not ended.
async fn open_in(rows: &Rows<'_>, hamlet: &HamletId) -> Result<u32> {
    Ok(rows
        .value(
            "SELECT COUNT(*) FROM road JOIN agent AS one ON one.id \
             = road.a JOIN agent AS other ON other.id = road.b \
             WHERE road.hamlet = ?1 AND road.up_at IS NULL AND \
             one.status != 'ended' AND other.status != 'ended'",
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .unwrap_or(0))
}

pub(super) struct Laying {
    ends: Option<(Lineage, Lineage)>,
    laid: bool,
    /// The open laid roads of the first end's hamlet.
    in_hamlet: u32,
}

impl Laying {
    async fn load(
        rows: &Rows<'_>,
        by: &impl Lead,
        a: &Named,
        b: &Named,
    ) -> Result<Self> {
        let (Some(a), Some(b)) = (
            tree::named(rows, by, a).await?,
            tree::named(rows, by, b).await?,
        ) else {
            return Ok(Self {
                ends: None,
                laid: false,
                in_hamlet: 0,
            });
        };
        let in_hamlet = match &a.hamlet {
            Some(hamlet) => open_in(rows, &hamlet.id).await?,
            None => 0,
        };
        Ok(Self {
            laid: between(rows, a.agent.agent, b.agent.agent).await?,
            ends: Some((a, b)),
            in_hamlet,
        })
    }
}

fn is_living_hire(lineage: &Lineage) -> bool {
    lineage.agent.kind == AgentKind::Hire
        && !matches!(lineage.agent.status, Status::Ended(_))
}

impl<By: Lead> Act<By> for LayRoad {
    type Output = Done<By::Why<LayRefusal>>;
    type Loaded = Laying;
    type Done = ();

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Laying> {
        Laying::load(rows, by, &self.a, &self.b).await
    }

    fn decide(
        &self,
        by: &By,
        laying: Laying,
    ) -> Decision<(), Self::Output> {
        let refuse =
            |why| Decision::Answer(Done::refused(By::refused(why)));
        let Some((a, b)) = &laying.ends else {
            return refuse(LayRefusal::NoSuchAgent);
        };
        if [a, b].iter().any(|end| end.agent.kind == AgentKind::Hand)
        {
            return refuse(LayRefusal::NotTwoHiresOfOneHamlet);
        }
        for end in [a, b] {
            if let Err(not_a_lead) = by.leads(end, end.agent.agent) {
                return Decision::Answer(Done::refused(not_a_lead));
            }
        }
        let (one, two) = (a.agent.agent, b.agent.agent);
        let hamlet = match (&a.hamlet, &b.hamlet) {
            (Some(hamlet), Some(other))
                if hamlet.id == other.id
                    && one != two
                    && is_living_hire(a)
                    && is_living_hire(b) =>
            {
                hamlet
            }
            _ => return refuse(LayRefusal::NotTwoHiresOfOneHamlet),
        };
        if laying.laid
            || a.agent.lead == Some(two)
            || b.agent.lead == Some(one)
        {
            return Decision::Again(());
        }
        let most = hamlet.limits.road.most;
        if laying.in_hamlet >= most {
            return refuse(LayRefusal::Full {
                hamlet: hamlet.id.clone(),
                most,
            });
        }
        Decision::Change(
            Decided::new(())
                .change(Change::Lay {
                    a: one,
                    b: two,
                    hamlet: hamlet.id.clone(),
                })
                .change(budget::Change::Road {
                    a: one,
                    b: two,
                    hamlet: hamlet.id.clone(),
                })
                .record(record::Entry::Road {
                    a: one,
                    b: two,
                    by: by.by(),
                }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

impl<By: Lead> Act<By> for TakeUpRoad {
    type Output = Done<By::Why<TakeUpRefusal>>;
    type Loaded = Laying;
    type Done = ();

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Laying> {
        Laying::load(rows, by, &self.a, &self.b).await
    }

    fn decide(
        &self,
        by: &By,
        laying: Laying,
    ) -> Decision<(), Self::Output> {
        let Some((a, b)) = &laying.ends else {
            return Decision::Answer(Done::refused(By::refused(
                TakeUpRefusal::NoSuchAgent,
            )));
        };
        for end in [a, b] {
            if let Err(not_a_lead) = by.leads(end, end.agent.agent) {
                return Decision::Answer(Done::refused(not_a_lead));
            }
        }
        if !laying.laid {
            return Decision::Again(());
        }
        let (a, b) = (a.agent.agent, b.agent.agent);
        Decision::Change(
            Decided::new(())
                .change(Change::TakeUp { a, b })
                .record(record::Entry::Unroad { a, b, by: by.by() }),
        )
    }

    fn answer(&self, (): (), _made: &Made) -> Self::Output {
        Done::Done
    }
}

pub(super) struct Asking {
    ends: roads::Ends,
    letter: LetterId,
}

impl Act<Agent> for RequestRoad {
    type Output = Done<AgentRefusal<RequestRoadRefusal>>;
    type Loaded = Asking;
    type Done = LetterId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Asking> {
        Ok(Asking {
            ends: roads::ends(rows, by, &self.to).await?,
            letter: letters::fresh(rows).await?,
        })
    }

    fn decide(
        &self,
        _by: &Agent,
        asking: Asking,
    ) -> Decision<LetterId, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Done::refused(AgentRefusal::Act(why)))
        };
        let Asking { ends, letter } = asking;
        let Some(to) = ends.to else {
            return refuse(RequestRoadRefusal::NoSuchAgent);
        };
        let from = ends.from;
        let lead = match from.lead {
            Some(lead) => lead,
            None if from.is_mayor => Hand::NUMBER,
            None => return refuse(RequestRoadRefusal::NoLead),
        };
        let named = if to.agent == Hand::NUMBER {
            "the Hand".to_owned()
        } else {
            format!("`hire#{}`", to.agent.0)
        };
        Decision::Change(Decided::new(letter).change(
            letters::Change::Keep {
                letter,
                kind: letters::Kind::Letter,
                from: from.agent,
                to: lead,
                way: Way::Tree,
                text: format!(
                    "I ask for a road to {named}: {}",
                    self.why
                ),
                task: self.task.as_ref().and_then(TaskId::of),
                conversation: None,
            },
        ))
    }

    fn deliveries(&self, _by: &Agent, letter: &LetterId) -> Calls {
        Calls::after_answer(Delivery::Letter(*letter))
    }

    fn answer(
        &self,
        _letter: LetterId,
        _made: &Made,
    ) -> Self::Output {
        Done::Done
    }
}
