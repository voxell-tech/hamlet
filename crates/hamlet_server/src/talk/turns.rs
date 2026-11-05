//! What a runtime asks of the server during a turn: its prompt, leave
//! for each model call and the charge of it, and the end of the turn.

use anyhow::{Context, Result};
use diesel::prelude::*;
use hamlet_protocol::runtime::{Attempt, Price, Selection, Usage};
use hamlet_store::Db;
use hamlet_store::schema::{attempt, usage};

use super::conversations::{self, Conversation};
use super::{Env, inputs, models, tasks};

/// The conversation `runtime` knows as `handle`.
pub(crate) fn conversation(
    db: &mut Db,
    runtime: &str,
    handle: &str,
) -> Result<Conversation> {
    conversations::by_handle(db, runtime, handle)?.with_context(
        || format!("{runtime} has no conversation {handle}"),
    )
}

/// Admits the model call `asked` of `runtime`, once (S20), with the
/// model its agent runs now. Who pays is its task's payer.
pub(crate) fn allow(
    db: &mut Db,
    env: &Env,
    runtime: &str,
    asked: &Attempt,
    at: i64,
) -> Result<Selection> {
    let conversation =
        conversation(db, runtime, &asked.conversation)?;
    let choice = models::choose(
        db,
        &env.settings,
        &env.overrides,
        conversation.agent,
    )?;
    let task = match &asked.submission {
        Some(submission) => {
            inputs::by_submission(db, conversation.id, submission)?
                .and_then(|i| i.task)
        }
        None => None,
    };
    let payer = match task {
        Some(task) => tasks::get(db, task)?.payer,
        None => tasks::payer_of(db, conversation.agent)?,
    };
    let price = Price {
        cache_read: 0.0,
        cache_write: 0.0,
        input: 0.0,
        output: 0.0,
    };
    diesel::insert_into(attempt::table)
        .values((
            attempt::runtime.eq(runtime),
            attempt::key.eq(&asked.key),
            attempt::conversation.eq(conversation.id),
            attempt::agent.eq(conversation.agent),
            attempt::payer.eq(&payer),
            attempt::task.eq(task),
            attempt::provider.eq(&choice.model.provider),
            attempt::model.eq(&choice.model.id),
            attempt::price.eq(serde_json::to_string(&price)?),
            attempt::at.eq(at),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    Ok(Selection {
        context_window: None,
        effort: None,
        max_tokens: None,
        model: choice.model,
        price,
    })
}

/// Charges what the call `key` of `runtime` used, once.
pub(crate) fn charge(
    db: &mut Db,
    runtime: &str,
    key: &str,
    used: &Usage,
    at: i64,
) -> Result<()> {
    let ordinal = usage::table
        .select(diesel::dsl::max(usage::ordinal))
        .first::<Option<i64>>(db)?
        .map_or(0, |last| last + 1);
    let micros = used.billed.unwrap_or(used.cost) * 1_000_000.0;
    diesel::insert_into(usage::table)
        .values((
            usage::runtime.eq(runtime),
            usage::key.eq(key),
            usage::ordinal.eq(ordinal),
            usage::input.eq(i64::try_from(used.input)?),
            usage::output.eq(i64::try_from(used.output)?),
            usage::cache_read.eq(i64::try_from(used.cache_read)?),
            usage::cache_write.eq(i64::try_from(used.cache_write)?),
            usage::micros.eq(micros.round().max(0.0) as i64),
            usage::ms.eq(i64::try_from(used.ms.unwrap_or(0))?),
            usage::estimated.eq(used.estimated_tokens.is_some()),
            usage::detail.eq(serde_json::to_string(used)?),
            usage::at.eq(at),
        ))
        .on_conflict_do_nothing()
        .execute(db)?;
    Ok(())
}
