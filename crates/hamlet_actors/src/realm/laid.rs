//! Roads between two hires: the `road` table, `LayRoad`,
//! `TakeUpRoad` and `RequestRoad` (H3).

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_actor::BindParam;
use hamlet_messages::address::Agent;
use hamlet_messages::agent::AgentKind;
use hamlet_messages::realm::world::RoadSeen;
use hamlet_messages::realm::{
    AgentName, AgentRefusal, Done, HamletId, LayRefusal, LayRoad,
    LetterId, Named, Refusable, RequestRoad, RequestRoadRefusal,
    TakeUpRefusal, TakeUpRoad,
};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use serde::{Deserialize, Serialize};

use super::act::{Act, Called, Decided, Decision, Paid, ToolAct};
use super::callers::Lead;
use super::calls::{Calls, Delivery, Made};
use super::change::{self, Affected, At, Rows, Writer, number};
use super::chiefs::limits;
use super::roads::{self, Way};
use super::tree::{self, Lineage, Status};
use super::world::Part;
use super::{budget, hire, letters, record};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    /// Laid by a lead or a chief between two hires of one hamlet.
    Tree,
    /// Agreed by the mayors of two hamlets.
    Agreed,
}

#[derive(Debug, PartialEq)]
pub(super) enum Change {
    Lay {
        a: AgentNumber,
        b: AgentNumber,
        hamlet: HamletId,
    },
    /// `hamlet` is the one of the mayor who agreed last.
    Agreed {
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
            lay(writer, a, b, hamlet, Kind::Tree).await
        }
        Change::Agreed { a, b, hamlet } => {
            lay(writer, a, b, hamlet, Kind::Agreed).await
        }
        Change::TakeUp { a, b } => take_up(writer, a, b).await,
        Change::Used { between } => used(writer, between).await,
    }
}

