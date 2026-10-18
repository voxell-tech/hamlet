//! Putting what another plugin sent in a plugin's inbox.

use bevy::prelude::*;
use hamlet_sdk::plugin::Status;

use crate::inbox::Inbox;
use crate::interface::exports::hamlet::plugin::plugin::Incoming;

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
