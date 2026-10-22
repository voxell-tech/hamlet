//! Roads inside a hamlet: one from each agent to the mayor's office,
//! and those built between agents.

use anyhow::{Result, bail};
use hamlet_model::log;
use serde_json::json;

use crate::hamlet::{Hamlet, MAYOR};

/// The messages a road has carried when it is shown paved.
const PAVED_FROM: u32 = 3;

/// The messages a road has carried when it is shown as a railway.
const RAIL_FROM: u32 = 10;

/// The use of a road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadUse {
    Overgrown,
    Dirt,
    Paved,
    Rail,
}

/// A road between two agents. An end is an agent's name, or the
/// mayor's office.
#[derive(Clone, Debug, PartialEq)]
pub struct Road {
    /// The two ends, in name order.
    pub ends: (String, String),
    /// Messages carried along it.
    pub messages: u32,
    /// The sender of the latest one.
    pub last_from: Option<String>,
    pub(crate) last_used: u64,
}

impl Road {
    pub(crate) fn new(a: &str, b: &str) -> Self {
        Road {
            ends: ends(a, b),
            messages: 0,
            last_from: None,
            last_used: 0,
        }
    }

    pub fn has(&self, name: &str) -> bool {
        self.ends.0 == name || self.ends.1 == name
    }

    /// The end that is not `name`.
    pub fn other(&self, name: &str) -> &str {
        if self.ends.0 == name {
            &self.ends.1
        } else {
            &self.ends.0
        }
    }

    pub fn joins(&self, a: &str, b: &str) -> bool {
        self.ends == ends(a, b)
    }

    pub(crate) fn carry(&mut self, from: &str, ts: u64) {
        self.messages += 1;
        self.last_from = Some(from.to_string());
        self.last_used = ts;
    }

    /// Its use. One nobody has used for `grows_over` days has grown
    /// over (H3), which the chief's limits set.
    pub fn level(&self, grows_over: u64) -> RoadUse {
        let idle = log::now_millis().saturating_sub(self.last_used);
        let long = grows_over.saturating_mul(24 * 60 * 60 * 1000);
        match self.messages {
            _ if self.last_used > 0 && idle > long => {
                RoadUse::Overgrown
            }
            ..PAVED_FROM => RoadUse::Dirt,
            PAVED_FROM..RAIL_FROM => RoadUse::Paved,
            _ => RoadUse::Rail,
        }
    }
}

/// Two names as the ends of a road, in name order.
pub fn ends(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

impl Hamlet {
    /// Every road inside the hamlet: one from each agent to the
    /// mayor's office, and those a mayor has built between agents.
    pub fn roads(&self) -> Vec<Road> {
        self.census().roads
    }

    pub fn has_road(&self, a: &str, b: &str) -> bool {
        a != b && self.roads().iter().any(|r| r.joins(a, b))
    }

    /// Lays a road between two agents and charges it to the hamlet
    /// (H3). The mayor's `reason` is kept in the record (H17).
    pub fn build_road(
        &self,
        a: &str,
        b: &str,
        reason: &str,
        task: &str,
    ) -> Result<()> {
        self.lay_road(a, b, reason, task, None)
    }

    /// Lays a road between two agents at the word of `chief`. It
    /// counts against `most_roads` and is charged to the hamlet like
    /// any other.
    pub(crate) fn chief_road(
        &self,
        chief: &str,
        a: &str,
        b: &str,
        reason: &str,
    ) -> Result<()> {
        let mayor = self.mayor().name;
        if [a, b].iter().any(|end| *end == MAYOR || **end == mayor) {
            bail!("every agent already has a road to the mayor");
        }
        self.lay_road(a, b, reason, "", Some(chief))
    }

    /// Takes up the road between two agents at the word of
    /// `chief`. What it cost is not given back. An agent's road
    /// to the mayor stays for as long as the agent does.
    pub(crate) fn remove_road(
        &self,
        chief: &str,
        a: &str,
        b: &str,
        reason: &str,
    ) -> Result<()> {
        let mayor = self.mayor().name;
        if [a, b].iter().any(|end| *end == MAYOR || **end == mayor) {
            bail!(
                "an agent's road to the mayor stays for as long as \
                 the agent does"
            );
        }
        if !self.has_road(a, b) {
            bail!("`{a}` and `{b}` have no road");
        }
        self.append(json!({
            "v": 1,
            "type": "road_removed",
            "ts": log::now_millis(),
            "a": a,
            "b": b,
            "by": chief,
            "reason": reason.trim(),
        }))
    }

    fn lay_road(
        &self,
        a: &str,
        b: &str,
        reason: &str,
        task: &str,
        chief: Option<&str>,
    ) -> Result<()> {
        let census = self.census();
        for name in [a, b] {
            if !census.residents.iter().any(|r| r.name == name) {
                bail!("no agent named `{name}` in this hamlet");
            }
        }
        if a == b {
            bail!("a road needs two different agents");
        }
        if census.roads.iter().any(|r| r.joins(a, b)) {
            bail!("`{a}` and `{b}` already have a road");
        }
        let limits = self.limits()?;
        let built = census
            .roads
            .iter()
            .filter(|r| r.ends.0 != MAYOR && r.ends.1 != MAYOR)
            .count();
        if built >= limits.most_roads {
            match chief {
                Some(_) => bail!(
                    "the hamlet has all the roads your limits allow \
                     ({built}). Raise `most_roads` in limits.toml \
                     to build more"
                ),
                None => bail!(
                    "the hamlet has all the roads the chief allows \
                     ({built}). Only the chief can raise \
                     `most_roads` in limits.toml"
                ),
            }
        }
        self.pay_for_road(&limits, task)?;
        self.append(json!({
            "v": 1,
            "type": "road_built",
            "ts": log::now_millis(),
            "a": a,
            "b": b,
            "by": chief.unwrap_or(MAYOR),
            "reason": reason.trim(),
        }))
    }

    /// Notes a message of `task` carried along the road between
    /// `from` and `to`.
    pub fn carried(
        &self,
        from: &str,
        to: &str,
        task: &str,
    ) -> Result<()> {
        self.append(json!({
            "v": 1,
            "type": "message",
            "ts": log::now_millis(),
            "from": from,
            "to": to,
            "task": task,
        }))
    }
}
