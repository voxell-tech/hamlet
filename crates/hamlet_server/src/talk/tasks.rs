//! Tasks (H12): what a chief's line, a retry, a reflection, an
//! errand, a dream or a worker starts, followed by every message and
//! turn it causes.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, HamletId, TaskId};
use hamlet_store::schema::task;

use crate::change::Change;
use crate::society::agents::{self, AgentKind};
use crate::society::{hires, workers};
use crate::text::text_enum;

text_enum! {
    pub(crate) enum Origin {
        Chief = "chief",
        Retry = "retry",
        Reflection = "reflection",
        Errand = "errand",
        Dream = "dream",
        Worker = "worker",
        Notice = "notice",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = task)]
pub(crate) struct Task {
    pub id: TaskId,
    pub origin: Origin,
    pub agent: AgentId,
    /// The chief overseeing it, who answers what it asks (H10).
    pub chief: Option<ChiefId>,
    /// The hamlet that pays for it; the realm when `None`.
    pub payer: Option<HamletId>,
    pub at: i64,
    /// A chief's instruction, as typed.
    pub text: Option<String>,
    pub messages: i64,
    pub done_at: Option<i64>,
}

/// Starts a task of `origin` at `agent`, paid for where the agent
/// works.
pub(crate) fn mint(
    cx: &mut Change<'_>,
    origin: Origin,
    agent: AgentId,
    chief: Option<ChiefId>,
    text: Option<&str>,
) -> Result<TaskId> {
    let payer = payer_of(cx.db, agent)?;
    Ok(diesel::insert_into(task::table)
        .values((
            task::origin.eq(origin),
            task::agent.eq(agent),
            task::chief.eq(chief),
            task::payer.eq(&payer),
            task::at.eq(cx.at),
            task::text.eq(text),
        ))
        .returning(task::id)
        .get_result(cx.db)?)
}

/// The hamlet that pays for what `agent` does on its own account: a
/// hire's hamlet, a worker's starter's, and the realm for the Hand.
pub(crate) fn payer_of(
    db: &mut Db,
    agent: AgentId,
) -> Result<Option<HamletId>> {
    let found = agents::get(db, agent)?;
    Ok(match found.kind {
        AgentKind::Hand => None,
        AgentKind::Hire => Some(hires::get(db, agent)?.hamlet),
        AgentKind::Worker => workers::get(db, agent)?.hamlet,
    })
}

pub(crate) fn get(db: &mut Db, id: TaskId) -> Result<Task> {
    task::table
        .find(id)
        .select(Task::as_select())
        .first(db)
        .optional()?
        .with_context(|| format!("no task {id}"))
}

/// Counts a message between agents against the task, unless it has
/// sent `most` already. Returns whether it was counted.
pub(crate) fn count_message(
    db: &mut Db,
    id: TaskId,
    most: i64,
) -> Result<bool> {
    let changed = diesel::update(
        task::table.find(id).filter(task::messages.lt(most)),
    )
    .set(task::messages.eq(task::messages + 1))
    .execute(db)?;
    Ok(changed == 1)
}

/// Marks the chief's instruction `id` answered.
pub(crate) fn done(db: &mut Db, id: TaskId, at: i64) -> Result<()> {
    diesel::update(
        task::table.find(id).filter(task::done_at.is_null()),
    )
    .set(task::done_at.eq(at))
    .execute(db)?;
    Ok(())
}

/// The latest instructions chiefs gave `agent`, newest first.
pub(crate) fn instructions(
    db: &mut Db,
    agent: AgentId,
    most: i64,
) -> Result<Vec<Task>> {
    Ok(task::table
        .filter(task::agent.eq(agent))
        .filter(task::origin.eq(Origin::Chief))
        .order(task::id.desc())
        .limit(most)
        .select(Task::as_select())
        .load(db)?)
}
