//! Requests waiting for the connection to send them.

use bevy_ecs::prelude::*;
use hamlet_protocol::client as protocol;
pub use protocol::RequestId;

/// Requests queued for the connection to send.
#[derive(Resource, Default)]
pub struct Outbox(Vec<protocol::ClientMessage>);

impl Outbox {
    /// A queued request with a fresh id, returned to its caller.
    pub fn send(
        &mut self,
        message: impl FnOnce(RequestId) -> protocol::ClientMessage,
    ) -> RequestId {
        let id = RequestId::new();
        self.0.push(message(id.clone()));
        id
    }

    /// The queued requests in sending order.
    pub fn iter(
        &self,
    ) -> impl Iterator<Item = &protocol::ClientMessage> {
        self.0.iter()
    }

    /// The queued requests removed in sending order.
    pub fn drain(
        &mut self,
    ) -> impl Iterator<Item = protocol::ClientMessage> + '_ {
        self.0.drain(..)
    }
}
