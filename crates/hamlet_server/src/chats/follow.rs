//! What the server does once an input of a conversation ends (C5,
//! C23, C28): instructions done, turns counted for reflection, faults
//! raised and queued lines carried on.

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::records;
use crate::harness::{
    BindingId, Harnesses, Input, InputMark, InputStatus,
};
use crate::oversight::{self, ChatPlace};
use crate::service::Service;

/// The automatic instruction to save a session's lessons in notes
/// and skills (C5).
pub(crate) const REFLECT_PROMPT: &str =
    "Reflection time (automatic, not from a user). Look back over \
     this session. Update NOTES.md with anything worth remembering \
     about the users or lessons learned, and save or improve a \
     skill for anything you did that is likely to come up again. If \
     nothing is worth saving, say so in one line.";

/// When a reflection is asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mode {
    /// After a turn, once enough turns were answered.
    Automatic,
    /// When a session is put away or the home closes, if any turn
    /// was answered.
    Owed,
    /// When the chief asks.
    Explicit,
}

/// The answered turns after which an agent reflects; none for 0.
pub(crate) fn reflect_every(service: &mut Service) -> Result<i64> {
    Ok(i64::try_from(service.settings()?.reflect_every)?)
}

/// Acts once on the end of the input `request` of `binding`.
pub(crate) async fn follow(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    request: &str,
) -> Result<()> {
    let store = service.home.store();
    let Some(standing) = store.standing(binding, request)? else {
        return Ok(());
    };
    if !store.follow(binding, request)? {
        return Ok(());
    }
    if store.binding(binding)?.is_some_and(|b| b.errand) {
        return Ok(());
    }
    let db = store.connection();
    let Some(kept) = records::kept(db, binding)? else {
        return Ok(());
    };
    let submission = standing.submission.clone();
    let failure = match &submission {
        Some(input) => answer_failure(db, binding, input)?,
        None => None,
    };
    let status = standing.status;
    let reason = standing.reason.as_deref();
    let last = if status == Some(InputStatus::Done) {
        "done"
    } else if failure.is_some()
        || matches!(reason, Some("model_error" | "no_model"))
    {
        "failed"
    } else {
        "stopped"
    };
    records::followed(db, binding, last, submission.as_deref())?;
    let mark = store.mark_of(binding, request)?;
    let agent = &kept.place.agent;
    if let Some(InputMark::Chief { task, .. }) = &mark {
        records::instruction_done(
            db,
            agent,
            task,
            crate::clock::now(),
        )?;
    }
    let reflection =
        matches!(mark, Some(InputMark::Reflection { .. }));
    if reflection {
        db.execute(
            "DELETE FROM chat_reflection WHERE binding = ?1 AND \
             request = ?2",
            params![binding.0, request],
        )?;
        if let Some(input) = &submission
            && store.placed(binding, request)?
        {
            records::enter(db, binding, input)?;
        }
    } else if status == Some(InputStatus::Done)
        && mark.as_ref().is_some_and(InputMark::is_chief_turn)
    {
        records::count_turns(db, binding, Some(1))?;
    }
    if let Some(chief) = &kept.place.chief
        && last == "failed"
        && failure.is_none()
        && !reflection
    {
        let about = match &standing.detail {
            Some(Value::String(detail)) => detail.clone(),
            _ if status == Some(InputStatus::Unanswered) => {
                reason.unwrap_or("The model failed.").to_owned()
            }
            _ => "The model failed.".to_owned(),
        };
        raise_fault(service, &kept.place, chief, about)?;
    }
    report_answer_failures(service, binding)?;
    let pending = reserved(service.db(), binding)?;
    if last != "stopped" || pending {
        request_reflection(
            service,
            harnesses,
            binding,
            Mode::Automatic,
        )
        .await?;
    }
    if service.chats.is_queued(binding)
        && let Some(input) = submission
    {
        super::write(
            service,
            harnesses,
            binding,
            &format!("queue:{input}"),
            "Continuing the queued messages.",
            true,
        )
        .await?;
    }
    Ok(())
}

fn raise_fault(
    service: &mut Service,
    place: &records::Place,
    chief: &str,
    about: String,
) -> Result<()> {
    let agent = &place.agent;
    let Some(record) = service.home.store().record(&agent.hamlet)?
    else {
        return Ok(());
    };
    oversight::raise_fault(
        service.db(),
        record.address_of(agent.number).to_owned(),
        agent.clone(),
        Some(ChatPlace {
            hamlet: agent.hamlet.clone(),
            number: agent.number,
            chief: chief.into(),
            session: place.session,
        }),
        about,
        crate::clock::now(),
    )?;
    Ok(())
}

/// The words of a final answer that failed the server's check in the
/// turn of `input`.
fn answer_failure(
    db: &Connection,
    binding: BindingId,
    input: &str,
) -> Result<Option<String>> {
    Ok(db
        .query_row(
            "SELECT words FROM chat_answer WHERE binding = ?1 AND \
             input = ?2",
            params![binding.0, input],
            |row| row.get(0),
        )
        .optional()?)
}

