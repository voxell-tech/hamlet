//! A tool call on its way through the app's thread, where observers
//! rule on it (C7).

use bevy_ecs::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

use crate::agent::{Gate, Ruling};
use crate::core::Settings;
use crate::session::worker::Msg;
use crate::tools::{Leave, Reach};

/// A tool call is about to run. An observer sets `deny` to block it
/// or `leave` to widen it, or takes `answer` to rule on it later
/// (C7).
#[derive(Event)]
pub(crate) struct ToolStarting {
    pub session: Entity,
    /// The agent making the call.
    pub who: String,
    pub name: String,
    pub args: String,
    pub reach: Reach,
    pub deny: Option<String>,
    pub leave: Leave,
    /// The sender of the ruling. Dropping it blocks the call.
    pub answer: Option<oneshot::Sender<Ruling>>,
}

/// Puts a tool call to the observers and, unless one of them took
/// it to rule on later, sends what they decided.
pub fn rule(world: &mut World, mut starting: ToolStarting) {
    // Read at every call, so the switch applies in the middle of a
    // turn.
    starting.leave.net |=
        world.resource::<Settings>().config.allow_net;
    world.trigger_ref(&mut starting);
    if let Some(answer) = starting.answer.take() {
        let _ = answer.send(match starting.deny {
            Some(reason) => Ruling::Deny(reason),
            None => Ruling::Go(starting.leave),
        });
    }
    // An observer that took it may have queued its ruling.
    world.flush();
}

/// The gate of the agent of `session`: a tool call goes to the
/// app's thread and waits there for its ruling.
pub(super) fn gate(
    out: UnboundedSender<Msg>,
    session: Entity,
) -> Gate {
    Box::new(move |request| {
        let (answer, ruling) = oneshot::channel();
        let _ = out.send(Msg::Gate(ToolStarting {
            session,
            who: request.who.to_string(),
            name: request.call.name.clone(),
            args: request.call.args.clone(),
            reach: request.reach,
            deny: None,
            leave: Leave::default(),
            answer: Some(answer),
        }));
        // No answer means nobody is left to approve it.
        Box::pin(async move {
            ruling.await.unwrap_or_else(|_| {
                Ruling::Deny(
                    "nobody is here to approve it; it did not run"
                        .into(),
                )
            })
        })
    })
}
