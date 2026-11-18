//! Dreams as rows: what dreamt, what it cost, and the commits it
//! made. A hire's dream brings what it learned home to its profile; a
//! profile's dream tidies its `main`.

use anyhow::{Context, Result};
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_store::Db;
use hamlet_store::ids::{
    AgentId, ConversationId, DreamId, ProfileId, TaskId,
};
use hamlet_store::schema::dream;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use crate::realm::change::Change;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum DreamKind {
    Hire,
    Profile,
}
hamlet_store::named_text!(DreamKind);

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    AsExpression,
    FromSqlRow,
    IntoStaticStr,
    EnumString,
    Display,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
#[diesel(sql_type = Text)]
pub(crate) enum Outcome {
    Done,
    /// It changed nothing.
    Nothing,
    Failed,
    Stopped,
}
hamlet_store::named_text!(Outcome);

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = dream)]
pub(crate) struct Dream {
    pub id: DreamId,
    pub kind: DreamKind,
    pub profile: ProfileId,
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

/// The latest dream of `profile` itself.
pub(crate) fn last_of_profile(
    db: &mut Db,
    profile: ProfileId,
) -> Result<Option<Dream>> {
    Ok(dream::table
        .filter(dream::profile.eq(profile))
        .filter(dream::kind.eq(DreamKind::Profile))
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
    profile: ProfileId,
    hire: Option<AgentId>,
    ending: bool,
) -> Result<DreamId> {
    Ok(diesel::insert_into(dream::table)
        .values((
            dream::kind.eq(kind),
            dream::profile.eq(profile),
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
