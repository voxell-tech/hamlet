//! The road rule: which agents have a road between them (H3, H14,
//! H30).

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{AgentName, HamletId, Named};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;
use serde::Serialize;

use super::change::Rows;
use super::chiefs::{self, limits};
use super::laid::{self, Kind};
use super::tree::{self, Lineage, Status};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Way {
    /// Between a lead and its hire, or the Hand and a mayor.
    Tree,
    Mayors,
    Laid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct End {
    pub agent: AgentNumber,
    pub name: AgentName,
    pub status: Status,
    pub lead: Option<AgentNumber>,
    pub hamlet: Option<HamletId>,
    pub is_mayor: bool,
    pub can_take_purpose: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ends {
    pub from: End,
    pub to: Option<End>,
    /// Whether an open road joins them that carries letters now.
    pub laid: bool,
}

impl Ends {
    pub fn way(&self) -> Option<Way> {
        way(&self.from, self.to.as_ref()?, self.laid)
    }
}

fn way(a: &End, b: &End, laid: bool) -> Option<Way> {
    let is_hand = |end: &End| end.agent == Hand::NUMBER;
    if a.lead == Some(b.agent)
        || b.lead == Some(a.agent)
        || (is_hand(a) && b.is_mayor)
        || (is_hand(b) && a.is_mayor)
    {
        Some(Way::Tree)
    } else if a.is_mayor && b.is_mayor {
        Some(Way::Mayors)
    } else if laid {
        Some(Way::Laid)
    } else {
        None
    }
}

pub(super) async fn ends(
    rows: &Rows<'_>,
    from: &Agent,
    to: &Named,
) -> Result<Ends> {
    let from = end_of(rows, from.0).await?;
    let Some(to) =
        tree::living_named(rows, from.hamlet.as_ref(), to).await?
    else {
        return Ok(Ends {
            from,
            to: None,
            laid: false,
        });
    };
    let between = limits::roads_between(rows).await?;
    Ok(Ends {
        laid: carries(
            laid::between(rows, from.agent, to).await?,
            between,
        ),
        to: Some(end_of(rows, to).await?),
        from,
    })
}

/// Every live agent `agent` has a road to, with the way.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<(AgentNumber, Way)>> {
    reached(rows, agent, |status| *status == Status::Live).await
}

/// Every agent `agent` has a road to and a prompt names, with the
/// way: the live ones and those still being hired, as today
/// (`crates/hamlet_server/src/society/roads.rs:109-156`).
pub(super) async fn named(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<(AgentNumber, Way)>> {
    reached(rows, agent, |status| !matches!(status, Status::Ended(_)))
        .await
}

async fn reached(
    rows: &Rows<'_>,
    agent: AgentNumber,
    admits: fn(&Status) -> bool,
) -> Result<Vec<(AgentNumber, Way)>> {
    let own = tree::lineage_of(rows, agent).await?;
    let between = limits::roads_between(rows).await?;
    let laid = laid::of(rows, agent)
        .await?
        .into_iter()
        .filter(|&(_, kind)| carries(Some(kind), between))
        .map(|(other, _)| other)
        .collect::<Vec<_>>();
    let mut others = own
        .agent
        .lead
        .into_iter()
        .chain(own.hires.iter().map(|hire| hire.agent))
        .chain(laid.iter().copied())
        .chain(chiefs::mayors(rows).await?)
        .chain([Hand::NUMBER])
        .collect::<BTreeSet<_>>();
    let own = end(&own);
    others.remove(&agent);
    let mut roads = Vec::new();
    for other in others {
        let other = end(&tree::lineage_of(rows, other).await?);
        let laid = laid.contains(&other.agent);
        if admits(&other.status)
            && let Some(way) = way(&own, &other, laid)
        {
            roads.push((other.agent, way));
        }
    }
    Ok(roads)
}

/// Whether a road of `kind` carries letters: an agreed one only
/// while the chief allows roads `between` hamlets.
fn carries(kind: Option<Kind>, between: bool) -> bool {
    match kind {
        None => false,
        Some(Kind::Tree) => true,
        Some(Kind::Agreed) => between,
    }
}

pub(super) async fn end_of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<End> {
    Ok(end(&tree::lineage_of(rows, agent).await?))
}

fn end(lineage: &Lineage) -> End {
    End {
        agent: lineage.agent.agent,
        name: lineage.agent.name.clone(),
        status: lineage.agent.status.clone(),
        lead: lineage.agent.lead,
        hamlet: lineage
            .hamlet
            .as_ref()
            .map(|hamlet| hamlet.id.clone()),
        is_mayor: lineage.is_mayor(),
        can_take_purpose: lineage.agent.can_take_purpose(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::Started;
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn of_holds_each_live_agent_way_admits() {
        let started = Started::new().await;
        let h = HamletId("h".to_owned());
        let k = HamletId("k".to_owned());
        started.found(&h, AgentNumber(2)).await;
        started.found(&k, AgentNumber(5)).await;
        for (agent, hamlet, lead) in [
            (3, &h, 2),
            (4, &h, 2),
            (6, &k, 5),
            (7, &h, 3),
            (8, &h, 2),
        ] {
            started
                .hire(
                    hamlet,
                    AgentNumber(agent),
                    Some(AgentNumber(lead)),
                )
                .await;
        }
        for agent in 1..=7 {
            started
                .commit(tree::Change::Live {
                    agent: AgentNumber(agent),
                })
                .await
                .unwrap();
        }
        let rows = started.rows();
        let mut all = Vec::new();
        for agent in 1..=8 {
            all.push(end(&tree::lineage_of(
                &rows,
                AgentNumber(agent),
            )
            .await
            .unwrap()));
        }

        for own in &all {
            let admitted = all
                .iter()
                .filter(|other| {
                    other.agent != own.agent
                        && other.status == Status::Live
                })
                .filter_map(|other| {
                    Some((other.agent, way(own, other, false)?))
                })
                .collect::<Vec<_>>();
            assert_eq!(
                of(&rows, own.agent).await.unwrap(),
                admitted,
                "agent {}",
                own.agent.0
            );
        }
        started.stop().await;
    }
}
