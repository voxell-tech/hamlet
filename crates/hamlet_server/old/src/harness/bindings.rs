//! Runtimes, the harness of each agent, conversations' bindings and
//! the inputs owed to them, as the realm database keeps them.

use anyhow::{Context, Result};
use diesel::deserialize::FromSqlRow;
use diesel::dsl::{self, case_when, exists, max, sql};
use diesel::expression::AsExpression;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Nullable, SingleValue, Text};
use diesel::sqlite::Sqlite;
use diesel::upsert::excluded;
use hamlet_protocol::runtime::{
    AdmittedStatus, ConfigureTools, Item, Mark, ModelRef,
    SettledStatus,
};

use super::mark::InputMark;
use crate::record::AgentRef;
use crate::schema::{
    harness_agent, harness_binding, harness_input, harness_item,
    harness_runtime,
};
use crate::{RuntimeInstance, Store};

/// The harness an agent's new conversations run in unless it has
/// another.
pub const DEFAULT_HARNESS: &str = "pi";

/// A conversation by the server's own number for it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord,
)]
pub struct BindingId(pub i64);

/// A conversation's tie to the runtime it began in.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub id: BindingId,
    pub agent: AgentRef,
    pub errand: bool,
    pub model: ModelRef,
    pub runtime: RuntimeInstance,
    /// The runtime's id for the conversation, once it made it.
    pub conversation: Option<String>,
    /// The conversation this one copies up to the entry `fork_at`.
    pub fork_of: Option<BindingId>,
    pub fork_at: Option<String>,
}

/// A runtime as the server keeps it.
#[derive(Clone, Debug)]
pub(crate) struct RuntimeRow {
    pub id: RuntimeInstance,
    pub adapter: Option<String>,
    pub version: Option<String>,
    pub begun: bool,
}

/// An input kept for a conversation and not yet admitted.
#[derive(Clone, Debug)]
pub(crate) struct Owed {
    pub request: String,
    pub content: String,
    pub steer: bool,
    pub mark: Option<Mark>,
    /// Written for people alone, with a boundary when `boundary`.
    pub write: bool,
    pub boundary: bool,
}

/// What a conversation is owed: an input or a write, kept until the
/// runtime admits it.
#[derive(Clone, Debug)]
pub struct Input<'a> {
    pub request: &'a str,
    pub content: &'a str,
    pub steer: bool,
    pub mark: Option<&'a InputMark>,
    pub write: bool,
    pub boundary: bool,
}

/// Where an input is in its runtime, as it last told it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = Text)]
pub enum InputStatus {
    Queued,
    Placed,
    Done,
    Unanswered,
}

impl InputStatus {
    /// Whether the input ended, answered or not; it never moves on.
    pub fn is_ended(self) -> bool {
        matches!(self, Self::Done | Self::Unanswered)
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Placed => "placed",
            Self::Done => "done",
            Self::Unanswered => "unanswered",
        }
    }

    fn named(name: &str) -> Option<Self> {
        [Self::Queued, Self::Placed, Self::Done, Self::Unanswered]
            .into_iter()
            .find(|status| status.as_str() == name)
    }
}

crate::store::named_text!(InputStatus);

impl From<AdmittedStatus> for InputStatus {
    fn from(status: AdmittedStatus) -> Self {
        match status {
            AdmittedStatus::Queued => Self::Queued,
            AdmittedStatus::Placed => Self::Placed,
            AdmittedStatus::Done => Self::Done,
            AdmittedStatus::Unanswered => Self::Unanswered,
        }
    }
}

impl From<SettledStatus> for InputStatus {
    fn from(status: SettledStatus) -> Self {
        match status {
            SettledStatus::Done => Self::Done,
            SettledStatus::Unanswered => Self::Unanswered,
        }
    }
}

/// How an input stands, as its runtime last told it.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub submission: Option<String>,
    pub status: Option<InputStatus>,
    pub reason: Option<String>,
    pub detail: Option<serde_json::Value>,
    pub followed: bool,
}

