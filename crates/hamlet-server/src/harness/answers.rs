//! The server's answers to what a runtime asks: tool calls, model
//! attempts, usage and prompt sections, decided against the realm as
//! it is when they are asked.

use std::collections::BTreeMap;

use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::bindings::Binding;
use super::link::{ModelRef, Section, Usage};
use super::tools;
use crate::operation::canonical;
use crate::{OperationId, RuntimeInstance, Store};

/// The answer to a tool call asked early or carried out.
#[derive(Debug, PartialEq)]
pub(crate) enum Answer {
    Allowed,
    /// Waiting for a chief; nothing of it is marked.
    Waiting(String),
    Refused(String),
    /// The marked result, for the agent.
    Result {
        text: String,
        error: bool,
    },
}

/// The answer, and whether the call began a new wait for a chief.
pub(crate) struct Asked {
    pub answer: Answer,
    pub new_wait: bool,
}

/// The state of a call's wait.
enum Gate {
    Pending,
    Allowed,
    Refused(String),
    /// The identity was asked with other arguments.
    Conflict,
}

const PENDING_WORDS: &str = "This call waits for a chief's answer.";
const STOPPED_WORDS: &str =
    "The turn this call belongs to was stopped.";
const REVOKED_WORDS: &str = "A chief refused this call.";

/// Asks early whether the call `key` of `binding` may run, keeping
/// its wait when it asks a chief.
pub(crate) fn allow(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    asks: fn(&str) -> bool,
) -> Result<Asked> {
    if let Err(words) = tools::parse(tool, arguments) {
        return Ok(Asked {
            answer: Answer::Refused(words),
            new_wait: false,
        });
    }
    let (gate, new_wait) =
        gate(store, binding, key, tool, arguments, asks)?;
    Ok(Asked {
        answer: match gate {
            Gate::Pending => Answer::Waiting(PENDING_WORDS.into()),
            Gate::Allowed => Answer::Allowed,
            Gate::Refused(words) => Answer::Refused(words),
            Gate::Conflict => Answer::Refused(tools::CONFLICT.into()),
        },
        new_wait,
    })
}

/// Carries out the call `key` of `binding`: its marked result when it
/// ran, else as its wait and the realm allow now.
pub(crate) fn operation(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    asks: fn(&str) -> bool,
    at: i64,
) -> Result<Asked> {
    let call = match tools::parse(tool, arguments) {
        Ok(call) => call,
        Err(words) => {
            return Ok(Asked {
                answer: Answer::Result {
                    text: words,
                    error: true,
                },
                new_wait: false,
            });
        }
    };
    let id = OperationId {
        runtime: binding.runtime.clone(),
        key: key.into(),
    };
    let run = |store: &mut Store| -> Result<Answer> {
        let (text, error) =
            tools::run(store, &id, binding, &call, at)?;
        Ok(Answer::Result { text, error })
    };
    // A marked result stands whatever has changed since (S18).
    if marked(store, &id)? {
        return Ok(Asked {
            answer: run(store)?,
            new_wait: false,
        });
    }
    let (gate, new_wait) =
        gate(store, binding, key, tool, arguments, asks)?;
    let answer = match gate {
        Gate::Pending => Answer::Waiting(PENDING_WORDS.into()),
        Gate::Refused(words) => Answer::Refused(words),
        Gate::Conflict => Answer::Refused(tools::CONFLICT.into()),
        Gate::Allowed => run(store)?,
    };
    Ok(Asked { answer, new_wait })
}

