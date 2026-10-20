//! Runtimes, the harness of each agent, conversations' bindings and
//! the inputs owed to them, as the realm database keeps them.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::link::{ConfigureTools, Item, Mark, ModelRef};
use crate::record::AgentRef;
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
    pub mark: Option<&'a Mark>,
    pub write: bool,
    pub boundary: bool,
}

/// How an input stands, as its runtime last told it.
#[derive(Clone, Debug, PartialEq)]
pub struct Standing {
    pub submission: Option<String>,
    /// `queued`, `placed`, `done` or `unanswered`.
    pub status: Option<String>,
    pub reason: Option<String>,
    pub detail: Option<serde_json::Value>,
    pub followed: bool,
}

impl Store {
    /// The harness `agent`'s new conversations run in.
    pub fn harness_of(&self, agent: &AgentRef) -> Result<String> {
        Ok(self
            .connection()
            .query_row(
                "SELECT harness FROM harness_agent WHERE hamlet = \
                 ?1 AND number = ?2",
                params![agent.hamlet, i64::try_from(agent.number)?],
                |row| row.get::<_, String>(0),
            )
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
        self.connection().execute(
            "INSERT INTO harness_agent (hamlet, number, harness) \
             VALUES (?1, ?2, ?3) ON CONFLICT (hamlet, number) DO \
             UPDATE SET harness = excluded.harness",
            params![
                agent.hamlet,
                i64::try_from(agent.number)?,
                harness
            ],
        )?;
        Ok(())
    }

    /// The binding `id`.
    pub fn binding(&self, id: BindingId) -> Result<Option<Binding>> {
        self.connection()
            .query_row(
                &format!("{BINDING} WHERE id = ?1"),
                [id.0],
                binding_row,
            )
            .optional()?
            .transpose()
    }

    /// The binding of the runtime `runtime`'s conversation
    /// `conversation`.
    pub fn bound(
        &self,
        runtime: &RuntimeInstance,
        conversation: &str,
    ) -> Result<Option<Binding>> {
        self.connection()
            .query_row(
                &format!(
                    "{BINDING} WHERE runtime = ?1 AND conversation \
                     = ?2"
                ),
                params![runtime.as_str(), conversation],
                binding_row,
            )
            .optional()?
            .transpose()
    }

    pub(crate) fn bindings_of(
        &self,
        runtime: &RuntimeInstance,
    ) -> Result<Vec<Binding>> {
        let mut query = self.connection().prepare(&format!(
            "{BINDING} WHERE runtime = ?1 ORDER BY id"
        ))?;
        query
            .query_map([runtime.as_str()], binding_row)?
            .map(|row| row?)
            .collect()
    }

    pub(crate) fn runtimes(&self) -> Result<Vec<RuntimeRow>> {
        let mut query = self.connection().prepare(
            "SELECT id, adapter, version, begun FROM \
             harness_runtime ORDER BY id",
        )?;
        query
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })?
            .map(|row| {
                let (id, adapter, version, begun) = row?;
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
        let kept = self
            .connection()
            .query_row(
                "SELECT id FROM harness_runtime WHERE harness = ?1 \
                 ORDER BY id LIMIT 1",
                [harness],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(id) = kept {
            return RuntimeInstance::new(id);
        }
        let id = RuntimeInstance::new(format!(
            "{harness}-{}",
            uuid::Uuid::new_v4().simple()
        ))?;
        self.connection().execute(
            "INSERT INTO harness_runtime (id, harness) VALUES (?1, \
             ?2)",
            params![id.as_str(), harness],
        )?;
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
        self.connection().execute(
            "UPDATE harness_runtime SET adapter = ?2, version = ?3, \
             begun = 1 WHERE id = ?1",
            params![runtime.as_str(), adapter, version],
        )?;
        Ok(())
    }

    pub(crate) fn add_binding(
        &mut self,
        agent: &AgentRef,
        errand: bool,
        model: &ModelRef,
        runtime: &RuntimeInstance,
    ) -> Result<BindingId> {
        self.connection().execute(
            "INSERT INTO harness_binding (hamlet, number, errand, \
             model, runtime) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                agent.hamlet,
                i64::try_from(agent.number)?,
                errand,
                serde_json::to_string(model)?,
                runtime.as_str()
            ],
        )?;
        Ok(BindingId(self.connection().last_insert_rowid()))
    }

    /// Binds a copy of `parent` up to its entry `at`, in its runtime,
    /// with the items it kept up to there.
    pub(crate) fn add_fork(
        &mut self,
        parent: &Binding,
        at: &str,
    ) -> Result<BindingId> {
        let ordinal = ordinal_of(at)?;
        let tx = self.transaction()?;
        tx.execute(
            "INSERT INTO harness_binding (hamlet, number, errand, \
             model, runtime, fork_of, fork_at) SELECT hamlet, \
             number, errand, model, runtime, id, ?2 FROM \
             harness_binding WHERE id = ?1",
            params![parent.id.0, at],
        )?;
        let fork = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO harness_item (binding, entry, ordinal, \
             item) SELECT ?2, entry, ordinal, item FROM \
             harness_item WHERE binding = ?1 AND ordinal <= ?3",
            params![parent.id.0, fork, ordinal],
        )?;
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
        self.connection().execute(
            "UPDATE harness_binding SET conversation = ?2, adapter \
             = ?3, version = ?4 WHERE id = ?1 AND conversation IS \
             NULL",
            params![binding.0, conversation, adapter, version],
        )?;
        Ok(())
    }