impl Store {
    /// The harness `agent`'s new conversations run in.
    pub fn harness_of(&mut self, agent: &AgentRef) -> Result<String> {
        Ok(harness_agent::table
            .find((&agent.hamlet, i64::try_from(agent.number)?))
            .select(harness_agent::harness)
            .first::<String>(self.connection())
            .optional()?
            .unwrap_or_else(|| DEFAULT_HARNESS.into()))
    }

    /// Sets the harness of `agent`'s later conversations; its
    /// existing ones stay where they began (S17).
    pub fn set_harness(
        &mut self,
        agent: &AgentRef,
        harness: &str,
    ) -> Result<()> {
        diesel::insert_into(harness_agent::table)
            .values((
                harness_agent::hamlet.eq(&agent.hamlet),
                harness_agent::number
                    .eq(i64::try_from(agent.number)?),
                harness_agent::harness.eq(harness),
            ))
            .on_conflict((
                harness_agent::hamlet,
                harness_agent::number,
            ))
            .do_update()
            .set(
                harness_agent::harness
                    .eq(excluded(harness_agent::harness)),
            )
            .execute(self.connection())?;
        Ok(())
    }

    /// The binding `id`.
    pub fn binding(
        &mut self,
        id: BindingId,
    ) -> Result<Option<Binding>> {
        harness_binding::table
            .find(id.0)
            .select(BindingRow::as_select())
            .first(self.connection())
            .optional()?
            .map(BindingRow::binding)
            .transpose()
    }

    /// The binding of the runtime `runtime`'s conversation
    /// `conversation`.
    pub fn bound(
        &mut self,
        runtime: &RuntimeInstance,
        conversation: &str,
    ) -> Result<Option<Binding>> {
        harness_binding::table
            .filter(harness_binding::runtime.eq(runtime.as_str()))
            .filter(harness_binding::conversation.eq(conversation))
            .select(BindingRow::as_select())
            .first(self.connection())
            .optional()?
            .map(BindingRow::binding)
            .transpose()
    }

    pub(crate) fn bindings_of(
        &mut self,
        runtime: &RuntimeInstance,
    ) -> Result<Vec<Binding>> {
        harness_binding::table
            .filter(harness_binding::runtime.eq(runtime.as_str()))
            .order(harness_binding::id)
            .select(BindingRow::as_select())
            .load(self.connection())?
            .into_iter()
            .map(BindingRow::binding)
            .collect()
    }

    /// The runtime `id`, as the server keeps it.
    pub(crate) fn runtime(
        &mut self,
        id: &RuntimeInstance,
    ) -> Result<RuntimeRow> {
        self.runtimes()?
            .into_iter()
            .find(|row| row.id == *id)
            .context("a runtime the store does not keep")
    }

    pub(crate) fn runtimes(&mut self) -> Result<Vec<RuntimeRow>> {
        harness_runtime::table
            .order(harness_runtime::id)
            .select((
                harness_runtime::id,
                harness_runtime::adapter,
                harness_runtime::version,
                harness_runtime::begun,
            ))
            .load::<(String, Option<String>, Option<String>, bool)>(
                self.connection(),
            )?
            .into_iter()
            .map(|(id, adapter, version, begun)| {
                Ok(RuntimeRow {
                    id: RuntimeInstance::new(id)?,
                    adapter,
                    version,
                    begun,
                })
            })
            .collect()
    }

    /// The runtime new conversations of `harness` go to, made when
    /// there is none.
    pub(crate) fn runtime_for(
        &mut self,
        harness: &str,
    ) -> Result<RuntimeInstance> {
        let kept = harness_runtime::table
            .filter(harness_runtime::harness.eq(harness))
            .order(harness_runtime::id)
            .select(harness_runtime::id)
            .first::<String>(self.connection())
            .optional()?;
        if let Some(id) = kept {
            return RuntimeInstance::new(id);
        }
        let id = RuntimeInstance::new(format!(
            "{harness}-{}",
            uuid::Uuid::new_v4().simple()
        ))?;
        diesel::insert_into(harness_runtime::table)
            .values((
                harness_runtime::id.eq(id.as_str()),
                harness_runtime::harness.eq(harness),
            ))
            .execute(self.connection())?;
        Ok(id)
    }

