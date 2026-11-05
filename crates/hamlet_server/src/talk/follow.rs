//! What follows an input's end, once: its conversation's last turn,
//! its chief's instruction answered, a question's answer and a
//! worker's carried back, and once its agent is idle, the agent's
//! lodge committed and its scholar's learning taken in.

use anyhow::Result;
use diesel::prelude::*;
use hamlet_protocol::client::Choice;
use hamlet_store::ids::{ConversationId, OfferId, TaskId};
use hamlet_store::schema::{answer_fault, objection, review};
use serde_json::Value;

use super::conversations::{self, Conversation, Kind, Last};
use super::errands::{self, Ended};
use super::inputs::{self, Input, InputKind, Status};
use super::{Env, items, letters, reflections, tasks};
use crate::calls::LIBRARIAN;
use crate::calls::reviews::REVIEW;
use crate::calls::welcome::{self, WELCOME};
use crate::change::Change;
use crate::jobs::{self, Job};
use crate::oversight::{self, Raise, WaitKind};
use crate::society::agents::{self, AgentKind, EndHow};
use crate::society::library::{self as shelf, Verdict};
use crate::society::{dreams, workers};

/// What a follow leaves to do once its change is kept.
#[derive(Debug, Default)]
pub(crate) struct Followed {
    /// The tool calls, by runtime and key, whose wait is over.
    pub wake: Vec<(String, String)>,
}

/// Follows the end of input `request` of `conversation`, once.
pub(crate) fn follow(
    cx: &mut Change<'_>,
    env: &Env,
    conversation: ConversationId,
    request: &str,
) -> Result<Followed> {
    let mut followed = Followed::default();
    if !inputs::follow(cx.db, conversation, request)? {
        return Ok(followed);
    }
    let Some(input) = inputs::find(cx.db, conversation, request)?
    else {
        return Ok(followed);
    };
    if input.is_write() {
        return Ok(followed);
    }
    // A turn a chief stopped ends however its runtime says, often
    // aborted; it failed only when nobody stopped it.
    let stopped = conversations::get(cx.db, conversation)?.stopped;
    let last = match (input.status, input.reason.as_deref()) {
        (Some(Status::Done), _) => Last::Done,
        (_, Some("stopped" | "cancelled")) => Last::Stopped,
        _ if stopped => Last::Stopped,
        _ => Last::Failed,
    };
    conversations::set_last(cx.db, conversation, last)?;
    if stopped && !inputs::is_busy(cx.db, conversation)? {
        conversations::set_stopped(cx.db, conversation, false)?;
    }
    if input.mark()?.is_chief_turn()
        && let Some(task) = input.task
    {
        tasks::done(cx.db, task, cx.at)?;
    }
    let failed = match last {
        Last::Done => None,
        Last::Stopped => Some("its turn was stopped".to_owned()),
        Last::Failed => Some(
            input
                .reason
                .clone()
                .unwrap_or_else(|| "its turn failed".into()),
        ),
    };
    if input.kind == InputKind::Question {
        followed.wake.extend(letters::answered(
            cx,
            conversation,
            request,
            failed.as_deref(),
        )?);
    }
    let kept = conversations::get(cx.db, conversation)?;
    let agent = kept.agent;
    cx.touched.agents.insert(agent);
    if kept.kind == Kind::Errand && input.kind == InputKind::Errand {
        followed.wake.extend(errand_ended(cx, &kept, &input)?);
        return Ok(followed);
    }
    if kept.kind == Kind::Dream && input.kind == InputKind::Dream {
        if let Some(dream) =
            dreams::of_conversation(cx.db, conversation)?
        {
            let summary =
                items::answer_of(cx.db, conversation, request)?
                    .and_then(|answer| {
                        answer.lines().next().map(String::from)
                    })
                    .unwrap_or_default();
            let last = match last {
                Last::Done => dreams::Outcome::Done,
                Last::Stopped => dreams::Outcome::Stopped,
                Last::Failed => dreams::Outcome::Failed,
            };
            jobs::add(
                cx,
                &Job::DreamEnd {
                    dream: dream.id,
                    last,
                    summary,
                },
            )?;
        }
        return Ok(followed);
    }
    if kept.kind == Kind::Worker && input.kind == InputKind::Worker {
        let answer = items::answer_of(cx.db, conversation, request)?;
        let how = match (&answer, last) {
            (Some(_), Last::Done) => EndHow::Answered,
            (_, Last::Stopped) => EndHow::Stopped,
            _ => EndHow::Failed,
        };
        workers::finish(cx, agent, how, answer.as_deref())?;
        let worker = workers::get(cx.db, agent)?;
        if let (Some(runtime), Some(key), Some(task)) =
            (&worker.call_runtime, &worker.call_key, input.task)
        {
            let words = match (&answer, &failed) {
                (Some(answer), None) => answer.clone(),
                (_, Some(failure)) => {
                    format!("It could not answer: {failure}")
                }
                _ => "It gave no answer.".into(),
            };
            followed.wake.extend(letters::carry(
                cx,
                letters::Carry {
                    runtime,
                    key,
                    from: agent,
                    task,
                    request: &format!("worker:{agent}"),
                    words: &words,
                    waits: worker.waits,
                    author: None,
                },
            )?);
        }
        return Ok(followed);
    }
    if input.kind == InputKind::Reflection {
        reflections::ended(cx, conversation, request)?;
    } else if kept.kind == Kind::Chat
        && last == Last::Done
        && input.mark()?.is_chief_turn()
    {
        reflections::answered(
            cx,
            conversation,
            env.settings.reflect_every,
        )?;
    }
    if kept.kind == Kind::Chat
        && last == Last::Failed
        && input.kind != InputKind::Reflection
    {
        fault(cx, &kept, &input)?;
    }
    let found = agents::get(cx.db, agent)?;
    if found.kind != AgentKind::Worker
        && found.is_living()
        && !inputs::is_agent_busy(cx.db, agent)?
    {
        jobs::add(cx, &Job::Commit { hire: agent })?;
        jobs::add(cx, &Job::TakeIn { hire: agent })?;
    }
    Ok(followed)
}

