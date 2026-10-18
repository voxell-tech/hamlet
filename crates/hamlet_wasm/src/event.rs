//! What the user does in a plugin panel, on its way to the plugin.

use bevy::prelude::*;
use hamlet_openui::source::Compiled;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::plugin::{ContributedBy, PluginInfo, Status};
use hamlet_sdk::tree::event::{Act, Interaction};
use hamlet_sdk::tree::id;

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Press, Toggle, Typed,
};
use crate::limits::Limits;

/// Puts the interaction in the inbox of the running plugin whose
/// panel it was done in. A panel drawn from the language answers for
/// itself.
pub fn deliver(
    event: On<Interaction>,
    panels: Query<(&Panel, &ContributedBy), Without<Compiled>>,
    mut plugins: Query<(&PluginInfo, &Status, &mut Inbox)>,
    limits: Res<Limits>,
    time: Option<Res<Time>>,
) {
    let Some((_, by)) =
        panels.iter().find(|(panel, _)| panel.id == event.panel)
    else {
        return;
    };
    let Ok((info, status, mut inbox)) = plugins.get_mut(by.0) else {
        return;
    };
    let Some(local) = id::local_id(&info.id, &event.panel) else {
        return;
    };
    if *status != Status::Running {
        return;
    }
    let now = time.map_or(0.0, |time| time.elapsed_secs());
    if !inbox.admit(now, limits.events_per_second) {
        warn!(
            "plugin {}: over {} events a second, one dropped",
            info.id, limits.events_per_second
        );
        return;
    }
    let (panel, id) = (local.to_string(), event.node.clone());
    inbox.push(match &event.act {
        Act::Click => Incoming::Click(Press { panel, id }),
        Act::Toggled(on) => {
            Incoming::Toggled(Toggle { panel, id, on: *on })
        }
        Act::Typed(text) => Incoming::TextChanged(Typed {
            panel,
            id,
            text: text.clone(),
        }),
    });
}

/// The panel an event was done in, by the id its plugin knows it by.
pub fn panel_of(incoming: &Incoming) -> Option<&str> {
    match incoming {
        Incoming::Tick(_)
        | Incoming::Granted(_)
        | Incoming::Denied(_) => None,
        Incoming::Click(press) => Some(&press.panel),
        Incoming::Toggled(toggle) => Some(&toggle.panel),
        Incoming::TextChanged(typed) => Some(&typed.panel),
    }
}