    /// Keeps that `runtime` made its store and reported these
    /// versions.
    pub(crate) fn runtime_began(
        &mut self,
        runtime: &RuntimeInstance,
        adapter: &str,
        version: &str,
    ) -> Result<()> {
        diesel::update(harness_runtime::table.find(runtime.as_str()))
            .set((
                harness_runtime::adapter.eq(adapter),
                harness_runtime::version.eq(version),
                harness_runtime::begun.eq(true),
            ))
            .execute(self.connection())?;
        Ok(())
    }

    pub(crate) fn add_binding(
        &mut self,
        agent: &AgentRef,
        errand: bool,
        model: &ModelRef,
        runtime: &RuntimeInstance,
    ) -> Result<BindingId> {
        let id = diesel::insert_into(harness_binding::table)
            .values((
                harness_binding::hamlet.eq(&agent.hamlet),
                harness_binding::number
                    .eq(i64::try_from(agent.number)?),
                harness_binding::errand.eq(errand),
                harness_binding::model
                    .eq(serde_json::to_string(model)?),
                harness_binding::runtime.eq(runtime.as_str()),
            ))
            .returning(harness_binding::id)
            .get_result(self.connection())?;
        Ok(BindingId(id))
    }

    /// Binds a copy of `parent` up to its entry `at`, in its runtime,
    /// with the items it kept up to there.
    pub(crate) fn add_fork(
        &mut self,
        parent: &Binding,
        at: &str,
    ) -> Result<BindingId> {
        let ordinal = ordinal_of(at)?;
        let mut tx = self.transaction()?;
        let fork = diesel::insert_into(harness_binding::table)
            .values(
                harness_binding::table
                    .filter(harness_binding::id.eq(parent.id.0))
                    .select((
                        harness_binding::hamlet,
                        harness_binding::number,
                        harness_binding::errand,
                        harness_binding::model,
                        harness_binding::runtime,
                        harness_binding::id.nullable(),
                        at.into_sql::<Nullable<Text>>(),
                    )),
            )
            .into_columns((
                harness_binding::hamlet,
                harness_binding::number,
                harness_binding::errand,
                harness_binding::model,
                harness_binding::runtime,
                harness_binding::fork_of,
                harness_binding::fork_at,
            ))
            .returning(harness_binding::id)
            .get_result::<i64>(&mut *tx)?;
        diesel::insert_into(harness_item::table)
            .values(
                harness_item::table
                    .filter(harness_item::binding.eq(parent.id.0))
                    .filter(harness_item::ordinal.le(ordinal))
                    .select((
                        fork.into_sql::<BigInt>(),
                        harness_item::entry,
                        harness_item::ordinal,
                        harness_item::item,
                    )),
            )
            .into_columns((
                harness_item::binding,
                harness_item::entry,
                harness_item::ordinal,
                harness_item::item,
            ))
            .execute(&mut *tx)?;
        tx.commit()?;
        Ok(BindingId(fork))
    }

    /// Keeps the runtime's id for the conversation of `binding` and
    /// the versions it began with.
    pub(crate) fn conversation_made(
        &mut self,
        binding: BindingId,
        conversation: &str,
        adapter: &str,
        version: &str,
    ) -> Result<()> {
        diesel::update(
            harness_binding::table
                .filter(harness_binding::id.eq(binding.0))
                .filter(harness_binding::conversation.is_null()),
        )
        .set((
            harness_binding::conversation.eq(conversation),
            harness_binding::adapter.eq(adapter),
            harness_binding::version.eq(version),
        ))
        .execute(self.connection())?;
        Ok(())
    }

    /// Keeps the tools `binding` offers; returns whether they
    /// changed.
    pub(crate) fn set_tools(
        &mut self,
        binding: BindingId,
        tools: &ConfigureTools,
    ) -> Result<bool> {
        let tools = serde_json::to_string(tools)?;
        Ok(diesel::update(
            harness_binding::table
                .filter(harness_binding::id.eq(binding.0))
                .filter(harness_binding::tools.is_not(&tools)),
        )
        .set(harness_binding::tools.eq(&tools))
        .execute(self.connection())?
            == 1)
    }

