//! What a WASM plugin asks of the engine's services, passed on, and
//! what they send it, put in its inbox and handed over in batches.

use std::time::Duration;

use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::system::{Query, Res};
use hamlet_engine_api::{Failure, Message, encode};
use hamlet_engine_bus::{Serve, Served};
use hamlet_engine_core::Budgets;
use hamlet_engine_plugins::{Asked, Deliver};

use crate::budget::Spending;
use crate::instance::Instance;
use crate::panel::{Inbox, Work};

/// The bytes a batch keeps aside for the length of its list.
const LIST: usize = 16;

/// Passes on what the plugin `plugin` asked for in its last call,
/// made `depth` deep.
pub(crate) fn pass_requests(
    plugin: Entity,
    instance: &mut Instance,
    depth: u32,
    asked: &mut MessageWriter<Asked>,
) {
    for (ticket, request) in instance.take_requests() {
        asked.write(Asked {
            plugin,
            ticket,
            request,
            depth: depth + 1,
        });
    }
}

/// Puts each message and call for a running plugin in its inbox. An
/// inbox holds a second's events at most; the rest are left out, and
/// a call left out leaves its caller out of budget.
pub(crate) fn deliver(
    mut delivered: MessageReader<Deliver>,
    mut calls: MessageReader<Serve>,
    mut inboxes: Query<(&mut Inbox, &mut Spending)>,
    budgets: Res<Budgets>,
    mut served: MessageWriter<Served>,
) {
    let most = budgets.events_per_second as usize;
    for call in calls.read() {
        match inboxes.get_mut(call.callee) {
            Ok((mut inbox, _)) if inbox.0.len() < most => {
                inbox.0.push_back(Work::Serve(call.clone()));
            }
            _ => {
                served.write(Served {
                    call: call.call,
                    result: Err(Failure::OutOfBudget(
                        "the plugin that serves it is busy".into(),
                    )),
                    fuel: 0,
                    time: Duration::ZERO,
                });
            }
        }
    }
    for Deliver {
        plugin,
        message,
        depth,
    } in delivered.read()
    {
        let Ok((mut inbox, mut spending)) = inboxes.get_mut(*plugin)
        else {
            continue;
        };
        if inbox.0.len() >= most {
            spending.dropped_events += 1;
            continue;
        }
        inbox.0.push_back(Work::Mail(message.clone(), *depth));
    }
}

/// The bytes of `first` and the messages after it in `inbox`, as
/// many as fit in `most` bytes, and the deepest of them. A reply too
/// large to hand over at all is handed over as a failure.
pub(crate) fn batch(
    first: (Message, u32),
    inbox: &mut Inbox,
    most: usize,
) -> (Vec<u8>, u32) {
    let room = most.saturating_sub(LIST);
    let (mut messages, mut depth) = (Vec::new(), 0);
    let mut used = 0;
    let mut next = Some(first);
    while let Some((message, at)) = next.take() {
        let size =
            encode(&message).map_or(usize::MAX, |bytes| bytes.len());
        if used + size <= room {
            used += size;
            messages.push(message);
            depth = depth.max(at);
        } else if messages.is_empty() {
            if let Message::Reply { ticket, .. } = message {
                messages.push(Message::Reply {
                    ticket,
                    reply: Err(Failure::Failed(
                        "the reply is too large to hand over".into(),
                    )),
                });
                depth = depth.max(at);
            }
        } else {
            inbox.0.push_front(Work::Mail(message, at));
            break;
        }
        if let Some(Work::Mail(..)) = inbox.0.front()
            && let Some(Work::Mail(message, at)) = inbox.0.pop_front()
        {
            next = Some((message, at));
        }
    }
    (encode(&messages).unwrap_or_default(), depth)
}
