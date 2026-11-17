//! Conversations, their inputs and turns, the prompts they are given,
//! and what follows an input's end. Each conversation is bound for
//! good to the runtime it began in (S17).

pub(crate) mod activity;
pub(crate) mod briefs;
pub(crate) mod chats;
pub(crate) mod completion;
pub(crate) mod dreams;
pub(crate) mod errands;
pub(crate) mod failures;
pub(crate) mod imports;
pub(crate) mod inputs;
pub(crate) mod items;
pub(crate) mod marks;
pub(crate) mod messages;
pub(crate) mod models;
pub(crate) mod prompt;
pub(crate) mod questions;
pub(crate) mod reflections;
pub(crate) mod tasks;
pub(crate) mod turns;
pub(crate) mod verdicts;

use anyhow::{Context, Result};
use diesel::deserialize::FromSqlRow;
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::Text;
use hamlet_protocol::runtime::{
    ConfigureTools, ConversationKind, ModelRef,
};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId};
use hamlet_store::schema::{conversation, item};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use crate::realm::change::Change;

/// What a queued item's id begins with, before its input's request.
pub(crate) const QUEUED: &str = "queued:";

pub(crate) fn plugin(app: &mut bevy_app::App) {
    app.add_plugins(dreams::plugin);
}

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
pub(crate) enum Kind {
    Chat,
    Worker,
    Errand,
    Dream,
}
hamlet_store::named_text!(Kind);

impl Kind {
    pub fn wire(self) -> ConversationKind {
        match self {
            Self::Chat => ConversationKind::Chat,
            Self::Worker => ConversationKind::Worker,
            Self::Errand => ConversationKind::Errand,
            Self::Dream => ConversationKind::Dream,
        }
    }
}

/// How a conversation's latest turn ended.
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
pub(crate) enum Last {
    Done,
    Failed,
    Stopped,
}
hamlet_store::named_text!(Last);

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = conversation)]
pub(crate) struct Conversation {
    pub id: ConversationId,
    pub agent: AgentId,
    pub kind: Kind,
    pub errand: Option<String>,
    pub owner: Option<String>,
    pub instructions: Option<String>,
    pub runtime: String,
    pub handle: Option<String>,
    pub adapter: Option<String>,
    pub version: Option<String>,
    pub model: String,
    pub fork_of: Option<ConversationId>,
    pub fork_at: Option<String>,
    pub previous: Option<ConversationId>,
    pub tools: Option<String>,
    pub tools_told: Option<String>,
    pub cancelling: bool,
    /// A chief stopped its running turn, whose end is then no
    /// failure.
    pub stopped: bool,
    /// A chief stopped it, and no queued turn starts until a line of
    /// theirs or a resume.
    pub held: bool,
    /// The chief's input that ended a hold, which runs before the
    /// rest queued.
    pub runs_first: Option<String>,
    pub last: Option<Last>,
    pub unreflected: i64,
    pub retiring: bool,
    pub entered: Option<String>,
    pub last_input: Option<String>,
    pub made: i64,
}

impl Conversation {
    pub fn model(&self) -> Result<ModelRef> {
        Ok(serde_json::from_str(&self.model)?)
    }

    /// The tools it offers, when narrowed.
    pub fn tools(&self) -> Result<Option<ConfigureTools>> {
        self.tools
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(Into::into)
    }
}

/// A conversation to begin.
#[derive(Clone, Debug)]
pub(crate) struct Begin {
    pub agent: AgentId,
    pub kind: Kind,
    /// An errand's kind, the key it is made once under, and its
    /// instructions.
    pub errand: Option<(String, String, String)>,
    /// The whole prompt of a conversation that is told its own, such
    /// as a dream's.
    pub instructions: Option<String>,
}

impl Begin {
    pub fn new(agent: AgentId, kind: Kind) -> Self {
        Self {
            agent,
            kind,
            errand: None,
            instructions: None,
        }
    }
}

/// Begins a conversation in `runtime` with `model`. The runtime makes
/// its own id for it when it is told.
pub(crate) fn begin(
    cx: &mut Change<'_>,
    begin: Begin,
    runtime: &str,
    model: &ModelRef,
    tools: Option<&ConfigureTools>,
) -> Result<ConversationId> {
    let (errand, owner, instructions) = match begin.errand {
        Some((errand, owner, instructions)) => {
            (Some(errand), Some(owner), Some(instructions))
        }
        None => (None, None, begin.instructions),
    };
    let id = diesel::insert_into(conversation::table)
        .values((
            conversation::agent.eq(begin.agent),
            conversation::kind.eq(begin.kind),
            conversation::errand.eq(errand),
            conversation::owner.eq(owner),
            conversation::instructions.eq(instructions),
            conversation::runtime.eq(runtime),
            conversation::model.eq(serde_json::to_string(model)?),
            conversation::tools
                .eq(tools.map(serde_json::to_string).transpose()?),
            conversation::made.eq(cx.at),
        ))
        .returning(conversation::id)
        .get_result(cx.db)?;
    cx.touched.agents.insert(begin.agent);
    Ok(id)
}

