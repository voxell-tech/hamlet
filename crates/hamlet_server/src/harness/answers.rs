//! The server's answers to what a runtime asks of model attempts and
//! their usage, decided against the realm as it is when they are
//! asked.

use anyhow::Result;
use diesel::dsl::max;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Nullable};
use hamlet_protocol::runtime::{
    self, ModelRef, Price, ServerMessage, Usage,
};

use super::bindings::{Binding, ifnull};
use super::process::Pi;
use super::{Harnesses, InputMark, Notice};
use crate::clock::now;
use crate::credentials::{Credentials, pack};
use crate::schema::{harness_attempt, harness_usage};
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
    let old = harness_attempt::table
        .find((binding.runtime.as_str(), key))
        .select((
            harness_attempt::binding,
            harness_attempt::provider,
            harness_attempt::model,
            harness_attempt::task,
        ))
        .first::<(i64, String, String, Option<String>)>(
            store.connection(),
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
    let payer = crate::society::payer(store.connection(), binding)?;
    if let Some(words) =
        crate::spend::can_spend(store.connection(), &payer, task, at)?
    {
        return Ok(Some(Stop { words, spend: true }));
    }
    diesel::insert_into(harness_attempt::table)
        .values((
            harness_attempt::runtime.eq(binding.runtime.as_str()),
            harness_attempt::key.eq(key),
            harness_attempt::binding.eq(binding.id.0),
            harness_attempt::provider.eq(&model.provider),
            harness_attempt::model.eq(&model.id),
            harness_attempt::at.eq(at),
            harness_attempt::task.eq(task),
            harness_attempt::price.eq(serde_json::to_string(price)?),
        ))
        .on_conflict_do_nothing()
        .execute(store.connection())?;
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
    let allowed = harness_attempt::table
        .find((runtime.as_str(), key))
        .select(harness_attempt::price)
        .first::<String>(store.connection())
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
    let ordinal = harness_usage::table
        .select(ifnull(
            max(harness_usage::ordinal + 1),
            Some(1).into_sql::<Nullable<BigInt>>(),
        ))
        .single_value()
        .assume_not_null();
    diesel::insert_into(harness_usage::table)
        .values((
            harness_usage::runtime.eq(runtime.as_str()),
            harness_usage::key.eq(key),
            harness_usage::input.eq(input),
            harness_usage::output.eq(output),
            harness_usage::cache_read.eq(cache_read),
            harness_usage::cache_write.eq(cache_write),
            harness_usage::micros.eq(amount.micros),
            harness_usage::at.eq(at),
            harness_usage::ms.eq(amount.ms),
            harness_usage::estimated.eq(amount.tokens
                - input
                - output
                - cache_read
                - cache_write),
            harness_usage::detail.eq(serde_json::to_string(usage)?),
            harness_usage::ordinal.eq(ordinal),
        ))
        .on_conflict_do_nothing()
        .execute(store.connection())?;
    Ok(Some(()))
}

impl Harnesses {
    /// Answers whether the model call `attempt` may be made, and with
    /// which model and credential.
    pub(super) async fn allow_attempt(
        &mut self,
        store: &mut Store,
        index: usize,
        attempt: runtime::Attempt,
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
                ServerMessage::Stopped(runtime::Stopped {
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
            ServerMessage::Allowance(runtime::Allowance {
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
        report: runtime::UsageReport,
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
                    ServerMessage::Charged(runtime::Charged {
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
                    runtime::ErrorCode::Failed,
                    "No model call was allowed under this key.",
                )
                .await;
            }
        }
        Ok(Vec::new())
    }
}
