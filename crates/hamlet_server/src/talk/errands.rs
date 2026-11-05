//! Errands: one model call made for a society act, such as the
//! librarian's answer or a lead's review, in a conversation of its
//! own made once under its owner's key. Its prompt is its
//! instructions alone, and it is paid for by the agent it is run for.

use anyhow::Result;
use hamlet_store::Db;
use hamlet_store::ids::{AgentId, ConversationId, TaskId};

use super::conversations::{self, Begin, Kind};
use super::inputs::{self, InputKind, Keep};
use super::marks::Mark;
use super::{Env, items, models, runtimes};
use crate::change::Change;
use crate::society::agents;

/// An errand to run.
pub(crate) struct Errand<'a> {
    /// What kind of errand it is, such as `librarian`.
    pub kind: &'a str,
    /// The key it is made once under.
    pub owner: &'a str,
    /// The agent it is run for and paid by.
    pub agent: AgentId,
    pub instructions: &'a str,
    /// What it is asked.
    pub question: &'a str,
    pub task: Option<TaskId>,
}

/// The request of an errand's one input.
fn request(owner: &str) -> String {
    format!("errand:{owner}")
}

/// Runs `errand` once: its conversation and its one input.
pub(crate) fn run(
    cx: &mut Change<'_>,
    env: &Env,
    errand: Errand<'_>,
) -> Result<ConversationId> {
    if let Some(found) =
        conversations::errand(cx.db, errand.kind, errand.owner)?
    {
        return Ok(found.id);
    }
    let harness = agents::get(cx.db, errand.agent)?.harness;
    let runtime = runtimes::of_harness(cx.db, &harness)?;
    let choice = models::choose(
        cx.db,
        &env.settings,
        &env.overrides,
        errand.agent,
    )?;
    let mut opening = Begin::new(errand.agent, Kind::Errand);
    opening.errand = Some((
        errand.kind.into(),
        errand.owner.into(),
        errand.instructions.into(),
    ));
    let conversation = conversations::begin(
        cx,
        opening,
        &runtime,
        &choice.model,
        None,
    )?;
    inputs::keep(
        cx,
        Keep {
            conversation,
            request: request(errand.owner),
            kind: InputKind::Errand,
            task: errand.task,
            content: errand.question.into(),
            mark: Mark::Errand { at: cx.at },
            steer: false,
            boundary: false,
        },
    )?;
    cx.touched.agents.insert(errand.agent);
    Ok(conversation)
}

/// How an errand ended.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Ended {
    Answered(String),
    Failed(String),
}

/// How the errand of `kind` made under `owner` ended, or `None` while
/// it runs.
pub(crate) fn ended(
    db: &mut Db,
    kind: &str,
    owner: &str,
) -> Result<Option<Ended>> {
    let Some(found) = conversations::errand(db, kind, owner)? else {
        return Ok(None);
    };
    let request = request(owner);
    let Some(input) = inputs::find(db, found.id, &request)? else {
        return Ok(None);
    };
    if !input.is_ended() {
        return Ok(None);
    }
    Ok(Some(match items::answer_of(db, found.id, &request)? {
        Some(answer)
            if input.status == Some(inputs::Status::Done) =>
        {
            Ended::Answered(answer)
        }
        _ => {
            Ended::Failed(input.reason.clone().unwrap_or_else(|| {
                "the errand gave no answer".into()
            }))
        }
    }))
}
