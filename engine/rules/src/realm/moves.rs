//! An agent moving to another hamlet with its house, once both mayors
//! agree (H14).

use anyhow::{Result, bail};
use hamlet_model::log;
use serde_json::{Value, json};

use super::{Realm, names};
use crate::hamlet::MAYOR;
use crate::record::text;

/// An agent that changed hamlet.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub agent: String,
    pub from: String,
    pub to: String,
    /// The agents it keeps a road to, now between hamlets.
    pub kept: Vec<String>,
    /// The roads neither mayor, or only one, agreed to keep.
    pub lapsed: Vec<String>,
}

impl Realm {
    /// Each mayor's agreement about moving `agent` to `to`: the
    /// agreeing hamlet and the roads it would keep.
    pub(super) fn move_agreements(
        events: &[Value],
        agent: &str,
        to: &str,
    ) -> Vec<(String, Vec<String>)> {
        let mut agreed = Vec::<(String, Vec<String>)>::new();
        for e in events {
            if text(e, "agent") != agent {
                continue;
            }
            match e["type"].as_str() {
                Some("move_agreed") if text(e, "to") == to => {
                    let hamlet = text(e, "hamlet");
                    agreed.retain(|(h, _)| h != &hamlet);
                    agreed.push((hamlet, names(&e["keep"])));
                }
                Some("agent_moved") => agreed.clear(),
                _ => {}
            }
        }
        agreed
    }

    /// The mayor of `hamlet` agrees that `agent` moves to the hamlet
    /// `to`, keeping its roads to the agents in `keep`. It moves once
    /// both mayors have agreed, and keeps only the roads both named
    /// (H14).
    pub fn agree_move(
        &self,
        hamlet: &str,
        agent: &str,
        to: &str,
        keep: &[String],
        reason: &str,
    ) -> Result<Option<Moved>> {
        let Some(from) = self.hamlet_of(agent) else {
            bail!("no agent named `{agent}`");
        };
        let (old, new) = (self.hamlet(&from)?, self.hamlet(to)?);
        if from == to {
            bail!("`{agent}` already lives in `{to}`");
        }
        if hamlet != from && hamlet != to {
            bail!(
                "only the mayor `{agent}` leaves and the mayor it \
                 joins decide this"
            );
        }
        if old.is_mayor(agent) {
            bail!("a mayor never leaves its hamlet");
        }
        let Some(house) = old.house_dir(agent) else {
            bail!(
                "the house of `{agent}` is its hamlet's own folder \
                 and cannot be moved"
            );
        };
        if new.is_full()? {
            bail!("the hamlet `{to}` is full");
        }
        self.append(json!({
            "v": 1,
            "type": "move_agreed",
            "ts": log::now_millis(),
            "agent": agent,
            "to": to,
            "hamlet": hamlet,
            "keep": keep,
            "reason": reason.trim(),
        }))?;
        let agreed =
            Realm::move_agreements(&self.events(), agent, to);
        let kept_by = |h: &str| {
            agreed.iter().find(|(by, _)| by == h).map(|(_, k)| k)
        };
        let (Some(leaving), Some(joining)) =
            (kept_by(&from), kept_by(to))
        else {
            return Ok(None);
        };
        let (kept, lapsed) = old
            .roads()
            .into_iter()
            .filter(|r| r.has(agent) && !r.has(MAYOR))
            .map(|r| r.other(agent).to_string())
            .partition::<Vec<_>, _>(|other| {
                leaving.contains(other) && joining.contains(other)
            });
        let resident = old.member(agent).expect("it lives there");
        let target = new.root().join("agents").join(agent);
        std::fs::create_dir_all(new.root().join("agents"))?;
        std::fs::rename(&house, &target)?;
        let ts = log::now_millis();
        old.append(json!({
            "v": 1, "type": "agent_left", "ts": ts,
            "name": agent, "to": to, "reason": reason.trim(),
        }))?;
        new.append(json!({
            "v": 1, "type": "agent_arrived", "ts": ts,
            "name": agent, "purpose": resident.purpose, "from": from,
        }))?;
        self.append(json!({
            "v": 1, "type": "agent_moved", "ts": ts,
            "agent": agent, "from": from, "to": to,
            "kept": kept, "lapsed": lapsed,
        }))?;
        for other in &kept {
            for by in [from.as_str(), to] {
                self.append(json!({
                    "v": 1, "type": "road_agreed", "ts": ts,
                    "a": agent, "b": other, "hamlet": by,
                    "reason": format!("kept when `{agent}` moved"),
                }))?;
            }
        }
        Ok(Some(Moved {
            agent: agent.to_string(),
            from,
            to: to.to_string(),
            kept,
            lapsed,
        }))
    }
}