    /// The tools `binding` offers, when its runtime has not been told
    /// them.
    pub(crate) fn tools_owed(
        &mut self,
        binding: BindingId,
    ) -> Result<Option<ConfigureTools>> {
        harness_binding::table
            .filter(harness_binding::id.eq(binding.0))
            .filter(harness_binding::tools.is_not_null())
            .filter(
                harness_binding::tools
                    .is_not(harness_binding::tools_told),
            )
            .select(harness_binding::tools)
            .first::<Option<String>>(self.connection())
            .optional()?
            .flatten()
            .map(|tools| Ok(serde_json::from_str(&tools)?))
            .transpose()
    }

    /// Keeps that the runtime of `binding` was told `tools`.
    pub(crate) fn tools_told(
        &mut self,
        binding: BindingId,
        tools: &str,
    ) -> Result<()> {
        diesel::update(harness_binding::table.find(binding.0))
            .set(harness_binding::tools_told.eq(tools))
            .execute(self.connection())?;
        Ok(())
    }

    /// Keeps an input for `binding` before it is submitted, once per
    /// request id; returns whether it is new.
    pub(crate) fn keep_input(
        &mut self,
        binding: BindingId,
        input: &Input<'_>,
    ) -> Result<bool> {
        let mark =
            input.mark.map(serde_json::to_string).transpose()?;
        let kept = diesel::insert_into(harness_input::table)
            .values(
                harness_input::table
                    .filter(harness_input::binding.eq(binding.0))
                    .select((
                        binding.0.into_sql::<BigInt>(),
                        input.request.into_sql::<Text>(),
                        ifnull(
                            max(harness_input::ordinal + 1),
                            Some(1).into_sql::<Nullable<BigInt>>(),
                        )
                        .assume_not_null(),
                        input.content.into_sql::<Text>(),
                        input.steer.into_sql::<Bool>(),
                        "kept".into_sql::<Text>(),
                        mark.into_sql::<Nullable<Text>>(),
                        input.write.into_sql::<Bool>(),
                        input.boundary.into_sql::<Bool>(),
                    )),
            )
            .into_columns((
                harness_input::binding,
                harness_input::request,
                harness_input::ordinal,
                harness_input::content,
                harness_input::steer,
                harness_input::state,
                harness_input::mark,
                harness_input::write,
                harness_input::boundary,
            ))
            .on_conflict((
                harness_input::binding,
                harness_input::request,
            ))
            .do_nothing()
            .execute(self.connection())?;
        Ok(kept == 1)
    }

    /// The inputs owed to `binding`, in the order they were kept:
    /// those not yet admitted, then, when `unsettled`, those
    /// admitted whose end the server has not heard.
    pub(crate) fn owed(
        &mut self,
        binding: BindingId,
        unsettled: bool,
    ) -> Result<Vec<Owed>> {
        harness_input::table
            .filter(harness_input::binding.eq(binding.0))
            .filter(
                harness_input::state.eq("kept").or(unsettled
                    .into_sql::<Bool>()
                    .and(harness_input::state.eq("admitted"))
                    .and(harness_input::followed.eq(false))
                    .and(unended())),
            )
            .order(harness_input::ordinal)
            .select((
                harness_input::request,
                harness_input::content,
                harness_input::steer,
                harness_input::mark,
                harness_input::write,
                harness_input::boundary,
            ))
            .load::<(String, String, bool, Option<String>, bool, bool)>(
                self.connection(),
            )?
            .into_iter()
            .map(|(request, content, steer, mark, write, boundary)| {
                Ok(Owed {
                    request,
                    content,
                    steer,
                    mark: mark
                        .map(|m| serde_json::from_str(&m))
                        .transpose()?,
                    write,
                    boundary,
                })
            })
            .collect()
    }

