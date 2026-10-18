//! What the plugins say of each other, and putting what one sent in
//! another's inbox.

use bevy::prelude::*;
use hamlet_sdk::capability::{Granted, Uninstalled};
use hamlet_sdk::plugin::{Manifest, PluginInfo, Status};

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::Incoming;

/// The plugins of the install folder that are installed, with what
/// their manifests and grants say.
pub(super) type Who<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PluginInfo,
        &'static Manifest,
        &'static Status,
        &'static Granted,
    ),
    Without<Uninstalled>,
>;

/// Puts `incoming` in the inbox of `to`, if it is running. Whether it
/// was put there.
pub(super) fn send(
    inboxes: &mut Query<(&Status, &mut Inbox)>,
    to: Entity,
    incoming: Incoming,
) -> bool {
    let Ok((status, mut inbox)) = inboxes.get_mut(to) else {
        return false;
    };
    if *status != Status::Running {
        return false;
    }
    inbox.push(incoming);
    true
}