/// What follows an errand's end: the librarian's answer carried to
/// the call that asked, or its verdict kept on the skill offered.
fn errand_ended(
    cx: &mut Change<'_>,
    kept: &Conversation,
    input: &Input,
) -> Result<Option<(String, String)>> {
    let (Some(kind), Some(owner)) = (&kept.errand, &kept.owner)
    else {
        return Ok(None);
    };
    let Some(ended) = errands::ended(cx.db, kind, owner)? else {
        return Ok(None);
    };
    if kind == REVIEW {
        reviewed(cx, owner, ended)?;
        return Ok(None);
    }
    if kind == WELCOME {
        welcome::judged(cx, owner, ended)?;
        return Ok(None);
    }
    if kind != LIBRARIAN {
        return Ok(None);
    }
    let Some(task) = input.task else {
        return Ok(None);
    };
    if let Some(call) = owner.strip_prefix("ask:") {
        let Some((runtime, key)) = call.split_once('/') else {
            return Ok(None);
        };
        let words = match ended {
            Ended::Answered(answer) => {
                format!("The librarian says: {answer}")
            }
            Ended::Failed(failure) => {
                format!("The librarian could not answer: {failure}")
            }
        };
        return letters::carry(
            cx,
            letters::Carry {
                runtime,
                key,
                from: kept.agent,
                task,
                request: &format!("librarian:{owner}"),
                words: &words,
                waits: true,
                author: Some("the librarian"),
            },
        );
    }
    let Some(id) = owner
        .strip_prefix("offer:")
        .and_then(|id| id.parse::<i64>().ok())
    else {
        return Ok(None);
    };
    let offer = shelf::get(cx.db, OfferId(id))?;
    let name = &offer.skill;
    let (verdict, words) = match ended {
        Ended::Answered(answer) => {
            let accepted = answer
                .split_whitespace()
                .next()
                .is_some_and(|first| {
                    first
                        .trim_matches(|c: char| !c.is_alphabetic())
                        .eq_ignore_ascii_case("ACCEPT")
                });
            if accepted {
                (
                    Verdict::Accepted,
                    format!(
                        "The librarian took `{name}`: {answer} It \
                         goes on the shelves for every agent; use \
                         the library's copy."
                    ),
                )
            } else {
                (
                    Verdict::Refused,
                    format!(
                        "The librarian did not take `{name}`: \
                         {answer} It stays in your lodge."
                    ),
                )
            }
        }
        Ended::Failed(failure) => (
            Verdict::Failed,
            format!(
                "The librarian could not judge `{name}`: {failure}"
            ),
        ),
    };
    if !shelf::judge(cx, offer.id, verdict, &words)? {
        return Ok(None);
    }
    if verdict == Verdict::Accepted {
        jobs::add(cx, &Job::Shelve { offer: offer.id })?;
    } else {
        shelf::event(
            cx.db,
            cx.at,
            shelf::Event {
                kind: shelf::EventKind::Refused,
                skill: name,
                agent: Some(offer.agent),
                chief: None,
                reason: Some(&words),
                commit: None,
            },
        )?;
    }
    letters::carry(
        cx,
        letters::Carry {
            runtime: &offer.runtime,
            key: &offer.key,
            from: offer.agent,
            task,
            request: &format!("librarian:{owner}"),
            words: &words,
            waits: true,
            author: Some("the librarian"),
        },
    )
}

