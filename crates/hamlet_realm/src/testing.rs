//! Helpers the tests of several modules share.

use bevy::prelude::*;
use hamlet_client::realm::Seen;
use hamlet_client::receive;
use hamlet_protocol::client as protocol;

/// Receives `state` as the server's whole state.
pub(crate) fn receive_state(app: &mut App, state: protocol::State) {
    receive(app.world_mut(), protocol::ServerMessage::State(state));
}

/// Receives `update` as the next revision after the one seen.
pub(crate) fn receive_change(
    app: &mut App,
    update: protocol::Update,
) {
    let seen = app.world().resource::<Seen>();
    let frame = protocol::ServerMessage::Change(protocol::Changes {
        epoch: seen.epoch,
        revision: seen.revision + 1,
        changes: vec![update],
    });
    receive(app.world_mut(), frame);
}
