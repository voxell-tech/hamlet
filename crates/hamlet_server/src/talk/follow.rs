//! What follows an input's end, once: its conversation's last turn,
//! its chief's instruction answered, and once its agent is idle, the
//! agent's lodge committed and its scholar's learning taken in.

use anyhow::Result;
use hamlet_store::ids::ConversationId;

use super::conversations::{self, Last};
use super::inputs::{self, Status};
use super::tasks;
use crate::change::Change;
use crate::jobs::{self, Job};
use crate::society::agents::{self, AgentKind};

/// Follows the end of input `request` of `conversation`, once.
pub(crate) fn follow(
    cx: &mut Change<'_>,
    conversation: ConversationId,
    request: &str,
) -> Result<()> {
    if !inputs::follow(cx.db, conversation, request)? {
        return Ok(());
    }
    let Some(input) = inputs::find(cx.db, conversation, request)?
    else {
        return Ok(());
    };
    if input.is_write() {
        return Ok(());
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
    let agent = conversations::get(cx.db, conversation)?.agent;
    cx.touched.agents.insert(agent);
    let found = agents::get(cx.db, agent)?;
    if found.kind != AgentKind::Worker
        && found.is_living()
        && !inputs::is_agent_busy(cx.db, agent)?
    {
        jobs::add(cx, &Job::Commit { hire: agent })?;
        jobs::add(cx, &Job::TakeIn { hire: agent })?;
    }
    Ok(())
}
