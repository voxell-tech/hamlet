//! Conversations, each bound for good to the runtime it began in
//! (S17).

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::runtime::{
    ConfigureTools, ConversationKind, ModelRef,
};
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ChiefId, ConversationId};
use hamlet_store::schema::{conversation, item};

use crate::change::Change;
use crate::text::text_enum;

text_enum! {
    pub(crate) enum Kind {
        Chat = "chat",
        Letters = "letters",
        Questions = "questions",
        Worker = "worker",
        Errand = "errand",
        Dream = "dream",
    }
}

impl Kind {
    pub fn wire(self) -> ConversationKind {
        match self {
            Self::Chat => ConversationKind::Chat,
            Self::Letters => ConversationKind::Letters,
            Self::Questions => ConversationKind::Questions,
            Self::Worker => ConversationKind::Worker,
            Self::Errand => ConversationKind::Errand,
            Self::Dream => ConversationKind::Dream,
        }
    }
}

text_enum! {
    /// How a conversation's latest turn ended.
    pub(crate) enum Last {
        Done = "done",
        Failed = "failed",
        Stopped = "stopped",
    }
}

#[derive(Clone, Debug, PartialEq, Queryable, Selectable)]
#[diesel(table_name = conversation)]
pub(crate) struct Conversation {
    pub id: ConversationId,
    pub agent: AgentId,
    pub kind: Kind,
    pub chief: Option<ChiefId>,
    pub session: Option<i64>,
    pub peer: Option<AgentId>,
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
    pub last: Option<Last>,
    pub unreflected: i64,
    pub retiring: bool,
    pub entered: Option<String>,
    pub last_input: Option<String>,
    pub budget_input: Option<String>,
    pub budget_words: Option<String>,
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
    pub chief: Option<ChiefId>,
    pub session: Option<i64>,
    pub peer: Option<AgentId>,
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
            chief: None,
            session: None,
            peer: None,
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
            conversation::chief.eq(begin.chief),
            conversation::session.eq(begin.session),
            conversation::peer.eq(begin.peer),
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
            conversation::chief.eq(of.chief),
            conversation::session.eq(of.session),
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

/// The conversation `agent` has with the sender `peer`, for letters
/// or questions.
pub(crate) fn with_peer(
    db: &mut Db,
    agent: AgentId,
    kind: Kind,
    peer: AgentId,
) -> Result<Option<ConversationId>> {
    Ok(conversation::table
        .filter(conversation::agent.eq(agent))
        .filter(conversation::kind.eq(kind))
        .filter(conversation::peer.eq(peer))
        .select(conversation::id)
        .first(db)
        .optional()?)
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
