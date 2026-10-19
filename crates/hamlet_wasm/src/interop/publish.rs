//! Publishing an event: a copy to every plugin that imports it and
//! may use the publisher.

use bevy::prelude::*;
use hamlet_sdk::capability::{Capability, Verdict, Wanted};
use hamlet_sdk::plugin::Status;

use super::calls::Calls;
use super::mail::{Who, send};
use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Publication, Published,
};
use crate::limits::Limits;
use crate::rate::Rate;

/// The events each plugin published lately.
#[derive(Resource, Default)]
pub(super) struct Flow(Rate);

impl Flow {
    /// Whether the plugin `publisher` may publish `more` bytes at
    /// `now`, after `spent` bytes in this `step`. What it may not is
    /// counted out, with a warning.
    pub(super) fn admit(
        &mut self,
        publisher: Entity,
        id: &str,
        (spent, more): (usize, usize),
        now: f32,
        limits: &Limits,
    ) -> bool {
        let why = if spent + more > limits.publish_bytes_per_step {
            format!(
                "over {} bytes of events in a step",
                limits.publish_bytes_per_step
            )
        } else if !self.0.within(
            publisher,
            now,
            1.0,
            limits.publishes_per_second,
        ) {
            format!(
                "over {} events a second",
                limits.publishes_per_second
            )
        } else {
            return true;
        };
        warn!("plugin {id}: {why}, an event dropped");
        false
    }
}

/// Gives the event `publication` of `publisher` to the plugins that
/// import it, if the publisher lists it under `exports.events`.
pub(super) fn publish(
    publisher: Entity,
    publication: &Publication,
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &Calls,
    limits: &Limits,
    commands: &mut Commands,
) {
    let Ok((_, info, manifest, ..)) = who.get(publisher) else {
        return;
    };
    let topic = &publication.topic;
    if !manifest.exports.events.contains(topic) {
        warn!(
            "plugin {}: the event `{topic}` is not in its exports, \
             dropped",
            info.id
        );
        return;
    }
    if publication.payload.len() > limits.call_bytes {
        warn!(
            "plugin {}: the event `{topic}` has {} bytes, the most \
             is {}, dropped",
            info.id,
            publication.payload.len(),
            limits.call_bytes
        );
        return;
    }
    fan_out(
        publisher,
        topic,
        &publication.payload,
        who,
        inboxes,
        calls,
        commands,
    );
}

/// The topic of the event that tells a collection changed. Its
/// payload is the collection's name.
const CHANGED: &str = "collection-changed";

/// Tells the plugins that import it that the collection `name` of the
/// plugin `owner` changed.
pub(super) fn changed(
    owner: &str,
    name: &str,
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &Calls,
    commands: &mut Commands,
) {
    let found = who.iter().find(|(_, info, manifest, ..)| {
        info.id == owner
            && manifest.exports.collection(name).is_some()
    });
    if let Some((publisher, ..)) = found {
        fan_out(
            publisher,
            CHANGED,
            name.as_bytes(),
            who,
            inboxes,
            calls,
            commands,
        );
    }
}

/// Gives a copy of the event `topic` of `publisher` with `payload` to
/// each plugin that imports it and holds `plugin.use` for the
/// publisher, unless an event of the publisher stopped it. A plugin
/// the chief is yet to decide for is asked, and does not get this
/// one.
pub(super) fn fan_out(
    publisher: Entity,
    topic: &str,
    payload: &[u8],
    who: &Who,
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    calls: &Calls,
    commands: &mut Commands,
) {
    let Ok((_, info, ..)) = who.get(publisher) else {
        return;
    };
    for (subscriber, _, manifest, _, granted) in who {
        if subscriber == publisher
            || !manifest.imports.event(&info.id, topic)
            || calls.barred(publisher, subscriber)
        {
            continue;
        }
        match granted.verdict_for(Capability::PluginUse, &info.id) {
            Some(Verdict::Allowed) => {
                send(
                    inboxes,
                    subscriber,
                    Incoming::Event(Published {
                        topic: topic.into(),
                        sender: info.id.clone(),
                        payload: payload.to_vec(),
                    }),
                );
            }
            Some(Verdict::Ask) => {
                commands.trigger(Wanted {
                    plugin: subscriber,
                    capability: Capability::PluginUse,
                    target: Some(info.id.clone()),
                    reason: String::new(),
                });
            }
            Some(Verdict::Denied) | None => {}
        }
    }
}
