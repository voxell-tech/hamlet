//! What a WASM plugin asks of the engine's services, passed on, and
//! what they send it, put in its inbox and handed over in batches.

use bevy::ecs::entity::Entity;
use bevy::ecs::message::{MessageReader, MessageWriter};
use bevy::ecs::system::Query;
use hamlet_engine_api::{Failure, Message, encode};
use hamlet_engine_bus::{Serve, Served};
use hamlet_engine_plugins::{Asked, Deliver};

use crate::instance::Instance;
use crate::panel::{Inbox, Work};

impl Instance {
    /// Passes on what the plugin `plugin` asked for in its last call,
    /// made `depth` deep.
    pub(crate) fn pass_requests(
        &mut self,
        plugin: Entity,
        depth: u32,
        asked: &mut MessageWriter<Asked>,
    ) {
        for (ticket, request) in self.take_requests() {
            asked.write(Asked {
                plugin,
                ticket,
                request,
                depth: depth.saturating_add(1),
            });
        }
    }
}

/// Puts each message and call for a running plugin in its inbox. A
/// call of a plugin that does not run fails.
pub(crate) fn deliver(
    mut delivered: MessageReader<Deliver>,
    mut calls: MessageReader<Serve>,
    mut inboxes: Query<&mut Inbox>,
    mut served: MessageWriter<Served>,
) {
    for call in calls.read() {
        match inboxes.get_mut(call.callee) {
            Ok(mut inbox) => {
                inbox.0.push_back(Work::Serve(call.clone()));
            }
            Err(_) => {
                served.write(Served {
                    call: call.call,
                    result: Err(Failure::Failed(
                        "the plugin that serves it is not running"
                            .into(),
                    )),
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
        if let Ok(mut inbox) = inboxes.get_mut(*plugin) {
            inbox.0.push_back(Work::Mail(message.clone(), *depth));
        }
    }
}

impl Inbox {
    /// The bytes of `first` and every message right after it here,
    /// and the deepest of them.
    pub(crate) fn batch(
        &mut self,
        first: (Message, u32),
    ) -> (Vec<u8>, u32) {
        let (mut messages, mut depth) = (vec![first.0], first.1);
        while let Some(Work::Mail(..)) = self.0.front() {
            if let Some(Work::Mail(message, at)) = self.0.pop_front()
            {
                messages.push(message);
                depth = depth.max(at);
            }
        }
        (encode(&messages).unwrap_or_default(), depth)
    }
}
