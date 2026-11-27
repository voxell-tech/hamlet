//! Roads between hamlets that both mayors agree to: the `agreement`
//! table and `AgreeRoad` (H3, H14).

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::roads::{
    AgreeRefusal, AgreeRoad, Agreed,
};
use hamlet_messages::realm::{
    AgentName, AgentRefusal, HamletId, Named, Refusable,
};
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{Act, Decided, Decision};
use super::callers::Decider as _;
use super::calls::Made;
use super::change::{Affected, Rows, Writer, number};
use super::chiefs::limits;
use super::tree::{self, Lineage, Status};
use super::{budget, laid, record, roads};

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    /// The mayor `by`'s half of the road between `a` and `b`.
    Agree {
        a: AgentNumber,
        b: AgentNumber,
        by: AgentNumber,
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
        Change::Agree { a, b, by } => {
            let mut params = laid::key(a, b);
            params.extend([
                number(by),
                BindParam::Integer(writer.at().0),
            ]);
            writer
                .execute(
                    "INSERT INTO agreement (a, b, by, at) VALUES \
                     (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
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
    /// `mine` and `theirs`, or the name that names no agent.
    ends: Result<(Lineage, Lineage), Named>,
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
        let mut agreeing = Self {
            mayor_of,
            ends: Err(mine.clone()),
            allowed: limits::roads_between(rows).await?,
            has_road: false,
            agreed_there: false,
        };
        let Some(mine) = tree::named(rows, by, mine).await? else {
            return Ok(agreeing);
        };
        let Some(theirs) = tree::named_in(rows, None, theirs).await?
        else {
            agreeing.ends = Err(theirs.clone());
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
                .value::<u32>(
                    "SELECT agreement.by FROM agreement JOIN hire \
                     ON hire.agent = agreement.by WHERE agreement.a \
                     = ?1 AND agreement.b = ?2 AND hire.hamlet != ?3",
                    params,
                )
                .await?
                .is_some();
        }
        agreeing.ends = Ok((mine, theirs));
        Ok(agreeing)
    }
}

/// What a mayor's half of the road between `mine` and `theirs` came
/// to.
#[derive(Serialize, Deserialize)]
pub(super) enum Outcome {
    Waiting { mine: AgentName, theirs: AgentName },
    Laid { mine: AgentName, theirs: AgentName },
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
        let no_such = |named: &Named| {
            refuse(AgreeRefusal::NoSuchAgent {
                address: named.to_string(),
            })
        };
        let (mine, theirs) = match &agreeing.ends {
            Ok(ends) => ends,
            Err(named) => return no_such(named),
        };
        let ends = [mine, theirs];
        for (end, named) in
            [(mine, &self.mine), (theirs, &self.theirs)]
        {
            if matches!(end.agent.status, Status::Ended(_)) {
                return no_such(named);
            }
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
        let names =
            (mine.agent.name.clone(), theirs.agent.name.clone());
        if agreeing.has_road {
            let (a, b) = names;
            return refuse(AgreeRefusal::RoadExists { a, b });
        }
        let (mine, theirs) = names;
        if !agreeing.agreed_there {
            return Decision::Change(
                Decided::new(Outcome::Waiting { mine, theirs })
                    .change(Change::Agree { a, b, by: by.0 }),
            );
        }
        let agreed = |hamlet: &HamletId| record::Entry::RoadAgreed {
            hamlet: hamlet.clone(),
            a,
            b,
            by: by.by(),
            why: self.why.clone(),
        };
        let charged = |hamlet: &HamletId| budget::Change::Road {
            a,
            b,
            hamlet: hamlet.clone(),
        };
        Decision::Change(
            Decided::new(Outcome::Laid { mine, theirs })
                .change(laid::Change::Agreed {
                    a,
                    b,
                    hamlet: own.clone(),
                })
                .change(Change::Spent { a, b })
                .change(charged(hamlets[0]))
                .change(charged(hamlets[1]))
                .record(agreed(hamlets[0]))
                .record(agreed(hamlets[1])),
        )
    }

    fn answer(&self, done: Outcome, _made: &Made) -> Self::Output {
        match done {
            Outcome::Waiting { mine, theirs } => {
                Agreed::Waiting { mine, theirs }
            }
            Outcome::Laid { mine, theirs } => {
                Agreed::Laid { mine, theirs }
            }
        }
    }
}