    /// How the input `request` of `binding` stands.
    pub fn standing(
        &mut self,
        binding: BindingId,
        request: &str,
    ) -> Result<Option<Standing>> {
        harness_input::table
            .find((binding.0, request))
            .select((
                harness_input::submission,
                harness_input::status,
                harness_input::reason,
                harness_input::detail,
                harness_input::followed,
            ))
            .first::<(
                Option<String>,
                Option<InputStatus>,
                Option<String>,
                Option<String>,
                bool,
            )>(self.connection())
            .optional()?
            .map(|(submission, status, reason, detail, followed)| {
                Ok(Standing {
                    submission,
                    status,
                    reason,
                    detail: detail
                        .map(|d| serde_json::from_str(&d))
                        .transpose()?,
                    followed,
                })
            })
            .transpose()
    }

    /// The request whose input the runtime admitted as `submission`.
    pub fn request_of(
        &mut self,
        binding: BindingId,
        submission: &str,
    ) -> Result<Option<String>> {
        Ok(harness_input::table
            .filter(harness_input::binding.eq(binding.0))
            .filter(harness_input::submission.eq(submission))
            .select(harness_input::request)
            .first(self.connection())
            .optional()?)
    }

    /// The mark of the input `request` of `binding`.
    pub(crate) fn mark_of(
        &mut self,
        binding: BindingId,
        request: &str,
    ) -> Result<Option<InputMark>> {
        harness_input::table
            .find((binding.0, request))
            .select(harness_input::mark)
            .first::<Option<String>>(self.connection())
            .optional()?
            .flatten()
            .map(|mark| {
                serde_json::from_str(&mark)
                    .context("reading an input's kept mark")
            })
            .transpose()
    }

    /// The mark of the input the runtime admitted as `submission`.
    pub(crate) fn input_mark(
        &mut self,
        binding: BindingId,
        submission: Option<&str>,
    ) -> Result<Option<InputMark>> {
        let Some(submission) = submission else {
            return Ok(None);
        };
        match self.request_of(binding, submission)? {
            Some(request) => self.mark_of(binding, &request),
            None => Ok(None),
        }
    }

    /// The chief's lines `binding` holds queued behind its turn,
    /// whose input is `turn`.
    pub(crate) fn queued_lines(
        &mut self,
        binding: BindingId,
        turn: Option<&str>,
    ) -> Result<usize> {
        let marks = harness_input::table
            .filter(harness_input::binding.eq(binding.0))
            .filter(harness_input::write.eq(false))
            .filter(
                harness_input::state.eq("kept").or(
                    harness_input::state
                        .eq("admitted")
                        .and(
                            harness_input::status
                                .eq(InputStatus::Queued),
                        )
                        .and(harness_input::submission.is_not(turn)),
                ),
            )
            .select(harness_input::mark)
            .load::<Option<String>>(self.connection())?;
        let mut lines = 0;
        for mark in marks.into_iter().flatten() {
            if let InputMark::Chief { .. } =
                serde_json::from_str(&mark)
                    .context("reading an input's kept mark")?
            {
                lines += 1;
            }
        }
        Ok(lines)
    }

    /// Whether the input `request` of `binding` placed an entry.
    pub(crate) fn placed(
        &mut self,
        binding: BindingId,
        request: &str,
    ) -> Result<bool> {
        Ok(diesel::select(exists(
            harness_item::table
                .filter(harness_item::binding.eq(binding.0))
                .filter(
                    sql::<Nullable<Text>>(
                        "json_extract(harness_item.item, \
                         '$.request')",
                    )
                    .eq(request),
                ),
        ))
        .get_result(self.connection())?)
    }

    /// Keeps how the input `request` of `binding` stands now.
    pub(crate) fn input_stands(
        &mut self,
        binding: BindingId,
        request: &str,
        submission: Option<&str>,
        status: InputStatus,
        reason: Option<&str>,
        detail: Option<&serde_json::Value>,
    ) -> Result<()> {
        let detail = detail.map(serde_json::to_string).transpose()?;
        let state = case_when::<_, _, Text>(
            (status == InputStatus::Unanswered).into_sql::<Bool>(),
            "dropped",
        )
        .when(harness_input::state.eq("kept"), "admitted")
        .otherwise(harness_input::state);
        // A status never goes back once the input ended.
        diesel::update(
            harness_input::table
                .filter(harness_input::binding.eq(binding.0))
                .filter(harness_input::request.eq(request))
                .filter(unended()),
        )
        .set((
            harness_input::submission.eq(ifnull(
                submission.into_sql::<Nullable<Text>>(),
                harness_input::submission,
            )),
            harness_input::status.eq(status),
            harness_input::reason.eq(reason),
            harness_input::detail.eq(detail),
            harness_input::state.eq(state),
        ))
        .execute(self.connection())?;
        Ok(())
    }

