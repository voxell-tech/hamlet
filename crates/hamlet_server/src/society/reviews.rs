//! Mayor reviews beside chief work and task-wide objection rulings.

use anyhow::{Context, Result};
use hamlet_protocol::v0::{Choice, LimitsAskingMayorReview};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::chats::records;
use crate::harness::{BindingId, Harnesses, InputMark};
use crate::oversight::{
    self, ChatPlace, Raising, Ruling, WaitKind, held,
};
use crate::record::AgentRef;
use crate::service::Service;

/// The society document kind of a review still waiting for the mayor
/// or for its work to end, the only ones the sweep reads.
const DUE: &str = "review";
/// The society document kind of a finished review.
const DONE: &str = "review-done";

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Instruction {
    pub binding: i64,
    pub waits: Vec<String>,
    pub how: Option<Ruling>,
    text: String,
    notified: bool,
}

#[derive(Serialize, Deserialize)]
struct Review {
    binding: i64,
    agent: AgentRef,
    mayor: AgentRef,
    chief: String,
    session: i64,
    text: String,
    prompt: String,
    done: bool,
    wait: Option<String>,
}

pub(crate) fn held(
    service: &mut Service,
    binding: BindingId,
    input: Option<&str>,
) -> Result<bool> {
    let Some(task) =
        super::marked_task(service.home.store(), binding, input)?
    else {
        return Ok(false);
    };
    let db = service.home.store().connection();
    let instruction = super::document::<Instruction>(
        db,
        "instruction-ruling",
        &task,
    )?;
    if instruction.how == Some(Ruling::Withdraw) {
        return Ok(true);
    }
    for wait in instruction.waits {
        if oversight::find(db, &wait)?.is_some_and(|wait| {
            wait.answers.contains(&Choice::Withdraw)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) async fn prepare(
    service: &mut Service,
    harnesses: &mut Harnesses,
    binding: BindingId,
    input: Option<&str>,
) -> Result<()> {
    let Some(InputMark::Chief {
        chief, text, task, ..
    }) = service.home.store().input_mark(binding, input)?
    else {
        return Ok(());
    };
    let task = task.as_str();
    let db = service.home.store().connection();
    if super::document::<Option<Review>>(db, DUE, task)?.is_some()
        || super::document::<Option<Review>>(db, DONE, task)?
            .is_some()
    {
        return Ok(());
    }
    let Some(kept) = records::kept(db, binding)? else {
        return Ok(());
    };
    let r = crate::record::read(db, &kept.place.agent.hamlet)?
        .context("the reviewed hamlet has no record")?;
    if r.mayor == kept.place.agent.number {
        return Ok(());
    }
    let review = r.limits.chief.asking.mayor_review.clone();
    if review == LimitsAskingMayorReview::Never {
        return Ok(());
    }
    if review == LimitsAskingMayorReview::Remote {
        let settings = service.settings()?;
        let mayor = r
            .agents
            .iter()
            .find(|agent| agent.number == r.mayor)
            .expect("mayor");
        let model =
            r.limits.chief.model_of(mayor, true, &settings.model);
        let (provider, model) =
            model.split_once("::").unwrap_or_default();
        let url = settings
            .base_url(provider)
            .or_else(|| service.models.base_url(provider, model))
            .and_then(|url| url::Url::parse(url).ok());
        let remote = url
            .and_then(|url| url.host_str().map(String::from))
            .is_some_and(|host| {
                !["localhost", "[::1]", "::1", "0.0.0.0"]
                    .contains(&host.as_str())
                    && !host.starts_with("127.")
            });
        if !remote {
            return Ok(());
        }
    }
    if crate::spend::can_spend(
        service.home.store().connection(),
        &r.id,
        Some(task),
        super::now(),
    )?
    .is_some()
    {
        return Ok(());
    }
    let text = format!(
        "{chief} told `{}`: {text}",
        r.name_of(
            kept.place.agent.number,
            crate::record::NameFallback::Recorded
        ),
    );
    let notes = service
        .houses
        .read_notes(&r.id, r.mayor)
        .await
        .unwrap_or_default();
    let review = Review {
        binding: binding.0,
        agent: kept.place.agent,
        mayor: AgentRef {
            hamlet: r.id.clone(),
            number: r.mayor,
        },
        chief: kept.place.chief.unwrap_or_default(),
        session: kept.place.session,
        prompt: format!(
            "You are the mayor of a hamlet of agents. A chief has \
             just instructed one of your agents directly. The work \
             has already begun and the chief always wins. If the \
             instruction is a mistake, answer OBJECT: <reason>. \
             Otherwise reply pass. Most instructions are fine.\n# \
             Your agents\n{}\n# Your notes\n{}\n{text}",
            r.agents
                .iter()
                .filter(|agent| agent.number != r.mayor)
                .map(|agent| format!(
                    "- {}: {}",
                    agent.name, agent.purpose
                ))
                .collect::<Vec<_>>()
                .join("\n"),
            crate::papers::cut(&notes, 6000, "")
        ),
        text,
        done: false,
        wait: None,
    };
    super::keep(
        service.home.store().connection(),
        DUE,
        task,
        &review,
    )?;
    super::errands::start(
        service,
        harnesses,
        &review.mayor,
        "review",
        &json!({"review":task}),
        &review.prompt,
        task,
    )
    .await?;
    Ok(())
}

/// Moves on every review still waiting for the mayor or for its
/// work to end, then tells each withdrawn instruction's conversation.
pub(crate) async fn sweep(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let jobs = service
        .home
        .store()
        .connection()
        .prepare(
            "SELECT key,value FROM society_document WHERE kind=?1",
        )?
        .query_map([DUE], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (task, value) in jobs {
        let review = serde_json::from_str::<Review>(&value)?;
        sweep_review(service, harnesses, &task, review).await?;
    }
    withdraw(service, harnesses).await
}

/// Raises the mayor's objection of `review` once its errand answers,
/// and lets the objection be only dismissed once the work of `task`
/// has ended. A review with nothing left to do is kept as done.
async fn sweep_review(
    service: &mut Service,
    harnesses: &mut Harnesses,
    task: &str,
    mut review: Review,
) -> Result<()> {
    if !review.done {
        let binding = super::errands::start(
            service,
            harnesses,
            &review.mayor,
            "review",
            &json!({"review":task}),
            &review.prompt,
            task,
        )
        .await?;
        let Some(answer) = super::errands::answer(service, binding)?
        else {
            return Ok(());
        };
        review.done = true;
        let ended = crate::harness::working_on(
            service.home.store().connection(),
            task,
        )?
        .is_empty();
        let tx = service.home.store().transaction()?;
        if let Some(reason) = objection(&answer) {
            object(&tx, task, &mut review, reason, ended)?;
        }
        super::keep(&tx, DUE, task, &review)?;
        tx.commit()?;
    }
    let db = service.home.store().connection();
    let ended = || {
        Ok::<_, anyhow::Error>(
            crate::harness::working_on(db, task)?.is_empty(),
        )
    };
    let settled = match &review.wait {
        None => true,
        Some(wait) if ended()? => {
            held::only_dismiss(
                db,
                wait,
                "(The work ended before you answered.)",
            )?;
            true
        }
        Some(_) => false,
    };
    if settled {
        let tx = service.home.store().transaction()?;
        super::keep(&tx, DONE, task, &review)?;
        tx.execute(
            "DELETE FROM society_document WHERE kind=?1 AND key=?2",
            [DUE, task],
        )?;
        tx.commit()?;
    }
    Ok(())
}

/// The reason of the mayor's answer `answer` when it objects.
fn objection(answer: &str) -> Option<&str> {
    answer
        .trim()
        .split_once(':')
        .filter(|(prefix, _)| prefix.eq_ignore_ascii_case("OBJECT"))
        .map(|(_, reason)| reason.trim())
        .filter(|reason| !reason.is_empty())
}

/// Raises the mayor's objection of `review` to the chief, unless the
/// instruction of `task` was already ruled on.
fn object(
    tx: &Connection,
    task: &str,
    review: &mut Review,
    reason: &str,
    ended: bool,
) -> Result<()> {
    let mut instruction = super::document::<Instruction>(
        tx,
        "instruction-ruling",
        task,
    )?;
    if instruction.how.is_some() {
        return Ok(());
    }
    review.wait = held::raise(
        tx,
        Raising {
            kind: WaitKind::Objection,
            by: if review.agent.hamlet == "home" {
                "mayor".into()
            } else {
                format!("mayor@{}", review.agent.hamlet)
            },
            agent: Some(review.agent.clone()),
            about: format!(
                "{}\nThe mayor objects: {reason}{}",
                review.text,
                if ended {
                    "\n(The work had already ended.)"
                } else {
                    ""
                }
            ),
            change: None,
            answers: if ended {
                vec![Choice::Dismiss]
            } else {
                vec![Choice::Go, Choice::Withdraw]
            },
            chat: Some(ChatPlace {
                hamlet: review.agent.hamlet.clone(),
                number: review.agent.number,
                chief: review.chief.clone(),
                session: review.session,
            }),
            call: None,
        },
        super::now(),
    )?;
    instruction.binding = review.binding;
    instruction.waits.extend(review.wait.clone());
    super::keep(tx, "instruction-ruling", task, &instruction)
}

/// Stops the work of each instruction the chief withdrew and tells
/// its conversation, once.
async fn withdraw(
    service: &mut Service,
    harnesses: &mut Harnesses,
) -> Result<()> {
    let withdrawals = service
        .home
        .store()
        .connection()
        .prepare(
            "SELECT key,value FROM society_document WHERE \
             kind='instruction-ruling' AND \
             json_extract(value,'$.how')='withdraw' AND \
             json_extract(value,'$.notified')=0",
        )?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (task, value) in withdrawals {
        let mut instruction =
            serde_json::from_str::<Instruction>(&value)?;
        for binding in crate::harness::working_on(
            service.home.store().connection(),
            &task,
        )? {
            crate::chats::stop_tools(service, binding);
            let waits = held::waits_holding(
                service.home.store().connection(),
                binding,
            )?;
            for wait in waits {
                if let Some((_, woken)) = oversight::settle(
                    service.home.store().connection(),
                    &wait,
                    Ruling::Stopped,
                    "The chief withdrew the instruction.",
                    None,
                    super::now(),
                )? {
                    for (runtime, key) in woken {
                        harnesses.wake(&runtime, &key).await;
                    }
                }
            }
            harnesses.cancel(service.home.store(), binding).await?;
        }
        crate::chats::write(
            service,
            harnesses,
            BindingId(instruction.binding),
            &format!("withdraw:{task}"),
            &format!(
                "The chief withdrew the instruction: {}",
                instruction.text
            ),
            false,
        )
        .await?;
        instruction.notified = true;
        super::keep(
            service.home.store().connection(),
            "instruction-ruling",
            &task,
            &instruction,
        )?;
    }
    Ok(())
}

pub(crate) fn settle(
    db: &Connection,
    wait: &str,
    ruling: Ruling,
    text: &str,
    chief: &str,
) -> Result<oversight::Woken> {
    let found = db
        .query_row(
            "SELECT key,value FROM society_document d WHERE \
             kind='instruction-ruling' AND EXISTS (SELECT 1 FROM \
             json_each(d.value,'$.waits') WHERE value=?1)",
            [wait],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                ))
            },
        )
        .optional()?;
    let mut woken = Vec::new();
    if let Some((task, value)) = found
        && matches!(ruling, Ruling::Go | Ruling::Withdraw)
    {
        let mut instruction =
            serde_json::from_str::<Instruction>(&value)?;
        instruction.how = Some(ruling);
        instruction.text = text.into();
        for id in &instruction.waits {
            if let Some((_, calls)) = oversight::settle(
                db,
                id,
                ruling,
                text,
                Some(chief),
                super::now(),
            )? {
                woken.extend(calls);
            }
        }
        super::keep(db, "instruction-ruling", &task, &instruction)?;
    }
    Ok(woken)
}