/// A copy of `of` that the runtime forks at its entry `at`.
pub(crate) fn fork(
    cx: &mut Change<'_>,
    of: &Conversation,
    at: &str,
) -> Result<ConversationId> {
    let id = diesel::insert_into(conversation::table)
        .values((
            conversation::agent.eq(of.agent),
            conversation::kind.eq(of.kind),
            conversation::runtime.eq(&of.runtime),
            conversation::model.eq(&of.model),
            conversation::fork_of.eq(of.id),
            conversation::fork_at.eq(at),
            conversation::previous.eq(of.id),
            conversation::tools.eq(&of.tools),
            conversation::made.eq(cx.at),
        ))
        .returning(conversation::id)
        .get_result(cx.db)?;
    // The fork's transcript is its parent's up to `at` from the
    // start, before its runtime keeps anything of it.
    let held = item::table
        .filter(item::conversation.eq(of.id))
        .order(item::ordinal)
        .select((item::entry, item::ordinal, item::body))
        .load::<(String, i64, String)>(cx.db)?;
    if let Some(end) = held.iter().position(|(entry, ..)| entry == at)
    {
        for (entry, ordinal, body) in &held[..=end] {
            diesel::insert_into(item::table)
                .values((
                    item::conversation.eq(id),
                    item::entry.eq(entry),
                    item::ordinal.eq(ordinal),
                    item::body.eq(body),
                ))
                .execute(cx.db)?;
        }
    }
    Ok(id)
}

pub(crate) fn get(
    db: &mut Db,
    id: ConversationId,
) -> Result<Conversation> {
    conversation::table
        .find(id)
        .select(Conversation::as_select())
        .first(db)
        .optional()?
        .with_context(|| format!("no conversation {id}"))
}

/// The conversation `runtime` knows as `handle`.
pub(crate) fn by_handle(
    db: &mut Db,
    runtime: &str,
    handle: &str,
) -> Result<Option<Conversation>> {
    Ok(conversation::table
        .filter(conversation::runtime.eq(runtime))
        .filter(conversation::handle.eq(handle))
        .select(Conversation::as_select())
        .first(db)
        .optional()?)
}

/// Every conversation bound to `runtime`, oldest first.
pub(crate) fn of_runtime(
    db: &mut Db,
    runtime: &str,
) -> Result<Vec<Conversation>> {
    Ok(conversation::table
        .filter(conversation::runtime.eq(runtime))
        .order(conversation::id)
        .select(Conversation::as_select())
        .load(db)?)
}

/// Every conversation of `agent`, oldest first.
pub(crate) fn of_agent(
    db: &mut Db,
    agent: AgentId,
) -> Result<Vec<Conversation>> {
    Ok(conversation::table
        .filter(conversation::agent.eq(agent))
        .order(conversation::id)
        .select(Conversation::as_select())
        .load(db)?)
}

/// The errand of `kind` made under `owner`.
pub(crate) fn errand(
    db: &mut Db,
    kind: &str,
    owner: &str,
) -> Result<Option<Conversation>> {
    Ok(conversation::table
        .filter(conversation::errand.eq(kind))
        .filter(conversation::owner.eq(owner))
        .select(Conversation::as_select())
        .first(db)
        .optional()?)
}

/// Keeps the runtime's id for `id` and the versions it began with.
pub(crate) fn bound(
    db: &mut Db,
    id: ConversationId,
    handle: &str,
    adapter: &str,
    version: &str,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set((
            conversation::handle.eq(handle),
            conversation::adapter.eq(adapter),
            conversation::version.eq(version),
        ))
        .execute(db)?;
    Ok(())
}

pub(crate) fn set_cancelling(
    db: &mut Db,
    id: ConversationId,
    cancelling: bool,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set(conversation::cancelling.eq(cancelling))
        .execute(db)?;
    Ok(())
}

pub(crate) fn set_stopped(
    db: &mut Db,
    id: ConversationId,
    stopped: bool,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set(conversation::stopped.eq(stopped))
        .execute(db)?;
    Ok(())
}

/// Holds `id`'s queue until a chief's line or a resume.
pub(crate) fn hold(
    cx: &mut Change<'_>,
    id: ConversationId,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set((
            conversation::held.eq(true),
            conversation::runs_first.eq(None::<String>),
        ))
        .execute(cx.db)?;
    cx.touched.queues.insert(id);
    Ok(())
}

/// Ends `id`'s hold, with the chief's input that ends it to run
/// first. Returns whether it was held.
pub(crate) fn release(
    cx: &mut Change<'_>,
    id: ConversationId,
    first: Option<&str>,
) -> Result<bool> {
    let released = diesel::update(
        conversation::table.find(id).filter(conversation::held),
    )
    .set((
        conversation::held.eq(false),
        conversation::runs_first.eq(first),
    ))
    .execute(cx.db)?
        == 1;
    if released {
        cx.touched.queues.insert(id);
    }
    Ok(released)
}

pub(crate) fn set_last(
    db: &mut Db,
    id: ConversationId,
    last: Last,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set(conversation::last.eq(last))
        .execute(db)?;
    Ok(())
}

/// Narrows the tools `id` offers; the runtime is told before its next
/// input.
pub(crate) fn set_tools(
    db: &mut Db,
    id: ConversationId,
    tools: &ConfigureTools,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set(conversation::tools.eq(serde_json::to_string(tools)?))
        .execute(db)?;
    Ok(())
}

/// Keeps that the runtime was told the tools `told`.
pub(crate) fn told_tools(
    db: &mut Db,
    id: ConversationId,
    told: &str,
) -> Result<()> {
    diesel::update(conversation::table.find(id))
        .set(conversation::tools_told.eq(told))
        .execute(db)?;
    Ok(())
}
