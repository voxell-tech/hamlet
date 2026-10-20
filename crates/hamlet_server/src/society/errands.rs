//! Durable one-off conversations and their waiting tool calls.

use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use super::Caller;
use crate::chats::records::{self, Place};
use crate::harness::link::Mark;
use crate::harness::{BindingId, Harnesses, Input, InputStatus};
use crate::record::AgentRef;
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
    let known = service
        .home
        .store()
        .connection()
        .query_row(
            "SELECT binding FROM society_binding WHERE errand=?1 \
             AND owner=?2",
            params![kind, owner],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    let binding = if let Some(binding) = known {
        let binding = BindingId(binding);
        if let Some(standing) =
            service.home.store().standing(binding, "errand")?
        {
            if !standing.status.is_some_and(InputStatus::is_ended)
                && ["hearing", "ballot"].contains(&kind)
            {
                let limits = service.limits_of(&agent.hamlet)?;
                let mark = records::mark_of(
                    service.home.store().connection(),
                    binding,
                    "errand",
                )?;
                let began = mark
                    .and_then(|m| {
                        m.data.get("at").and_then(Value::as_i64)
                    })
                    .unwrap_or(super::now());
                if super::now().saturating_sub(began)
                    >= limits["votes"]["ballotMinutes"]
                        .as_i64()
                        .unwrap_or(5)
                        .saturating_mul(60_000)
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
        let db = service.home.store().connection();
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
        db.execute(
            "INSERT INTO society_binding \
             (binding,payer,errand,owner,instructions) VALUES \
             (?1,?2,?3,?4,?5)",
            params![binding.0, agent.hamlet, kind, owner, prompt],
        )?;
        binding
    };
    super::configure(service, harnesses, binding).await?;
    let mark = Mark {
        kind: "hamlet.errand".into(),
        data: [
            ("task".into(), task.into()),
            ("at".into(), super::now().into()),
        ]
        .into(),
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
        Some(InputStatus::Done) => super::letters::answer_of(
            service.home.store().connection(),
            binding,
            "errand",
        ),
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
    let owners = service
        .home
        .store()
        .connection()
        .prepare(
            "SELECT owner FROM society_binding s WHERE errand IS \
             NOT NULL AND EXISTS (SELECT 1 FROM harness_input i \
             WHERE i.binding=s.binding AND i.request='errand' AND \
             i.status IN ('done','unanswered'))",
        )?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
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
