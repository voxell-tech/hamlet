//! The tree of hires: the Hand above every lead, and each hire under
//! its parent.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, HamletId};
use hamlet_store::schema::{agent, hamlet, hire};

/// The living hires whose parent is `parent`, or the living leads of
/// living hamlets when `parent` is `None`, the Hand.
pub(crate) fn children(
    db: &mut Db,
    parent: Option<AgentId>,
) -> Result<Vec<AgentId>> {
    Ok(match parent {
        Some(parent) => hire::table
            .inner_join(agent::table)
            .filter(hire::parent.eq(parent))
            .filter(agent::ended_at.is_null())
            .order(hire::agent)
            .select(hire::agent)
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

/// The parent of `hire`: `None` for a lead, which is under the Hand.
pub(crate) fn parent(
    db: &mut Db,
    hire: AgentId,
) -> Result<Option<AgentId>> {
    Ok(hire::table
        .find(hire)
        .select(hire::parent)
        .first::<Option<AgentId>>(db)?)
}

/// Whether `ancestor` is above `hire` in its tree.
pub(crate) fn is_above(
    db: &mut Db,
    ancestor: AgentId,
    hire: AgentId,
) -> Result<bool> {
    let mut at = hire;
    while let Some(up) = parent(db, at)? {
        if up == ancestor {
            return Ok(true);
        }
        at = up;
    }
    Ok(false)
}

/// Every living hire under `ancestor`, at any depth.
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
