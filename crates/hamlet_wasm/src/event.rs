//! What the user does in a plugin panel, on its way to the plugin.

use bevy::prelude::*;
use hamlet_sdk::dock::Panel;
use hamlet_sdk::plugin::{ContributedBy, PluginInfo, Status};
use hamlet_sdk::tree::event::{Act, Interaction};
use hamlet_sdk::tree::id;

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::{
    Incoming, Press, Toggle, Typed,
};

/// Puts the interaction in the inbox of the running plugin whose
/// panel it was done in.
pub fn deliver(
    event: On<Interaction>,
    panels: Query<(&Panel, &ContributedBy)>,
    mut plugins: Query<(&PluginInfo, &Status, &mut Inbox)>,
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
        Incoming::Tick(_) => None,
        Incoming::Click(press) => Some(&press.panel),
        Incoming::Toggled(toggle) => Some(&toggle.panel),
        Incoming::TextChanged(typed) => Some(&typed.panel),
    }
}
