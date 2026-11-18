//! The tree of agents: the Hand above every lead, and each agent
//! under its parent.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};
use hamlet_store::schema::{agent, hamlet, hamlet_agent};

/// The living agents whose parent is `parent`, or the living leads of
/// living hamlets when `parent` is `None`, the Hand.
pub(crate) fn children(
    db: &mut Db,
    parent: Option<AgentId>,
) -> Result<Vec<AgentId>> {
    Ok(match parent {
        Some(parent) => hamlet_agent::table
            .inner_join(agent::table)
            .filter(hamlet_agent::parent.eq(parent))
            .filter(agent::ended_at.is_null())
            .order(hamlet_agent::agent)
            .select(hamlet_agent::agent)
            .load(db)?,
        None => hamlet::table
            .inner_join(agent::table.on(agent::id.eq(hamlet::lead)))
            .filter(hamlet::removed_at.is_null())
            .filter(agent::ended_at.is_null())
            .order(hamlet::id)
            .select(hamlet::lead)
            .load(db)?,
    })
}

/// The parent of `agent`: `None` for a lead, which is under the Hand.
pub(crate) fn parent(
    db: &mut Db,
    agent: AgentId,
) -> Result<Option<AgentId>> {
    Ok(hamlet_agent::table
        .find(agent)
        .select(hamlet_agent::parent)
        .first::<Option<AgentId>>(db)?)
}

/// Whether `ancestor` is above `agent` in its tree.
pub(crate) fn is_above(
    db: &mut Db,
    ancestor: AgentId,
    agent: AgentId,
) -> Result<bool> {
    let mut at = agent;
    while let Some(up) = parent(db, at)? {
        if up == ancestor {
            return Ok(true);
        }
        at = up;
    }
    Ok(false)
}

/// Every living agent under `ancestor`, at any depth.
pub(crate) fn under(
    db: &mut Db,
    ancestor: AgentId,
) -> Result<Vec<AgentId>> {
    let mut out = Vec::new();
    let mut next = vec![ancestor];
    while let Some(at) = next.pop() {
        for child in children(db, Some(at))? {
            out.push(child);
            next.push(child);
        }
    }
    Ok(out)
}

/// The lead of `hamlet`.
pub(crate) fn lead(
    db: &mut Db,
    hamlet: &HamletId,
) -> Result<AgentId> {
    Ok(hamlet::table.find(hamlet).select(hamlet::lead).first(db)?)
}