    /// Keeps the tools `binding` offers; returns whether they
    /// changed.
    pub(crate) fn set_tools(
        &mut self,
        binding: BindingId,
        tools: &ConfigureTools,
    ) -> Result<bool> {
        Ok(self.connection().execute(
            "UPDATE harness_binding SET tools = ?2 WHERE id = ?1 \
             AND tools IS NOT ?2",
            params![binding.0, serde_json::to_string(tools)?],
        )? == 1)
    }

    /// The tools `binding` offers, when its runtime has not been told
    /// them.
    pub(crate) fn tools_owed(
        &self,
        binding: BindingId,
    ) -> Result<Option<ConfigureTools>> {
        self.connection()
            .query_row(
                "SELECT tools FROM harness_binding WHERE id = ?1 \
                 AND tools IS NOT NULL AND tools IS NOT tools_told",
                [binding.0],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .map(|tools| Ok(serde_json::from_str(&tools)?))
            .transpose()
    }

    /// Keeps that the runtime of `binding` was told `tools`.
    pub(crate) fn tools_told(
        &mut self,
        binding: BindingId,
        tools: &str,
    ) -> Result<()> {
        self.connection().execute(
            "UPDATE harness_binding SET tools_told = ?2 WHERE id = \
             ?1",
            params![binding.0, tools],
        )?;
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
        let kept = self.connection().execute(
            "INSERT INTO harness_input (binding, request, ordinal, \
             content, steer, state, mark, write, boundary) SELECT \
             ?1, ?2, coalesce(max(ordinal), 0) + 1, ?3, ?4, 'kept', \
             ?5, ?6, ?7 FROM harness_input WHERE binding = ?1 ON \
             CONFLICT (binding, request) DO NOTHING",
            params![
                binding.0,
                input.request,
                input.content,
                input.steer,
                mark,
                input.write,
                input.boundary
            ],
        )?;
        Ok(kept == 1)
    }

    /// The inputs owed to `binding`, in the order they were kept:
    /// those not yet admitted, then, when `unsettled`, those
    /// admitted whose end the server has not heard.
    pub(crate) fn owed(
        &self,
        binding: BindingId,
        unsettled: bool,
    ) -> Result<Vec<Owed>> {
        let mut query = self.connection().prepare(
            "SELECT request, content, steer, mark, write, boundary \
             FROM harness_input WHERE binding = ?1 AND (state = \
             'kept' OR (?2 AND state = 'admitted' AND followed = 0 \
             AND coalesce(status, '') NOT IN ('done', \
             'unanswered'))) ORDER BY ordinal",
        )?;
        query
            .query_map(params![binding.0, unsettled], |row| {
                Ok((
                    Owed {
                        request: row.get(0)?,
                        content: row.get(1)?,
                        steer: row.get(2)?,
                        mark: None,
                        write: row.get(4)?,
                        boundary: row.get(5)?,
                    },
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .map(|row| {
                let (mut owed, mark) = row?;
                owed.mark = mark
                    .map(|m| serde_json::from_str(&m))
                    .transpose()?;
                Ok(owed)
            })
            .collect()
    }

    /// How the input `request` of `binding` stands.
    pub fn standing(
        &self,
        binding: BindingId,
        request: &str,
    ) -> Result<Option<Standing>> {
        self.connection()
            .query_row(
                "SELECT submission, status, reason, detail, \
                 followed FROM harness_input WHERE binding = ?1 AND \
                 request = ?2",
                params![binding.0, request],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, bool>(4)?,
                    ))
                },
            )
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
        &self,
        binding: BindingId,
        submission: &str,
    ) -> Result<Option<String>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT request FROM harness_input WHERE binding = \
                 ?1 AND submission = ?2",
                params![binding.0, submission],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Keeps how the input `request` of `binding` stands now.
    pub(crate) fn input_stands(
        &mut self,
        binding: BindingId,
        request: &str,
        submission: Option<&str>,
        status: &str,
        reason: Option<&str>,
        detail: Option<&serde_json::Value>,
    ) -> Result<()> {
        let detail = detail.map(serde_json::to_string).transpose()?;
        // A status never goes back once the input ended.
        self.connection().execute(
            "UPDATE harness_input SET submission = coalesce(?3, \
             submission), status = ?4, reason = ?5, detail = ?6, \
             state = CASE WHEN ?4 = 'unanswered' THEN 'dropped' \
             WHEN state = 'kept' THEN 'admitted' ELSE state END \
             WHERE binding = ?1 AND request = ?2 AND \
             coalesce(status, '') NOT IN ('done', 'unanswered')",
            params![
                binding.0, request, submission, status, reason,
                detail
            ],
        )?;
        Ok(())
    }

    /// Keeps that the server acted on the end of the input `request`;
    /// returns whether it had not yet.
    pub fn follow(
        &mut self,
        binding: BindingId,
        request: &str,
    ) -> Result<bool> {
        Ok(self.connection().execute(
            "UPDATE harness_input SET followed = 1 WHERE binding = \
             ?1 AND request = ?2 AND followed = 0",
            params![binding.0, request],
        )? == 1)
    }

    /// The inputs whose end the server has heard and not acted on.
    pub fn unfollowed(&self) -> Result<Vec<(BindingId, String)>> {
        let mut query = self.connection().prepare(
            "SELECT binding, request FROM harness_input WHERE \
             followed = 0 AND status IN ('done', 'unanswered') \
             ORDER BY binding, ordinal",
        )?;
        Ok(query
            .query_map([], |row| {
                Ok((BindingId(row.get(0)?), row.get(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Keeps `item` of `binding`; returns whether it is new.
    pub(crate) fn keep_item(
        &mut self,
        binding: BindingId,
        item: &Item,
    ) -> Result<bool> {
        Ok(self.connection().execute(
            "INSERT INTO harness_item (binding, entry, ordinal, \
             item) VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
            params![
                binding.0,
                item.entry,
                ordinal_of(&item.entry)?,
                serde_json::to_string(item)?
            ],
        )? == 1)
    }

    /// The items `binding` kept, oldest first.
    pub fn items(&self, binding: BindingId) -> Result<Vec<Item>> {
        let mut query = self.connection().prepare(
            "SELECT item FROM harness_item WHERE binding = ?1 ORDER \
             BY ordinal",
        )?;
        query
            .query_map([binding.0], |row| row.get::<_, String>(0))?
            .map(|item| Ok(serde_json::from_str(&item?)?))
            .collect()
    }

    /// Whether `binding` kept the entry `entry`.
    pub(crate) fn has_item(
        &self,
        binding: BindingId,
        entry: &str,
    ) -> Result<bool> {
        Ok(self
            .connection()
            .query_row(
                "SELECT 1 FROM harness_item WHERE binding = ?1 AND \
                 entry = ?2",
                params![binding.0, entry],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Begins cancelling `binding`, in one commit: its owed inputs
    /// are dropped, so no restart submits them, its calls not carried
    /// out are refused, and the cancel is owed to its runtime until
    /// it confirms it.
    pub(crate) fn begin_cancel(
        &mut self,
        binding: BindingId,
    ) -> Result<()> {
        let tx = self.transaction()?;
        tx.execute(
            "UPDATE harness_input SET state = 'dropped' WHERE \
             binding = ?1 AND state = 'kept'",
            [binding.0],
        )?;
        tx.execute(
            "UPDATE harness_binding SET cancelling = 1 WHERE id = ?1",
            [binding.0],
        )?;
        // A call of the stopped turn the gate allowed, asked for
        // again after the stop or after a restart, runs
        // nothing (H34); a marked result stands.
        let open = tx
            .prepare(
                "SELECT runtime, key FROM gate_call WHERE binding = \
                 ?1 AND block IS NULL",
            )?
            .query_map([binding.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (runtime, key) in open {
            if !crate::operation::is_marked(&tx, &runtime, &key)? {
                tx.execute(
                    "UPDATE gate_call SET block = ?3 WHERE runtime \
                     = ?1 AND key = ?2",
                    params![
                        runtime,
                        key,
                        super::answers::STOPPED_WORDS
                    ],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Whether a cancel of `binding` is still owed to its runtime.
    pub(crate) fn cancelling(
        &self,
        binding: BindingId,
    ) -> Result<bool> {
        Ok(self.connection().query_row(
            "SELECT cancelling FROM harness_binding WHERE id = ?1",
            [binding.0],
            |row| row.get::<_, bool>(0),
        )?)
    }

    /// Keeps that the runtime confirmed the cancel of `binding`.
    pub(crate) fn cancelled(
        &mut self,
        binding: BindingId,
    ) -> Result<()> {
        self.connection().execute(
            "UPDATE harness_binding SET cancelling = 0 WHERE id = ?1",
            [binding.0],
        )?;
        Ok(())
    }
}

const BINDING: &str = "SELECT id, hamlet, number, errand, model, \
                       runtime, conversation, fork_of, fork_at FROM \
                       harness_binding";

/// The order of the entry `entry`: a runtime's entry ids are numbers
/// that grow.
fn ordinal_of(entry: &str) -> Result<i64> {
    entry
        .parse::<i64>()
        .with_context(|| format!("the entry id {entry} is no number"))
}

fn binding_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<Binding>> {
    let number = row.get::<_, i64>(2)?;
    let model = row.get::<_, String>(4)?;
    let runtime = row.get::<_, String>(5)?;
    let id = BindingId(row.get(0)?);
    let hamlet = row.get::<_, String>(1)?;
    let errand = row.get::<_, bool>(3)?;
    let conversation = row.get::<_, Option<String>>(6)?;
    let fork_of = row.get::<_, Option<i64>>(7)?.map(BindingId);
    let fork_at = row.get::<_, Option<String>>(8)?;
    Ok((|| {
        Ok(Binding {
            id,
            agent: AgentRef {
                hamlet,
                number: u64::try_from(number)?,
            },
            errand,
            model: serde_json::from_str(&model)
                .context("reading a binding's model")?,
            runtime: RuntimeInstance::new(runtime)?,
            conversation,
            fork_of,
            fork_at,
        })
    })())
}
