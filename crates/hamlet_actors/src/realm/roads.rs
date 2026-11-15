//! The road rule: which agents have a road between them (H3, H14,
//! H30).

use std::collections::BTreeSet;

use anyhow::Result;
use hamlet_messages::address::Agent;
use hamlet_messages::realm::{HamletId, Named};
use hamlet_messages::sender::Hand;
use hamlet_messages::workspace::AgentNumber;

use super::change::Rows;
use super::tree::{self, Lineage, Status};
use super::{chiefs, laid};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Way {
    /// Between a lead and its hire, or the Hand and a mayor.
    Tree,
    Mayors,
    Laid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct End {
    pub agent: AgentNumber,
    pub status: Status,
    pub lead: Option<AgentNumber>,
    pub hamlet: Option<HamletId>,
    pub is_mayor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Ends {
    pub from: End,
    pub to: Option<End>,
    /// Whether an open laid road joins them.
    pub laid: bool,
}

impl Ends {
    #[expect(dead_code, reason = "U6 and U11 call it")]
    pub fn way(&self) -> Option<Way> {
        way(&self.from, self.to.as_ref()?, self.laid)
    }
}

/// The way between `a` and `b`, whatever their status.
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

#[expect(dead_code, reason = "U6 and U11 call it")]
pub(super) async fn ends(
    rows: &Rows<'_>,
    from: &Agent,
    to: &Named,
) -> Result<Ends> {
    let from = end(&tree::lineage_of(rows, from.0).await?);
    let Some(to) =
        tree::resolve(rows, from.hamlet.as_ref(), to).await?
    else {
        return Ok(Ends {
            from,
            to: None,
            laid: false,
        });
    };
    Ok(Ends {
        laid: laid::between(rows, from.agent, to).await?,
        to: Some(end(&tree::lineage_of(rows, to).await?)),
        from,
    })
}

/// Every agent `agent` has a road to, with the way. A road's other
/// end is live.
pub(super) async fn of(
    rows: &Rows<'_>,
    agent: AgentNumber,
) -> Result<Vec<(AgentNumber, Way)>> {
    let own = tree::lineage_of(rows, agent).await?;
    let laid = laid::of(rows, agent).await?;
    let mut others = own
        .agent
        .lead
        .into_iter()
        .chain(own.hires.iter().map(|hire| hire.agent))
        .chain(laid.iter().copied())
        .collect::<BTreeSet<_>>();
    let own = end(&own);
    if own.agent == Hand::NUMBER || own.is_mayor {
        others.extend(chiefs::mayors(rows).await?);
        others.insert(Hand::NUMBER);
    }
    others.remove(&agent);
    let mut roads = Vec::new();
    for other in others {
        let other = end(&tree::lineage_of(rows, other).await?);
        let laid = laid.contains(&other.agent);
        if let (Status::Live, Some(way)) =
            (&other.status, way(&own, &other, laid))
        {
            roads.push((other.agent, way));
        }
    }
    Ok(roads)
}

fn end(lineage: &Lineage) -> End {
    End {
        agent: lineage.agent.agent,
        status: lineage.agent.status.clone(),
        lead: lineage.agent.lead,
        hamlet: lineage
            .hamlet
            .as_ref()
            .map(|hamlet| hamlet.id.clone()),
        is_mayor: lineage.is_mayor(),
    }
}
