//! What follows an input's end, once: its conversation's last turn,
//! its chief's instruction answered, a question's answer and a
//! worker's carried back, and once its agent is idle, the agent's
//! lodge committed and its scholar's learning taken in.

use anyhow::Result;
use hamlet_store::ids::ConversationId;

use super::conversations::{self, Kind, Last};
use super::inputs::{self, InputKind, Status};
use super::{items, letters, tasks};
use crate::change::Change;
use crate::jobs::{self, Job};
use crate::society::agents::{self, AgentKind, EndHow};
use crate::society::workers;

/// What a follow leaves to do once its change is kept.
#[derive(Debug, Default)]
pub(crate) struct Followed {
    /// The tool calls, by runtime and key, whose wait is over.
    pub wake: Vec<(String, String)>,
}

/// Follows the end of input `request` of `conversation`, once.
pub(crate) fn follow(
    cx: &mut Change<'_>,
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
    let last = match (input.status, input.reason.as_deref()) {
        (Some(Status::Done), _) => Last::Done,
        (_, Some("stopped" | "cancelled")) => Last::Stopped,
        _ => Last::Failed,
    };
    conversations::set_last(cx.db, conversation, last)?;
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
                },
            )?);
        }
        return Ok(followed);
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
