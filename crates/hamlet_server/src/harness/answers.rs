//! The server's answers to what a runtime asks of model attempts and
//! their usage, decided against the realm as it is when they are
//! asked.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};

use super::bindings::Binding;
use super::link::{self, ModelRef, Price, ServerMessage, Usage};
use super::process::Pi;
use super::{Harnesses, InputMark, Notice};
use crate::clock::now;
use crate::credentials::{Credentials, pack};
use crate::{RuntimeInstance, Store};

pub(crate) const STOPPED_WORDS: &str =
    "The turn this call belongs to was stopped.";

/// Why an attempt may not be made.
pub(crate) struct Stop {
    pub words: String,
    /// A cap of spend stopped it, which the chief is told of.
    pub spend: bool,
}

/// Admits an attempt with its task and price, or returns what
/// stopped it.
pub(crate) fn attempt(
    store: &mut Store,
    binding: &Binding,
    key: &str,
    model: &ModelRef,
    task: Option<&str>,
    price: &Price,
    at: i64,
) -> Result<Option<Stop>> {
    let old = store
        .connection()
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
            .then(|| Stop {
                words: "This attempt key already names another \
                        model call."
                    .into(),
                spend: false,
            }));
    }
    if let Some(words) = crate::spend::can_spend(
        store.connection(),
        &crate::society::payer(store.connection(), binding)?,
        task,
        at,
    )? {
        return Ok(Some(Stop { words, spend: true }));
    }
    store.connection().execute(
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
            serde_json::to_string(price)?
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
        .connection()
        .query_row(
            "SELECT price FROM harness_attempt WHERE runtime = ?1 \
             AND key = ?2",
            params![runtime.as_str(), key],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    let Some(price) = allowed else {
        return Ok(None);
    };
    let price = serde_json::from_str::<Price>(&price)?;
    let amount = crate::spend::amount(usage, &price);
    let [input, output, cache_read, cache_write] = [
        usage.input,
        usage.output,
        usage.cache_read,
        usage.cache_write,
    ]
    .map(|count| i64::try_from(count).unwrap_or(i64::MAX));
    store.connection().execute(
        "INSERT INTO harness_usage (runtime, key, input, output, \
         cache_read, cache_write, micros, at, ms, estimated, \
         detail, ordinal) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, \
         ?9, ?10, ?11, (SELECT COALESCE(MAX(ordinal),0)+1 FROM \
         harness_usage)) ON CONFLICT DO NOTHING",
        params![
            runtime.as_str(),
            key,
            input,
            output,
            cache_read,
            cache_write,
            amount.micros,
            at,
            amount.ms,
            amount.tokens - input - output - cache_read - cache_write,
            serde_json::to_string(usage)?
        ],
    )?;
    Ok(Some(()))
}

impl Harnesses {
    /// Answers whether the model call `attempt` may be made, and with
    /// which model and credential.
    pub(super) async fn allow_attempt(
        &mut self,
        store: &mut Store,
        index: usize,
        attempt: link::Attempt,
        credentials: &Credentials,
    ) -> Result<Vec<Notice>> {
        let Some(binding) = self
            .conversation(
                store,
                index,
                &attempt.conversation,
                &attempt.id,
            )
            .await?
        else {
            return Ok(Vec::new());
        };
        let task = crate::society::task_of(
            store,
            &binding,
            attempt.task.as_deref(),
        )?;
        let selection =
            self.choices(store)?.select(store, &binding.agent)?;
        let re = attempt.id;
        if let Some(stop) = self::attempt(
            store,
            &binding,
            &attempt.key,
            &selection.model,
            task.as_deref(),
            &selection.price,
            now(),
        )? {
            // A cap of spend ends the turn as stopped, with its words
            // kept for the chief.
            let notice = stop
                .spend
                .then(|| (&InputMark::notice(&stop.words)).into());
            let words = stop.words;
            self.send(index, |v, id| {
                ServerMessage::Stopped(link::Stopped {
                    v,
                    id,
                    re,
                    words,
                    notice,
                })
            })
            .await;
            return Ok(Vec::new());
        }
        let provider = &selection.model.provider;
        self.credentials.grant(index, &attempt.key, provider);
        let serving = self.serving(store)?;
        let credential = (provider != Pi::FAUX_PROVIDER)
            .then(|| credentials.model(provider, &serving))
            .flatten()
            .and_then(|credential| pack(credential).ok());
        self.send(index, |v, id| {
            ServerMessage::Allowance(link::Allowance {
                credential,
                id,
                re,
                selection,
                v,
            })
        })
        .await;
        Ok(vec![Notice::Attempted {
            binding: binding.id,
        }])
    }

    /// Charges the usage `report` tells to its attempt.
    pub(super) async fn charge_usage(
        &mut self,
        store: &mut Store,
        index: usize,
        report: link::UsageReport,
    ) -> Result<Vec<Notice>> {
        self.credentials.finish(index, &report.key);
        let runtime = self.runtimes[index].id.clone();
        let charged = charge(
            store,
            &runtime,
            &report.key,
            &report.usage,
            now(),
        )?;
        let re = report.id;
        match charged {
            Some(()) => {
                self.send(index, |v, id| {
                    ServerMessage::Charged(link::Charged {
                        id,
                        re,
                        v,
                    })
                })
                .await;
            }
            None => {
                self.error(
                    index,
                    Some(re),
                    link::ErrorCode::Failed,
                    "No model call was allowed under this key.",
                )
                .await;
            }
        }
        Ok(Vec::new())
    }
}
