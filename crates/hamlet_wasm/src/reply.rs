//! The chief's answer to a plugin's request, on its way to the
//! plugin.

use bevy::prelude::*;
use hamlet_sdk::capability::Replied;
use hamlet_sdk::plugin::{PluginInfo, Status};

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::Incoming;

/// Puts the answer in the inbox of the running plugin it is for, as
/// `granted` or `denied` naming the capability, and the target after
/// a `:` where the capability is scoped.
pub fn reply(
    replied: On<Replied>,
    mut plugins: Query<(&PluginInfo, &Status, &mut Inbox)>,
) {
    let mut name = replied.capability.name().to_string();
    if let Some(target) = &replied.target {
        name = format!("{name}:{target}");
    }
    for (info, status, mut inbox) in &mut plugins {
        if info.id == replied.plugin && *status == Status::Running {
            inbox.push(match replied.allowed {
                true => Incoming::Granted(name.clone()),
                false => Incoming::Denied(name.clone()),
            });
        }
    }
}
