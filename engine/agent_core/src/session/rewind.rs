//! A conversation taken back to one of its messages: that message
//! and what followed it leave the log's standing lines, the agent's
//! memory and the transcript.

use bevy_ecs::prelude::*;
use hamlet_model::log;

use crate::core::Settings;
use crate::session::deliver::{queue, staffed};
use crate::session::output::output;
use crate::session::usage::carried;
use crate::session::worker::Job;
use crate::session::{
    Entry, Item, Reply, Seat, SessionKey, Status, Transcript, Usage,
    place, reseat,
};

/// Takes a conversation back to just before the message `entry` of
/// its transcript. Nothing happens while a turn runs in it.
#[derive(Event, Clone)]
pub struct Rewind {
    pub session: Entity,
    pub entry: u64,
}

/// A conversation was taken back to just before a message.
#[derive(Event, Clone)]
pub struct Rewound {
    pub session: Entity,
    /// The time the message was said, in milliseconds since 1970.
    pub at: u64,
    /// The words of the message.
    pub text: String,
}

/// Finds the message in the log and has the session's worker take
/// the log back to it.
pub(super) fn rewind(world: &mut World, ask: Rewind) {
    let Rewind { session, entry } = ask;
    let Some(key) = world.get::<SessionKey>(session).cloned() else {
        return;
    };
    let message = world
        .get::<Transcript>(session)
        .and_then(|said| said.get(entry));
    if !matches!(message, Some(Entry::User { .. })) {
        return;
    }
    if world.get::<Status>(session).is_some_and(Status::working) {
        let said = "A turn is running here. Stop it, then rewind.";
        return output(world, session, Reply::Notice(said.into()));
    }
    let Ok((seat, _, path)) = place(world, &key) else {
        return;
    };
    // The office changed hands, or the agent moved: the conversation
    // shown is not the one the log there holds.
    if world.get::<Seat>(session) != Some(&seat) {
        let _ = reseat(world, session, &key);
        return;
    }
    let secrets = world.resource::<Settings>().config.keys();
    let events = log::read_events(&path).unwrap_or_default();
    let line = world
        .get::<Transcript>(session)
        .and_then(|said| said.line_of(entry, &events, &secrets));
    let Some(to) = line else {
        // Neither it nor anything after it reached the log.
        return rewound(world, session, entry);
    };
    match staffed(world, session, &key) {
        Ok(()) => queue(world, session, Job::Rewind { to, entry }),
        Err(e) => {
            output(world, session, Reply::Notice(format!("{e:#}")))
        }
    }
}

/// Drops the message `entry` of `session` and what followed it from
/// its transcript, once its log no longer holds them.
pub(super) fn rewound(
    world: &mut World,
    session: Entity,
    entry: u64,
) {
    let gone = world
        .get_mut::<Transcript>(session)
        .and_then(|mut said| said.rewind(entry));
    let Some(Item {
        entry: Entry::User { text, .. },
        at,
        ..
    }) = gone
    else {
        return;
    };
    let events = world
        .get::<SessionKey>(session)
        .and_then(|key| place(world, key).ok())
        .map(|(_, _, path)| {
            log::read_events(&path).unwrap_or_default()
        });
    if let (Some(events), Some(mut usage)) =
        (events, world.get_mut::<Usage>(session))
    {
        // The agent carries less. What was spent stays spent.
        let size = carried(&events);
        if usage.last_input_tokens != size {
            usage.last_input_tokens = size;
        }
    }
    world.trigger(Rewound { session, at, text });
}
