//! Dreams as rows: what dreamt, what it cost, and the commits it
//! made. A hire's dream brings what it learned home to its scholar; a
//! scholar's dream tidies its `main`.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_store::Db;
use hamlet_store::ids::{
    AgentId, ConversationId, DreamId, ScholarId, TaskId,
};
use hamlet_store::schema::dream;

use crate::change::Change;
use crate::text::text_enum;

text_enum! {
    pub(crate) enum DreamKind {
        Hire = "hire",
        Scholar = "scholar",
    }
}

text_enum! {
    pub(crate) enum Outcome {
        Done = "done",
        /// It changed nothing.
        Nothing = "nothing",
        Failed = "failed",
        Stopped = "stopped",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = dream)]
pub(crate) struct Dream {
    pub id: DreamId,
    pub kind: DreamKind,
    pub scholar: ScholarId,
    pub hire: Option<AgentId>,
    pub ending: bool,
    pub conversation: Option<ConversationId>,
    pub task: Option<TaskId>,
    pub began: i64,
    pub ended: Option<i64>,
    pub outcome: Option<Outcome>,
    pub main_commit: Option<String>,
    pub branch_commit: Option<String>,
}

pub(crate) fn get(db: &mut Db, id: DreamId) -> Result<Dream> {
    dream::table
        .find(id)
        .select(Dream::as_select())
        .first(db)
        .optional()?
        .with_context(|| format!("no dream {id}"))
}

/// The dream running now, if one is.
pub(crate) fn running(db: &mut Db) -> Result<Option<Dream>> {
    Ok(dream::table
        .filter(dream::ended.is_null())
        .select(Dream::as_select())
        .first(db)
        .optional()?)
}

/// The latest dream of `hire`.
pub(crate) fn last_of_hire(
    db: &mut Db,
    hire: AgentId,
) -> Result<Option<Dream>> {
    Ok(dream::table
        .filter(dream::hire.eq(hire))
        .order(dream::id.desc())
        .select(Dream::as_select())
        .first(db)
        .optional()?)
}

/// The latest dream of `scholar` itself.
pub(crate) fn last_of_scholar(
    db: &mut Db,
    scholar: ScholarId,
) -> Result<Option<Dream>> {
    Ok(dream::table
        .filter(dream::scholar.eq(scholar))
        .filter(dream::kind.eq(DreamKind::Scholar))
        .order(dream::id.desc())
        .select(Dream::as_select())
        .first(db)
        .optional()?)
}

/// The dream whose conversation is `conversation`.
pub(crate) fn of_conversation(
    db: &mut Db,
    conversation: ConversationId,
) -> Result<Option<Dream>> {
    Ok(dream::table
        .filter(dream::conversation.eq(conversation))
        .select(Dream::as_select())
        .first(db)
        .optional()?)
}

/// Keeps a dream begun.
pub(crate) fn begin(
    cx: &mut Change<'_>,
    kind: DreamKind,
    scholar: ScholarId,
    hire: Option<AgentId>,
    ending: bool,
) -> Result<DreamId> {
    Ok(diesel::insert_into(dream::table)
        .values((
            dream::kind.eq(kind),
            dream::scholar.eq(scholar),
            dream::hire.eq(hire),
            dream::ending.eq(ending),
            dream::began.eq(cx.at),
        ))
        .returning(dream::id)
        .get_result(cx.db)?)
}

/// Keeps the conversation and task a dream runs in.
pub(crate) fn runs_in(
    db: &mut Db,
    id: DreamId,
    conversation: ConversationId,
    task: TaskId,
) -> Result<()> {
    diesel::update(dream::table.find(id))
        .set((
            dream::conversation.eq(conversation),
            dream::task.eq(task),
        ))
        .execute(db)?;
    Ok(())
}

/// Keeps how a dream ended and the commits it made.
pub(crate) fn end(
    db: &mut Db,
    id: DreamId,
    outcome: Outcome,
    main: Option<&str>,
    branch: Option<&str>,
    at: i64,
) -> Result<()> {
    diesel::update(
        dream::table.find(id).filter(dream::ended.is_null()),
    )
    .set((
        dream::ended.eq(at),
        dream::outcome.eq(outcome),
        dream::main_commit.eq(main),
        dream::branch_commit.eq(branch),
    ))
    .execute(db)?;
    Ok(())
}
