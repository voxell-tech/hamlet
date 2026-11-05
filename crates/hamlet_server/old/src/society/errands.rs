//! Durable one-off conversations and their waiting tool calls.

use anyhow::Result;
use diesel::dsl::exists;
use diesel::prelude::*;
use serde_json::{Value, json};

use super::Caller;
use crate::chats::records::{self, Place};
use crate::harness::{
    BindingId, Harnesses, Input, InputMark, InputStatus,
};
use crate::record::AgentRef;
use crate::schema::{harness_input, society_binding};
use crate::service::Service;

pub(super) async fn start(
    service: &mut Service,
    harnesses: &mut Harnesses,
    agent: &AgentRef,
    kind: &str,
    owner: &Value,
    prompt: &str,
    task: &str,
) -> Result<BindingId> {
    let owner = owner.to_string();
    let known = society_binding::table
        .filter(society_binding::errand.eq(kind))
        .filter(society_binding::owner.eq(&owner))
        .select(society_binding::binding)
        .first::<i64>(service.db())
        .optional()?;
    let binding = if let Some(binding) = known {
        let binding = BindingId(binding);
        if let Some(standing) =
            service.home.store().standing(binding, "errand")?
        {
            if !standing.status.is_some_and(InputStatus::is_ended)
                && ["hearing", "ballot"].contains(&kind)
            {
                let ballot = service
                    .limits_of(&agent.hamlet)?
                    .votes
                    .ballot_minutes;
                let began = service
                    .home
                    .store()
                    .mark_of(binding, "errand")?
                    .map_or_else(crate::clock::now, |mark| mark.at());
                if crate::clock::now().saturating_sub(began)
                    >= crate::clock::minutes(ballot)
                {
                    harnesses
                        .cancel(service.home.store(), binding)
                        .await?;
                }
            }
            return Ok(binding);
        }
        binding
    } else {
        let (binding, _) =
            harnesses.bind(service.home.store(), agent, true).await?;
        let db = service.db();
        records::keep(
            db,
            binding,
            &Place {
                agent: agent.clone(),
                chief: None,
                session: 0,
            },
            None,
            0,
        )?;
        diesel::insert_into(society_binding::table)
            .values((
                society_binding::binding.eq(binding.0),
                society_binding::payer.eq(&agent.hamlet),
                society_binding::errand.eq(kind),
                society_binding::owner.eq(&owner),
                society_binding::instructions.eq(prompt),
            ))
            .execute(db)?;
        binding
    };
    super::configure(service, harnesses, binding).await?;
    let mark = InputMark::Errand {
        task: task.into(),
        at: crate::clock::now(),
    };
    harnesses
        .submit(
            service.home.store(),
            binding,
            &Input {
                request: "errand",
                content: prompt,
                steer: false,
                mark: Some(&mark),
                write: false,
                boundary: false,
            },
        )
        .await?;
    Ok(binding)
}

pub(super) fn answer(
    service: &mut Service,
    binding: BindingId,
) -> Result<Option<String>> {
    let Some(standing) =
        service.home.store().standing(binding, "errand")?
    else {
        return Ok(None);
    };
    match standing.status {
        Some(InputStatus::Done) => {
            super::letters::answer_of(service.db(), binding, "errand")
        }
        Some(InputStatus::Unanswered) => Ok(Some(format!(
            "It could not finish: {}",
            standing.reason.as_deref().unwrap_or("the model failed")
        ))),
        _ => Ok(None),
    }
}

pub(super) async fn run(
    service: &mut Service,
    harnesses: &mut Harnesses,
    caller: &Caller<'_>,
    kind: &str,
    prompt: &str,
) -> Result<Option<String>> {
    let task = caller.task.clone().unwrap_or_else(|| {
        format!(
            "errand:{}:{}",
            caller.id.runtime.as_str(),
            caller.id.key
        )
    });
    let binding = start(service, harnesses, &caller.place.agent, kind, &json!({"runtime":caller.id.runtime.as_str(),"key":caller.id.key}), prompt, &task).await?;
    answer(service, binding)
}

pub(crate) async fn sweep(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let owners = society_binding::table
        .filter(society_binding::errand.is_not_null())
        .filter(exists(
            harness_input::table
                .filter(
                    harness_input::binding
                        .eq(society_binding::binding),
                )
                .filter(harness_input::request.eq("errand"))
                .filter(
                    harness_input::status
                        .eq_any(crate::harness::ended()),
                ),
        ))
        .select(society_binding::owner.assume_not_null())
        .load::<String>(service.db())?;
    for owner in owners {
        let owner = serde_json::from_str::<Value>(&owner)?;
        if let (Some(runtime), Some(key)) =
            (owner["runtime"].as_str(), owner["key"].as_str())
        {
            harnesses
                .wake(&crate::RuntimeInstance::new(runtime)?, key)
                .await;
        }
    }
    Ok(())
}

/// The instructions the errand of `binding` was given.
pub(crate) fn instructions(
    db: &mut SqliteConnection,
    binding: BindingId,
) -> Result<Option<String>> {
    Ok(society_binding::table
        .find(binding.0)
        .select(society_binding::instructions)
        .first::<Option<String>>(db)
        .optional()?
        .flatten())
}
