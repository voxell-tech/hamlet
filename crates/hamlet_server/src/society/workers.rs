//! Workers: short-lived agents anyone starts for one task, each a
//! child of whoever started it.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, HamletId};
use hamlet_store::schema::{agent, worker};

use super::agents::{self, AgentKind, EndHow};
use super::hires;
use super::refusal::Refusal;
use crate::change::{Change, Done, done, refuse};

/// Who starts a worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Starter {
    Agent(AgentId),
    Chief(ChiefId),
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = worker)]
pub(crate) struct Worker {
    pub agent: AgentId,
    pub starter: Option<AgentId>,
    pub chief: Option<ChiefId>,
    pub hamlet: Option<HamletId>,
    pub brief: String,
    pub tools: String,
    pub model: Option<String>,
    pub waits: bool,
    pub call_runtime: Option<String>,
    pub call_key: Option<String>,
    pub answer: Option<String>,
    pub nickname: Option<String>,
}

impl Worker {
    /// The built-in tools it was given.
    pub fn tools(&self) -> Result<Vec<String>> {
        Ok(serde_json::from_str(&self.tools)?)
    }
}

/// A worker to start.
#[derive(Clone, Debug)]
pub(crate) struct Start {
    pub starter: Starter,
    /// The hamlet a chief starts it in; an agent's worker works
    /// where the agent does.
    pub hamlet: Option<HamletId>,
    pub brief: String,
    pub tools: Vec<String>,
    pub model: Option<String>,
    /// Whether the starter waits for its answer.
    pub waits: bool,
    /// The tool call that started it, by its runtime and key.
    pub call: Option<(String, String)>,
}

pub(crate) fn start(
    cx: &mut Change<'_>,
    start: Start,
) -> Done<AgentId> {
    if start.brief.trim().is_empty() {
        refuse!(Refusal::NoBrief);
    }
    let hamlet = match start.starter {
        Starter::Agent(starter) => {
            let found = agents::get(cx.db, starter)?;
            if !found.is_living() {
                refuse!(hires::left(cx.db, starter)?);
            }
            match found.kind {
                AgentKind::Worker => refuse!(Refusal::Worker),
                AgentKind::Hand => None,
                AgentKind::Hire => {
                    Some(hires::get(cx.db, starter)?.hamlet)
                }
            }
        }
        Starter::Chief(_) => start.hamlet,
    };
    let id = agents::make(cx, AgentKind::Worker)?;
    let (starter, chief) = match start.starter {
        Starter::Agent(agent) => (Some(agent), None),
        Starter::Chief(chief) => (None, Some(chief)),
    };
    let (call_runtime, call_key) = start.call.unzip();
    diesel::insert_into(worker::table)
        .values((
            worker::agent.eq(id),
            worker::starter.eq(starter),
            worker::chief.eq(chief),
            worker::hamlet.eq(&hamlet),
            worker::brief.eq(start.brief.trim()),
            worker::tools.eq(serde_json::to_string(&start.tools)?),
            worker::model.eq(&start.model),
            worker::waits.eq(start.waits),
            worker::call_runtime.eq(call_runtime),
            worker::call_key.eq(call_key),
        ))
        .execute(cx.db)?;
    if let Some(starter) = starter {
        cx.touched.agents.insert(starter);
    }
    Ok(Ok(id))
}

pub(crate) fn find(
    db: &mut Db,
    id: AgentId,
) -> Result<Option<Worker>> {
    Ok(worker::table
        .find(id)
        .select(Worker::as_select())
        .first(db)
        .optional()?)
}

pub(crate) fn get(db: &mut Db, id: AgentId) -> Result<Worker> {
    find(db, id)?.with_context(|| format!("no worker {id}"))
}

/// Calls the living worker `id` `nickname` in the world, or by its
/// address again when none.
pub(crate) fn set_nickname(
    cx: &mut Change<'_>,
    id: AgentId,
    nickname: Option<&str>,
) -> Done<()> {
    get(cx.db, id)?;
    done!(hires::require_living(cx.db, id));
    let nickname = match hires::check_nickname(nickname) {
        Ok(nickname) => nickname,
        Err(refusal) => refuse!(refusal),
    };
    diesel::update(worker::table.find(id))
        .set(worker::nickname.eq(nickname))
        .execute(cx.db)?;
    cx.touched.agents.insert(id);
    Ok(Ok(()))
}

/// Ends the worker `id` with its answer, when it gave one.
pub(crate) fn finish(
    cx: &mut Change<'_>,
    id: AgentId,
    how: EndHow,
    answer: Option<&str>,
) -> Result<()> {
    diesel::update(worker::table.find(id))
        .set(worker::answer.eq(answer))
        .execute(cx.db)?;
    agents::end(cx, id, how, "")
}

/// The living workers `starter` started.
pub(crate) fn of_agent(
    db: &mut Db,
    starter: AgentId,
) -> Result<Vec<Worker>> {
    Ok(worker::table
        .inner_join(agent::table)
        .filter(worker::starter.eq(starter))
        .filter(agent::ended_at.is_null())
        .order(worker::agent)
        .select(Worker::as_select())
        .load(db)?)
}
