//! What every agent has, whatever its kind: when it came, the harness
//! of its new conversations, and how it ended.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{Actor, AgentId};
use hamlet_store::schema::{agent, hand, permission_grant, worker};

use super::hires;
use crate::change::{Change, Done, refuse};
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

text_enum! {
    /// How hard an agent's model thinks (H29a), weakest first.
    pub(crate) enum Effort {
        Off = "off",
        Minimal = "minimal",
        Low = "low",
        Medium = "medium",
        High = "high",
        XHigh = "xhigh",
        Max = "max",
    }
}

impl Effort {
    /// Every effort, weakest first.
    pub const ALL: [Self; 7] = [
        Self::Off,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
    ];

    /// The effort as the protocols carry it.
    pub fn wire(self) -> hamlet_protocol::Effort {
        match self {
            Self::Off => hamlet_protocol::Effort::Off,
            Self::Minimal => hamlet_protocol::Effort::Minimal,
            Self::Low => hamlet_protocol::Effort::Low,
            Self::Medium => hamlet_protocol::Effort::Medium,
            Self::High => hamlet_protocol::Effort::High,
            Self::XHigh => hamlet_protocol::Effort::XHigh,
            Self::Max => hamlet_protocol::Effort::Max,
        }
    }

    /// The effort the protocols name.
    pub fn of(wire: hamlet_protocol::Effort) -> Self {
        match wire {
            hamlet_protocol::Effort::Off => Self::Off,
            hamlet_protocol::Effort::Minimal => Self::Minimal,
            hamlet_protocol::Effort::Low => Self::Low,
            hamlet_protocol::Effort::Medium => Self::Medium,
            hamlet_protocol::Effort::High => Self::High,
            hamlet_protocol::Effort::XHigh => Self::XHigh,
            hamlet_protocol::Effort::Max => Self::Max,
        }
    }

    /// The effort among `supported` nearest to this one, looking up
    /// first and then down; this one when `supported` is empty.
    pub fn clamped(self, supported: &[Self]) -> Self {
        let at = self as usize;
        Self::ALL[at..]
            .iter()
            .chain(Self::ALL[..at].iter().rev())
            .find(|e| supported.contains(e))
            .copied()
            .unwrap_or(self)
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
    pub effort: Option<Effort>,
    pub fast: bool,
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

/// Chooses how hard the living agent `id` thinks, or lets it follow
/// the realm's setting again when `None`.
pub(crate) fn set_effort(
    cx: &mut Change<'_>,
    id: AgentId,
    effort: Option<Effort>,
) -> Done<()> {
    if !get(cx.db, id)?.is_living() {
        refuse!(hires::left(cx.db, id)?);
    }
    diesel::update(agent::table.find(id))
        .set(agent::effort.eq(effort))
        .execute(cx.db)?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Turns the provider's fast tier on or off for the living agent
/// `id`.
pub(crate) fn set_fast(
    cx: &mut Change<'_>,
    id: AgentId,
    fast: bool,
) -> Done<()> {
    if !get(cx.db, id)?.is_living() {
        refuse!(hires::left(cx.db, id)?);
    }
    diesel::update(agent::table.find(id))
        .set(agent::fast.eq(fast))
        .execute(cx.db)?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Chooses the model of the living agent `id`, or lets it follow its
/// hamlet's or the realm's default when `None` (H29). A lead may not
/// change a chief's choice for a hire.
pub(crate) fn set_model(
    cx: &mut Change<'_>,
    id: AgentId,
    model: Option<&str>,
    reason: &str,
) -> Done<()> {
    let found = get(cx.db, id)?;
    match found.kind {
        AgentKind::Hire => hires::set_model(cx, id, model, reason),
        AgentKind::Hand => {
            diesel::update(hand::table)
                .set(hand::model.eq(model))
                .execute(cx.db)?;
            cx.touched.agents.insert(id);
            Ok(Ok(()))
        }
        AgentKind::Worker => {
            if !found.is_living() {
                refuse!(hires::left(cx.db, id)?);
            }
            diesel::update(worker::table.find(id))
                .set(worker::model.eq(model))
                .execute(cx.db)?;
            cx.touched.agents.insert(id);
            Ok(Ok(()))
        }
    }
}

/// Every hire, living or ended, oldest first.
pub(crate) fn all_hires(db: &mut Db) -> Result<Vec<AgentId>> {
    Ok(agent::table
        .filter(agent::kind.eq(AgentKind::Hire))
        .order(agent::id)
        .select(agent::id)
        .load(db)?)
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
