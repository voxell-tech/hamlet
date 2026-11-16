//! Roads between hamlets that both mayors agree to: the `agreement`
//! table and `AgreeRoad` (H3, H14).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::{
    AgentRefusal, AgreeRefusal, AgreeRoad, Agreed, HamletId, Named,
    Refusable,
};
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::callers::Decider as _;
use super::calls::Made;
use super::change::{Affected, Rows, Writer, number};
use super::tree::{self, Lineage, Status};
use super::{laid, record, roads};

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The mayor `by`'s half of the road between `a` and `b`.
    Agree {
        a: AgentNumber,
        b: AgentNumber,
        by: AgentNumber,
        why: Option<String>,
    },
    /// The road between `a` and `b` is laid: its agreements are
    /// spent.
    Spent { a: AgentNumber, b: AgentNumber },
}

pub(super) async fn apply(
    writer: &Writer<'_>,
    change: Change,
) -> Result<Affected> {
    match change {
        Change::Agree { a, b, by, why } => {
            let mut params = laid::key(a, b);
            params.extend([
                number(by),
                why.map_or(BindParam::Null, BindParam::Text),
                BindParam::Integer(writer.at().0),
            ]);
            writer
                .execute(
                    "INSERT INTO agreement (a, b, by, why, at) \
                     VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT DO \
                     NOTHING",
                    params,
                )
                .await?;
        }
        Change::Spent { a, b } => {
            writer
                .execute(
                    "DELETE FROM agreement WHERE a = ?1 AND b = ?2",
                    laid::key(a, b),
                )
                .await?;
        }
    }
    Ok(Affected::default())
}

pub(super) struct Agreeing {
    /// The caller's hamlet, when it is its mayor.
    mayor_of: Option<HamletId>,
    /// `mine` and `theirs`.
    ends: Option<(Lineage, Lineage)>,
    allowed: bool,
    has_road: bool,
    /// Whether the mayor of the other end's hamlet agreed already.
    agreed_there: bool,
}

impl Agreeing {
    async fn load(
        rows: &Rows<'_>,
        by: &Agent,
        mine: &Named,
        theirs: &Named,
    ) -> Result<Self> {
        let mayor_of = tree::lineage(rows, by.0)
            .await?
            .filter(Lineage::is_mayor)
            .and_then(|caller| caller.hamlet)
            .map(|hamlet| hamlet.id);
        let mine = tree::named(rows, by, mine).await?;
        let theirs = match tree::resolve(rows, None, theirs).await? {
            Some(theirs) => tree::lineage(rows, theirs).await?,
            None => None,
        };
        let mut agreeing = Self {
            mayor_of,
            ends: None,
            allowed: roads::allows_between_hamlets(rows).await?,
            has_road: false,
            agreed_there: false,
        };
        let (Some(mine), Some(theirs)) = (mine, theirs) else {
            return Ok(agreeing);
        };
        let (a, b) = (mine.agent.agent, theirs.agent.agent);
        agreeing.has_road =
            roads::ends(rows, &Agent(a), &Named::Agent { agent: b })
                .await?
                .way()
                .is_some();
        if let Some(own) = &agreeing.mayor_of {
            let mut params = laid::key(a, b);
            params.push(BindParam::Text(own.0.clone()));
            agreeing.agreed_there = rows
                .value::<i64>(
                    "SELECT 1 FROM agreement JOIN hire ON \
                     hire.agent = agreement.by WHERE agreement.a = \
                     ?1 AND agreement.b = ?2 AND hire.hamlet != ?3",
                    params,
                )
                .await?
                .is_some();
        }
        agreeing.ends = Some((mine, theirs));
        Ok(agreeing)
    }
}

/// What a mayor's half came to.
#[derive(Serialize, Deserialize)]
pub(super) enum Outcome {
    Waiting,
    Laid,
}

impl Act<Agent> for AgreeRoad {
    type Output = Agreed<AgentRefusal<AgreeRefusal>>;
    type Loaded = Agreeing;
    type Done = Outcome;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Agreeing> {
        Agreeing::load(rows, by, &self.mine, &self.theirs).await
    }

    fn decide(
        &self,
        by: &Agent,
        agreeing: Agreeing,
    ) -> Decision<Outcome, Self::Output> {
        let refuse = |why| {
            Decision::Answer(Agreed::refused(AgentRefusal::Act(why)))
        };
        let Some(own) = &agreeing.mayor_of else {
            return refuse(AgreeRefusal::NotAMayor);
        };
        let Some((mine, theirs)) = &agreeing.ends else {
            return refuse(AgreeRefusal::NoSuchAgent);
        };
        let ends = [mine, theirs];
        if ends
            .iter()
            .any(|end| matches!(end.agent.status, Status::Ended(_)))
        {
            return refuse(AgreeRefusal::NoSuchAgent);
        }
        if !agreeing.allowed {
            return refuse(AgreeRefusal::NoRoadsBetween);
        }
        let hamlets = match (&mine.hamlet, &theirs.hamlet) {
            (Some(one), Some(other))
                if ends
                    .iter()
                    .all(|end| end.agent.kind == AgentKind::Hire) =>
            {
                [&one.id, &other.id]
            }
            _ => return refuse(AgreeRefusal::NotHires),
        };
        if hamlets[0] == hamlets[1] {
            return refuse(AgreeRefusal::SameHamlet);
        }
        if !hamlets.contains(&own) {
            return refuse(AgreeRefusal::NotAMayor);
        }
        let (a, b) = (mine.agent.agent, theirs.agent.agent);
        if agreeing.has_road {
            return refuse(AgreeRefusal::RoadExists { a, b });
        }
        if !agreeing.agreed_there {
            return Decision::Change(
                Decided::new(Outcome::Waiting).change(
                    Change::Agree {
                        a,
                        b,
                        by: by.0,
                        why: self.why.clone(),
                    },
                ),
            );
        }
        let road = |a, b| record::Entry::Road { a, b, by: by.by() };
        Decision::Change(
            Decided::new(Outcome::Laid)
                .change(laid::Change::Agreed {
                    a,
                    b,
                    hamlet: own.clone(),
                })
                .change(Change::Spent { a, b })
                .record(road(a, b))
                .record(road(b, a)),
        )
    }

    fn answer(&self, done: Outcome, _made: &Made) -> Self::Output {
        match done {
            Outcome::Waiting => Agreed::Waiting,
            Outcome::Laid => Agreed::Laid,
        }
    }
}