    /// Keeps that the server acted on the end of the input `request`;
    /// returns whether it had not yet.
    pub fn follow(
        &mut self,
        binding: BindingId,
        request: &str,
    ) -> Result<bool> {
        Ok(diesel::update(
            harness_input::table
                .filter(harness_input::binding.eq(binding.0))
                .filter(harness_input::request.eq(request))
                .filter(harness_input::followed.eq(false)),
        )
        .set(harness_input::followed.eq(true))
        .execute(self.connection())?
            == 1)
    }

    /// The inputs whose end the server has heard and not acted on.
    pub fn unfollowed(&mut self) -> Result<Vec<(BindingId, String)>> {
        Ok(harness_input::table
            .filter(harness_input::followed.eq(false))
            .filter(harness_input::status.eq_any(ended()))
            .order((harness_input::binding, harness_input::ordinal))
            .select((harness_input::binding, harness_input::request))
            .load::<(i64, String)>(self.connection())?
            .into_iter()
            .map(|(binding, request)| (BindingId(binding), request))
            .collect())
    }

    /// Keeps `item` of `binding`; returns whether it is new.
    pub(crate) fn keep_item(
        &mut self,
        binding: BindingId,
        item: &Item,
    ) -> Result<bool> {
        Ok(diesel::insert_into(harness_item::table)
            .values((
                harness_item::binding.eq(binding.0),
                harness_item::entry.eq(&item.entry),
                harness_item::ordinal.eq(ordinal_of(&item.entry)?),
                harness_item::item.eq(serde_json::to_string(item)?),
            ))
            .on_conflict_do_nothing()
            .execute(self.connection())?
            == 1)
    }

    /// The items `binding` kept, oldest first.
    pub fn items(&mut self, binding: BindingId) -> Result<Vec<Item>> {
        items(self.connection(), binding)
    }

    /// Whether `binding` kept the entry `entry`.
    pub(crate) fn has_item(
        &mut self,
        binding: BindingId,
        entry: &str,
    ) -> Result<bool> {
        Ok(diesel::select(exists(
            harness_item::table.find((binding.0, entry)),
        ))
        .get_result(self.connection())?)
    }

