//! Dreams as rows.

use anyhow::{Context, Result};
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_store::Db;
use hamlet_store::ids::{
    AgentId, ConversationId, DreamId, ProfileId, TaskId,
};
use hamlet_store::schema::{asked_dream, dream};
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
    Agent,
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
    pub agent: Option<AgentId>,
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

/// The latest dream of `agent`.
pub(crate) fn last_of_agent(
    db: &mut Db,
    agent: AgentId,
) -> Result<Option<Dream>> {
    Ok(dream::table
        .filter(dream::agent.eq(agent))
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

/// Who a dream asked for is of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Asked {
    Agent(AgentId),
    Profile(ProfileId),
}

/// Keeps a dream of `asked` asked for, unless one waits already.
pub(crate) fn ask(db: &mut Db, asked: Asked, at: i64) -> Result<()> {
    let (agent, profile) = match asked {
        Asked::Agent(agent) => (Some(agent), None),
        Asked::Profile(profile) => (None, Some(profile)),
    };
    let waiting = asked_dream::table
        .filter(asked_dream::agent.is(agent))
        .filter(asked_dream::profile.is(profile))
        .count()
        .get_result::<i64>(db)?;
    if waiting > 0 {
        return Ok(());
    }
    diesel::insert_into(asked_dream::table)
        .values((
            asked_dream::agent.eq(agent),
            asked_dream::profile.eq(profile),
            asked_dream::at.eq(at),
        ))
        .execute(db)?;
    Ok(())
}

/// The dreams asked for that wait, the oldest first.
pub(crate) fn asked(db: &mut Db) -> Result<Vec<Asked>> {
    let rows = asked_dream::table
        .order(asked_dream::id)
        .select((asked_dream::agent, asked_dream::profile))
        .load::<(Option<AgentId>, Option<ProfileId>)>(db)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match row {
            (Some(agent), _) => Some(Asked::Agent(agent)),
            (None, Some(profile)) => Some(Asked::Profile(profile)),
            (None, None) => None,
        })
        .collect())
}

/// Lets go of the dream of `asked` that waits, if one does.
pub(crate) fn forget(db: &mut Db, asked: Asked) -> Result<()> {
    match asked {
        Asked::Agent(agent) => {
            diesel::delete(
                asked_dream::table
                    .filter(asked_dream::agent.eq(agent)),
            )
            .execute(db)?;
        }
        Asked::Profile(profile) => {
            diesel::delete(
                asked_dream::table
                    .filter(asked_dream::profile.eq(profile)),
            )
            .execute(db)?;
        }
    }
    Ok(())
}

/// Keeps a dream begun, which answers the one of it asked for.
pub(crate) fn begin(
    cx: &mut Change<'_>,
    kind: DreamKind,
    profile: ProfileId,
    agent: Option<AgentId>,
    ending: bool,
) -> Result<DreamId> {
    let asked = match agent {
        Some(agent) => Asked::Agent(agent),
        None => Asked::Profile(profile),
    };
    forget(cx.db, asked)?;
    Ok(diesel::insert_into(dream::table)
        .values((
            dream::kind.eq(kind),
            dream::profile.eq(profile),
            dream::agent.eq(agent),
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