/// Keeps that the final answer of the turn of `input` failed with
/// `words`.
pub(crate) fn fail_answer(
    db: &Connection,
    binding: BindingId,
    input: &str,
    words: &str,
) -> Result<()> {
    db.execute(
        "INSERT INTO chat_answer (binding, input, words) VALUES \
         (?1, ?2, ?3) ON CONFLICT DO NOTHING",
        params![binding.0, input, words],
    )?;
    Ok(())
}

/// Raises a fault once for each failed answer of `binding`.
fn report_answer_failures(
    service: &mut Service,
    binding: BindingId,
) -> Result<()> {
    let db = service.db();
    let Some(kept) = records::kept(db, binding)? else {
        return Ok(());
    };
    let Some(chief) = kept.place.chief.clone() else {
        return Ok(());
    };
    let words = db
        .prepare(
            "SELECT input, words FROM chat_answer WHERE binding = \
             ?1 AND reported = 0 ORDER BY input",
        )?
        .query_map([binding.0], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (input, about) in words {
        service.db().execute(
            "UPDATE chat_answer SET reported = 1 WHERE binding = ?1 \
             AND input = ?2",
            params![binding.0, input],
        )?;
        raise_fault(service, &kept.place, &chief, about)?;
    }
    Ok(())
}

/// Whether reflections of `binding` are reserved and not yet ended.
pub(crate) fn reserved(
    db: &Connection,
    binding: BindingId,
) -> Result<bool> {
    Ok(db
        .query_row(
            "SELECT 1 FROM chat_reflection WHERE binding = ?1",
            [binding.0],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Asks the agent of `binding` to reflect, when `mode` and its count
/// of answered turns call for it. Reserves the reflection before it
/// is submitted, so either half carries on after a crash (S3). Gives
/// the request submitted last.
pub(crate) async fn request_reflection(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    mode: Mode,
) -> Result<Option<String>> {
    let every = reflect_every(service)?;
    let db = service.db();
    let Some(kept) = records::kept(db, binding)? else {
        return Ok(None);
    };
    let Some(chief) = kept.place.chief.clone() else {
        return Ok(None);
    };
    let agent = kept.place.agent.clone();
    let in_chat = records::sessions(db, &agent, &chief)?
        .iter()
        .any(|session| session.binding == binding);
    if !in_chat {
        return Ok(None);
    }
    let mut reservations = reservations(db, binding)?;
    if reservations.is_empty() || mode == Mode::Explicit {
        let count = kept.unreflected;
        if (count == 0 && mode != Mode::Explicit)
            || (mode == Mode::Automatic
                && (every == 0 || (!kept.retiring && count < every)))
        {
            if reservations.is_empty() {
                return Ok(None);
            }
        } else {
            let ordinal =
                records::reserve_reflection(db, &agent, &chief)?;
            let request =
                format!("reflect:{}:{ordinal}", kept.place.session);
            let task = super::new_task();
            let at = crate::clock::now();
            db.execute(
                "INSERT INTO chat_reflection (binding, request, \
                 ordinal, count, task, at) VALUES (?1, ?2, ?3, ?4, \
                 ?5, ?6)",
                params![binding.0, request, ordinal, count, task, at],
            )?;
            if kept.retiring {
                super::write_notice(
                    service,
                    harnesses,
                    binding,
                    "Putting this session away. The agent keeps \
                     what is worth keeping first.",
                )
                .await?;
            }
            super::write_notice(
                service,
                harnesses,
                binding,
                &format!(
                    "Reflecting {count} turn{}.",
                    if count == 1 { "" } else { "s" }
                ),
            )
            .await?;
            reservations = self::reservations(service.db(), binding)?;
        }
    }
    let mayor = service
        .home
        .store()
        .record(&agent.hamlet)?
        .is_some_and(|record| record.mayor == agent.number);
    let words = if mayor {
        REFLECT_PROMPT.to_owned()
    } else {
        format!(
            "{REFLECT_PROMPT}\nThen, in one line: did anything \
             about how the hamlet is run get in the way of your \
             work? If nothing did, say so."
        )
    };
    let mut submitted = None;
    for (request, task, at) in reservations {
        let mark = InputMark::Reflection { task, at };
        harnesses
            .submit(
                service.home.store(),
                binding,
                &Input {
                    request: &request,
                    content: &words,
                    steer: false,
                    mark: Some(&mark),
                    write: false,
                    boundary: false,
                },
            )
            .await?;
        submitted = Some(request);
    }
    Ok(submitted)
}

fn reservations(
    db: &Connection,
    binding: BindingId,
) -> Result<Vec<(String, String, i64)>> {
    let mut query = db.prepare(
        "SELECT request, task, at FROM chat_reflection WHERE \
         binding = ?1 ORDER BY ordinal",
    )?;
    Ok(query
        .query_map([binding.0], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
