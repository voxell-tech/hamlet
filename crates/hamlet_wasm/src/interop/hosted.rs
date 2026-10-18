//! The events of the host, from the read models that ordinary systems
//! keep: given to the plugins that import them and hold the
//! capability each names.

use bevy::prelude::*;
use hamlet_sdk::agents::{AgentList, SelectedAgent};
use hamlet_sdk::capability::{Granted, Uninstalled, Verdict, Wanted};
use hamlet_sdk::events::{
    AGENTS_CHANGED, HOST, HostEvent, SELECTION_CHANGED,
};
use hamlet_sdk::plugin::{Manifest, Status};

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Published,
};

/// The plugins that may be given an event.
type Listeners<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Manifest,
        &'static Status,
        &'static Granted,
        &'static mut Inbox,
    ),
    Without<Uninstalled>,
>;

/// Tells the subscribers that an agent came, left, was renamed or
/// moved.
pub(super) fn agents_changed(
    _list: Res<AgentList>,
    mut listeners: Listeners,
    mut commands: Commands,
) {
    give(&AGENTS_CHANGED, b"", &mut listeners, &mut commands);
}

/// Tells the subscribers which agent is selected.
pub(super) fn selection_changed(
    selected: Res<SelectedAgent>,
    mut listeners: Listeners,
    mut commands: Commands,
) {
    let name = selected.0.as_deref().unwrap_or("");
    give(
        &SELECTION_CHANGED,
        name.as_bytes(),
        &mut listeners,
        &mut commands,
    );
}

/// Gives `event` with `payload` to each running plugin that imports
/// it and holds its capability. A plugin the chief is yet to decide
/// for is asked, and does not get this one.
fn give(
    event: &HostEvent,
    payload: &[u8],
    listeners: &mut Listeners,
    commands: &mut Commands,
) {
    for (plugin, manifest, status, granted, mut inbox) in listeners {
        if *status != Status::Running
            || !manifest.imports.event(HOST, event.topic)
        {
            continue;
        }
        match granted.verdict(event.capability) {
            Some(Verdict::Allowed) => {
                inbox.push(Incoming::Event(Published {
                    topic: event.topic.into(),
                    sender: HOST.into(),
                    payload: payload.to_vec(),
                }))
            }
            Some(Verdict::Ask) => commands.trigger(Wanted {
                plugin,
                capability: event.capability,
                target: None,
                reason: String::new(),
            }),
            Some(Verdict::Denied) | None => {}
        }
    }
}