fn marked(store: &Store, id: &OperationId) -> Result<bool> {
    Ok(store
        .db
        .query_row(
            "SELECT 1 FROM marker WHERE runtime = ?1 AND key = ?2",
            params![id.runtime.as_str(), id.key],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The call's wait, kept the first time it is asked: pending when the
/// tool asks a chief, else allowed.
fn gate(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    tool: &str,
    arguments: &BTreeMap<String, Value>,
    asks: fn(&str) -> bool,
) -> Result<(Gate, bool)> {
    let digest = digest(tool, arguments)?;
    let kept = store
        .db
        .query_row(
            "SELECT digest, state, words FROM harness_wait WHERE \
             runtime = ?1 AND key = ?2",
            params![binding.runtime.as_str(), key],
            |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((kept, state, words)) = kept else {
        let pending = asks(tool);
        store.db.execute(
            "INSERT INTO harness_wait (runtime, key, binding, tool, \
             digest, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                binding.runtime.as_str(),
                key,
                binding.id.0,
                tool,
                digest,
                if pending { "pending" } else { "allowed" }
            ],
        )?;
        return Ok(if pending {
            (Gate::Pending, true)
        } else {
            (Gate::Allowed, false)
        });
    };
    if kept != digest {
        return Ok((Gate::Conflict, false));
    }
    Ok((
        match state.as_str() {
            "pending" => Gate::Pending,
            "allowed" => Gate::Allowed,
            "refused" => Gate::Refused(
                words.unwrap_or_else(|| REVOKED_WORDS.into()),
            ),
            other => bail!("a wait in the state {other}"),
        },
        false,
    ))
}

fn digest(
    tool: &str,
    arguments: &BTreeMap<String, Value>,
) -> Result<Vec<u8>> {
    let mut request = String::new();
    canonical(&serde_json::to_value(arguments)?, &mut request);
    let mut hash = Sha256::new();
    hash.update(tool);
    hash.update([0]);
    hash.update(request);
    Ok(hash.finalize().to_vec())
}

/// What a chief's answer to a call's wait did.
#[derive(Debug, PartialEq, Eq)]
pub enum Decided {
    /// The wait now allows or refuses the call.
    Answered,
    /// The call already ran; taking it back undoes nothing.
    AlreadyRan,
    /// There is no such wait, or it was already refused.
    Unchanged,
}

impl Store {
    /// A chief's answer to the wait of the call `key` of `runtime`. A
    /// refusal also takes back a call allowed but not yet carried
    /// out.
    pub fn decide(
        &mut self,
        runtime: &RuntimeInstance,
        key: &str,
        allow: bool,
    ) -> Result<Decided> {
        let id = OperationId {
            runtime: runtime.clone(),
            key: key.into(),
        };
        if marked(self, &id)? {
            return Ok(Decided::AlreadyRan);
        }
        let changed = if allow {
            self.db.execute(
                "UPDATE harness_wait SET state = 'allowed' WHERE \
                 runtime = ?1 AND key = ?2 AND state = 'pending'",
                params![runtime.as_str(), key],
            )?
        } else {
            self.db.execute(
                "UPDATE harness_wait SET state = 'refused', words = \
                 ?3 WHERE runtime = ?1 AND key = ?2 AND state != \
                 'refused'",
                params![runtime.as_str(), key, REVOKED_WORDS],
            )?
        };
        Ok(if changed == 1 {
            Decided::Answered
        } else {
            Decided::Unchanged
        })
    }

    /// Lapses every pending wait of `binding`; returns their runtime
    /// and keys.
    pub(crate) fn lapse_waits(
        &mut self,
        binding: &Binding,
    ) -> Result<Vec<String>> {
        let mut query = self.db.prepare(
            "UPDATE harness_wait SET state = 'refused', words = ?2 \
             WHERE binding = ?1 AND state = 'pending' RETURNING key",
        )?;
        let keys = query
            .query_map(params![binding.id.0, STOPPED_WORDS], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(keys)
    }
}

/// Admits an attempt with its task and price, or returns the cap that
/// stopped it.
pub(crate) fn attempt(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    model: &ModelRef,
    task: Option<&str>,
    price: Option<&Value>,
    at: i64,
) -> Result<Option<String>> {
    let old = store
        .db
        .query_row(
            "SELECT binding, provider, model, task FROM \
             harness_attempt WHERE runtime=?1 AND key=?2",
            params![binding.runtime.as_str(), key],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some((id, provider, name, previous_task)) = old {
        return Ok((id != binding.id.0
            || provider != model.provider
            || name != model.id
            || previous_task.as_deref() != task)
            .then(|| {
                "This attempt key already names another model call."
                    .into()
            }));
    }
    if let Some(words) = crate::spend::can_spend(
        store.connection(),
        &binding.agent.hamlet,
        task,
        at,
    )? {
        return Ok(Some(words));
    }
    store.db.execute(
        "INSERT INTO harness_attempt (runtime, key, binding, \
         provider, model, at, task, price) VALUES (?1, ?2, ?3, ?4, \
         ?5, ?6, ?7, ?8) ON CONFLICT DO NOTHING",
        params![
            binding.runtime.as_str(),
            key,
            binding.id.0,
            model.provider,
            model.id,
            at,
            task,
            price.map(Value::to_string)
        ],
    )?;
    Ok(None)
}

/// Charges `usage` to the attempt `key` of `runtime` once, however
/// often it is reported. `None` when no such attempt was allowed.
pub(crate) fn charge(
    store: &mut Store,
    runtime: &RuntimeInstance,
    key: &str,
    usage: &Usage,
    at: i64,
) -> Result<Option<()>> {
    let allowed = store
        .db
        .query_row(
            "SELECT price FROM harness_attempt WHERE runtime = ?1 \
             AND key = ?2",
            params![runtime.as_str(), key],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?;
    let Some(price) = allowed else {
        return Ok(None);
    };
    let price = price
        .map(|p| serde_json::from_str::<serde_json::Value>(&p))
        .transpose()?;
    let amount = crate::spend::amount(usage, price.as_ref());
    store.db.execute(
        "INSERT INTO harness_usage (runtime, key, input, output, \
         cache_read, cache_write, micros, at, ms, estimated, \
         detail, ordinal) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, \
         ?9, ?10, ?11, (SELECT COALESCE(MAX(ordinal),0)+1 FROM \
         harness_usage)) ON CONFLICT DO NOTHING",
        params![
            runtime.as_str(),
            key,
            usage.input,
            usage.output,
            usage.cache_read,
            usage.cache_write,
            amount.micros,
            at,
            amount.ms,
            amount.tokens
                - usage.input
                - usage.output
                - usage.cache_read
                - usage.cache_write,
            serde_json::to_string(usage)?
        ],
    )?;
    Ok(Some(()))
}

/// The prompt sections of the conversation of `binding`.
pub(crate) fn prompt(
    store: &Store,
    binding: &Binding,
) -> Result<Vec<Section>> {
    let place = &binding.agent;
    let Some(record) = store.record(&place.hamlet)? else {
        return Ok(Vec::new());
    };
    let Some(agent) = record
        .agents
        .iter()
        .find(|agent| agent.number == place.number)
    else {
        return Ok(Vec::new());
    };
    let role = if record.mayor == place.number {
        "the mayor of"
    } else {
        "an agent of"
    };
    Ok(vec![Section {
        name: "identity".into(),
        text: format!(
            "You are {}, {role} the hamlet {}.",
            agent.name, record.id
        ),
    }])
}
