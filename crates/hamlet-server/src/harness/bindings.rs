//! Runtimes, the harness of each agent, conversations' bindings and
//! the inputs owed to them, as the realm database keeps them.

use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};

use super::link::ModelRef;
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
}

impl Store {
    /// The harness `agent`'s new conversations run in.
    pub fn harness_of(&self, agent: &AgentRef) -> Result<String> {
        Ok(self
            .db
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
        self.db.execute(
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
        self.db
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
        self.db
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
        let mut query = self.db.prepare(&format!(
            "{BINDING} WHERE runtime = ?1 ORDER BY id"
        ))?;
        query
            .query_map([runtime.as_str()], binding_row)?
            .map(|row| row?)
            .collect()
    }

    pub(crate) fn runtimes(&self) -> Result<Vec<RuntimeRow>> {
        let mut query = self.db.prepare(
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
            .db
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
        self.db.execute(
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
        self.db.execute(
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
        self.db.execute(
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
        Ok(BindingId(self.db.last_insert_rowid()))
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
        self.db.execute(
            "UPDATE harness_binding SET conversation = ?2, adapter \
             = ?3, version = ?4 WHERE id = ?1 AND conversation IS \
             NULL",
            params![binding.0, conversation, adapter, version],
        )?;
        Ok(())
    }

    /// Keeps an input for `binding` before it is submitted, once per
    /// request id; returns whether it is new.
    pub(crate) fn keep_input(
        &mut self,
        binding: BindingId,
        request: &str,
        content: &str,
        steer: bool,
    ) -> Result<bool> {
        let kept = self.db.execute(
            "INSERT INTO harness_input (binding, request, ordinal, \
             content, steer, state) SELECT ?1, ?2, \
             coalesce(max(ordinal), 0) + 1, ?3, ?4, 'kept' FROM \
             harness_input WHERE binding = ?1 ON CONFLICT (binding, \
             request) DO NOTHING",
            params![binding.0, request, content, steer],
        )?;
        Ok(kept == 1)
    }

    /// The inputs owed to `binding`, in the order they were kept.
    pub(crate) fn owed(
        &self,
        binding: BindingId,
    ) -> Result<Vec<Owed>> {
        let mut query = self.db.prepare(
            "SELECT request, content, steer FROM harness_input \
             WHERE binding = ?1 AND state = 'kept' ORDER BY ordinal",
        )?;
        Ok(query
            .query_map([binding.0], |row| {
                Ok(Owed {
                    request: row.get(0)?,
                    content: row.get(1)?,
                    steer: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub(crate) fn input_admitted(
        &mut self,
        binding: BindingId,
        request: &str,
    ) -> Result<()> {
        self.db.execute(
            "UPDATE harness_input SET state = 'admitted' WHERE \
             binding = ?1 AND request = ?2 AND state = 'kept'",
            params![binding.0, request],
        )?;
        Ok(())
    }

    /// Begins cancelling `binding`, in one commit: its owed inputs
    /// are dropped, so no restart submits them, its waits lapse, and
    /// the cancel is owed to its runtime until it confirms it.
    /// Returns the keys of the waits that were pending, which the
    /// runtime holds open.
    pub(crate) fn begin_cancel(
        &mut self,
        binding: BindingId,
    ) -> Result<Vec<String>> {
        let tx = self.db.transaction()?;
        tx.execute(
            "UPDATE harness_input SET state = 'dropped' WHERE \
             binding = ?1 AND state = 'kept'",
            [binding.0],
        )?;
        tx.execute(
            "UPDATE harness_binding SET cancelling = 1 WHERE id = ?1",
            [binding.0],
        )?;
        let pending = tx
            .prepare(
                "SELECT key FROM harness_wait WHERE binding = ?1 \
                 AND state = 'pending'",
            )?
            .query_map([binding.0], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // A call of the stopped turn not carried out yet, allowed or
        // not, runs nothing (H34); a marked result stands.
        tx.execute(
            "UPDATE harness_wait SET state = 'refused', words = ?2 \
             WHERE binding = ?1 AND state IN ('pending', 'allowed') \
             AND NOT EXISTS (SELECT 1 FROM marker WHERE \
             marker.runtime = harness_wait.runtime AND marker.key = \
             harness_wait.key)",
            params![binding.0, super::answers::STOPPED_WORDS],
        )?;
        tx.commit()?;
        Ok(pending)
    }

    /// Whether a cancel of `binding` is still owed to its runtime.
    pub(crate) fn cancelling(
        &self,
        binding: BindingId,
    ) -> Result<bool> {
        Ok(self.db.query_row(
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
        self.db.execute(
            "UPDATE harness_binding SET cancelling = 0 WHERE id = ?1",
            [binding.0],
        )?;
        Ok(())
    }
}

const BINDING: &str = "SELECT id, hamlet, number, errand, model, \
                       runtime, conversation FROM harness_binding";

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
        })
    })())
}