/// The `road` key of the pair `a`, `b`, which `agreement` shares.
pub(super) fn key(a: AgentNumber, b: AgentNumber) -> Vec<BindParam> {
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
    kind: Kind,
) -> Result<Affected> {
    let mut params = key(a, b);
    params.extend([
        BindParam::Text(hamlet.0),
        BindParam::Integer(writer.at().0),
        BindParam::Text(change::case_name(&kind)?),
    ]);
    writer
        .execute(
            "INSERT INTO road (a, b, hamlet, at, kind) VALUES (?1, \
             ?2, ?3, ?4, ?5) ON CONFLICT (a, b) DO UPDATE SET \
             hamlet = ?3, kind = ?5, used_at = NULL, up_at = NULL, \
             at = MAX(?4, at + 1)",
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

/// The kind of the open road that joins `a` and `b`.
pub(super) async fn between(
    rows: &Rows<'_>,
    a: AgentNumber,
    b: AgentNumber,
) -> Result<Option<Kind>> {
    rows.value::<String>(
        "SELECT kind FROM road WHERE a = ?1 AND b = ?2 AND up_at IS \
         NULL",
        key(a, b),
    )
    .await?
    .map(change::from_case_name)
    .transpose()
}

/// The other ends of `agent`'s open roads, with each road's kind.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<(AgentNumber, Kind)>> {
    rows.rows::<(u32, String)>(
        "SELECT CASE WHEN a = ?1 THEN b ELSE a END, kind FROM road \
         WHERE (a = ?1 OR b = ?1) AND up_at IS NULL ORDER BY 1",
        vec![number(agent)],
    )
    .await?
    .into_iter()
    .map(|(other, kind)| {
        Ok((AgentNumber(other), change::from_case_name(kind)?))
    })
    .collect()
}

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

const LIVE_ROADS: &str = "road JOIN agent AS one ON one.id = road.a \
                          JOIN agent AS other ON other.id = road.b \
                          WHERE road.up_at IS NULL AND one.status \
                          != 'ended' AND other.status != 'ended'";

/// The roads a client sees: an agreed one only while the chief
/// allows roads between hamlets.
pub(super) async fn live_roads(
    rows: &Rows<'_>,
) -> Result<Vec<RoadSeen>> {
    let between = limits::roads_between(rows).await?;
    Ok(rows
        .rows::<(u32, u32)>(
            &format!(
                "SELECT road.a, road.b FROM {LIVE_ROADS} AND \
                 (road.kind = 'tree' OR ?1) ORDER BY road.a, road.b"
            ),
            vec![BindParam::Integer(i64::from(between))],
        )
        .await?
        .into_iter()
        .map(|(a, b)| RoadSeen {
            a: AgentNumber(a),
            b: AgentNumber(b),
        })
        .collect())
}

async fn live_roads_in(
    rows: &Rows<'_>,
    hamlet: &HamletId,
) -> Result<u32> {
    Ok(rows
        .value(
            &format!(
                "SELECT COUNT(*) FROM {LIVE_ROADS} AND road.hamlet \
                 = ?1 AND road.kind = 'tree'"
            ),
            vec![BindParam::Text(hamlet.0.clone())],
        )
        .await?
        .unwrap_or(0))
}

pub(super) struct Laying {
    /// Both ends, or the name that names no agent.
    ends: Result<(Lineage, Lineage), Named>,
    laid: bool,
    live_roads: u32,
}

impl Laying {
    async fn load(
        rows: &Rows<'_>,
        by: &impl Lead,
        a: &Named,
        b: &Named,
    ) -> Result<Self> {
        let missing = |named: &Named| Self {
            ends: Err(named.clone()),
            laid: false,
            live_roads: 0,
        };
        let Some(one) = tree::named(rows, by, a).await? else {
            return Ok(missing(a));
        };
        let Some(other) = tree::named(rows, by, b).await? else {
            return Ok(missing(b));
        };
        let (a, b) = (one, other);
        let live_roads = match &a.hamlet {
            Some(hamlet) => live_roads_in(rows, &hamlet.id).await?,
            None => 0,
        };
        Ok(Self {
            laid: between(rows, a.agent.agent, b.agent.agent)
                .await?
                .is_some(),
            ends: Ok((a, b)),
            live_roads,
        })
    }
}

impl<By: Lead> Act<By> for LayRoad {
    type Output = By::OnRoad<By::Why<LayRefusal>>;
    type Loaded = Laying;
    type Done = (AgentName, AgentName);

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Laying> {
        Laying::load(rows, by, &self.a, &self.b).await
    }

    fn decide(
        &self,
        by: &By,
        laying: Laying,
    ) -> Decision<(AgentName, AgentName), Self::Output> {
        let refuse = |why| {
            Decision::Answer(Self::Output::refused(By::refused(why)))
        };
        let (a, b) = match &laying.ends {
            Ok(ends) => ends,
            Err(named) => {
                return refuse(LayRefusal::NoSuchAgent {
                    address: named.to_string(),
                });
            }
        };
        if [a, b].iter().any(|end| end.agent.kind != AgentKind::Hire)
        {
            return refuse(LayRefusal::NotTwoHiresOfOneHamlet);
        }
        for end in [a, b] {
            if let Err(not_a_lead) = by.leads(end) {
                return Decision::Answer(Self::Output::refused(
                    not_a_lead,
                ));
            }
        }
        let (one, two) = (a.agent.agent, b.agent.agent);
        if one == two {
            return refuse(LayRefusal::NotTwoHiresOfOneHamlet);
        }
        for end in [a, b] {
            if let Status::Ended(left) = &end.agent.status {
                return refuse(LayRefusal::Left {
                    hire: end.agent.name.clone(),
                    left: left.clone(),
                });
            }
        }
        let hamlet = match (&a.hamlet, &b.hamlet) {
            (Some(hamlet), Some(other)) if hamlet.id == other.id => {
                hamlet
            }
            _ => return refuse(LayRefusal::NotTwoHiresOfOneHamlet),
        };
        if laying.laid
            || a.agent.lead == Some(two)
            || b.agent.lead == Some(one)
        {
            return refuse(LayRefusal::RoadExists {
                a: a.agent.name.clone(),
                b: b.agent.name.clone(),
            });
        }
        let most = hamlet.limits.road.most;
        if laying.live_roads >= most {
            return refuse(LayRefusal::Full {
                hamlet: hamlet.id.clone(),
                most,
            });
        }
        Decision::Change(
            Decided::new((
                a.agent.name.clone(),
                b.agent.name.clone(),
            ))
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
                why: self.why.clone(),
            }),
        )
    }

    fn answer(
        &self,
        (a, b): (AgentName, AgentName),
        _made: &Made,
    ) -> Self::Output {
        By::on_road(a, b)
    }
}

