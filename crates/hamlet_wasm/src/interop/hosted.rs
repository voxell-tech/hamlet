//! The events of the host, from the read models that ordinary systems
//! keep: given to the plugins that import them and hold the
//! capability each names.

use bevy::prelude::*;
use hamlet_core::session::{Phase, TurnObserved};
use hamlet_sdk::agents::{AgentList, SelectedAgent};
use hamlet_sdk::capability::{Granted, Uninstalled, Verdict, Wanted};
use hamlet_sdk::events::{
    AFTER_TURN, AGENTS_CHANGED, BEFORE_TURN, HOST, HostEvent,
    SELECTION_CHANGED,
};
use hamlet_sdk::plugin::{Manifest, Status, plain};

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
    give(&AGENTS_CHANGED, b"", None, &mut listeners, &mut commands);
}

/// Tells the subscribers that a turn began or ended, with the
/// agent, its hamlet, the turn's number and the names of the tools
/// it used. A plugin hears of the hamlets its ask lists, or of every
/// hamlet where it lists none. It is told and nothing more: it has
/// no way to change or stop the turn.
pub(super) fn turn_observed(
    observed: On<TurnObserved>,
    mut listeners: Listeners,
    mut commands: Commands,
) {
    let seen = observed.event();
    let event = match seen.phase {
        Phase::Before => &BEFORE_TURN,
        Phase::After => &AFTER_TURN,
    };
    let payload = format!(
        "agent={}\nhamlet={}\nturn={}\ntools={}",
        plain(&seen.agent),
        plain(&seen.hamlet),
        seen.number,
        seen.tools.join(",")
    );
    give(
        event,
        payload.as_bytes(),
        Some(&seen.hamlet),
        &mut listeners,
        &mut commands,
    );
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
        None,
        &mut listeners,
        &mut commands,
    );
}

/// Gives `event` with `payload` to each running plugin that imports
/// it and holds its capability. A plugin the chief is yet to decide
/// for is asked, and does not get this one. Where the event is of a
/// `hamlet`, a plugin whose ask lists hamlets, and not that one,
/// does not get it.
fn give(
    event: &HostEvent,
    payload: &[u8],
    hamlet: Option<&str>,
    listeners: &mut Listeners,
    commands: &mut Commands,
) {
    for (plugin, manifest, status, granted, mut inbox) in listeners {
        let listed = granted.scope(event.capability);
        if *status != Status::Running
            || !manifest.imports.event(HOST, event.topic)
            || hamlet.is_some_and(|hamlet| {
                !listed.is_empty()
                    && !listed.iter().any(|h| h == hamlet)
            })
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
