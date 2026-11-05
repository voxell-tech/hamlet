//! What every agent has, whatever its kind: when it came, the harness
//! of its new conversations, and how it ended.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId};
use hamlet_store::schema::{agent, permission_grant};

use crate::change::Change;
use crate::text::text_enum;

/// The harness an agent's conversations run in unless it is given
/// another.
pub(crate) const HARNESS: &str = "pi";

text_enum! {
    pub(crate) enum AgentKind {
        Hand = "hand",
        Hire = "hire",
        Worker = "worker",
    }
}

text_enum! {
    /// How an agent ended.
    pub(crate) enum EndHow {
        /// A lead or a chief ended the hire.
        Ended = "ended",
        /// The hire was merged into another.
        Merged = "merged",
        /// Its scholar left the city.
        Scholar = "scholar",
        /// Its hamlet was removed.
        Hamlet = "hamlet",
        /// The worker answered its task.
        Answered = "answered",
        /// The worker's task failed.
        Failed = "failed",
        /// The worker was stopped.
        Stopped = "stopped",
    }
}

impl EndHow {
    /// The words for an agent that ended this way.
    pub fn words(&self) -> &'static str {
        match self {
            Self::Ended => "it was ended",
            Self::Merged => "it was merged into another hire",
            Self::Scholar => "its scholar left the city",
            Self::Hamlet => "its hamlet was removed",
            Self::Answered => "it answered its task",
            Self::Failed => "its task failed",
            Self::Stopped => "it was stopped",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = agent)]
pub(crate) struct Agent {
    pub id: AgentId,
    pub kind: AgentKind,
    pub since: i64,
    pub harness: String,
    pub ended_at: Option<i64>,
    pub ended_by: Option<Actor>,
    pub ended_how: Option<EndHow>,
    pub ended_reason: Option<String>,
    pub erased: bool,
}

impl Agent {
    pub fn is_living(&self) -> bool {
        self.ended_at.is_none()
    }
}

pub(crate) fn make(
    cx: &mut Change<'_>,
    kind: AgentKind,
) -> Result<AgentId> {
    let id = diesel::insert_into(agent::table)
        .values((
            agent::kind.eq(kind),
            agent::since.eq(cx.at),
            agent::harness.eq(HARNESS),
        ))
        .returning(agent::id)
        .get_result(cx.db)?;
    cx.touched.agents.insert(id);
    Ok(id)
}

pub(crate) fn find(
    db: &mut Db,
    id: AgentId,
) -> Result<Option<Agent>> {
    Ok(agent::table
        .find(id)
        .select(Agent::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: AgentId) -> Result<Agent> {
    find(db, id)?.with_context(|| format!("no agent {id}"))
}

/// Every living agent, oldest first.
pub(crate) fn living(db: &mut Db) -> Result<Vec<AgentId>> {
    Ok(agent::table
        .filter(agent::ended_at.is_null())
        .order(agent::id)
        .select(agent::id)
        .load(db)?)
}

/// Ends the agent `id`: what waited on it lapses and what it was
/// granted is taken back (H28).
pub(crate) fn end(
    cx: &mut Change<'_>,
    id: AgentId,
    how: EndHow,
    reason: &str,
) -> Result<()> {
    diesel::update(agent::table.find(id))
        .set((
            agent::ended_at.eq(cx.at),
            agent::ended_by.eq(cx.actor),
            agent::ended_how.eq(how),
            agent::ended_reason.eq(reason),
        ))
        .execute(cx.db)?;
    diesel::update(
        permission_grant::table
            .filter(permission_grant::agent.eq(id))
            .filter(permission_grant::revoked_at.is_null()),
    )
    .set((
        permission_grant::revoked_at.eq(cx.at),
        permission_grant::revoked_by.eq(Actor::Realm),
    ))
    .execute(cx.db)?;
    crate::oversight::lapse(cx, id, crate::oversight::Ruling::Ended)?;
    cx.touched.agents.insert(id);
    Ok(())
}

/// Brings the agent `id` back, as it was before it ended.
pub(crate) fn come_back(
    cx: &mut Change<'_>,
    id: AgentId,
) -> Result<()> {
    diesel::update(agent::table.find(id))
        .set((
            agent::ended_at.eq(None::<i64>),
            agent::ended_by.eq(None::<Actor>),
            agent::ended_how.eq(None::<EndHow>),
            agent::ended_reason.eq(None::<String>),
        ))
        .execute(cx.db)?;
    cx.touched.agents.insert(id);
    Ok(())
}

/// Marks the ended agent `id` erased, for good.
pub(crate) fn erase(cx: &mut Change<'_>, id: AgentId) -> Result<()> {
    diesel::update(agent::table.find(id))
        .set(agent::erased.eq(true))
        .execute(cx.db)?;
    cx.touched.agents.insert(id);
    Ok(())
}