    /// Begins cancelling `binding`, in one commit: its owed inputs
    /// are dropped, so no restart submits them, its calls not carried
    /// out are refused, and the cancel is owed to its runtime until
    /// it confirms it.
    pub(crate) fn begin_cancel(
        &mut self,
        binding: BindingId,
    ) -> Result<()> {
        let mut tx = self.transaction()?;
        diesel::update(
            harness_input::table
                .filter(harness_input::binding.eq(binding.0))
                .filter(harness_input::state.eq("kept")),
        )
        .set(harness_input::state.eq("dropped"))
        .execute(&mut *tx)?;
        diesel::update(harness_binding::table.find(binding.0))
            .set(harness_binding::cancelling.eq(true))
            .execute(&mut *tx)?;
        // A call of the stopped turn the gate allowed, asked for
        // again after the stop or after a restart, runs
        // nothing (H34); a marked result stands.
        crate::oversight::held::block_unrun(
            &mut tx,
            binding,
            super::answers::STOPPED_WORDS,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Whether a cancel of `binding` is still owed to its runtime.
    pub(crate) fn cancelling(
        &mut self,
        binding: BindingId,
    ) -> Result<bool> {
        Ok(harness_binding::table
            .find(binding.0)
            .select(harness_binding::cancelling)
            .first(self.connection())?)
    }

    /// Keeps that the runtime confirmed the cancel of `binding`.
    pub(crate) fn cancelled(
        &mut self,
        binding: BindingId,
    ) -> Result<()> {
        diesel::update(harness_binding::table.find(binding.0))
            .set(harness_binding::cancelling.eq(false))
            .execute(self.connection())?;
        Ok(())
    }
}

/// Whether the input admitted as `submission` of `binding` may still
/// run: its binding is not cancelling, and it was neither dropped nor
/// ended.
pub(crate) fn input_runs(
    db: &mut SqliteConnection,
    binding: BindingId,
    submission: &str,
) -> Result<bool> {
    Ok(diesel::select(exists(
        harness_input::table
            .inner_join(harness_binding::table)
            .filter(harness_input::binding.eq(binding.0))
            .filter(harness_input::submission.eq(submission))
            .filter(harness_binding::cancelling.eq(false))
            .filter(harness_input::state.ne("dropped"))
            .filter(unended()),
    ))
    .get_result(db)?)
}

/// The conversations, errands aside, whose inputs of the task `task`
/// were not dropped and have not ended.
pub(crate) fn working_on(
    db: &mut SqliteConnection,
    task: &str,
) -> Result<Vec<BindingId>> {
    Ok(harness_input::table
        .inner_join(harness_binding::table)
        .filter(harness_binding::errand.eq(false))
        .filter(harness_input::state.ne("dropped"))
        .filter(unended())
        .filter(
            sql::<Nullable<Text>>(
                "json_extract(harness_input.mark, '$.data.task')",
            )
            .eq(task),
        )
        .select(harness_input::binding)
        .distinct()
        .load::<i64>(db)?
        .into_iter()
        .map(BindingId)
        .collect())
}

/// The conversation `binding` was forked from, if any.
pub(crate) fn fork_of(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Option<BindingId>> {
    Ok(harness_binding::table
        .find(binding.0)
        .select(harness_binding::fork_of)
        .first::<Option<i64>>(db)
        .optional()?
        .flatten()
        .map(BindingId))
}

/// The items `binding` kept, oldest first.
pub(crate) fn items(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Vec<Item>> {
    harness_item::table
        .filter(harness_item::binding.eq(binding.0))
        .order(harness_item::ordinal)
        .select(harness_item::item)
        .load::<String>(db)?
        .into_iter()
        .map(|item| Ok(serde_json::from_str(&item)?))
        .collect()
}

/// The statuses of an input that ended.
pub(crate) fn ended() -> Vec<InputStatus> {
    [
        InputStatus::Queued,
        InputStatus::Placed,
        InputStatus::Done,
        InputStatus::Unanswered,
    ]
    .into_iter()
    .filter(|status| status.is_ended())
    .collect()
}

/// Holds when an input's status has not ended, or it has none yet.
#[dsl::auto_type]
pub(crate) fn unended() -> _ {
    let statuses: Vec<InputStatus> = ended();
    harness_input::status
        .is_null()
        .or(harness_input::status.ne_all(statuses))
}

#[declare_sql_function]
extern "SQL" {
    /// `value` unless it is NULL, else `otherwise`.
    fn ifnull<T: SingleValue>(value: T, otherwise: T) -> T;
}

/// The order of the entry `entry`: a runtime's entry ids are numbers
/// that grow.
fn ordinal_of(entry: &str) -> Result<i64> {
    entry
        .parse::<i64>()
        .with_context(|| format!("the entry id {entry} is no number"))
}

/// A binding's row in `harness_binding`.
#[derive(Queryable, Selectable)]
#[diesel(table_name = harness_binding, check_for_backend(Sqlite))]
struct BindingRow {
    id: i64,
    hamlet: String,
    number: i64,
    errand: bool,
    model: String,
    runtime: String,
    conversation: Option<String>,
    fork_of: Option<i64>,
    fork_at: Option<String>,
}

impl BindingRow {
    fn binding(self) -> Result<Binding> {
        Ok(Binding {
            id: BindingId(self.id),
            agent: AgentRef {
                hamlet: self.hamlet,
                number: u64::try_from(self.number)?,
            },
            errand: self.errand,
            model: serde_json::from_str(&self.model)
                .context("reading a binding's model")?,
            runtime: RuntimeInstance::new(self.runtime)?,
            conversation: self.conversation,
            fork_of: self.fork_of.map(BindingId),
            fork_at: self.fork_at,
        })
    }
}