/// Keeps how a lead's review of the instruction `owner` names ended,
/// raising its objection for the chief to rule on.
fn reviewed(
    cx: &mut Change<'_>,
    owner: &str,
    ended: Ended,
) -> Result<()> {
    let Some(task) = owner
        .strip_prefix("review:")
        .and_then(|task| task.parse::<i64>().ok())
        .map(TaskId)
    else {
        return Ok(());
    };
    let reason = match &ended {
        Ended::Answered(answer) => answer
            .trim()
            .split_once(':')
            .filter(|(word, _)| {
                word.trim_matches(|c: char| !c.is_alphabetic())
                    .eq_ignore_ascii_case("OBJECT")
            })
            .map(|(_, reason)| reason.trim().to_owned())
            .filter(|reason| !reason.is_empty()),
        Ended::Failed(_) => None,
    };
    let outcome = match (&ended, &reason) {
        (Ended::Failed(_), _) => "failed",
        (_, Some(_)) => "object",
        (_, None) => "pass",
    };
    diesel::update(review::table.find(task))
        .set(review::outcome.eq(outcome))
        .execute(cx.db)?;
    let Some(reason) = reason else {
        return Ok(());
    };
    let objected = objection::table
        .find(task)
        .count()
        .get_result::<i64>(cx.db)?
        > 0;
    let found = tasks::get(cx.db, task)?;
    let Some(conversation) = inputs::of_task(cx.db, task)? else {
        return Ok(());
    };
    if objected || found.done_at.is_some() {
        return Ok(());
    }
    let lead = review::table
        .find(task)
        .select(review::lead)
        .first::<hamlet_store::ids::AgentId>(cx.db)?;
    let lead_name = crate::society::addresses::name_of(cx.db, lead)?;
    let name =
        crate::society::addresses::name_of(cx.db, found.agent)?;
    let about = format!(
        "A chief told `{name}`: {}\n{lead_name} objects: {reason}",
        found.text.unwrap_or_default()
    );
    let on = oversight::raise(
        cx,
        Raise {
            kind: WaitKind::Objection,
            danger: None,
            agent: found.agent,
            scholar: None,
            conversation: Some(conversation),
            task: Some(task),
            about,
            change: None,
            answers: vec![Choice::Go, Choice::Withdraw],
            questions: None,
        },
    )?;
    diesel::insert_into(objection::table)
        .values((
            objection::task.eq(task),
            objection::agent.eq(lead),
            objection::source.eq("review"),
            objection::reason.eq(&reason),
            objection::wait.eq(on),
            objection::at.eq(cx.at),
        ))
        .execute(cx.db)?;
    Ok(())
}

/// Raises a fault for a chief's chat whose turn failed, unless its
/// final answer's check already did.
fn fault(
    cx: &mut Change<'_>,
    kept: &Conversation,
    input: &Input,
) -> Result<()> {
    let checked = match &input.submission {
        Some(submission) => {
            answer_fault::table
                .find((kept.id, submission))
                .count()
                .get_result::<i64>(cx.db)?
                > 0
        }
        None => false,
    };
    if checked {
        return Ok(());
    }
    let about = match input
        .detail
        .as_deref()
        .map(serde_json::from_str::<Value>)
    {
        Some(Ok(Value::String(detail))) => detail,
        _ => input
            .reason
            .clone()
            .unwrap_or_else(|| "The model failed.".into()),
    };
    oversight::raise(
        cx,
        Raise {
            kind: WaitKind::Fault,
            danger: None,
            agent: kept.agent,
            scholar: None,
            conversation: Some(kept.id),
            task: input.task,
            about,
            change: None,
            answers: vec![Choice::Dismiss],
            questions: None,
        },
    )?;
    Ok(())
}
