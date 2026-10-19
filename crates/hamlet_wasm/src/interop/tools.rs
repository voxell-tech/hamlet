//! A tool an agent calls: it goes to the plugin that gave it as a
//! command from the sender `agent`, and what the plugin replies goes
//! back to the agent as text.

use bevy::prelude::*;
use hamlet_agents_api::tools::AgentToolCalled;
use hamlet_sdk::capability::Capability;
use hamlet_sdk::plugin::Status;

use super::calls::Calls;
use super::mail::{Who, send};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    CommandCall, Incoming,
};
use crate::limits::Limits;

/// The sender the plugin sees on a tool an agent called. It is never
/// the chief, and a plugin that gives tools cannot act as the chief.
pub const AGENT: &str = "agent";

pub(super) fn called(
    mut called: On<AgentToolCalled>,
    who: Who,
    mut inboxes: Query<(&Status, &mut Inbox)>,
    mut calls: ResMut<Calls>,
    limits: Res<Limits>,
) {
    let call = called.event_mut();
    let Some(answer) = call.answer.take() else {
        return;
    };
    let found =
        who.iter().find(|(_, info, ..)| info.id == call.plugin);
    let target = match found {
        Some((entity, info, _, Status::Running, granted))
            if granted.holds(Capability::AgentTool) =>
        {
            (entity, info)
        }
        _ => {
            answer.send(Err(format!(
                "`{}` is not giving tools now",
                call.plugin
            )));
            return;
        }
    };
    let (entity, info) = target;
    let args = postcard::to_allocvec(&call.args).unwrap_or_default();
    let refusal = if args.len() > limits.call_bytes {
        Some(format!(
            "the arguments have {} bytes, the most is {}",
            args.len(),
            limits.call_bytes
        ))
    } else if calls.agents_waiting(entity)
        >= limits.tool_calls_in_flight
    {
        Some(format!(
            "{} calls of agents are waiting, the most is {}",
            calls.agents_waiting(entity),
            limits.tool_calls_in_flight
        ))
    } else {
        None
    };
    if let Some(why) = refusal {
        answer.send(Err(why));
        return;
    }
    let id = calls.open_for_agent(
        (entity, &info.id),
        limits.call_frames,
        answer,
    );
    info!(
        "the agent {} of {} calls {}/{}",
        call.agent, call.hamlet, info.id, call.tool
    );
    let sent = send(
        &mut inboxes,
        entity,
        Incoming::Command(CommandCall {
            call_id: id,
            sender: AGENT.into(),
            command: call.tool.clone(),
            args,
        }),
    );
    if !sent {
        calls.close(id);
        calls.tell_agent(id, Err("it is not running".into()));
    }
}

/// What a plugin replied, as the text an agent is shown, cut to what
/// a tool may return.
pub(super) fn text(bytes: &[u8], limits: &Limits) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    if text.len() > limits.tool_result_bytes {
        text.truncate(
            text.floor_char_boundary(limits.tool_result_bytes),
        );
        text.push_str(
            "\n[cut: the text was longer than a tool may return]",
        );
    }
    text
}