impl<By: Lead> Act<By> for TakeUpRoad {
    type Output = By::OnRoad<By::Why<TakeUpRefusal>>;
    type Loaded = Laying;
    type Done = (AgentName, AgentName);

    async fn load(&self, by: &By, rows: &Rows<'_>) -> Result<Laying> {
        Laying::load(rows, by, &self.a, &self.b).await
    }

    fn decide(
        &self,
        by: &By,
        laying: Laying,
    ) -> Decision<(AgentName, AgentName), Self::Output> {
        let (a, b) = match &laying.ends {
            Ok(ends) => ends,
            Err(named) => {
                return Decision::Answer(Self::Output::refused(
                    By::refused(TakeUpRefusal::NoSuchAgent {
                        address: named.to_string(),
                    }),
                ));
            }
        };
        for end in [a, b] {
            if let Err(not_a_lead) = by.leads(end) {
                return Decision::Answer(Self::Output::refused(
                    not_a_lead,
                ));
            }
        }
        let names = (a.agent.name.clone(), b.agent.name.clone());
        let (a, b) = (a.agent.agent, b.agent.agent);
        if !laying.laid {
            let (a, b) = names;
            return Decision::Answer(Self::Output::refused(
                By::refused(TakeUpRefusal::NoRoad { a, b }),
            ));
        }
        Decision::Change(
            Decided::new(names)
                .change(Change::TakeUp { a, b })
                .record(record::Entry::Unroad {
                    a,
                    b,
                    by: by.by(),
                    why: self.why.clone(),
                }),
        )
    }

    fn answer(
        &self,
        (a, b): (AgentName, AgentName),
        _made: &Made,
    ) -> Self::Output {
        By::on_road(a, b)
    }
}

pub(super) struct Asking {
    from: AgentNumber,
    /// The other end, with how the letter names it.
    to: Option<(roads::End, String)>,
    lead: Option<roads::End>,
    letter: LetterId,
}

impl Act<Agent> for Paid<RequestRoad> {
    type Output = Done<AgentRefusal<RequestRoadRefusal>>;
    type Loaded = Asking;
    type Done = LetterId;

    async fn load(
        &self,
        by: &Agent,
        rows: &Rows<'_>,
    ) -> Result<Asking> {
        let ends = roads::ends(rows, by, &self.act.to).await?;
        let lead = ends
            .from
            .lead
            .or(ends.from.is_mayor.then_some(Hand::NUMBER));
        let lead = match lead {
            Some(lead) => Some(roads::end_of(rows, lead).await?),
            None => None,
        };
        let to = match ends.to {
            Some(to) => {
                let name =
                    record::agent_name(rows, to.agent).await?.name;
                Some((to, name))
            }
            None => None,
        };
        Ok(Asking {
            from: ends.from.agent,
            to,
            lead,
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
        let Asking {
            from,
            to,
            lead,
            letter,
        } = asking;
        let Some((_, name)) = to
            .filter(|(to, _)| !matches!(to.status, Status::Ended(_)))
        else {
            return refuse(RequestRoadRefusal::NoSuchAgent {
                address: self.act.to.to_string(),
            });
        };
        let Some(lead) = lead else {
            return refuse(RequestRoadRefusal::NoLead);
        };
        if lead.status == Status::Hiring {
            return Decision::WaitFor {
                row: hire::row(lead.agent),
                refusal: Done::refused(AgentRefusal::Act(
                    RequestRoadRefusal::StillHiring {
                        lead: lead.name.clone(),
                    },
                )),
            };
        }
        let text = format!(
            "I ask for a road to {name}: {}",
            self.act.why.as_deref().unwrap_or_default().trim()
        );
        let written = letters::Written {
            kind: letters::Kind::Letter,
            text: &text,
            task: self.task.as_ref(),
            conversation: None,
        };
        Decision::Change(written.keep(letter, from, &lead, Way::Tree))
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

impl ToolAct<Agent> for RequestRoad {
    type Run = Paid<RequestRoad>;

    fn run(self, called: &Called) -> Paid<RequestRoad> {
        Paid {
            task: called.task.clone(),
            act: self,
        }
    }
}
